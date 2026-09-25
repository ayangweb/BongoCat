use bongocat_config::ShortcutTarget;
use std::sync::Arc;

type ShortcutHandler =
    dyn Fn(&ShortcutTarget) -> Result<ShortcutDispatch, ShortcutDispatchError> + Send + Sync;

/// Dispatches a matched shortcut target without exposing configuration strings
/// or platform key codes to the operating-system adapter. The application owns
/// the typed mapping and supplies this callback; the platform owner only
/// forwards the already-matched target.
#[derive(Clone)]
pub struct ShortcutDispatcher {
    handler: Arc<ShortcutHandler>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ShortcutDispatch {
    Triggered,
    ApplicationQueued,
    IgnoredApplicationCommand,
    IgnoredInactiveModel,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ShortcutDispatchError {
    ApplicationQueueFull,
    RuntimeQueueFull,
    RuntimeStopped,
}

impl ShortcutDispatcher {
    pub fn new<F>(handler: F) -> Self
    where
        F: Fn(&ShortcutTarget) -> Result<ShortcutDispatch, ShortcutDispatchError>
            + Send
            + Sync
            + 'static,
    {
        Self {
            handler: Arc::new(handler),
        }
    }

    pub fn execute(
        &self,
        target: &ShortcutTarget,
    ) -> Result<ShortcutDispatch, ShortcutDispatchError> {
        (self.handler)(target)
    }
}

#[cfg(test)]
mod dispatcher_tests {
    use super::*;
    use bongocat_config::{ShortcutCommand, ShortcutTarget};

    #[test]
    fn forwards_targets_to_the_application_handler() {
        let dispatcher = ShortcutDispatcher::new(|target| {
            assert_eq!(
                target,
                &ShortcutTarget::Application(ShortcutCommand::ToggleOverlay)
            );
            Ok(ShortcutDispatch::ApplicationQueued)
        });
        assert_eq!(
            dispatcher.execute(&ShortcutTarget::Application(ShortcutCommand::ToggleOverlay)),
            Ok(ShortcutDispatch::ApplicationQueued)
        );
    }

    #[test]
    fn preserves_the_handler_error_for_platform_diagnostics() {
        let dispatcher = ShortcutDispatcher::new(|_| Err(ShortcutDispatchError::RuntimeQueueFull));
        assert_eq!(
            dispatcher.execute(&ShortcutTarget::Application(ShortcutCommand::ToggleOverlay)),
            Err(ShortcutDispatchError::RuntimeQueueFull)
        );
    }
}

pub use global::{
    GlobalShortcutCounters, GlobalShortcutService, GlobalShortcutServiceError, ShortcutHotkeyError,
};

mod global {
    //! OS-registered global shortcuts backed by the `global-hotkey` crate
    //! (ADR-0044). A single owner thread holds the platform manager, mirrors
    //! the shared [`bongocat_config::ShortcutTable`] into real OS
    //! registrations, and forwards pressed events to the dispatcher.
    //!
    //! Platform notes:
    //! - Windows: `RegisterHotKey` posts `WM_HOTKEY` to the message queue of
    //!   the thread that created the manager's hidden window, so the owner
    //!   thread runs a Win32 message pump.
    //! - macOS: hot key events arrive through the Carbon handler on the main
    //!   event loop; creation and registration from the owner thread were
    //!   verified against `RegisterEventHotKey` in a controlled experiment.
    //!   Bindings whose keys have no Carbon scancode (ScrollLock, Pause)
    //!   cannot register and are reported as registration failures instead of
    //!   blocking the remaining bindings.
    #[cfg(test)]
    use super::ShortcutDispatch;
    use super::{ShortcutDispatchError, ShortcutDispatcher};
    use bongocat_config::{
        CompiledShortcuts, ShortcutChord, ShortcutModifiers, ShortcutTable, ShortcutTarget,
    };
    use global_hotkey::hotkey::{Code, HotKey, Modifiers};
    use global_hotkey::{GlobalHotKeyEvent, GlobalHotKeyManager, HotKeyState};
    use std::collections::{BTreeSet, HashMap};
    use std::panic::{AssertUnwindSafe, catch_unwind};
    use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
    use std::sync::mpsc;
    use std::sync::{Arc, Mutex};
    use std::time::Duration;

    /// How often the owner thread re-reads the shared shortcut table. Every
    /// table publisher (`set_shortcuts`, capture suspend/resume, behavior
    /// toggles) goes through `ShortcutTable::replace`, so polling covers all
    /// of them without a dedicated notification channel.
    const TABLE_POLL_INTERVAL: Duration = Duration::from_millis(50);
    const STARTUP_TIMEOUT: Duration = Duration::from_secs(2);

    #[derive(Debug, Clone, Eq, PartialEq)]
    pub enum GlobalShortcutServiceError {
        ManagerUnavailable(String),
        StartupTimedOut,
        WorkerPanicked,
    }

    impl std::fmt::Display for GlobalShortcutServiceError {
        fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            match self {
                Self::ManagerUnavailable(message) => {
                    write!(formatter, "global hotkey manager unavailable: {message}")
                }
                Self::StartupTimedOut => {
                    formatter.write_str("global shortcut service startup timed out")
                }
                Self::WorkerPanicked => formatter.write_str("global shortcut worker panicked"),
            }
        }
    }

    impl std::error::Error for GlobalShortcutServiceError {}

    /// Live counters for diagnostics consumers; the input pipeline no longer
    /// counts shortcut dispatch because it no longer performs matching.
    #[derive(Debug, Default)]
    pub struct GlobalShortcutCounters {
        pub registration_failures: AtomicU64,
        pub queue_overflows: AtomicU64,
        pub runtime_stopped_events: AtomicU64,
    }

    #[derive(Clone, Debug, PartialEq)]
    struct Registration {
        hotkey: HotKey,
        target: ShortcutTarget,
    }

    /// The OS registration surface the mirror drives. Production uses the real
    /// manager; tests drive it with a fake so that reconciling a changed table
    /// can be exercised without taking global hotkeys away from the machine
    /// running the tests.
    trait HotkeyRegistrar {
        fn register(&self, hotkey: HotKey) -> Result<(), String>;
        fn unregister(&self, hotkey: HotKey) -> Result<(), String>;
    }

    impl HotkeyRegistrar for GlobalHotKeyManager {
        fn register(&self, hotkey: HotKey) -> Result<(), String> {
            GlobalHotKeyManager::register(self, hotkey).map_err(|error| error.to_string())
        }

        fn unregister(&self, hotkey: HotKey) -> Result<(), String> {
            GlobalHotKeyManager::unregister(self, hotkey).map_err(|error| error.to_string())
        }
    }

    /// What a table change requires of the live OS registrations.
    #[derive(Debug, Default)]
    struct RegistrationPlan {
        /// Chords the table dropped: unregister them and forget them.
        removed: Vec<Registration>,
        /// Chords the table added: register them and remember them.
        added: Vec<Registration>,
        /// Chords the table still binds, behind a different target.
        retargeted: Vec<(u32, ShortcutTarget)>,
    }

    /// Compares the live registrations with the desired ones.
    ///
    /// A registration's identity is its hotkey id — the chord — and not the
    /// whole binding, because two models may legitimately bind the same chord:
    /// each model counts its own default behavior chords from the primary
    /// modifier's first digit, and only one model is live at a time. A model
    /// switch therefore leaves most chords registered while the target behind
    /// every one of them moves to the incoming model. A mirror that answered
    /// "already registered" for such a chord would keep the platform pointing at
    /// the outgoing model, and the dispatcher drops those targets as an inactive
    /// model — the incoming model's behavior shortcuts would silently do nothing.
    fn plan_registrations(
        registered: &HashMap<u32, Registration>,
        desired: &[Registration],
    ) -> RegistrationPlan {
        let desired_ids: BTreeSet<u32> = desired.iter().map(|entry| entry.hotkey.id).collect();
        let mut plan = RegistrationPlan::default();
        for (id, entry) in registered {
            if !desired_ids.contains(id) {
                plan.removed.push(entry.clone());
            }
        }
        for entry in desired {
            match registered.get(&entry.hotkey.id) {
                None => plan.added.push(entry.clone()),
                Some(registered) if registered.target != entry.target => {
                    plan.retargeted
                        .push((entry.hotkey.id, entry.target.clone()));
                }
                Some(_) => {}
            }
        }
        plan
    }

    /// Applies the plan to the OS registrations, keeping `registered` as the
    /// single record of what the platform currently holds.
    fn mirror_registrations<M: HotkeyRegistrar>(
        manager: &M,
        desired: &[Registration],
        registered: &mut HashMap<u32, Registration>,
        pressed: &mut PressEdges,
        registration_failures: &Mutex<Vec<String>>,
        counters: &GlobalShortcutCounters,
    ) {
        let plan = plan_registrations(registered, desired);
        // Bindings that just left the table never report a release, so a
        // retained held id would swallow the first press after the same chord
        // is bound again.
        let bound: BTreeSet<u32> = desired.iter().map(|entry| entry.hotkey.id).collect();
        pressed.retain(&bound);

        for entry in plan.removed {
            if let Err(error) = manager.unregister(entry.hotkey) {
                counters
                    .registration_failures
                    .fetch_add(1, Ordering::Relaxed);
                record_failure(registration_failures, error);
            }
            // Forgetting a chord the platform no longer holds is what lets it
            // be registered again once it re-enters the table.
            registered.remove(&entry.hotkey.id);
        }

        // A chord that stayed registered is already held by the OS; only the
        // target behind it can change. Re-registering it would be refused (or
        // duplicate the registration) instead of retargeting it.
        for (id, target) in plan.retargeted {
            if let Some(registered) = registered.get_mut(&id) {
                registered.target = target;
            }
        }

        for entry in plan.added {
            match manager.register(entry.hotkey) {
                Ok(()) => {
                    registered.insert(entry.hotkey.id, entry);
                }
                Err(error) => {
                    counters
                        .registration_failures
                        .fetch_add(1, Ordering::Relaxed);
                    record_failure(registration_failures, format!("{}: {error}", entry.hotkey));
                }
            }
        }
    }

    /// A long-lived owner of the platform global hotkey manager. Dropping or
    /// stopping the service unregisters every binding it registered.
    pub struct GlobalShortcutService {
        stop: Arc<AtomicBool>,
        owner: Option<std::thread::JoinHandle<()>>,
        registration_failures: Arc<Mutex<Vec<String>>>,
        counters: Arc<GlobalShortcutCounters>,
    }

    impl GlobalShortcutService {
        /// Starts the owner thread. The manager is created on that thread, so
        /// callers do not need to be on the platform main thread.
        pub fn start(
            table: ShortcutTable,
            dispatcher: ShortcutDispatcher,
        ) -> Result<Self, GlobalShortcutServiceError> {
            let stop = Arc::new(AtomicBool::new(false));
            let worker_stop = Arc::clone(&stop);
            let registration_failures = Arc::new(Mutex::new(Vec::new()));
            let counters = Arc::new(GlobalShortcutCounters::default());
            let (startup_sender, startup_receiver) = mpsc::sync_channel(1);
            let thread_failures = Arc::clone(&registration_failures);
            let thread_counters = Arc::clone(&counters);
            let owner = std::thread::Builder::new()
                .name("bongocat-global-shortcuts".into())
                .spawn(move || {
                    let result = catch_unwind(AssertUnwindSafe(|| {
                        run_shortcut_owner(
                            table,
                            dispatcher,
                            worker_stop,
                            &startup_sender,
                            thread_failures,
                            thread_counters,
                        )
                    }));
                    match result {
                        Ok(Ok(())) => {}
                        Ok(Err(error)) => {
                            let _ = startup_sender.send(Err(error));
                        }
                        Err(_) => {
                            let _ = startup_sender
                                .send(Err(GlobalShortcutServiceError::WorkerPanicked));
                        }
                    }
                })
                .map_err(|_| GlobalShortcutServiceError::WorkerPanicked)?;
            match startup_receiver.recv_timeout(STARTUP_TIMEOUT) {
                Ok(Ok(())) => Ok(Self {
                    stop,
                    owner: Some(owner),
                    registration_failures,
                    counters,
                }),
                Ok(Err(error)) => {
                    stop.store(true, Ordering::Release);
                    let _ = owner.join();
                    Err(error)
                }
                Err(_) => {
                    stop.store(true, Ordering::Release);
                    let _ = owner.join();
                    Err(GlobalShortcutServiceError::StartupTimedOut)
                }
            }
        }

        /// Stops the owner thread and unregisters every binding. `Drop` covers
        /// the remaining paths.
        pub fn stop(mut self) -> Result<(), GlobalShortcutServiceError> {
            self.stop.store(true, Ordering::Release);
            match self.owner.take() {
                Some(owner) => owner
                    .join()
                    .map_err(|_| GlobalShortcutServiceError::WorkerPanicked),
                None => Ok(()),
            }
        }

        /// Bindings the platform refused (already taken by another app, or no
        /// platform scancode). The remaining bindings stay registered.
        pub fn registration_failures(&self) -> Vec<String> {
            self.registration_failures
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .clone()
        }

        pub fn counters(&self) -> &GlobalShortcutCounters {
            &self.counters
        }
    }

    impl Drop for GlobalShortcutService {
        fn drop(&mut self) {
            if let Some(owner) = self.owner.take() {
                self.stop.store(true, Ordering::Release);
                let _ = owner.join();
            }
        }
    }

    fn run_shortcut_owner(
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

    fn record_failure(failures: &Mutex<Vec<String>>, failure: impl Into<String>) {
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
    fn resolve_trigger<'a>(
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
    struct PressEdges {
        held: BTreeSet<u32>,
    }

    impl PressEdges {
        /// Records one hotkey event and reports whether it is a press edge that
        /// must be dispatched.
        fn observe(&mut self, state: HotKeyState, id: u32) -> bool {
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
        fn retain(&mut self, registered: &BTreeSet<u32>) {
            self.held.retain(|id| registered.contains(id));
        }
    }

    #[cfg(target_os = "macos")]
    fn wait_and_pump(interval: Duration) {
        std::thread::sleep(interval);
    }

    #[cfg(target_os = "windows")]
    fn wait_and_pump(interval: Duration) {
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

    fn desired_registrations(compiled: &CompiledShortcuts) -> (Vec<Registration>, Vec<String>) {
        let mut registrations = Vec::new();
        let mut unsupported = Vec::new();
        for shortcut in compiled.iter() {
            match shortcut_hotkey(shortcut.chord()) {
                Ok(hotkey) => registrations.push(Registration {
                    hotkey,
                    target: shortcut.target().clone(),
                }),
                Err(error) => {
                    unsupported.push(format!("{}: {error}", shortcut.chord().canonical()))
                }
            }
        }
        (registrations, unsupported)
    }

    /// Maps a validated configuration chord onto a `global-hotkey` hotkey.
    /// Modifier aliases and key tokens were already normalized by
    /// `ShortcutChord::parse`; the canonical token set is a closed vocabulary
    /// (single letters, digits and the named keys of `NAMED_SHORTCUT_KEYS`).
    fn shortcut_hotkey(chord: &ShortcutChord) -> Result<HotKey, ShortcutHotkeyError> {
        let bits = chord.modifiers().bits();
        let mut modifiers = Modifiers::empty();
        if bits & ShortcutModifiers::CONTROL != 0 {
            modifiers |= Modifiers::CONTROL;
        }
        if bits & ShortcutModifiers::ALT != 0 {
            modifiers |= Modifiers::ALT;
        }
        if bits & ShortcutModifiers::SHIFT != 0 {
            modifiers |= Modifiers::SHIFT;
        }
        if bits & ShortcutModifiers::META != 0 {
            modifiers |= Modifiers::META;
        }
        let key = shortcut_code(chord.key())?;
        Ok(HotKey::new(Some(modifiers), key))
    }

    #[derive(Debug, Clone, Copy, Eq, PartialEq)]
    pub struct ShortcutHotkeyError;

    impl std::fmt::Display for ShortcutHotkeyError {
        fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            formatter.write_str("the key has no global hotkey mapping")
        }
    }

    impl std::error::Error for ShortcutHotkeyError {}

    fn shortcut_code(key: &str) -> Result<Code, ShortcutHotkeyError> {
        let bytes = key.as_bytes();
        if bytes.len() == 1 {
            let byte = bytes[0];
            if byte.is_ascii_uppercase() {
                return code_from_name(&format!("Key{}", byte as char));
            }
            if byte.is_ascii_digit() {
                return code_from_name(&format!("Digit{}", byte as char));
            }
        }
        let named = match key {
            "-" => "Minus",
            "=" => "Equal",
            other => other,
        };
        code_from_name(named)
    }

    fn code_from_name(name: &str) -> Result<Code, ShortcutHotkeyError> {
        name.parse::<Code>().map_err(|_| ShortcutHotkeyError)
    }

    #[cfg(test)]
    mod tests {
        use super::*;
        use bongocat_config::{
            ModelBehaviorAction, ModelBehaviorBinding, ShortcutBinding, ShortcutCommand,
            ShortcutConfig,
        };

        fn hotkey(chord: &str) -> HotKey {
            shortcut_hotkey(&ShortcutChord::parse(chord).expect("valid chord"))
                .expect("mapped hotkey")
        }

        /// A model behavior binding of one model, with the expression name
        /// shared by every model so that only the model id distinguishes them.
        fn behavior(chord: &str, model_id: &str) -> Registration {
            Registration {
                hotkey: hotkey(chord),
                target: ShortcutTarget::ModelBehavior {
                    model_id: model_id.to_owned(),
                    action: ModelBehaviorAction::Expression {
                        name: "happy".to_owned(),
                    },
                },
            }
        }

        fn command(chord: &str, command: ShortcutCommand) -> Registration {
            Registration {
                hotkey: hotkey(chord),
                target: ShortcutTarget::Application(command),
            }
        }

        /// Records the chords the mirror holds registered, so a test can assert
        /// both the calls and the state the mirror leaves behind. It rejects a
        /// re-registration of a chord it already holds, like the real manager.
        #[derive(Default)]
        struct FakeRegistrar {
            bound: std::cell::RefCell<BTreeSet<u32>>,
        }

        impl HotkeyRegistrar for FakeRegistrar {
            fn register(&self, hotkey: HotKey) -> Result<(), String> {
                assert!(
                    self.bound.borrow_mut().insert(hotkey.id),
                    "the OS refuses a chord it already holds"
                );
                Ok(())
            }

            fn unregister(&self, hotkey: HotKey) -> Result<(), String> {
                assert!(
                    self.bound.borrow_mut().remove(&hotkey.id),
                    "the OS cannot release a chord it does not hold"
                );
                Ok(())
            }
        }

        #[test]
        fn repeated_pressed_events_for_a_held_chord_dispatch_their_target_once() {
            let registration = command("Control+B", ShortcutCommand::ToggleOverlay);
            let registered = HashMap::from([(registration.hotkey.id, registration.clone())]);
            let mut pressed = PressEdges::default();
            let event = |state| GlobalHotKeyEvent {
                id: registration.hotkey.id,
                state,
            };
            assert_eq!(
                resolve_trigger(&mut pressed, event(HotKeyState::Pressed), &registered).cloned(),
                Some(registration.clone())
            );
            for _ in 0..5 {
                assert_eq!(
                    resolve_trigger(&mut pressed, event(HotKeyState::Pressed), &registered),
                    None,
                    "an OS key repeat must not dispatch the target again"
                );
            }
            assert_eq!(
                resolve_trigger(&mut pressed, event(HotKeyState::Released), &registered),
                None
            );
            assert_eq!(
                resolve_trigger(&mut pressed, event(HotKeyState::Pressed), &registered).cloned(),
                Some(registration.clone())
            );
            // A chord that is no longer bound reports no release, so its events
            // must not leave held state behind either.
            assert_eq!(
                resolve_trigger(&mut pressed, event(HotKeyState::Pressed), &HashMap::new()),
                None
            );
            assert_eq!(
                resolve_trigger(&mut pressed, event(HotKeyState::Pressed), &registered),
                None,
                "the still-held chord stays silent after an unbound event"
            );
        }

        #[test]
        fn one_hold_dispatches_a_single_press_edge_however_many_repeats_arrive() {
            let mut pressed = PressEdges::default();
            assert!(pressed.observe(HotKeyState::Pressed, 7));
            for _ in 0..5 {
                assert!(
                    !pressed.observe(HotKeyState::Pressed, 7),
                    "an OS key repeat must not dispatch a second trigger"
                );
            }
            assert!(!pressed.observe(HotKeyState::Released, 7));
            assert!(
                pressed.observe(HotKeyState::Pressed, 7),
                "the next physical press dispatches again"
            );
        }

        #[test]
        fn releases_and_idle_bindings_do_not_interfere() {
            let mut pressed = PressEdges::default();
            assert!(!pressed.observe(HotKeyState::Released, 7));
            assert!(pressed.observe(HotKeyState::Pressed, 7));
            assert!(pressed.observe(HotKeyState::Pressed, 9));
            assert!(!pressed.observe(HotKeyState::Pressed, 9));
            assert!(!pressed.observe(HotKeyState::Pressed, 7));
            assert!(!pressed.observe(HotKeyState::Released, 9));
            assert!(pressed.observe(HotKeyState::Pressed, 9));
        }

        #[test]
        fn a_table_change_re_arms_bindings_it_unregistered() {
            let mut pressed = PressEdges::default();
            assert!(pressed.observe(HotKeyState::Pressed, 7));
            // The chord leaves the table while it is still held, so no release
            // ever arrives for it.
            pressed.retain(&BTreeSet::from([9]));
            assert!(
                pressed.observe(HotKeyState::Pressed, 7),
                "a chord that left the table is armed again"
            );
            // A change that keeps the id retains the held state.
            pressed.retain(&BTreeSet::from([7]));
            assert!(!pressed.observe(HotKeyState::Pressed, 7));
        }

        fn mirror(
            registrar: &FakeRegistrar,
            registered: &mut HashMap<u32, Registration>,
            desired: &[Registration],
        ) {
            mirror_registrations(
                registrar,
                desired,
                registered,
                &mut PressEdges::default(),
                &Mutex::new(Vec::new()),
                &GlobalShortcutCounters::default(),
            );
        }

        #[test]
        fn a_model_switch_hands_the_shared_chords_to_the_incoming_model() {
            // Both models count their behavior chords from the primary
            // modifier's first digit, so the live model binds the same chords
            // before and after a switch and only the model behind them changes.
            let open_settings = command("Meta+O", ShortcutCommand::OpenSettings);
            let outgoing = behavior("Control+1", "standard");
            let incoming = behavior("Control+1", "keyboard");
            assert_eq!(outgoing.hotkey.id, incoming.hotkey.id);
            let registrar = FakeRegistrar::default();
            let mut registered = HashMap::new();
            mirror(
                &registrar,
                &mut registered,
                &[open_settings.clone(), outgoing],
            );

            mirror(
                &registrar,
                &mut registered,
                &[open_settings.clone(), incoming.clone()],
            );

            assert_eq!(
                registered
                    .get(&incoming.hotkey.id)
                    .map(|entry| &entry.target),
                Some(&incoming.target),
                "the incoming model must answer the chord it shares with the outgoing one"
            );
            assert_eq!(
                registrar.bound.borrow().len(),
                2,
                "the shared chord stayed registered instead of being registered twice"
            );
        }

        #[test]
        fn a_chord_that_left_the_table_is_registered_again_when_it_returns() {
            let standard = behavior("Control+1", "standard");
            let keyboard = behavior("Control+2", "keyboard");
            let registrar = FakeRegistrar::default();
            let mut registered = HashMap::new();
            mirror(&registrar, &mut registered, std::slice::from_ref(&standard));
            // The switch to the other model drops the chord entirely.
            mirror(&registrar, &mut registered, std::slice::from_ref(&keyboard));
            assert!(!registered.contains_key(&standard.hotkey.id));

            // Switching back binds the same chord again.
            mirror(&registrar, &mut registered, std::slice::from_ref(&standard));

            assert_eq!(
                registered
                    .get(&standard.hotkey.id)
                    .map(|entry| &entry.target),
                Some(&standard.target),
                "a chord that re-enters the table must be registered again"
            );
            assert!(registrar.bound.borrow().contains(&standard.hotkey.id));
        }

        #[test]
        fn an_unchanged_table_leaves_the_registrations_alone() {
            let desired = vec![
                command("Meta+O", ShortcutCommand::OpenSettings),
                behavior("Control+1", "standard"),
            ];
            let registrar = FakeRegistrar::default();
            let mut registered = HashMap::new();
            mirror(&registrar, &mut registered, &desired);
            mirror(&registrar, &mut registered, &desired);

            assert_eq!(registered.len(), desired.len());
            assert_eq!(registrar.bound.borrow().len(), desired.len());
        }

        #[test]
        fn modifier_bits_and_letter_digit_keys_map_onto_global_hotkeys() {
            let control_shift_b = hotkey("Control+Shift+B");
            assert_eq!(control_shift_b.key, Code::KeyB);
            assert!(control_shift_b.mods.contains(Modifiers::CONTROL));
            assert!(control_shift_b.mods.contains(Modifiers::SHIFT));
            assert!(!control_shift_b.mods.contains(Modifiers::ALT));

            assert_eq!(hotkey("Alt+M").key, Code::KeyM);
            assert_eq!(hotkey("Shift+7").key, Code::Digit7);
            assert_eq!(hotkey("0").key, Code::Digit0);
            let meta_o = hotkey("Meta+O");
            // `HotKey::new` normalizes META onto SUPER; both map to the
            // platform Cmd/Win modifier inside global-hotkey.
            assert!(meta_o.mods.contains(Modifiers::SUPER));
            assert!(!meta_o.mods.contains(Modifiers::CONTROL));
        }

        #[test]
        fn named_key_tokens_map_onto_the_closed_code_vocabulary() {
            assert_eq!(hotkey("Control+-").key, Code::Minus);
            assert_eq!(hotkey("Control+=").key, Code::Equal);
            assert_eq!(hotkey("Shift+Enter").key, Code::Enter);
            assert_eq!(hotkey("Escape").key, Code::Escape);
            assert_eq!(hotkey("F12").key, Code::F12);
            assert_eq!(hotkey("ArrowUp").key, Code::ArrowUp);
            assert_eq!(hotkey("BracketLeft").key, Code::BracketLeft);
            assert_eq!(hotkey("PrintScreen").key, Code::PrintScreen);
        }

        #[test]
        fn equal_chords_produce_equal_hotkey_ids_for_event_routing() {
            assert_eq!(hotkey("Control+Shift+B"), hotkey("shift+control+KeyB"));
            assert_ne!(hotkey("Control+B"), hotkey("Control+Shift+B"));
        }

        #[test]
        fn desired_registrations_preserve_targets_and_unique_ids() {
            let compiled = ShortcutConfig {
                commands_enabled: true,
                commands: vec![ShortcutBinding {
                    command: "toggle_overlay".to_owned(),
                    shortcut: "Control+Shift+B".to_owned(),
                }],
                model_behaviors: vec![ModelBehaviorBinding {
                    model_id: "standard".to_owned(),
                    behavior_id: "expression:happy".to_owned(),
                    shortcut: "Alt+M".to_owned(),
                }],
            }
            .compile()
            .expect("compiled shortcuts");
            let (registrations, unsupported) = desired_registrations(&compiled);
            assert!(unsupported.is_empty());
            assert_eq!(registrations.len(), 2);
            let ids: BTreeSet<u32> = registrations.iter().map(|r| r.hotkey.id).collect();
            assert_eq!(ids.len(), 2);
            assert!(registrations.iter().any(|registration| matches!(
                registration.target,
                ShortcutTarget::Application(ShortcutCommand::ToggleOverlay)
            )));
            assert!(registrations.iter().any(|registration| matches!(
                registration.target,
                ShortcutTarget::ModelBehavior { .. }
            )));
        }

        /// End-to-end proof that the owner thread really registers with the
        /// OS: a rare, harmless chord (Ctrl+Alt+0) is registered for the
        /// duration of the test and unregistered afterwards. Ignored by
        /// default because it touches process-global OS registration state;
        /// run it explicitly with `cargo test -p bongocat-platform --
        /// owner_thread_registers -- --ignored`.
        #[test]
        #[ignore = "registers a real OS-wide hotkey for the test duration"]
        fn owner_thread_registers_and_unregisters_real_hotkeys() {
            let compiled = ShortcutConfig {
                commands_enabled: true,
                commands: vec![ShortcutBinding {
                    command: "toggle_overlay".to_owned(),
                    shortcut: "Control+Alt+0".to_owned(),
                }],
                model_behaviors: Vec::new(),
            }
            .compile()
            .expect("compiled shortcuts");
            let service = GlobalShortcutService::start(
                ShortcutTable::new(compiled),
                ShortcutDispatcher::new(|_| Ok(ShortcutDispatch::IgnoredApplicationCommand)),
            )
            .expect("global shortcut service starts");
            // The owner thread must have registered the binding with the OS
            // (no startup error, no registration failure).
            assert!(service.registration_failures().is_empty());
            std::thread::sleep(std::time::Duration::from_millis(200));
            service.stop().expect("service stops and unregisters");
        }
    }
}
