//! Pure Wayland global shortcuts through the XDG Desktop Portal.

use super::*;
use ashpd::{
    AppID,
    desktop::{
        CreateSessionOptions, Session,
        global_shortcuts::{BindShortcutsOptions, GlobalShortcuts, NewShortcut},
    },
    register_host_app,
};
use bongocat_config::{BUNDLE_ID, ShortcutTablePublication};
use futures_util::{FutureExt, StreamExt, future::Either};
use std::{fmt::Arguments, future::Future};

const PORTAL_RETRY_INTERVAL: Duration = Duration::from_secs(1);
const PORTAL_CALL_TIMEOUT: Duration = Duration::from_secs(2);
const PORTAL_CLOSE_TIMEOUT: Duration = Duration::from_secs(1);

#[derive(Clone, Debug, Eq, PartialEq)]
struct PortalRegistration {
    id: String,
    trigger: String,
    description: String,
    target: ShortcutTarget,
}

impl PortalRegistration {
    fn new_shortcut(&self) -> NewShortcut {
        NewShortcut::new(&self.id, &self.description).preferred_trigger(self.trigger.as_str())
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum SessionOutcome {
    Stop,
    PublicationChanged,
}

pub(crate) fn run_shortcut_owner(
    table: ShortcutTable,
    dispatcher: ShortcutDispatcher,
    stop: Arc<AtomicBool>,
    startup: &mpsc::SyncSender<Result<(), GlobalShortcutServiceError>>,
    registration_failures: Arc<Mutex<Vec<String>>>,
    counters: Arc<GlobalShortcutCounters>,
) -> Result<(), GlobalShortcutServiceError> {
    // Portal binding may wait for the desktop's confirmation surface. Product
    // startup must not wait for that interaction.
    let _ = startup.send(Ok(()));
    async_io::block_on(run_portal_owner(
        table,
        dispatcher,
        stop,
        registration_failures,
        counters,
    ));
    Ok(())
}

async fn run_portal_owner(
    table: ShortcutTable,
    dispatcher: ShortcutDispatcher,
    stop: Arc<AtomicBool>,
    registration_failures: Arc<Mutex<Vec<String>>>,
    counters: Arc<GlobalShortcutCounters>,
) {
    let mut host_registered = false;
    loop {
        if stop.load(Ordering::Acquire) {
            return;
        }
        let publication = table.load_publication();
        if publication.capture_suspended || publication.shortcuts.iter().next().is_none() {
            replace_failures(&registration_failures, Vec::new(), &counters);
            async_io::Timer::after(TABLE_POLL_INTERVAL).await;
            continue;
        }

        match run_portal_session(
            &table,
            &publication,
            &dispatcher,
            &stop,
            &registration_failures,
            &counters,
            &mut host_registered,
        )
        .await
        {
            Ok(SessionOutcome::Stop) => return,
            Ok(SessionOutcome::PublicationChanged) => {}
            Err(failure) => {
                portal_trace(format_args!("session failed: {failure}"));
                replace_failures(&registration_failures, vec![failure], &counters);
                match wait_for_retry(&table, &publication, &stop).await {
                    SessionOutcome::Stop => return,
                    SessionOutcome::PublicationChanged => {}
                }
            }
        }
    }
}

async fn run_portal_session(
    table: &ShortcutTable,
    initial_publication: &ShortcutTablePublication,
    dispatcher: &ShortcutDispatcher,
    stop: &AtomicBool,
    registration_failures: &Mutex<Vec<String>>,
    counters: &GlobalShortcutCounters,
    host_registered: &mut bool,
) -> Result<SessionOutcome, String> {
    let app_id = AppID::try_from(BUNDLE_ID)
        .map_err(|_| "global shortcuts have an invalid application identifier".to_owned())?;
    let mut publication = initial_publication.clone();
    let mut registrations = portal_registrations(&publication.shortcuts)?;
    portal_trace(format_args!(
        "starting session with {} requested shortcut(s)",
        registrations.len()
    ));
    if !*host_registered {
        register_portal_host(app_id).await?;
        *host_registered = true;
    }
    let portal = portal_call(
        GlobalShortcuts::new(),
        "the global shortcuts portal is unavailable",
    )
    .await?;
    portal_trace(format_args!(
        "connected to GlobalShortcuts portal version {}",
        portal.version()
    ));
    let activated = portal_call(
        portal.receive_activated(),
        "the global shortcuts activation stream is unavailable",
    )
    .await?;
    let deactivated = portal_call(
        portal.receive_deactivated(),
        "the global shortcuts release stream is unavailable",
    )
    .await?;
    let changed = portal_call(
        portal.receive_shortcuts_changed(),
        "the global shortcuts change stream is unavailable",
    )
    .await?;
    let session = portal_call(
        portal.create_session(CreateSessionOptions::default()),
        "the global shortcuts portal did not create a session",
    )
    .await?;
    portal_trace(format_args!("created session"));
    let closed = match portal_call(
        session.receive_closed(),
        "the global shortcuts session cannot observe closure",
    )
    .await
    {
        Ok(closed) => closed,
        Err(error) => {
            close_session(&session).await;
            return Err(error);
        }
    };

    let requested: Vec<_> = registrations
        .iter()
        .map(PortalRegistration::new_shortcut)
        .collect();
    let response = {
        portal_trace(format_args!("requesting shortcut bindings"));
        let bind =
            portal.bind_shortcuts(&session, &requested, None, BindShortcutsOptions::default());
        let publication_change = wait_for_publication_change(table, &publication, stop);
        futures_util::pin_mut!(bind, publication_change);
        match futures_util::future::select(bind, publication_change).await {
            Either::Left((result, _)) => match result {
                Ok(request) => match request.response() {
                    Ok(response) => response,
                    Err(error) => {
                        portal_trace(format_args!("binding request was rejected: {error}"));
                        close_session(&session).await;
                        return Err("the global shortcuts request was not accepted".to_owned());
                    }
                },
                Err(error) => {
                    portal_trace(format_args!("binding call failed: {error}"));
                    close_session(&session).await;
                    return Err(
                        "the global shortcuts portal did not bind the requested shortcuts"
                            .to_owned(),
                    );
                }
            },
            Either::Right((outcome, _)) => {
                close_session(&session).await;
                return Ok(outcome);
            }
        }
    };

    let mut accepted: BTreeSet<String> = response
        .shortcuts()
        .iter()
        .map(|shortcut| shortcut.id().to_owned())
        .collect();
    portal_trace(format_args!(
        "portal accepted {} of {} shortcut(s)",
        accepted.len(),
        registrations.len()
    ));
    publish_binding_failures(registration_failures, &registrations, &accepted, counters);
    let mut targets = portal_targets(&registrations);
    let mut held = BTreeSet::new();

    futures_util::pin_mut!(activated, deactivated, changed, closed);
    loop {
        if stop.load(Ordering::Acquire) {
            close_session(&session).await;
            return Ok(SessionOutcome::Stop);
        }

        let latest = table.load_publication();
        if latest != publication {
            if latest.capture_suspended {
                close_session(&session).await;
                return Ok(SessionOutcome::PublicationChanged);
            }
            let next = match portal_registrations(&latest.shortcuts) {
                Ok(next) => next,
                Err(error) => {
                    close_session(&session).await;
                    return Err(error);
                }
            };
            if portal_binding_identity(&next) != portal_binding_identity(&registrations) {
                close_session(&session).await;
                return Ok(SessionOutcome::PublicationChanged);
            }
            registrations = next;
            targets = portal_targets(&registrations);
            publication = latest;
        }

        while let Some(Some(event)) = activated.next().now_or_never() {
            let id = event.shortcut_id();
            portal_trace(format_args!(
                "received activation signal (accepted={}, target={})",
                accepted.contains(id),
                targets.contains_key(id)
            ));
            if accepted.contains(id)
                && held.insert(id.to_owned())
                && let Some(target) = targets.get(id)
            {
                dispatch(dispatcher, target, counters);
                portal_trace(format_args!("dispatched activation"));
            }
        }
        while let Some(Some(event)) = deactivated.next().now_or_never() {
            held.remove(event.shortcut_id());
        }
        while let Some(Some(event)) = changed.next().now_or_never() {
            accepted = event
                .shortcuts()
                .iter()
                .map(|shortcut| shortcut.id().to_owned())
                .collect();
            held.retain(|id| accepted.contains(id));
            portal_trace(format_args!(
                "portal changed the accepted set to {} shortcut(s)",
                accepted.len()
            ));
            publish_binding_failures(registration_failures, &registrations, &accepted, counters);
        }
        if closed.next().now_or_never().flatten().is_some() {
            return Err("the global shortcuts portal closed the session".to_owned());
        }

        async_io::Timer::after(TABLE_POLL_INTERVAL).await;
    }
}

async fn wait_for_publication_change(
    table: &ShortcutTable,
    publication: &ShortcutTablePublication,
    stop: &AtomicBool,
) -> SessionOutcome {
    loop {
        if stop.load(Ordering::Acquire) {
            return SessionOutcome::Stop;
        }
        if table.load_publication() != *publication {
            return SessionOutcome::PublicationChanged;
        }
        async_io::Timer::after(TABLE_POLL_INTERVAL).await;
    }
}

async fn wait_for_retry(
    table: &ShortcutTable,
    publication: &ShortcutTablePublication,
    stop: &AtomicBool,
) -> SessionOutcome {
    let started = std::time::Instant::now();
    loop {
        if stop.load(Ordering::Acquire) {
            return SessionOutcome::Stop;
        }
        if table.load_publication() != *publication || started.elapsed() >= PORTAL_RETRY_INTERVAL {
            return SessionOutcome::PublicationChanged;
        }
        async_io::Timer::after(TABLE_POLL_INTERVAL).await;
    }
}

async fn close_session(session: &Session<GlobalShortcuts>) {
    let close = session.close();
    let timeout = async_io::Timer::after(PORTAL_CLOSE_TIMEOUT);
    futures_util::pin_mut!(close, timeout);
    let _ = futures_util::future::select(close, timeout).await;
}

async fn portal_call<T>(
    call: impl Future<Output = ashpd::Result<T>>,
    failure: &'static str,
) -> Result<T, String> {
    let timeout = async_io::Timer::after(PORTAL_CALL_TIMEOUT);
    futures_util::pin_mut!(call, timeout);
    match futures_util::future::select(call, timeout).await {
        Either::Left((Ok(value), _)) => Ok(value),
        Either::Left((Err(error), _)) => {
            portal_trace(format_args!("{failure}: {error}"));
            Err(failure.to_owned())
        }
        Either::Right(_) => {
            portal_trace(format_args!("{failure}: timed out"));
            Err(failure.to_owned())
        }
    }
}

async fn register_portal_host(app_id: AppID) -> Result<(), String> {
    let register = register_host_app(app_id);
    let timeout = async_io::Timer::after(PORTAL_CALL_TIMEOUT);
    futures_util::pin_mut!(register, timeout);
    match futures_util::future::select(register, timeout).await {
        Either::Left((result, _)) => host_registration_result(result),
        Either::Right(_) => Err("the desktop portal could not identify BongoCat".to_owned()),
    }
}

fn host_registration_result(result: ashpd::Result<()>) -> Result<(), String> {
    match result {
        Ok(()) => {
            portal_trace(format_args!("registered host application"));
            Ok(())
        }
        Err(error) => {
            portal_trace(format_args!(
                "host application registration failed: {error}"
            ));
            Err("the desktop portal could not identify BongoCat".to_owned())
        }
    }
}

fn portal_trace(message: Arguments<'_>) {
    if std::env::var_os("BONGOCAT_PORTAL_TRACE").is_some_and(|value| value == "1") {
        eprintln!("BongoCat GlobalShortcuts: {message}");
    }
}

fn dispatch(
    dispatcher: &ShortcutDispatcher,
    target: &ShortcutTarget,
    counters: &GlobalShortcutCounters,
) {
    match dispatcher.execute(target) {
        Ok(_) => {}
        Err(ShortcutDispatchError::ApplicationQueueFull)
        | Err(ShortcutDispatchError::RuntimeQueueFull) => {
            counters.queue_overflows.fetch_add(1, Ordering::Relaxed);
        }
        Err(ShortcutDispatchError::RuntimeStopped) => {
            counters
                .runtime_stopped_events
                .fetch_add(1, Ordering::Relaxed);
        }
    }
}

fn portal_registrations(compiled: &CompiledShortcuts) -> Result<Vec<PortalRegistration>, String> {
    compiled
        .iter()
        .map(|shortcut| {
            let chord = shortcut.chord();
            Ok(PortalRegistration {
                id: format!(
                    "shortcut-{:02x}-{:04x}",
                    chord.modifiers().bits(),
                    chord.key_hid_usage()
                ),
                trigger: portal_trigger(chord)?,
                description: format!("BongoCat shortcut ({})", chord.canonical()),
                target: shortcut.target().clone(),
            })
        })
        .collect()
}

fn portal_targets(registrations: &[PortalRegistration]) -> HashMap<String, ShortcutTarget> {
    registrations
        .iter()
        .map(|registration| (registration.id.clone(), registration.target.clone()))
        .collect()
}

fn portal_binding_identity(registrations: &[PortalRegistration]) -> Vec<(&str, &str)> {
    registrations
        .iter()
        .map(|registration| (registration.id.as_str(), registration.trigger.as_str()))
        .collect()
}

fn publish_binding_failures(
    failures: &Mutex<Vec<String>>,
    registrations: &[PortalRegistration],
    accepted: &BTreeSet<String>,
    counters: &GlobalShortcutCounters,
) {
    let refused = registrations
        .iter()
        .filter(|registration| !accepted.contains(&registration.id))
        .map(|registration| {
            format!(
                "{}: the desktop portal did not bind this shortcut",
                registration.description
            )
        })
        .collect();
    replace_failures(failures, refused, counters);
}

fn replace_failures(
    failures: &Mutex<Vec<String>>,
    next: Vec<String>,
    counters: &GlobalShortcutCounters,
) {
    let mut current = failures
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    if *current == next {
        return;
    }
    counters
        .registration_failures
        .fetch_add(next.len() as u64, Ordering::Relaxed);
    *current = next;
}

fn portal_trigger(chord: &ShortcutChord) -> Result<String, String> {
    let mut parts = Vec::with_capacity(5);
    let modifiers = chord.modifiers().bits();
    if modifiers & ShortcutModifiers::CONTROL != 0 {
        parts.push("CTRL");
    }
    if modifiers & ShortcutModifiers::ALT != 0 {
        parts.push("ALT");
    }
    if modifiers & ShortcutModifiers::SHIFT != 0 {
        parts.push("SHIFT");
    }
    if modifiers & ShortcutModifiers::META != 0 {
        parts.push("LOGO");
    }
    parts.push(portal_key_name(chord.key())?);
    Ok(parts.join("+"))
}

fn portal_key_name(key: &str) -> Result<&'static str, String> {
    if key.len() == 1 {
        let byte = key.as_bytes()[0];
        if byte.is_ascii_uppercase() {
            return Ok(match byte {
                b'A' => "a",
                b'B' => "b",
                b'C' => "c",
                b'D' => "d",
                b'E' => "e",
                b'F' => "f",
                b'G' => "g",
                b'H' => "h",
                b'I' => "i",
                b'J' => "j",
                b'K' => "k",
                b'L' => "l",
                b'M' => "m",
                b'N' => "n",
                b'O' => "o",
                b'P' => "p",
                b'Q' => "q",
                b'R' => "r",
                b'S' => "s",
                b'T' => "t",
                b'U' => "u",
                b'V' => "v",
                b'W' => "w",
                b'X' => "x",
                b'Y' => "y",
                b'Z' => "z",
                _ => unreachable!("checked ASCII uppercase"),
            });
        }
        if byte.is_ascii_digit() {
            return Ok(match byte {
                b'0' => "0",
                b'1' => "1",
                b'2' => "2",
                b'3' => "3",
                b'4' => "4",
                b'5' => "5",
                b'6' => "6",
                b'7' => "7",
                b'8' => "8",
                b'9' => "9",
                _ => unreachable!("checked ASCII digit"),
            });
        }
    }
    match key {
        "-" => Ok("minus"),
        "=" => Ok("equal"),
        "Enter" => Ok("Return"),
        "Escape" => Ok("Escape"),
        "Backspace" => Ok("BackSpace"),
        "Tab" => Ok("Tab"),
        "Space" => Ok("space"),
        "BracketLeft" => Ok("bracketleft"),
        "BracketRight" => Ok("bracketright"),
        "Backslash" => Ok("backslash"),
        "Semicolon" => Ok("semicolon"),
        "Quote" => Ok("apostrophe"),
        "Backquote" => Ok("grave"),
        "Comma" => Ok("comma"),
        "Period" => Ok("period"),
        "Slash" => Ok("slash"),
        "CapsLock" => Ok("Caps_Lock"),
        "F1" => Ok("F1"),
        "F2" => Ok("F2"),
        "F3" => Ok("F3"),
        "F4" => Ok("F4"),
        "F5" => Ok("F5"),
        "F6" => Ok("F6"),
        "F7" => Ok("F7"),
        "F8" => Ok("F8"),
        "F9" => Ok("F9"),
        "F10" => Ok("F10"),
        "F11" => Ok("F11"),
        "F12" => Ok("F12"),
        "PrintScreen" => Ok("Print"),
        "ScrollLock" => Ok("Scroll_Lock"),
        "Pause" => Ok("Pause"),
        "Insert" => Ok("Insert"),
        "Home" => Ok("Home"),
        "PageUp" => Ok("Prior"),
        "Delete" => Ok("Delete"),
        "End" => Ok("End"),
        "PageDown" => Ok("Next"),
        "ArrowRight" => Ok("Right"),
        "ArrowLeft" => Ok("Left"),
        "ArrowDown" => Ok("Down"),
        "ArrowUp" => Ok("Up"),
        _ => Err(format!("{key}: the key has no desktop portal mapping")),
    }
}
