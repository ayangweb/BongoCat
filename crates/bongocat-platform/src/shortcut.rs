use bongocat_config::{ModelBehaviorAction, ShortcutTarget};
use bongocat_runtime::{
    ExpressionId, MotionId, MotionPriority, RuntimeClient, SendError, ShortcutAction,
};
use std::sync::mpsc::SyncSender;

/// Dispatches matched model behavior shortcuts without exposing configuration
/// strings or platform key codes to the runtime. The global shortcut service
/// resolves an OS-registered hotkey event to its configured target and hands
/// it here; application-level targets are intentionally reported as ignored
/// until the settings service owns their persistence-aware command path.
#[derive(Clone)]
pub struct ShortcutDispatcher {
    runtime: RuntimeClient,
    application_sink: Option<SyncSender<bongocat_config::ShortcutCommand>>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ShortcutDispatch {
    Triggered,
    ApplicationQueued,
    IgnoredApplicationCommand,
    IgnoredInactiveModel,
}

#[derive(Debug)]
pub enum ShortcutDispatchError {
    Runtime(SendError),
    ApplicationQueueFull,
}

impl PartialEq for ShortcutDispatchError {
    fn eq(&self, other: &Self) -> bool {
        matches!(
            (self, other),
            (Self::ApplicationQueueFull, Self::ApplicationQueueFull)
                | (Self::Runtime(_), Self::Runtime(_))
        )
    }
}

impl Eq for ShortcutDispatchError {}

impl ShortcutDispatcher {
    pub fn new(runtime: RuntimeClient) -> Self {
        Self {
            runtime,
            application_sink: None,
        }
    }

    pub fn with_application_sink(
        runtime: RuntimeClient,
        application_sink: SyncSender<bongocat_config::ShortcutCommand>,
    ) -> Self {
        Self {
            runtime,
            application_sink: Some(application_sink),
        }
    }

    pub fn execute(
        &self,
        target: &ShortcutTarget,
    ) -> Result<ShortcutDispatch, ShortcutDispatchError> {
        match target {
            ShortcutTarget::Application(command) => match self.application_sink.as_ref() {
                Some(sender) => sender
                    .try_send(*command)
                    .map(|()| ShortcutDispatch::ApplicationQueued)
                    .map_err(|_| ShortcutDispatchError::ApplicationQueueFull),
                None => Ok(ShortcutDispatch::IgnoredApplicationCommand),
            },
            ShortcutTarget::ModelBehavior { model_id, action } => {
                let Some(active) = self.runtime.snapshot().active_model else {
                    return Ok(ShortcutDispatch::IgnoredInactiveModel);
                };
                if active.id.as_str() != model_id {
                    return Ok(ShortcutDispatch::IgnoredInactiveModel);
                }
                let action = match action {
                    ModelBehaviorAction::Motion { group, index } => ShortcutAction::StartMotion {
                        motion: MotionId::new(group, *index).expect("validated motion group"),
                        priority: MotionPriority::Normal,
                    },
                    ModelBehaviorAction::Expression { name } => ShortcutAction::SetExpression(
                        ExpressionId::new(name).expect("validated expression name"),
                    ),
                };
                self.runtime
                    .trigger_shortcut(action)
                    .map(|_| ShortcutDispatch::Triggered)
                    .map_err(ShortcutDispatchError::Runtime)
            }
        }
    }
}

#[cfg(test)]
mod dispatcher_tests {
    use super::*;
    use bongocat_config::{CompiledShortcuts, ShortcutBinding, ShortcutCommand, ShortcutConfig};

    fn compiled(shortcut: &str, command: &str) -> CompiledShortcuts {
        ShortcutConfig {
            commands: vec![ShortcutBinding {
                command: command.to_owned(),
                shortcut: shortcut.to_owned(),
            }],
            model_behaviors: Vec::new(),
        }
        .compile()
        .expect("compiled shortcuts")
    }

    fn first_target(compiled: &CompiledShortcuts) -> ShortcutTarget {
        compiled
            .iter()
            .next()
            .expect("one binding")
            .target()
            .clone()
    }

    #[test]
    fn application_targets_without_a_sink_are_ignored() {
        let runtime = bongocat_runtime::RuntimeOwner::start(true, 16);
        let client = runtime.client();
        client
            .wait_for_revision(1, std::time::Duration::from_secs(1))
            .expect("runtime ready");
        let dispatcher = ShortcutDispatcher::new(client.clone());
        let target = first_target(&compiled("Control+B", "toggle_overlay"));
        assert_eq!(
            dispatcher.execute(&target),
            Ok(ShortcutDispatch::IgnoredApplicationCommand)
        );
        runtime
            .shutdown(std::time::Duration::from_secs(1))
            .expect("runtime stop");
    }

    #[test]
    fn application_targets_are_queued_for_the_application_owner() {
        let runtime = bongocat_runtime::RuntimeOwner::start(true, 16);
        let client = runtime.client();
        client
            .wait_for_revision(1, std::time::Duration::from_secs(1))
            .expect("runtime ready");
        let (sender, receiver) = std::sync::mpsc::sync_channel(1);
        let dispatcher = ShortcutDispatcher::with_application_sink(client.clone(), sender);
        let target = first_target(&compiled("Meta+O", "open_settings"));
        assert_eq!(
            dispatcher.execute(&target),
            Ok(ShortcutDispatch::ApplicationQueued)
        );
        assert_eq!(
            receiver
                .recv_timeout(std::time::Duration::from_secs(1))
                .expect("open settings command"),
            ShortcutCommand::OpenSettings
        );
        runtime
            .shutdown(std::time::Duration::from_secs(1))
            .expect("runtime stop");
    }

    #[test]
    fn model_behaviors_without_an_active_model_are_ignored() {
        let runtime = bongocat_runtime::RuntimeOwner::start(true, 16);
        let client = runtime.client();
        client
            .wait_for_revision(1, std::time::Duration::from_secs(1))
            .expect("runtime ready");
        let dispatcher = ShortcutDispatcher::new(client.clone());
        let target = ShortcutTarget::ModelBehavior {
            model_id: "standard".to_owned(),
            action: ModelBehaviorAction::Expression {
                name: "happy".to_owned(),
            },
        };
        assert_eq!(
            dispatcher.execute(&target),
            Ok(ShortcutDispatch::IgnoredInactiveModel)
        );
        runtime
            .shutdown(std::time::Duration::from_secs(1))
            .expect("runtime stop");
    }
}

#[cfg(any(target_os = "macos", target_os = "windows"))]
pub use global::{
    GlobalShortcutCounters, GlobalShortcutService, GlobalShortcutServiceError, ShortcutHotkeyError,
};

#[cfg(any(target_os = "macos", target_os = "windows"))]
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
    use super::{ShortcutDispatchError, ShortcutDispatcher};
    use bongocat_config::{
        CompiledShortcuts, ShortcutChord, ShortcutModifiers, ShortcutTable, ShortcutTarget,
    };
    use bongocat_runtime::SendError;
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

    #[derive(Clone)]
    struct Registration {
        hotkey: HotKey,
        target: ShortcutTarget,
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

        let mut registered: HashMap<u32, HotKey> = HashMap::new();
        let mut targets: HashMap<u32, ShortcutTarget> = HashMap::new();
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
                let desired_ids: BTreeSet<u32> =
                    desired.iter().map(|entry| entry.hotkey.id).collect();
                for (id, hotkey) in registered.iter() {
                    if !desired_ids.contains(id)
                        && let Err(error) = manager.unregister(*hotkey)
                    {
                        counters
                            .registration_failures
                            .fetch_add(1, Ordering::Relaxed);
                        record_failure(&registration_failures, error.to_string());
                    }
                }
                for entry in &desired {
                    if registered.contains_key(&entry.hotkey.id) {
                        continue;
                    }
                    match manager.register(entry.hotkey) {
                        Ok(()) => {
                            registered.insert(entry.hotkey.id, entry.hotkey);
                            targets.insert(entry.hotkey.id, entry.target.clone());
                        }
                        Err(error) => {
                            counters
                                .registration_failures
                                .fetch_add(1, Ordering::Relaxed);
                            record_failure(
                                &registration_failures,
                                format!("{}: {error}", entry.hotkey),
                            );
                        }
                    }
                }
                snapshot = Some(latest);
            }

            while let Ok(event) = GlobalHotKeyEvent::receiver().try_recv() {
                if event.state() != HotKeyState::Pressed {
                    continue;
                }
                let Some(target) = targets.get(&event.id()) else {
                    continue;
                };
                match dispatcher.execute(target) {
                    Ok(_) => {}
                    Err(ShortcutDispatchError::ApplicationQueueFull)
                    | Err(ShortcutDispatchError::Runtime(SendError::QueueFull(_))) => {
                        counters.queue_overflows.fetch_add(1, Ordering::Relaxed);
                    }
                    Err(ShortcutDispatchError::Runtime(SendError::RuntimeStopped(_))) => {
                        counters
                            .runtime_stopped_events
                            .fetch_add(1, Ordering::Relaxed);
                    }
                }
            }

            wait_and_pump(TABLE_POLL_INTERVAL);
        }

        for hotkey in registered.into_values() {
            let _ = manager.unregister(hotkey);
        }
        Ok(())
    }

    fn record_failure(failures: &Mutex<Vec<String>>, failure: impl Into<String>) {
        failures
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .push(failure.into());
    }

    #[cfg(target_os = "macos")]
    fn wait_and_pump(interval: Duration) {
        std::thread::sleep(interval);
    }

    #[cfg(target_os = "windows")]
    fn wait_and_pump(interval: Duration) {
        use windows::Win32::Foundation::HWND;
        use windows::Win32::System::Threading::{
            MWMO_INPUTAVAILABLE, MsgWaitForMultipleObjectsEx, QS_ALLINPUT,
        };
        use windows::Win32::UI::WindowsAndMessaging::{
            DispatchMessageW, MSG, PM_REMOVE, PeekMessageW, TranslateMessage,
        };
        // SAFETY: no Win32 objects are created here; the calls only wait for
        // and drain this thread's message queue so the `WM_HOTKEY` messages
        // for the manager's hidden window (created on this thread) reach its
        // window procedure.
        unsafe {
            let _ = MsgWaitForMultipleObjectsEx(
                0,
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
            ModelBehaviorBinding, ShortcutBinding, ShortcutCommand, ShortcutConfig,
        };

        fn hotkey(chord: &str) -> HotKey {
            shortcut_hotkey(&ShortcutChord::parse(chord).expect("valid chord"))
                .expect("mapped hotkey")
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
            let runtime = bongocat_runtime::RuntimeOwner::start(true, 16);
            let client = runtime.client();
            client
                .wait_for_revision(1, std::time::Duration::from_secs(1))
                .expect("runtime ready");
            let compiled = ShortcutConfig {
                commands: vec![ShortcutBinding {
                    command: "toggle_overlay".to_owned(),
                    shortcut: "Control+Alt+0".to_owned(),
                }],
                model_behaviors: Vec::new(),
            }
            .compile()
            .expect("compiled shortcuts");
            let (sender, receiver) = std::sync::mpsc::sync_channel(1);
            let service = GlobalShortcutService::start(
                ShortcutTable::new(compiled),
                ShortcutDispatcher::with_application_sink(client.clone(), sender),
            )
            .expect("global shortcut service starts");
            // The owner thread must have registered the binding with the OS
            // (no startup error, no registration failure).
            assert!(service.registration_failures().is_empty());
            std::thread::sleep(std::time::Duration::from_millis(200));
            service.stop().expect("service stops and unregisters");
            // The application sink stayed empty: no hotkey was pressed during
            // the test.
            assert!(
                receiver
                    .recv_timeout(std::time::Duration::from_millis(50))
                    .is_err()
            );
            runtime
                .shutdown(std::time::Duration::from_secs(1))
                .expect("runtime stop");
        }
    }
}
