//! The thread that holds the platform manager and pumps its messages.
//!
//! On Windows the platform posts hotkey events to the message queue of the
//! thread that created the manager's window, so the owner has to pump messages
//! rather than sleep — which is why the two `wait_and_pump` definitions below
//! exist and why one of them is platform-gated. The edges table is what turns
//! the platform's pressed/released pairs into one event per press: the platform
//! repeats a held key, and a repeat is not a second shortcut.

use super::*;

#[cfg(not(target_os = "linux"))]
pub(crate) fn run_shortcut_owner(
    table: ShortcutTable,
    dispatcher: ShortcutDispatcher,
    stop: Arc<AtomicBool>,
    startup: &mpsc::SyncSender<Result<(), GlobalShortcutServiceError>>,
    registration_failures: Arc<Mutex<Vec<String>>>,
    counters: Arc<GlobalShortcutCounters>,
) -> Result<(), GlobalShortcutServiceError> {
    let manager = GlobalHotKeyManager::new().map_err(|error| {
        let error = GlobalShortcutServiceError::ManagerUnavailable(error.to_string());
        let _ = startup.send(Err(error.clone()));
        error
    })?;
    let _ = startup.send(Ok(()));

    let mut registered: HashMap<u32, Registration> = HashMap::new();
    let mut pressed = PressEdges::default();
    let mut snapshot: Option<CompiledShortcuts> = None;

    while !stop.load(Ordering::Acquire) {
        let latest = table.load();
        if snapshot.as_ref() != Some(&latest) {
            let (desired, unsupported) = desired_registrations(&latest);
            {
                let mut failures = registration_failures
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner);
                failures.clear();
                failures.extend(unsupported);
            }
            mirror_registrations(
                &manager,
                &desired,
                &mut registered,
                &mut pressed,
                &registration_failures,
                &counters,
            );
            snapshot = Some(latest);
        }

        while let Ok(event) = GlobalHotKeyEvent::receiver().try_recv() {
            let Some(registration) = resolve_trigger(&mut pressed, event, &registered) else {
                continue;
            };
            match dispatcher.execute(&registration.target) {
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

        wait_and_pump(TABLE_POLL_INTERVAL);
    }

    for registration in registered.into_values() {
        let _ = manager.unregister(registration.hotkey);
    }
    Ok(())
}

pub(crate) fn record_failure(failures: &Mutex<Vec<String>>, failure: impl Into<String>) {
    failures
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .push(failure.into());
}

/// Consumes one hotkey event and returns the registration it must dispatch.
///
/// Only the press edge produces a registration: the operating system
/// repeats `Pressed` for as long as a chord stays held, and those repeats
/// must be dropped instead of triggering the binding again. An event for a
/// chord that is no longer bound is dropped without touching the held state,
/// because such a binding never reports its release.
pub(crate) fn resolve_trigger<'a>(
    pressed: &mut PressEdges,
    event: GlobalHotKeyEvent,
    registered: &'a HashMap<u32, Registration>,
) -> Option<&'a Registration> {
    let registration = registered.get(&event.id)?;
    pressed
        .observe(event.state, event.id)
        .then_some(registration)
}

/// Tracks which registered hotkeys are physically held.
///
/// Both backends report one `Pressed` event per operating-system key
/// repeat while a chord stays down: Windows forwards every `WM_HOTKEY`
/// from the message pump, and the Carbon handler forwards every
/// auto-repeat. Dispatching on `Pressed` alone therefore turns a single
/// key press into a burst of triggers. Only the transition into the held
/// state is a press edge, and the matching `Released` re-arms the binding,
/// so each physical press dispatches its target once.
#[derive(Default)]
pub(crate) struct PressEdges {
    pub(crate) held: BTreeSet<u32>,
}

impl PressEdges {
    /// Records one hotkey event and reports whether it is a press edge that
    /// must be dispatched.
    pub(crate) fn observe(&mut self, state: HotKeyState, id: u32) -> bool {
        match state {
            HotKeyState::Pressed => self.held.insert(id),
            HotKeyState::Released => {
                self.held.remove(&id);
                false
            }
        }
    }

    /// Drops held ids a table change unregistered. Such a binding never
    /// reports its release, and keeping the stale id would swallow the
    /// first press after the same chord is bound again.
    pub(crate) fn retain(&mut self, registered: &BTreeSet<u32>) {
        self.held.retain(|id| registered.contains(id));
    }
}

#[cfg(target_os = "macos")]
pub(crate) fn wait_and_pump(interval: Duration) {
    std::thread::sleep(interval);
}

#[cfg(target_os = "windows")]
pub(crate) fn wait_and_pump(interval: Duration) {
    use windows::Win32::UI::WindowsAndMessaging::{
        DispatchMessageW, MSG, MWMO_INPUTAVAILABLE, MsgWaitForMultipleObjectsEx, PM_REMOVE,
        PeekMessageW, QS_ALLINPUT, TranslateMessage,
    };
    // SAFETY: no Win32 objects are created here; the calls only wait for
    // and drain this thread's message queue so the `WM_HOTKEY` messages
    // for the manager's hidden window (created on this thread) reach its
    // window procedure.
    unsafe {
        let _ = MsgWaitForMultipleObjectsEx(
            None,
            interval.as_millis().min(u32::MAX as u128) as u32,
            QS_ALLINPUT,
            MWMO_INPUTAVAILABLE,
        );
        let mut message = MSG::default();
        while PeekMessageW(&mut message, None, 0, 0, PM_REMOVE).as_bool() {
            let _ = TranslateMessage(&message);
            DispatchMessageW(&message);
        }
    }
}
