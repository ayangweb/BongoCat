// Hide the console window in packaged (release) builds so the product runs as a
// pure GUI application. Debug builds keep the console for developer logging.
#![cfg_attr(
    all(not(debug_assertions), target_os = "windows"),
    windows_subsystem = "windows"
)]
#![forbid(unsafe_code)]

#[cfg(any(target_os = "macos", target_os = "windows"))]
use async_io::Timer;
#[cfg(any(target_os = "macos", target_os = "windows"))]
use bongocat_live2d::CoreLogHandle;
#[cfg(any(target_os = "macos", target_os = "windows"))]
use bongocat_overlay::{
    OverlayContextMenuRequest, OverlayInteractionSinks, OverlaySessionOptions, OverlayWindowBounds,
    ProductOverlaySession,
};
#[cfg(any(target_os = "macos", target_os = "windows"))]
use bongocat_platform::{GlobalShortcutService, ShortcutDispatcher};
#[cfg(target_os = "windows")]
use bongocat_platform::{
    SingleInstance, SingleInstanceAction, SingleInstanceEnvironment, SingleInstanceStart,
};
#[cfg(any(target_os = "macos", target_os = "windows"))]
use bongocat_platform::{SystemMenu, SystemMenuAction, SystemMenuPresentation};
#[cfg(any(target_os = "macos", target_os = "windows"))]
use bongocat_runtime::hover_hide_delay_ms;
#[cfg(any(target_os = "macos", target_os = "windows"))]
use bongocat_ui::SettingsView;
#[cfg(any(target_os = "macos", target_os = "windows"))]
use bongocat_ui::{
    SettingsClient, SettingsError, SettingsErrorCode, SettingsModelAvailability, SettingsModelKey,
    SettingsModelOrigin, SettingsOverlay, SettingsSnapshot, SettingsWindowHandle,
    open_settings_window,
};
#[cfg(any(target_os = "macos", target_os = "windows"))]
use gpui_kit::{
    App, Application as GpuiApplication, Global, QuitMode, assets::AllAssets,
    platform::current_platform,
};
#[cfg(any(target_os = "macos", target_os = "windows"))]
use gpui_kit::{AsyncApp, Context, Window};
#[cfg(all(
    feature = "storage-test-injection",
    any(target_os = "macos", target_os = "windows")
))]
use gpui_kit::{px, size};
#[cfg(any(target_os = "macos", target_os = "windows"))]
use std::sync::atomic::{AtomicBool, Ordering};
#[cfg(target_os = "windows")]
use std::{cell::RefCell, rc::Rc};
#[cfg(any(target_os = "macos", target_os = "windows"))]
use std::{
    env, fmt,
    io::{self, Write},
    path::Path,
    path::PathBuf,
    process::{Command, Stdio},
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};
#[cfg(all(
    feature = "storage-test-injection",
    any(target_os = "macos", target_os = "windows")
))]
use zip::ZipArchive;

#[cfg(any(target_os = "macos", target_os = "windows"))]
struct StatusIconRequest {
    visible: bool,
    reply: std::sync::mpsc::SyncSender<Result<(), SettingsError>>,
}

#[cfg(any(target_os = "macos", target_os = "windows"))]
fn system_menu_presentation(snapshot: &SettingsSnapshot) -> SystemMenuPresentation {
    let locale = match snapshot.resolved_language.code() {
        "zh-CN" => "zh-CN",
        _ => "en-US",
    };
    let text = |key| bongocat_i18n::text(locale, key).to_owned();
    SystemMenuPresentation {
        title: text("system_menu.title"),
        tooltip: text("system_menu.title"),
        open_settings: text("system_menu.open_settings"),
        show_overlay: text("system_menu.show_overlay"),
        hide_overlay: text("system_menu.hide_overlay"),
        click_through: text("system_menu.click_through"),
        check_for_updates: text("system_menu.check_for_updates"),
        open_source: text("system_menu.open_source"),
        restart: text("system_menu.restart"),
        quit: text("system_menu.quit"),
        version: bongocat_i18n::format_text(
            locale,
            "system_menu.version",
            &[("version", bongocat_app::PRODUCT_VERSION.to_owned())],
        ),
        overlay_visible: snapshot.overlay_visible,
        click_through_enabled: snapshot.overlay.click_through,
        // A Production build stays gated on its channel and release signing key
        // (`bongocat_app::update_check_available`): without them a check can only fail.
        // A Development build can never update either, but the update window is where
        // that is *explained* (`Unavailable · DevelopmentBuild`), so its entry stays
        // clickable and clicking it shows the development-build explanation.
        update_check_available: bongocat_app::update_check_available()
            || matches!(
                bongocat_app::BUILD_ENVIRONMENT,
                bongocat_config::BuildEnvironment::Development
            ),
    }
}

#[cfg(any(target_os = "macos", target_os = "windows"))]
async fn apply_system_menu_overlay_action(
    client: SettingsClient,
    action: SystemMenuAction,
) -> Result<bool, String> {
    let snapshot = client
        .read_snapshot()
        .await
        .map_err(|error| error.to_string())?;
    let revision = snapshot
        .config_revision
        .ok_or_else(|| "system menu cannot update configuration during recovery".to_owned())?;
    match action {
        SystemMenuAction::ToggleOverlayVisibility => {
            client
                .set_overlay_visible(revision, !snapshot.overlay_visible)
                .await
        }
        SystemMenuAction::ToggleClickThrough => {
            client
                .set_overlay_settings(
                    revision,
                    SettingsOverlay {
                        click_through: !snapshot.overlay.click_through,
                        ..snapshot.overlay
                    },
                )
                .await
        }
        _ => return Err("invalid system menu overlay action".to_owned()),
    }
    .map(|_| true)
    .map_err(|error| error.to_string())
}

#[cfg(any(target_os = "macos", target_os = "windows"))]
fn restart_product() -> Result<(), String> {
    Command::new(env::current_exe().map_err(|error| error.to_string())?)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map(|_| ())
        .map_err(|error| error.to_string())
}

#[cfg(any(target_os = "macos", target_os = "windows"))]
const OVERLAY_PLACEMENT_DEBOUNCE: Duration = Duration::from_millis(150);

/// How long the product is given to finish starting before the first automatic check.
///
/// The check is opt-in and must never compete with startup for the network or the
/// window server.
#[cfg(any(target_os = "macos", target_os = "windows"))]
const AUTOMATIC_UPDATE_CHECK_STARTUP_DELAY: Duration = Duration::from_secs(10);

/// How often a long-running process re-checks after the first automatic check.
#[cfg(any(target_os = "macos", target_os = "windows"))]
const AUTOMATIC_UPDATE_CHECK_INTERVAL: Duration = Duration::from_secs(24 * 60 * 60);

/// How long the automatic check waits for its own result to be published.
#[cfg(any(target_os = "macos", target_os = "windows"))]
const AUTOMATIC_UPDATE_CHECK_SETTLE_ATTEMPTS: u32 = 120;
#[cfg(any(target_os = "macos", target_os = "windows"))]
const AUTOMATIC_UPDATE_CHECK_SETTLE_INTERVAL: Duration = Duration::from_millis(500);

/// How many 50ms ticks the settings-window smoke waits for its first frame.
///
/// The page assertions read state that only a render assigns, so the smoke has
/// to wait for a frame rather than for a fixed delay: on a loaded machine the
/// old 500ms start-up delay was not always enough.
#[cfg(any(target_os = "macos", target_os = "windows"))]
const SMOKE_FIRST_FRAME_WAIT_TICKS: u32 = 120;

#[cfg(any(target_os = "macos", target_os = "windows"))]
#[derive(Default)]
struct OverlayPlacementDebouncer {
    last_sent_at: Option<Instant>,
    pending: Option<OverlayWindowBounds>,
}

#[cfg(any(target_os = "macos", target_os = "windows"))]
impl OverlayPlacementDebouncer {
    fn observe(
        &mut self,
        bounds: OverlayWindowBounds,
        now: Instant,
    ) -> Option<OverlayWindowBounds> {
        self.pending = Some(bounds);
        if self
            .last_sent_at
            .is_none_or(|last| now.saturating_duration_since(last) >= OVERLAY_PLACEMENT_DEBOUNCE)
        {
            self.last_sent_at = Some(now);
            self.pending
        } else {
            None
        }
    }

    fn mark_sent(&mut self, bounds: OverlayWindowBounds) {
        if self.pending == Some(bounds) {
            self.pending = None;
        }
    }

    fn flush(&mut self, now: Instant) -> Option<OverlayWindowBounds> {
        let pending = self.pending;
        if pending.is_some() {
            self.last_sent_at = Some(now);
        }
        pending
    }
}

#[cfg(any(target_os = "macos", target_os = "windows"))]
#[derive(Clone)]
struct ProductStatusIcon {
    sender: std::sync::mpsc::SyncSender<StatusIconRequest>,
}

#[cfg(any(target_os = "macos", target_os = "windows"))]
impl bongocat_app::StatusIconCapability for ProductStatusIcon {
    fn set_visible(&self, visible: bool) -> Result<(), SettingsError> {
        let (reply, receiver) = std::sync::mpsc::sync_channel(1);
        self.sender
            .try_send(StatusIconRequest { visible, reply })
            .map_err(|_| SettingsError::new(SettingsErrorCode::StatusIconUpdateFailed))?;
        receiver
            .recv_timeout(Duration::from_secs(2))
            .map_err(|_| SettingsError::new(SettingsErrorCode::StatusIconUpdateFailed))?
    }
}

#[cfg(target_os = "windows")]
struct TaskbarIconRequest {
    visible: bool,
    reply: std::sync::mpsc::SyncSender<Result<(), SettingsError>>,
}

#[cfg(target_os = "windows")]
#[derive(Clone)]
struct ProductTaskbarIcon {
    sender: std::sync::mpsc::SyncSender<TaskbarIconRequest>,
}

#[cfg(target_os = "windows")]
impl bongocat_app::TaskbarIconCapability for ProductTaskbarIcon {
    fn set_visible(&self, visible: bool) -> Result<(), SettingsError> {
        let (reply, receiver) = std::sync::mpsc::sync_channel(1);
        self.sender
            .try_send(TaskbarIconRequest { visible, reply })
            .map_err(|_| SettingsError::new(SettingsErrorCode::TaskbarIconUpdateFailed))?;
        receiver
            .recv_timeout(Duration::from_secs(2))
            .map_err(|_| SettingsError::new(SettingsErrorCode::TaskbarIconUpdateFailed))?
    }
}

#[cfg(any(target_os = "macos", target_os = "windows"))]
const DEFAULT_RUN_SECONDS: u64 = 0;

#[cfg(any(target_os = "macos", target_os = "windows"))]
fn gpui_application() -> GpuiApplication {
    // Quitting is owned by the product shutdown paths (tray menu, smoke recipes,
    // update restart), not by window bookkeeping: the product stays alive behind
    // the overlay and the status icon even when every product window is closed,
    // so GPUI must never auto-quit on last-window-closed.
    GpuiApplication::new_inaccessible(current_platform(false)).with_quit_mode(QuitMode::Explicit)
}

#[cfg(any(target_os = "macos", target_os = "windows"))]
#[derive(Clone, Debug, Eq, PartialEq)]
struct RunOptions {
    run_duration: Duration,
    /// Set for every accepted argument except `--run-seconds` and the help flag.
    ///
    /// Smoke and diagnostic harnesses are launched by scripts and CI on machines where nobody can
    /// answer a native dialog, so the startup permission prompt is skipped for them. A product
    /// start (`--run-seconds 0`, including the login item) and a plain `cargo run` always check,
    /// exactly like a packaged Production build.
    automated_verification: bool,
    settings_window_smoke: bool,
    settings_window_open_smoke: bool,
    models_page_smoke: bool,
    hidden_model_switch_smoke: bool,
    #[cfg(feature = "storage-test-injection")]
    settings_window_state_smoke: bool,
    #[cfg(feature = "storage-test-injection")]
    panic_diagnostics_smoke: bool,
    #[cfg(feature = "storage-test-injection")]
    panic_diagnostics_smoke_child: bool,
    #[cfg(feature = "storage-test-injection")]
    diagnostics_export_smoke: bool,
    #[cfg(feature = "storage-test-injection")]
    diagnostics_export_failure_smoke: bool,
    system_menu_smoke: bool,
    startup_permission_smoke: bool,
    #[cfg(target_os = "macos")]
    application_reopen_smoke: bool,
    #[cfg(target_os = "macos")]
    startup_item_smoke: bool,
    #[cfg(target_os = "windows")]
    single_instance_smoke: bool,
    #[cfg(target_os = "windows")]
    single_instance_ready_file: Option<PathBuf>,
    #[cfg(target_os = "windows")]
    single_instance_result_file: Option<PathBuf>,
}

#[cfg(any(target_os = "macos", target_os = "windows"))]
impl RunOptions {
    fn parse(arguments: impl IntoIterator<Item = String>) -> Result<Self, RunOptionsError> {
        let mut arguments = arguments.into_iter();
        let mut run_seconds = DEFAULT_RUN_SECONDS;
        let mut automated_verification = false;
        let mut settings_window_smoke = false;
        let mut settings_window_open_smoke = false;
        let mut models_page_smoke = false;
        let mut hidden_model_switch_smoke = false;
        #[cfg(feature = "storage-test-injection")]
        let mut settings_window_state_smoke = false;
        #[cfg(feature = "storage-test-injection")]
        let mut panic_diagnostics_smoke = false;
        #[cfg(feature = "storage-test-injection")]
        let mut panic_diagnostics_smoke_child = false;
        #[cfg(feature = "storage-test-injection")]
        let mut diagnostics_export_smoke = false;
        #[cfg(feature = "storage-test-injection")]
        let mut diagnostics_export_failure_smoke = false;
        let mut system_menu_smoke = false;
        let mut startup_permission_smoke = false;
        #[cfg(target_os = "macos")]
        let mut application_reopen_smoke = false;
        #[cfg(target_os = "macos")]
        let mut startup_item_smoke = false;
        #[cfg(target_os = "windows")]
        let mut single_instance_smoke = false;
        #[cfg(target_os = "windows")]
        let mut single_instance_ready_file = None;
        #[cfg(target_os = "windows")]
        let mut single_instance_result_file = None;
        while let Some(argument) = arguments.next() {
            // Every accepted argument except the bounded run duration and the help flag selects a
            // smoke or diagnostic harness; see `RunOptions::automated_verification`. An unknown
            // argument still fails below, so the flag never turns a real start into a harness run.
            if !matches!(argument.as_str(), "--run-seconds" | "--help" | "-h") {
                automated_verification = true;
            }
            match argument.as_str() {
                "--run-seconds" => {
                    let value = arguments.next().ok_or_else(|| {
                        RunOptionsError::new("--run-seconds requires an integer value")
                    })?;
                    run_seconds = value.parse().map_err(|_| {
                        RunOptionsError::new("--run-seconds must be a non-negative integer")
                    })?;
                }
                "--settings-window-smoke" => settings_window_smoke = true,
                "--settings-window-open-smoke" => settings_window_open_smoke = true,
                "--models-page-smoke" => {
                    models_page_smoke = true;
                    settings_window_smoke = true;
                }
                "--hidden-model-switch-smoke" => hidden_model_switch_smoke = true,
                #[cfg(feature = "storage-test-injection")]
                "--settings-window-state-smoke" => settings_window_state_smoke = true,
                #[cfg(feature = "storage-test-injection")]
                "--panic-diagnostics-smoke" => panic_diagnostics_smoke = true,
                #[cfg(feature = "storage-test-injection")]
                "--panic-diagnostics-smoke-child" => panic_diagnostics_smoke_child = true,
                #[cfg(feature = "storage-test-injection")]
                "--diagnostics-export-smoke" => diagnostics_export_smoke = true,
                #[cfg(feature = "storage-test-injection")]
                "--diagnostics-export-failure-smoke" => diagnostics_export_failure_smoke = true,
                "--system-menu-smoke" => system_menu_smoke = true,
                "--startup-permission-smoke" => startup_permission_smoke = true,
                #[cfg(target_os = "macos")]
                "--application-reopen-smoke" => application_reopen_smoke = true,
                #[cfg(target_os = "macos")]
                "--startup-item-smoke" => startup_item_smoke = true,
                #[cfg(target_os = "windows")]
                "--single-instance-smoke" => single_instance_smoke = true,
                #[cfg(target_os = "windows")]
                "--single-instance-ready-file" => {
                    let value = arguments.next().ok_or_else(|| {
                        RunOptionsError::new("--single-instance-ready-file requires a file path")
                    })?;
                    if value.is_empty() {
                        return Err(RunOptionsError::new(
                            "--single-instance-ready-file requires a non-empty file path",
                        ));
                    }
                    single_instance_ready_file = Some(PathBuf::from(value));
                }
                #[cfg(target_os = "windows")]
                "--single-instance-result-file" => {
                    let value = arguments.next().ok_or_else(|| {
                        RunOptionsError::new("--single-instance-result-file requires a file path")
                    })?;
                    if value.is_empty() {
                        return Err(RunOptionsError::new(
                            "--single-instance-result-file requires a non-empty file path",
                        ));
                    }
                    single_instance_result_file = Some(PathBuf::from(value));
                }
                "--help" | "-h" => return Err(RunOptionsError::help()),
                _ => {
                    return Err(RunOptionsError::new(format!(
                        "unknown argument {argument:?}"
                    )));
                }
            }
        }
        #[cfg(target_os = "windows")]
        if (!single_instance_smoke)
            && (single_instance_ready_file.is_some() || single_instance_result_file.is_some())
        {
            return Err(RunOptionsError::new(
                "single-instance marker arguments require --single-instance-smoke",
            ));
        }
        Ok(Self {
            run_duration: Duration::from_secs(run_seconds),
            automated_verification,
            settings_window_smoke,
            settings_window_open_smoke,
            models_page_smoke,
            hidden_model_switch_smoke,
            #[cfg(feature = "storage-test-injection")]
            settings_window_state_smoke,
            #[cfg(feature = "storage-test-injection")]
            panic_diagnostics_smoke,
            #[cfg(feature = "storage-test-injection")]
            panic_diagnostics_smoke_child,
            #[cfg(feature = "storage-test-injection")]
            diagnostics_export_smoke,
            #[cfg(feature = "storage-test-injection")]
            diagnostics_export_failure_smoke,
            system_menu_smoke,
            startup_permission_smoke,
            #[cfg(target_os = "macos")]
            application_reopen_smoke,
            #[cfg(target_os = "macos")]
            startup_item_smoke,
            #[cfg(target_os = "windows")]
            single_instance_smoke,
            #[cfg(target_os = "windows")]
            single_instance_ready_file,
            #[cfg(target_os = "windows")]
            single_instance_result_file,
        })
    }

    fn opens_settings_window_on_start(&self) -> bool {
        let mut opens_settings_window =
            self.settings_window_smoke || self.settings_window_open_smoke;
        #[cfg(target_os = "macos")]
        {
            opens_settings_window |= self.application_reopen_smoke;
        }
        #[cfg(target_os = "windows")]
        {
            opens_settings_window |= self.single_instance_smoke;
        }
        opens_settings_window
    }
}

#[cfg(any(target_os = "macos", target_os = "windows"))]
#[derive(Debug, Eq, PartialEq)]
struct RunOptionsError {
    message: String,
    help: bool,
}

#[cfg(any(target_os = "macos", target_os = "windows"))]
impl RunOptionsError {
    fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            help: false,
        }
    }

    fn help() -> Self {
        Self {
            message: usage().to_owned(),
            help: true,
        }
    }
}

#[cfg(any(target_os = "macos", target_os = "windows"))]
impl fmt::Display for RunOptionsError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.help {
            formatter.write_str(&self.message)
        } else {
            write!(formatter, "{}\n\n{}", self.message, usage())
        }
    }
}

#[cfg(any(target_os = "macos", target_os = "windows"))]
impl std::error::Error for RunOptionsError {}

#[cfg(any(target_os = "macos", target_os = "windows"))]
fn usage() -> &'static str {
    #[cfg(all(target_os = "windows", feature = "storage-test-injection"))]
    return "Usage: bongocat-app [--run-seconds <seconds>] [--settings-window-smoke] [--settings-window-open-smoke] [--models-page-smoke] [--hidden-model-switch-smoke] [--settings-window-state-smoke] [--panic-diagnostics-smoke] [--diagnostics-export-smoke] [--diagnostics-export-failure-smoke] [--system-menu-smoke] [--startup-permission-smoke] [--single-instance-smoke] [--single-instance-ready-file <path>] [--single-instance-result-file <path>]\n\nThe application runs until it is explicitly quit by default. A positive value enables a bounded diagnostic run.";

    #[cfg(all(target_os = "windows", not(feature = "storage-test-injection")))]
    return "Usage: bongocat-app [--run-seconds <seconds>] [--settings-window-smoke] [--settings-window-open-smoke] [--models-page-smoke] [--hidden-model-switch-smoke] [--system-menu-smoke] [--startup-permission-smoke] [--single-instance-smoke] [--single-instance-ready-file <path>] [--single-instance-result-file <path>]\n\nThe application runs until it is explicitly quit by default. A positive value enables a bounded diagnostic run.";

    #[cfg(all(target_os = "macos", feature = "storage-test-injection"))]
    return "Usage: bongocat-app [--run-seconds <seconds>] [--settings-window-smoke] [--settings-window-open-smoke] [--models-page-smoke] [--hidden-model-switch-smoke] [--settings-window-state-smoke] [--panic-diagnostics-smoke] [--diagnostics-export-smoke] [--diagnostics-export-failure-smoke] [--system-menu-smoke] [--startup-permission-smoke] [--application-reopen-smoke] [--startup-item-smoke]\n\nThe application runs until it is explicitly quit by default. A positive value enables a bounded diagnostic run.";

    #[cfg(all(target_os = "macos", not(feature = "storage-test-injection")))]
    "Usage: bongocat-app [--run-seconds <seconds>] [--settings-window-smoke] [--settings-window-open-smoke] [--models-page-smoke] [--hidden-model-switch-smoke] [--system-menu-smoke] [--startup-permission-smoke] [--application-reopen-smoke] [--startup-item-smoke]\n\nThe application runs until it is explicitly quit by default. A positive value enables a bounded diagnostic run."
}

#[cfg(any(target_os = "macos", target_os = "windows"))]
#[derive(Debug)]
struct ProductRunError {
    failures: Vec<String>,
}

#[cfg(any(target_os = "macos", target_os = "windows"))]
impl fmt::Display for ProductRunError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "product run failed: {}",
            self.failures.join("; ")
        )
    }
}

#[cfg(any(target_os = "macos", target_os = "windows"))]
impl std::error::Error for ProductRunError {}

#[cfg(any(target_os = "macos", target_os = "windows"))]
struct ProductCoordinator {
    _core_log: CoreLogHandle,
    #[cfg(target_os = "macos")]
    overlay: Option<ProductOverlaySession>,
    #[cfg(target_os = "windows")]
    overlay: Rc<RefCell<Option<ProductOverlaySession>>>,
    settings_service: Option<bongocat_app::ApplicationSettingsService>,
    settings_window: Option<SettingsWindowHandle>,
    /// The worker that owns the update pipeline.
    update_service: Option<bongocat_app::ApplicationUpdateService>,
    /// The open update window, if any.
    update_window: Option<bongocat_ui::UpdateWindowHandle>,
    /// The display language the update window opens with.
    ///
    /// Kept here because the system menu loop already reads the settings snapshot
    /// every 50 ms; the update window then opens without a blocking read on the GPUI
    /// thread and keeps itself in sync afterwards.
    update_language: bongocat_ui::SettingsLanguage,
    /// The appearance the update window opens with.
    ///
    /// The same reason as `update_language`: the update window has to apply the
    /// product's theme on its first frame, and the settings snapshot only reaches it
    /// on the next poll. Opening on the default would let it clear an override the
    /// settings window has already installed (ADR-0048).
    update_appearance_theme: bongocat_ui::SettingsTheme,
    /// When a completed install that needs a restart was first observed.
    #[cfg(target_os = "macos")]
    update_installed_since: Option<Instant>,
    /// Whether the post-install restart has already been started.
    #[cfg(target_os = "macos")]
    update_restart_started: bool,
    system_menu: Option<SystemMenu>,
    #[cfg(target_os = "windows")]
    taskbar_icon_visible: bool,
    #[cfg(target_os = "macos")]
    application_reopens: u64,
    #[cfg(target_os = "windows")]
    single_instance: Option<SingleInstance>,
    #[cfg(target_os = "windows")]
    single_instance_wakes: u64,
    frame_source_running: bool,
    frame_source_shutdown: FrameSourceShutdown,
    shortcut_signals: bongocat_app::ApplicationShortcutSignals,
    shortcut_service: Option<bongocat_platform::GlobalShortcutService>,
    frame_ticks: u64,
    expect_visible_frame: bool,
    failures: Arc<Mutex<Vec<String>>>,
    #[cfg(target_os = "windows")]
    shutdown_requested: Arc<AtomicBool>,
    #[cfg(target_os = "windows")]
    shutdown_flush_complete: Arc<AtomicBool>,
}

#[cfg(any(target_os = "macos", target_os = "windows"))]
impl Global for ProductCoordinator {}

#[cfg(any(target_os = "macos", target_os = "windows"))]
#[derive(Clone, Debug, Default)]
struct FrameSourceShutdown {
    stop_requested: Arc<AtomicBool>,
    stopped: Arc<AtomicBool>,
}

#[cfg(any(target_os = "macos", target_os = "windows"))]
impl FrameSourceShutdown {
    fn request_stop(&self) {
        self.stop_requested.store(true, Ordering::Release);
    }

    fn stop_requested(&self) -> bool {
        self.stop_requested.load(Ordering::Acquire)
    }

    fn is_stopped(&self) -> bool {
        self.stopped.load(Ordering::Acquire)
    }

    fn run_guard(&self) -> FrameSourceRunGuard {
        FrameSourceRunGuard {
            stopped: Arc::clone(&self.stopped),
        }
    }

    async fn wait_for_stop(&self) -> bool {
        const MAX_ATTEMPTS: u32 = 200;
        for _ in 0..MAX_ATTEMPTS {
            if self.is_stopped() {
                return true;
            }
            Timer::after(Duration::from_millis(10)).await;
        }
        self.is_stopped()
    }
}

#[cfg(any(target_os = "macos", target_os = "windows"))]
struct FrameSourceRunGuard {
    stopped: Arc<AtomicBool>,
}

#[cfg(any(target_os = "macos", target_os = "windows"))]
impl Drop for FrameSourceRunGuard {
    fn drop(&mut self) {
        self.stopped.store(true, Ordering::Release);
    }
}

#[cfg(any(target_os = "macos", target_os = "windows"))]
fn record_failure(failures: &Arc<Mutex<Vec<String>>>, failure: impl Into<String>) {
    failures
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .push(failure.into());
}

#[cfg(any(target_os = "macos", target_os = "windows"))]
const fn native_theme_for_startup(
    theme: bongocat_config::Theme,
) -> Option<bongocat_platform::AppTheme> {
    match theme {
        bongocat_config::Theme::System => None,
        bongocat_config::Theme::Light => Some(bongocat_platform::AppTheme::Light),
        bongocat_config::Theme::Dark => Some(bongocat_platform::AppTheme::Dark),
    }
}

/// Run one update against the settings view, retrying while GPUI cannot hand the
/// window over.
///
/// The window is pre-rendered and kept for the product lifetime on both platforms, so a
/// close no longer releases the view; what can still fail is `AsyncApp::update` returning
/// `Err` while the platform is inside a window callback of its own.
#[cfg(any(target_os = "macos", target_os = "windows"))]
async fn update_settings_window<R>(
    cx: &mut AsyncApp,
    window_handle: &SettingsWindowHandle,
    mut update: impl FnMut(
        &mut SettingsView,
        &mut Window,
        &mut Context<SettingsView>,
    ) -> Result<R, String>,
) -> Result<R, String> {
    const MAX_ATTEMPTS: u32 = 200;
    for attempt in 0..MAX_ATTEMPTS {
        match window_handle.update(cx, |view, window, cx| update(view, window, cx)) {
            Ok(Ok(result)) => return Ok(result),
            Ok(Err(error)) => return Err(error),
            Err(_) if attempt + 1 < MAX_ATTEMPTS => {
                Timer::after(Duration::from_millis(5)).await;
            }
            Err(error) => {
                return Err(format!(
                    "settings window remained unavailable after {MAX_ATTEMPTS} attempts: {error}"
                ));
            }
        }
    }
    unreachable!("the bounded settings update loop always returns")
}

#[cfg(target_os = "windows")]
fn request_windows_product_quit(shutdown_requested: &AtomicBool) {
    shutdown_requested.store(true, Ordering::Release);
}

#[cfg(any(target_os = "macos", target_os = "windows"))]
fn finish_product_quit(cx: &mut App) {
    #[cfg(target_os = "macos")]
    cx.quit();

    #[cfg(target_os = "windows")]
    {
        if let Some(coordinator) = cx.try_global::<ProductCoordinator>() {
            coordinator
                .shutdown_requested
                .store(true, Ordering::Release);
            coordinator
                .shutdown_flush_complete
                .store(true, Ordering::Release);
        }
    }
}

#[cfg(any(target_os = "macos", target_os = "windows"))]
fn request_product_quit(cx: &mut App) {
    let window = cx
        .try_global::<ProductCoordinator>()
        .and_then(|coordinator| coordinator.settings_window.clone());
    if let Some(window) = window
        && window.request_quit_after_flush(cx).is_ok()
    {
        return;
    }
    finish_product_quit(cx);
}

#[cfg(target_os = "windows")]
fn start_windows_product_shutdown(cx: &mut App) {
    if !cx.has_global::<ProductCoordinator>() {
        return;
    }
    let shutdown = begin_product_shutdown(cx);
    cx.spawn(async move |_| {
        let failures = shutdown.finish().await;
        let exit_code = windows_product_exit_code(&failures);
        bongocat_platform::terminate_after_product_shutdown(exit_code);
    })
    .detach();
}

#[cfg(any(target_os = "macos", target_os = "windows"))]
struct ProductShutdown {
    coordinator: ProductCoordinator,
    overlay: ProductOverlaySession,
    settings_service: bongocat_app::ApplicationSettingsService,
    update_service: Option<bongocat_app::ApplicationUpdateService>,
}

#[cfg(any(target_os = "macos", target_os = "windows"))]
impl ProductShutdown {
    async fn finish(self) -> Arc<Mutex<Vec<String>>> {
        let failures = Arc::clone(&self.coordinator.failures);
        // The update worker is joined first: it is the only thing that touches the
        // installation, and it must not be mid-install while the runtime tears down.
        if let Some(update_service) = self.update_service
            && let Err(error) = update_service.join()
        {
            record_failure(&failures, error.to_string());
        }
        if !self.coordinator.frame_source_shutdown.wait_for_stop().await {
            record_failure(
                &failures,
                "product frame source did not stop before runtime shutdown",
            );
        }
        let settings_client = self.settings_service.client();
        if let Ok(bounds) = self.overlay.window_bounds() {
            for _ in 0..20 {
                if settings_client
                    .update_overlay_window_placement(
                        bounds.x,
                        bounds.y,
                        bounds.width,
                        bounds.height,
                    )
                    .is_ok()
                {
                    break;
                }
                async_io::Timer::after(Duration::from_millis(10)).await;
            }
        }
        if let Err(error) = settings_client.shutdown().await {
            record_failure(&failures, error.to_string());
        }
        if let Err(error) = self.settings_service.join() {
            record_failure(&failures, error.to_string());
        }
        match self.overlay.finish_after_runtime_shutdown() {
            Ok(report) if self.coordinator.expect_visible_frame && report.frames_presented == 0 => {
                record_failure(&failures, "product overlay presented no frames");
            }
            Ok(report) if !report.placement_fully_visible => {
                record_failure(&failures, "product overlay left the display bounds");
            }
            Ok(_) => {}
            Err(error) => record_failure(&failures, error.to_string()),
        }
        failures
    }
}

#[cfg(any(target_os = "macos", target_os = "windows"))]
fn begin_product_shutdown(cx: &mut App) -> ProductShutdown {
    let mut coordinator = cx.remove_global::<ProductCoordinator>();
    coordinator.frame_source_running = false;
    coordinator.frame_source_shutdown.request_stop();
    #[cfg(target_os = "windows")]
    if let Some(single_instance) = coordinator.single_instance.take()
        && let Err(error) = single_instance.shutdown()
    {
        record_failure(&coordinator.failures, error.to_string());
    }
    if let Some(system_menu) = coordinator.system_menu.take()
        && let Err(error) = system_menu.shutdown()
    {
        record_failure(&coordinator.failures, error.to_string());
    }
    #[cfg(target_os = "macos")]
    let mut overlay = coordinator
        .overlay
        .take()
        .expect("product overlay owner is present");
    #[cfg(target_os = "windows")]
    let mut overlay = {
        let mut overlay = coordinator.overlay.borrow_mut();
        overlay.take().expect("product overlay owner is present")
    };
    // Registered hotkeys must stop consuming keys before any service that
    // would re-trigger them is torn down.
    if let Some(shortcut_service) = coordinator.shortcut_service.take()
        && let Err(error) = shortcut_service.stop()
    {
        record_failure(&coordinator.failures, error.to_string());
    }
    if let Err(error) = overlay.stop_input() {
        record_failure(&coordinator.failures, error.to_string());
    }
    let settings_service = coordinator
        .settings_service
        .take()
        .expect("settings service owner is present");
    let update_service = coordinator.update_service.take();
    ProductShutdown {
        coordinator,
        overlay,
        settings_service,
        update_service,
    }
}

#[cfg(target_os = "macos")]
fn exit_after_automated_smoke(failures: &Arc<Mutex<Vec<String>>>) {
    let failures = failures
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    if failures.is_empty() {
        return;
    }
    let mut stderr = io::stderr().lock();
    let _ = writeln!(stderr, "product run failed: {}", failures.join("; "));
    let _ = stderr.flush();
    // AppKit terminates the process after gpui's `on_app_quit` future completes, without
    // returning from `NSApplication::run()`. This is therefore the only reachable exit-code
    // boundary on macOS automated runs (TODO P7-MACOS-SMOKE-EXIT-CODE). Normal product quits
    // never call this helper: they do not set `automated_verification`.
    std::process::exit(1);
}

#[cfg(target_os = "windows")]
fn windows_product_exit_code(failures: &Arc<Mutex<Vec<String>>>) -> i32 {
    let failures = failures
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    if failures.is_empty() {
        return 0;
    }
    let mut stderr = io::stderr().lock();
    let _ = writeln!(stderr, "product run failed: {}", failures.join("; "));
    let _ = stderr.flush();
    1
}

#[cfg(any(target_os = "macos", target_os = "windows"))]
fn ensure_settings_window(cx: &mut App) -> Result<SettingsWindowHandle, String> {
    let (existing, taskbar_icon_visible) = cx
        .try_global::<ProductCoordinator>()
        .map(|coordinator| {
            (
                coordinator.settings_window.clone(),
                #[cfg(target_os = "windows")]
                coordinator.taskbar_icon_visible,
                #[cfg(not(target_os = "windows"))]
                true,
            )
        })
        .unwrap_or((None, true));
    if let Some(window_handle) = existing {
        match window_handle.update(cx, |view, window, cx| {
            #[cfg(target_os = "windows")]
            bongocat_platform::set_taskbar_icon_visible(window, taskbar_icon_visible)
                .map_err(|error| error.to_string())?;
            view.reopen(window, cx)
        }) {
            Ok(Ok(())) => {
                cx.activate(true);
                return Ok(window_handle);
            }
            Ok(Err(error)) => return Err(error),
            Err(_) => {}
        }
    }

    let (settings_client, window_state) = cx
        .try_global::<ProductCoordinator>()
        .and_then(|coordinator| coordinator.settings_service.as_ref())
        .map(|service| (service.client(), service.window_state()))
        .ok_or_else(|| "settings service owner is unavailable".to_owned())?;
    let window_handle = open_settings_window(
        settings_client,
        window_state,
        taskbar_icon_visible,
        finish_product_quit,
        open_update_window_and_check,
        cx,
    )?;
    cx.global_mut::<ProductCoordinator>().settings_window = Some(window_handle.clone());
    Ok(window_handle)
}

/// The open update window, opening it first when there is none.
///
/// The window is a singleton like the settings window: the system menu, the About
/// page and an automatic check all route through here, so a second request focuses
/// the existing window instead of stacking another one.
#[cfg(any(target_os = "macos", target_os = "windows"))]
fn ensure_update_window(cx: &mut App) -> Result<bongocat_ui::UpdateWindowHandle, String> {
    let (existing, update_client, settings_client, language, appearance_theme) = {
        let coordinator = cx
            .try_global::<ProductCoordinator>()
            .ok_or_else(|| "product coordinator is unavailable".to_owned())?;
        let update_service = coordinator
            .update_service
            .as_ref()
            .ok_or_else(|| "update service is unavailable".to_owned())?;
        let settings_client = coordinator
            .settings_service
            .as_ref()
            .ok_or_else(|| "settings service is unavailable".to_owned())?
            .client();
        (
            coordinator.update_window.clone(),
            update_service.client(),
            settings_client,
            coordinator.update_language,
            coordinator.update_appearance_theme,
        )
    };
    if let Some(window_handle) = existing
        && window_handle.activate(cx).is_ok()
    {
        cx.activate(true);
        return Ok(window_handle);
    }
    let window_handle = bongocat_ui::open_update_window(
        update_client,
        settings_client,
        language,
        appearance_theme,
        cx,
    )?;
    cx.global_mut::<ProductCoordinator>().update_window = Some(window_handle.clone());
    Ok(window_handle)
}

/// Open the update window and start a check in it.
#[cfg(any(target_os = "macos", target_os = "windows"))]
fn open_update_window_and_check(cx: &mut App) {
    let result = ensure_update_window(cx).and_then(|window_handle| {
        window_handle
            .update(cx, |view, _, cx| view.check(cx))
            .map_err(|error| error.to_string())
    });
    if let Err(error) = result {
        record_update_window_failure(cx, error);
    }
}

/// Show the update window without starting a check.
///
/// The automatic check already ran, so opening the window here only surfaces its
/// result; asking for another check would repeat the request the user did not make.
#[cfg(any(target_os = "macos", target_os = "windows"))]
fn show_update_window(cx: &mut App) {
    if let Err(error) = ensure_update_window(cx) {
        record_update_window_failure(cx, error);
    }
}

#[cfg(any(target_os = "macos", target_os = "windows"))]
fn record_update_window_failure(cx: &App, error: String) {
    if let Some(failures) = cx
        .try_global::<ProductCoordinator>()
        .map(|coordinator| Arc::clone(&coordinator.failures))
    {
        record_failure(&failures, error);
    }
}

/// Whether the update window is currently on screen.
#[cfg(any(target_os = "macos", target_os = "windows"))]
fn update_window_is_open(cx: &mut App) -> bool {
    cx.try_global::<ProductCoordinator>()
        .and_then(|coordinator| coordinator.update_window.as_ref())
        .is_some_and(bongocat_ui::UpdateWindowHandle::is_open)
}

/// The phase the update worker is currently publishing.
#[cfg(any(target_os = "macos", target_os = "windows"))]
fn published_update_phase(cx: &mut App) -> Option<bongocat_ui::UpdatePhase> {
    cx.try_global::<ProductCoordinator>()
        .and_then(|coordinator| coordinator.update_service.as_ref())
        .map(|service| service.state().phase())
}

/// Ask the update worker for a check, unless this build cannot update at all.
#[cfg(any(target_os = "macos", target_os = "windows"))]
fn request_update_check(cx: &mut App) -> bool {
    let Some(client) = cx
        .try_global::<ProductCoordinator>()
        .and_then(|coordinator| coordinator.update_service.as_ref())
        .map(bongocat_app::ApplicationUpdateService::client)
    else {
        return false;
    };
    if matches!(
        client.snapshot().phase,
        bongocat_ui::UpdatePhase::Unavailable { .. }
    ) {
        return false;
    }
    client.request_check().is_ok()
}

/// Whether the update window asked for the process to be replaced.
#[cfg(any(target_os = "macos", target_os = "windows"))]
fn take_update_restart_request(cx: &mut App) -> bool {
    cx.try_global::<ProductCoordinator>()
        .and_then(|coordinator| coordinator.update_service.as_ref())
        .is_some_and(bongocat_app::ApplicationUpdateService::take_restart_request)
}

/// How long a completed install stays visible before the process is replaced.
///
/// The install already succeeded at this point and the running build is executing the
/// previous release's files, so the delay exists only to let the window show what
/// happened before it disappears.
#[cfg(target_os = "macos")]
const UPDATE_RESTART_DELAY: Duration = Duration::from_millis(1200);

/// Whether a completed install has been on screen long enough to restart into it.
///
/// Split out from the poll so the boundary is testable without a running product.
#[cfg(target_os = "macos")]
fn restart_delay_elapsed(observed_at: Instant, now: Instant) -> bool {
    now.saturating_duration_since(observed_at) >= UPDATE_RESTART_DELAY
}

/// Start the post-install restart once, from either the window's request or the
/// observed install.
///
/// The window asks for the restart, but it can be closed, and a closed window would
/// leave the process running the previous release's deleted files. Watching the
/// published phase here means the restart happens whether or not anyone is looking.
/// Returns whether the restart was started, which ends the calling loop.
#[cfg(target_os = "macos")]
fn poll_update_restart(cx: &mut App) -> bool {
    if !cx.has_global::<ProductCoordinator>() {
        return false;
    }
    let requested = take_update_restart_request(cx);
    let install_completed = matches!(
        published_update_phase(cx),
        Some(bongocat_ui::UpdatePhase::Installed {
            restart_required: true,
            ..
        })
    );
    if !requested && !install_completed {
        return false;
    }
    {
        let coordinator = cx.global_mut::<ProductCoordinator>();
        if coordinator.update_restart_started {
            return false;
        }
        if !requested {
            let now = Instant::now();
            let observed_at = *coordinator.update_installed_since.get_or_insert(now);
            if !restart_delay_elapsed(observed_at, now) {
                return false;
            }
        }
        coordinator.update_restart_started = true;
    }
    restart_after_update(cx);
    true
}

/// Replace this process with the build that was just installed.
///
/// The install already deleted the previous release from disk, so the running process
/// is executing files that no longer exist and everything that reads the installation
/// lazily would fail. The product is therefore shut down in the documented order
/// first, and only then is the process image replaced; the new build starts from a
/// complete, quiesced state.
#[cfg(target_os = "macos")]
fn restart_after_update(cx: &mut App) {
    if !cx.has_global::<ProductCoordinator>() {
        return;
    }
    let shutdown = begin_product_shutdown(cx);
    cx.spawn(async move |_| {
        let failures = shutdown.finish().await;
        {
            let failures = failures
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            for failure in failures.iter() {
                let mut stderr = io::stderr().lock();
                let _ = writeln!(stderr, "bongocat: {failure}");
            }
        }
        // `exec` replaces the process image and only returns on failure, so reaching
        // the next line means the new build could not be started.
        let _ = bongocat_update::UpdateRuntime::for_current_build(
            bongocat_app::BUILD_ENVIRONMENT,
            bongocat_app::PRODUCT_VERSION,
            bongocat_update::UpdateDiagnosticsTracker::default(),
        )
        .restart();
        let mut stderr = io::stderr().lock();
        let _ = writeln!(
            stderr,
            "bongocat: the updated build was installed but could not be started"
        );
        std::process::exit(1);
    })
    .detach();
}

#[cfg(target_os = "windows")]
fn apply_taskbar_icon_visibility(cx: &mut App, visible: bool) -> Result<(), SettingsError> {
    let window_handle = cx
        .try_global::<ProductCoordinator>()
        .and_then(|coordinator| coordinator.settings_window.clone())
        .ok_or_else(|| SettingsError::new(SettingsErrorCode::TaskbarIconUpdateFailed))?;
    window_handle
        .update(cx, |_, window, _| {
            bongocat_platform::set_taskbar_icon_visible(window, visible)
        })
        .map_err(|_| SettingsError::new(SettingsErrorCode::TaskbarIconUpdateFailed))?
        .map_err(|_| SettingsError::new(SettingsErrorCode::TaskbarIconUpdateFailed))?;
    cx.global_mut::<ProductCoordinator>().taskbar_icon_visible = visible;
    Ok(())
}

#[cfg(target_os = "windows")]
fn product_taskbar_icon_state(cx: &mut App) -> Result<(bool, bool), String> {
    let window_handle = cx
        .try_global::<ProductCoordinator>()
        .and_then(|coordinator| coordinator.settings_window.clone())
        .ok_or_else(|| "settings window is unavailable".to_owned())?;
    window_handle
        .update(cx, |view, window, _| {
            bongocat_platform::taskbar_icon_is_visible(window)
                .map(|visible| (visible, view.window_hidden()))
                .map_err(|error| error.to_string())
        })
        .map_err(|error| error.to_string())?
}

#[cfg(any(target_os = "macos", target_os = "windows"))]
fn product_overlay_state(cx: &mut App) -> Result<(u64, bool), String> {
    let coordinator = cx
        .try_global::<ProductCoordinator>()
        .ok_or_else(|| "product coordinator is unavailable".to_owned())?;
    #[cfg(target_os = "macos")]
    let overlay = coordinator
        .overlay
        .as_ref()
        .ok_or_else(|| "product overlay is unavailable".to_owned())?;
    #[cfg(target_os = "windows")]
    let overlay = coordinator.overlay.borrow();
    #[cfg(target_os = "windows")]
    let overlay = overlay
        .as_ref()
        .ok_or_else(|| "product overlay is unavailable".to_owned())?;
    Ok((overlay.model_generation(), overlay.is_visible()))
}

#[cfg(any(target_os = "macos", target_os = "windows"))]
fn toggle_settings_window(cx: &mut App) -> Result<(), String> {
    let existing = cx
        .try_global::<ProductCoordinator>()
        .and_then(|coordinator| coordinator.settings_window.clone());
    let Some(window_handle) = existing else {
        ensure_settings_window(cx)?;
        return Ok(());
    };

    // Closing settings only hides the pre-rendered window; the coordinator keeps the
    // handle on both platforms so the next open shows the same view.
    match window_handle.update(cx, |view, window, cx| {
        if view.window_hidden() {
            view.reopen(window, cx)
        } else {
            view.hide(window, cx)
        }
    }) {
        Ok(result) => result?,
        Err(_) => {
            ensure_settings_window(cx)?;
            return Ok(());
        }
    }

    Ok(())
}

#[cfg(any(target_os = "macos", target_os = "windows"))]
fn handle_shortcut_toggle_settings(cx: &mut App) {
    let requested = cx
        .try_global::<ProductCoordinator>()
        .is_some_and(|coordinator| coordinator.shortcut_signals.take_open_settings_request());
    if !requested {
        return;
    }
    if let Err(error) = toggle_settings_window(cx)
        && let Some(failures) = cx
            .try_global::<ProductCoordinator>()
            .map(|coordinator| Arc::clone(&coordinator.failures))
    {
        record_failure(&failures, error);
    }
}

#[cfg(target_os = "windows")]
fn build_single_instance_environment() -> SingleInstanceEnvironment {
    match bongocat_app::BUILD_ENVIRONMENT {
        bongocat_config::BuildEnvironment::Development => SingleInstanceEnvironment::Development,
        bongocat_config::BuildEnvironment::Production => SingleInstanceEnvironment::Production,
    }
}

#[cfg(any(target_os = "macos", target_os = "windows"))]
fn write_smoke_status(status: &str) -> io::Result<()> {
    let mut stdout = io::stdout().lock();
    writeln!(stdout, "bongocat-app: {status}")?;
    stdout.flush()
}

#[cfg(target_os = "windows")]
fn write_smoke_marker(path: &Path, status: &str) -> io::Result<()> {
    let mut file = atomic_write_file::AtomicWriteFile::open(path)?;
    writeln!(file, "{status}")?;
    file.commit()
}

/// Reports the startup permission state the product would act on, without showing any prompt.
///
/// This is the repeatable acceptance path for both platforms: it is run once while the capability
/// is missing and once while it is granted, and it never writes product state.
#[cfg(any(target_os = "macos", target_os = "windows"))]
fn run_startup_permission_smoke() -> Result<(), Box<dyn std::error::Error>> {
    let state = if bongocat_platform::startup_permission_available() {
        "available"
    } else {
        "missing"
    };
    write_smoke_status(&format!(
        "startup permission {} is {state}",
        bongocat_platform::STARTUP_PERMISSION_CAPABILITY
    ))?;
    Ok(())
}

#[cfg(target_os = "macos")]
fn run_startup_item_smoke() -> Result<(), Box<dyn std::error::Error>> {
    use bongocat_platform::{
        StartupItemEnvironment, StartupItemState, set_startup_item_enabled, startup_item_state,
    };

    if bongocat_app::BUILD_ENVIRONMENT != bongocat_config::BuildEnvironment::Production {
        return Err("startup-item mutation smoke requires a Production build".into());
    }
    let environment = StartupItemEnvironment::Production;
    let original = startup_item_state(environment)?;
    write_smoke_status(&format!("startup-item original state {original:?}"))?;

    let exercise: Result<(), String> = (|| match original {
        StartupItemState::Disabled | StartupItemState::NotFound => {
            let enabled =
                set_startup_item_enabled(environment, true).map_err(|error| error.to_string())?;
            if !matches!(
                enabled,
                StartupItemState::Enabled | StartupItemState::RequiresApproval
            ) {
                Err(format!(
                    "startup-item enable returned an unexpected state: {enabled:?}"
                ))
            } else {
                Ok(())
            }
        }
        StartupItemState::Enabled | StartupItemState::RequiresApproval => {
            let disabled =
                set_startup_item_enabled(environment, false).map_err(|error| error.to_string())?;
            if disabled != StartupItemState::Disabled {
                Err(format!(
                    "startup-item disable returned an unexpected state: {disabled:?}"
                ))
            } else {
                Ok(())
            }
        }
        StartupItemState::Unsupported(reason) => Err(format!(
            "startup-item capability is unsupported: {reason:?}"
        )),
        StartupItemState::Stale => Err(format!(
            "startup-item bundle produced an invalid initial state: {original:?}"
        )),
    })();

    let restoration = match original {
        StartupItemState::Disabled | StartupItemState::NotFound => {
            set_startup_item_enabled(environment, false)
        }
        StartupItemState::Enabled | StartupItemState::RequiresApproval => {
            set_startup_item_enabled(environment, true)
        }
        state => Ok(state),
    };
    exercise.map_err(io::Error::other)?;
    let restored = restoration?;
    let restored_matches = restored == original
        || (original == StartupItemState::NotFound && restored == StartupItemState::Disabled);
    if !restored_matches {
        return Err(format!(
            "startup-item state was not restored: expected {original:?}, got {restored:?}"
        )
        .into());
    }
    write_smoke_status(&format!("startup-item restored state {restored:?}"))?;
    Ok(())
}

#[cfg(all(
    feature = "storage-test-injection",
    any(target_os = "macos", target_os = "windows")
))]
struct SmokeRoot(PathBuf);

#[cfg(all(
    feature = "storage-test-injection",
    any(target_os = "macos", target_os = "windows")
))]
impl SmokeRoot {
    fn cleanup(mut self) -> io::Result<()> {
        let result = std::fs::remove_dir_all(&self.0);
        self.0 = PathBuf::new();
        result
    }
}

#[cfg(all(
    feature = "storage-test-injection",
    any(target_os = "macos", target_os = "windows")
))]
impl Drop for SmokeRoot {
    fn drop(&mut self) {
        if !self.0.as_os_str().is_empty() {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }
}

#[cfg(all(
    feature = "storage-test-injection",
    any(target_os = "macos", target_os = "windows")
))]
fn run_settings_window_state_smoke() -> Result<(), Box<dyn std::error::Error>> {
    use bongocat_config::{
        ApplicationState, BuildEnvironment, ConfigStore, Language, StateStore, StorageLayout,
        Theme, WindowPlacement,
    };
    use bongocat_ui::SettingsLanguage;

    const RESIZED_WIDTH: u32 = 700;
    const RESIZED_HEIGHT: u32 = 520;

    let root = env::temp_dir().join(format!(
        "bongocat-settings-window-state-smoke-{}",
        std::process::id()
    ));
    if root.exists() {
        std::fs::remove_dir_all(&root)?;
    }
    let root = SmokeRoot(root);
    let layout = StorageLayout::under(&root.0, BuildEnvironment::Development);
    let config_store = ConfigStore::new(layout.clone())?;
    let mut config = config_store.load_or_default()?.config;
    config.appearance.theme = Theme::Dark;
    config.appearance.language = Language::ChineseSimplified;
    config_store.commit(&config)?;
    drop(config_store);
    StateStore::new(layout.clone()).commit(&ApplicationState::with_settings_window(Some(
        WindowPlacement::new(999_000, 999_000, 800, 600, false)?,
    )))?;
    let application =
        bongocat_app::Application::start_with_layout_for_smoke(layout.clone(), preset_root())?;
    let service = bongocat_app::ApplicationSettingsService::start(application)?;
    let client = service.client();
    let window_state = service.window_state();
    let gpui_application = gpui_application().with_assets(AllAssets);
    gpui_application.run(move |cx| {
        let window =
            match open_settings_window(
                client.clone(),
                window_state.clone(),
                true,
                |cx| cx.quit(),
                |_: &mut App| {},
                cx,
            ) {
                Ok(window) => window,
                Err(error) => {
                    let _ = writeln!(
                        io::stderr().lock(),
                        "settings window state smoke failed: {error}"
                    );
                    let _ = std::fs::remove_dir_all(&root.0);
                    std::process::exit(1);
                }
            };
        cx.spawn(async move |cx| {
            let result = async {
                let mut general_verified = false;
                let mut last_general_error = None;
                for _ in 0..200 {
                    let general =
                        window.update(cx, |view, _, cx| {
                            view.show_general_page_for_smoke(cx)?;
                            if view.resolved_language_for_smoke()
                                != Some(SettingsLanguage::ChineseSimplified)
                            {
                                return Err(
                                    "settings snapshot did not resolve Simplified Chinese"
                                        .to_owned(),
                                );
                            }
                            Ok(())
                        });
                    match general {
                        Ok(Ok(())) => {
                            general_verified = true;
                            break;
                        }
                        Ok(Err(error)) => last_general_error = Some(error),
                        Err(error) => last_general_error = Some(error.to_string()),
                    }
                    Timer::after(Duration::from_millis(10)).await;
                }
                if !general_verified {
                    let detail = last_general_error
                        .unwrap_or_else(|| "settings view was unavailable".to_owned());
                    return Err(io::Error::other(format!(
                        "settings window did not apply the configured theme and localization: {detail}"
                    ))
                    .into());
                }
                write_smoke_status("Chinese General localization verified")?;
                let mut shortcuts_verified = false;
                let mut last_shortcuts_error = None;
                for _ in 0..200 {
                    let shortcuts = window.update(cx, |view, _, cx| {
                        view.show_shortcuts_page_for_smoke(cx)
                    });
                    match shortcuts {
                        Ok(Ok(())) => {
                            shortcuts_verified = true;
                            break;
                        }
                        Ok(Err(error)) => last_shortcuts_error = Some(error),
                        Err(error) => last_shortcuts_error = Some(error.to_string()),
                    }
                    Timer::after(Duration::from_millis(10)).await;
                }
                if !shortcuts_verified {
                    let detail = last_shortcuts_error
                        .unwrap_or_else(|| "settings view was unavailable".to_owned());
                    return Err(io::Error::other(format!(
                        "settings window did not apply Shortcuts localization: {detail}"
                    ))
                    .into());
                }
                write_smoke_status("Chinese Shortcuts localization verified")?;
                let mut models_verified = false;
                let mut last_models_error = None;
                for _ in 0..200 {
                    let models =
                        window.update(cx, |view, _, cx| {
                            view.show_models_localization_for_smoke(cx)
                        });
                    match models {
                        Ok(Ok(())) => {
                            models_verified = true;
                            break;
                        }
                        Ok(Err(error)) => last_models_error = Some(error),
                        Err(error) => last_models_error = Some(error.to_string()),
                    }
                    Timer::after(Duration::from_millis(10)).await;
                }
                if !models_verified {
                    let detail = last_models_error
                        .unwrap_or_else(|| "settings view was unavailable".to_owned());
                    return Err(io::Error::other(format!(
                        "settings window did not apply Models localization: {detail}"
                    ))
                    .into());
                }
                write_smoke_status("Chinese Models localization verified")?;
                let mut initial = None;
                let mut last_initial = window_state.placement();
                for _ in 0..200 {
                    let current = window_state.placement();
                    last_initial = current;
                    if current.is_some_and(|placement| {
                        placement.x != 999_000
                            && placement.y != 999_000
                            && (placement.width, placement.height) == (800, 600)
                    }) {
                        initial = current;
                        break;
                    }
                    Timer::after(Duration::from_millis(10)).await;
                }
                let initial = initial.ok_or_else(|| {
                    let size = last_initial
                        .map(|placement| format!("{}x{}", placement.width, placement.height))
                        .unwrap_or_else(|| "unavailable".to_owned());
                    io::Error::other(format!(
                        "default settings window content size was {size}, expected 800x600"
                    ))
                })?;
                window
                    .update(cx, |_, window, _| {
                        window.resize(size(
                            px(RESIZED_WIDTH as f32),
                            px(RESIZED_HEIGHT as f32),
                        ));
                    })
                    .map_err(|error| {
                        io::Error::other(format!("resize settings window: {error}"))
                    })?;
                let mut expected = None;
                let mut last_resized = window_state.placement();
                for _ in 0..200 {
                    let current = window_state.placement();
                    last_resized = current;
                    if current.is_some_and(|placement| {
                        (placement.width, placement.height) == (RESIZED_WIDTH, RESIZED_HEIGHT)
                    })
                    {
                        expected = current;
                        break;
                    }
                    Timer::after(Duration::from_millis(10)).await;
                }
                let expected = expected.ok_or_else(|| {
                    let observed = last_resized
                        .map(|placement| format!("{}x{}", placement.width, placement.height))
                        .unwrap_or_else(|| "unavailable".to_owned());
                    io::Error::other(format!(
                        "settings window bounds observer reported {observed}, expected {RESIZED_WIDTH}x{RESIZED_HEIGHT}"
                    ))
                })?;
                if (expected.x, expected.y, expected.maximized)
                    != (initial.x, initial.y, initial.maximized)
                {
                    return Err(io::Error::other(
                        "resizing settings window changed its position or maximized state",
                    )
                    .into());
                }
                client
                    .shutdown()
                    .await
                    .map_err(|error| io::Error::other(error.to_string()))?;
                service
                    .join()
                    .map_err(|error| io::Error::other(error.to_string()))?;
                let persisted = StateStore::new(layout.clone()).load_or_default().state;
                let expected = WindowPlacement::new(
                    expected.x,
                    expected.y,
                    expected.width,
                    expected.height,
                    expected.maximized,
                )?;
                if persisted.settings_window != Some(expected) {
                    return Err(io::Error::other(
                        "settings window state did not match observed GPUI bounds",
                    )
                    .into());
                }
                let restarted = bongocat_app::Application::start_with_layout_for_smoke(
                    layout,
                    preset_root(),
                )?;
                if restarted.settings_window_placement() != Some(expected) {
                    return Err(io::Error::other(
                        "application restart did not restore settings window state",
                    )
                    .into());
                }
                restarted.shutdown()?;
                root.cleanup()?;
                write_smoke_status("settings window state restored after restart")?;
                Ok::<(), Box<dyn std::error::Error>>(())
            }
            .await;
            if let Err(error) = result {
                let mut stderr = io::stderr().lock();
                let _ = writeln!(stderr, "settings window state smoke failed: {error}");
                let _ = stderr.flush();
                std::process::exit(1);
            }
            cx.update(|cx| cx.quit());
        })
        .detach();
    });
    Ok(())
}

#[cfg(all(
    feature = "storage-test-injection",
    any(target_os = "macos", target_os = "windows")
))]
const PANIC_DIAGNOSTICS_SMOKE_ROOT_ENV: &str = "BONGOCAT_PANIC_DIAGNOSTICS_SMOKE_ROOT";

#[cfg(all(
    feature = "storage-test-injection",
    any(target_os = "macos", target_os = "windows")
))]
const PANIC_DIAGNOSTICS_SMOKE_PAYLOAD: &str = "panic-smoke-sensitive-payload";

#[cfg(all(
    feature = "storage-test-injection",
    any(target_os = "macos", target_os = "windows")
))]
fn run_panic_diagnostics_smoke_child() -> Result<(), Box<dyn std::error::Error>> {
    use bongocat_config::{BuildEnvironment, StorageLayout};

    let root = env::var_os(PANIC_DIAGNOSTICS_SMOKE_ROOT_ENV)
        .ok_or("panic diagnostics child is missing its isolated storage root")?;
    let root = PathBuf::from(root);
    if !root.is_absolute() {
        return Err("panic diagnostics child storage root must be absolute".into());
    }
    let layout = StorageLayout::under(&root, BuildEnvironment::Development);
    let mut application =
        bongocat_app::Application::start_with_layout_for_smoke(layout, preset_root())?;
    application.install_process_panic_hook();
    panic!("{PANIC_DIAGNOSTICS_SMOKE_PAYLOAD}: {}", root.display());
}

#[cfg(all(
    feature = "storage-test-injection",
    any(target_os = "macos", target_os = "windows")
))]
fn read_application_logs(directory: &Path) -> io::Result<String> {
    let mut paths = Vec::new();
    for entry in std::fs::read_dir(directory)? {
        let entry = entry?;
        if !entry.file_type()?.is_file() {
            continue;
        }
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if name.starts_with("application-") && name.contains(".jsonl") {
            paths.push(entry.path());
        }
    }
    paths.sort();
    let mut logs = String::new();
    for path in paths {
        logs.push_str(&std::fs::read_to_string(path)?);
    }
    Ok(logs)
}

#[cfg(all(
    feature = "storage-test-injection",
    any(target_os = "macos", target_os = "windows")
))]
fn run_diagnostics_export_smoke() -> Result<(), Box<dyn std::error::Error>> {
    use bongocat_config::{BuildEnvironment, StorageLayout};

    let root = env::temp_dir().join(format!(
        "bongocat-diagnostics-export-smoke-{}",
        std::process::id()
    ));
    if root.exists() {
        std::fs::remove_dir_all(&root)?;
    }
    let root = SmokeRoot(root);
    let layout = StorageLayout::under(&root.0, BuildEnvironment::Development);
    let application =
        bongocat_app::Application::start_with_layout_for_smoke(layout.clone(), preset_root())?;
    let service = bongocat_app::ApplicationSettingsService::start(application)?;
    let client = service.client();
    let exported = client.export_diagnostics_blocking()?;
    let status = exported
        .diagnostics_export
        .ok_or("diagnostics export did not return a typed result")?;
    if status.format_version != 1
        || status.preview_bundle_format_version != 1
        || status.preview_bundle_entry_count != 3
        || status.bytes_written == 0
        || status.preview_bundle_bytes_written == 0
    {
        return Err("diagnostics export returned an invalid typed result".into());
    }

    let diagnostics = layout.logs.join("diagnostics.json");
    let preview = layout.logs.join("diagnostics-preview.zip");
    if !diagnostics.is_file() || !preview.is_file() {
        return Err("diagnostics export did not create both private files".into());
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if std::fs::metadata(&diagnostics)?.permissions().mode() & 0o777 != 0o600
            || std::fs::metadata(&preview)?.permissions().mode() & 0o777 != 0o600
        {
            return Err("diagnostics export did not preserve private file permissions".into());
        }
    }
    let mut archive = ZipArchive::new(std::fs::File::open(&preview)?)?;
    let mut entries = (0..archive.len())
        .map(|index| archive.by_index(index).map(|entry| entry.name().to_owned()))
        .collect::<Result<Vec<_>, _>>()?;
    entries.sort_unstable();
    if entries
        != [
            "application-events.jsonl",
            "diagnostics.json",
            "manifest.json",
        ]
    {
        return Err("diagnostics preview archive entries diverged from the v1 contract".into());
    }

    client.shutdown_blocking()?;
    service.join()?;
    root.cleanup()?;
    write_smoke_status("diagnostics export completed with a private preview bundle")?;
    Ok(())
}

#[cfg(all(
    feature = "storage-test-injection",
    any(target_os = "macos", target_os = "windows")
))]
fn run_diagnostics_export_failure_smoke() -> Result<(), Box<dyn std::error::Error>> {
    use bongocat_config::{BuildEnvironment, StorageLayout};

    let root = env::temp_dir().join(format!(
        "bongocat-diagnostics-export-failure-smoke-{}",
        std::process::id()
    ));
    if root.exists() {
        std::fs::remove_dir_all(&root)?;
    }
    let root = SmokeRoot(root);
    let layout = StorageLayout::under(&root.0, BuildEnvironment::Development);
    let application =
        bongocat_app::Application::start_with_layout_for_smoke(layout.clone(), preset_root())?;
    let service = bongocat_app::ApplicationSettingsService::start(application)?;
    let client = service.client();
    let first = client.export_diagnostics_blocking()?;
    let first_status = first
        .diagnostics_export
        .ok_or("initial diagnostics export did not return a typed result")?;
    let diagnostics = layout.logs.join("diagnostics.json");
    let preview = layout.logs.join("diagnostics-preview.zip");
    let previous_diagnostics = std::fs::read(&diagnostics)?;
    let previous_preview = std::fs::read(&preview)?;

    // A directory at the destination is an OS-level replace/open failure. The writer must
    // reject it before touching the existing diagnostics or preview bytes.
    std::fs::remove_file(&diagnostics)?;
    std::fs::create_dir(&diagnostics)?;
    let error = client
        .export_diagnostics_blocking()
        .expect_err("diagnostics export must reject a directory destination");
    if error.code() != bongocat_ui::SettingsErrorCode::DiagnosticsExportFailed {
        return Err("diagnostics export returned an unstable filesystem failure code".into());
    }
    if std::fs::read(&preview)? != previous_preview {
        return Err("failed diagnostics export changed the previous preview bundle".into());
    }
    std::fs::remove_dir(&diagnostics)?;
    std::fs::write(&diagnostics, &previous_diagnostics)?;

    #[cfg(target_os = "macos")]
    {
        // Marking the existing preview bundle immutable makes the atomic commit fail after the
        // staging file has been fully written. Unlike a directory or a read-only parent, this is
        // an OS-level failure the current process cannot bypass through `set_private_directory`,
        // so it deterministically exercises the commit-failure recovery path.
        let immutable = std::process::Command::new("/usr/bin/chflags")
            .arg("uchg")
            .arg(&preview)
            .status()?;
        if !immutable.success() {
            return Err("failed to mark the previous preview bundle immutable".into());
        }
        let result = client.export_diagnostics_blocking();
        let cleared = std::process::Command::new("/usr/bin/chflags")
            .arg("nouchg")
            .arg(&preview)
            .status()?;
        if !cleared.success() {
            return Err("failed to clear the immutable preview bundle flag".into());
        }
        let error =
            result.expect_err("diagnostics export must reject an immutable preview destination");
        if error.code() != bongocat_ui::SettingsErrorCode::DiagnosticsExportFailed {
            return Err("immutable diagnostics preview returned an unstable error code".into());
        }
        if std::fs::read(&preview)? != previous_preview {
            return Err("preview commit failure changed the previous preview bundle".into());
        }
    }

    let staging = std::fs::read_dir(&layout.logs)?
        .filter_map(Result::ok)
        .map(|entry| entry.file_name())
        .filter_map(|name| name.into_string().ok())
        .filter(|name| name.starts_with(".diagnostics-preview.zip."))
        .collect::<Vec<_>>();
    if !staging.is_empty() {
        return Err(format!("failed export left staging files: {staging:?}").into());
    }
    if first_status.preview_bundle_entry_count != 3 {
        return Err("initial diagnostics export returned an invalid preview status".into());
    }

    client.shutdown_blocking()?;
    service.join()?;
    root.cleanup()?;
    write_smoke_status("diagnostics export filesystem failures preserved the previous bundle")?;
    Ok(())
}

#[cfg(all(
    feature = "storage-test-injection",
    any(target_os = "macos", target_os = "windows")
))]
fn run_panic_diagnostics_smoke() -> Result<(), Box<dyn std::error::Error>> {
    use bongocat_config::{BuildEnvironment, StorageLayout};

    const PANICKED_RECORD: &str =
        "{\"component\":\"application\",\"level\":\"error\",\"code\":\"panicked\"}";
    const CLEAN_SHUTDOWN_RECORD: &str =
        "{\"component\":\"application\",\"level\":\"info\",\"code\":\"shutdown_completed\"}";

    let root = env::temp_dir().join(format!(
        "bongocat-panic-diagnostics-smoke-{}",
        std::process::id()
    ));
    if root.exists() {
        std::fs::remove_dir_all(&root)?;
    }
    let root = SmokeRoot(root);
    let layout = StorageLayout::under(&root.0, BuildEnvironment::Development);
    let mut child = std::process::Command::new(env::current_exe()?)
        .arg("--panic-diagnostics-smoke-child")
        .env(PANIC_DIAGNOSTICS_SMOKE_ROOT_ENV, &root.0)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()?;
    let deadline = std::time::Instant::now() + Duration::from_secs(10);
    let timed_out = loop {
        if child.try_wait()?.is_some() {
            break false;
        }
        if std::time::Instant::now() >= deadline {
            child.kill()?;
            break true;
        }
        std::thread::sleep(Duration::from_millis(10));
    };
    let child = child.wait_with_output()?;
    if timed_out {
        return Err("panic diagnostics child exceeded 10 seconds".into());
    }
    if child.status.success() {
        return Err("panic diagnostics child exited successfully".into());
    }
    let child_output = format!(
        "{}{}",
        String::from_utf8_lossy(&child.stdout),
        String::from_utf8_lossy(&child.stderr)
    );
    if child_output.contains(PANIC_DIAGNOSTICS_SMOKE_PAYLOAD)
        || child_output.contains(root.0.to_string_lossy().as_ref())
    {
        return Err("panic diagnostics child exposed its payload or storage path".into());
    }

    let run_marker = layout.logs.join("application-running.marker");
    if !run_marker.is_file() {
        return Err("panic diagnostics child did not preserve the unclean run marker".into());
    }
    let crashed_logs = read_application_logs(&layout.logs)?;
    if !crashed_logs.lines().any(|line| line == PANICKED_RECORD) {
        return Err("panic diagnostics child did not persist the stable panic record".into());
    }
    if crashed_logs.contains(PANIC_DIAGNOSTICS_SMOKE_PAYLOAD)
        || crashed_logs.contains(root.0.to_string_lossy().as_ref())
    {
        return Err("persistent panic diagnostics exposed their payload or storage path".into());
    }
    let config_after_crash = std::fs::read(&layout.config)?;

    let restarted =
        bongocat_app::Application::start_with_layout_for_smoke(layout.clone(), preset_root())?;
    let diagnostics = restarted.application_log_diagnostics();
    if diagnostics.events.previous_run_unclean != 1 || diagnostics.events.started != 1 {
        return Err("application restart did not classify the aborted run as unclean".into());
    }
    restarted.shutdown()?;
    if run_marker.exists() {
        return Err("clean restart shutdown did not remove the run marker".into());
    }
    if std::fs::read(&layout.config)? != config_after_crash {
        return Err("panic diagnostics or restart changed the current configuration".into());
    }
    let completed_logs = read_application_logs(&layout.logs)?;
    if !completed_logs
        .lines()
        .any(|line| line == CLEAN_SHUTDOWN_RECORD)
    {
        return Err("clean restart did not persist its completed shutdown record".into());
    }

    write_smoke_status("panic diagnostics recovered after crash")?;
    root.cleanup()?;
    Ok(())
}

#[cfg(any(target_os = "macos", target_os = "windows"))]
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let run_options = match RunOptions::parse(env::args().skip(1)) {
        Ok(options) => options,
        Err(error) if error.help => {
            writeln!(io::stdout().lock(), "{error}")?;
            return Ok(());
        }
        Err(error) => return Err(Box::new(error)),
    };
    // The native surfaces that can only follow the *system* theme — the ComCtl32 alerts,
    // the Win32 menus and the shell file dialog — render dark only if the process asks
    // for it before the first window exists, so this is the earliest point that can ask.
    // A failure is not fatal: those surfaces then keep the system appearance, which is
    // the documented fallback, and refusing to start over a cosmetic switch would be
    // worse (ADR-0048).
    let _ = bongocat_platform::init_native_theme();
    #[cfg(feature = "storage-test-injection")]
    if run_options.settings_window_state_smoke {
        return run_settings_window_state_smoke();
    }
    #[cfg(feature = "storage-test-injection")]
    if run_options.panic_diagnostics_smoke_child {
        return run_panic_diagnostics_smoke_child();
    }
    #[cfg(feature = "storage-test-injection")]
    if run_options.panic_diagnostics_smoke {
        return run_panic_diagnostics_smoke();
    }
    #[cfg(feature = "storage-test-injection")]
    if run_options.diagnostics_export_smoke {
        return run_diagnostics_export_smoke();
    }
    #[cfg(feature = "storage-test-injection")]
    if run_options.diagnostics_export_failure_smoke {
        return run_diagnostics_export_failure_smoke();
    }
    #[cfg(target_os = "macos")]
    if run_options.startup_item_smoke {
        return run_startup_item_smoke();
    }
    if run_options.startup_permission_smoke {
        return run_startup_permission_smoke();
    }
    #[cfg(target_os = "windows")]
    let single_instance = match SingleInstance::acquire(build_single_instance_environment())? {
        SingleInstanceStart::Primary(single_instance) => single_instance,
        SingleInstanceStart::SecondaryNotified => {
            write_smoke_status("secondary instance notified primary")?;
            #[cfg(target_os = "windows")]
            if let Some(path) = run_options.single_instance_result_file.as_deref() {
                write_smoke_marker(path, "secondary instance notified primary")?;
            }
            return Ok(());
        }
    };
    let mut application = bongocat_app::Application::start(preset_root())?;
    application.install_process_panic_hook();
    let core_log = CoreLogHandle::install(application.logs_directory().join("cubism-core.jsonl"))?;
    let core_log_reporter = core_log.reporter();
    application.set_core_log_diagnostics_provider(move || {
        let stats = core_log_reporter.stats();
        bongocat_app::CoreLogDiagnostics {
            written: stats.written,
            dropped: stats.dropped,
            rotated: stats.rotated,
            pruned: stats.pruned,
            bytes: stats.bytes,
            retained_files: stats.retained_files,
            retained_bytes: stats.retained_bytes,
        }
    });
    // The update worker publishes anonymous check/download/install counters and the
    // last stable error code through the same export boundary as every other
    // subsystem. The tracker is created here because handing the application to the
    // settings service is what moves it out of reach.
    let update_diagnostics = bongocat_update::UpdateDiagnosticsTracker::default();
    application.set_update_diagnostics_tracker(update_diagnostics.clone());
    // Resolve the persisted preference before moving `application` into the settings
    // service. The process-wide native appearance must be installed before the overlay
    // window exists; otherwise its native context menu only becomes themed after the
    // settings window happens to apply the same preference (ADR-0048).
    let initial_native_theme = native_theme_for_startup(application.config().appearance.theme);

    // The startup permission check is non-blocking (ADR-0032, amended 2026-09-15): the
    // language is resolved here, but the check itself runs on its own worker after the
    // product windows exist, inside the GPUI run loop. The status is not persisted or
    // logged, exactly as before.
    let permission_language = application.effective_language();
    let permission_check_enabled = !run_options.automated_verification;

    let overlay_options = OverlaySessionOptions {
        click_through: application.config().overlay.click_through,
        always_on_top: application.config().overlay.always_on_top,
        scale_percent: application.config().overlay.scale_percent,
        opacity_percent: application.config().overlay.opacity_percent,
        corner_radius_percent: application.config().overlay.corner_radius_percent,
        hide_on_pointer_hover: application.config().overlay.hide_on_pointer_hover,
        hide_on_pointer_hover_delay_ms: hover_hide_delay_ms(
            application
                .config()
                .overlay
                .hide_on_pointer_hover_delay_seconds,
        ),
        keep_inside_screen: application.config().overlay.keep_inside_screen,
        maximum_fps: application.config().model.maximum_fps,
        window_bounds: application.overlay_window_placement().map(|placement| {
            OverlayWindowBounds::new(placement.x, placement.y, placement.width, placement.height)
        }),
    };
    // Startup model restore: activate the configured selection, or fall back
    // to the always-available standard preset when it is missing or unusable;
    // see `Application::restore_startup_model`.
    application.restore_startup_model()?;
    let runtime_client = application.runtime_client();
    let (shortcut_sender, shortcut_receiver) = std::sync::mpsc::sync_channel(64);
    let (context_menu_sender, context_menu_receiver) =
        std::sync::mpsc::sync_channel::<OverlayContextMenuRequest>(1);
    let (status_icon_sender, status_icon_receiver) = std::sync::mpsc::sync_channel(4);
    let status_icon = Arc::new(ProductStatusIcon {
        sender: status_icon_sender,
    });
    let initial_status_icon_visible = application.config().application.show_status_icon;
    #[cfg(target_os = "windows")]
    let (taskbar_icon_sender, taskbar_icon_receiver) = std::sync::mpsc::sync_channel(4);
    #[cfg(target_os = "windows")]
    let taskbar_icon = Arc::new(ProductTaskbarIcon {
        sender: taskbar_icon_sender,
    });
    #[cfg(target_os = "windows")]
    let initial_taskbar_icon_visible = application.config().application.show_taskbar_icon;
    let shortcut_signals = bongocat_app::ApplicationShortcutSignals::default();
    let input_producer = application.input_producer();
    let cursor_producer = application.cursor_producer();
    let gamepad_axis_producer = application.gamepad_axis_producer();
    let render_consumer = application.take_render_consumer()?;
    let expect_visible_frame = application.config().overlay.visible;
    let frame_runtime_client = runtime_client.clone();
    let frame_source_shutdown = FrameSourceShutdown::default();
    let failures = Arc::new(Mutex::new(Vec::new()));
    let run_failures = Arc::clone(&failures);
    // Global shortcuts are OS registrations owned by a dedicated platform
    // service thread (ADR-0044); the input pipeline no longer matches edges.
    let shortcut_service = match GlobalShortcutService::start(
        application.shortcut_table(),
        ShortcutDispatcher::with_application_sink(runtime_client.clone(), shortcut_sender),
    ) {
        Ok(service) => Some(service),
        Err(error) => {
            return Err(Box::new(ProductRunError {
                failures: vec![error.to_string()],
            }));
        }
    };
    #[cfg(target_os = "windows")]
    let shutdown_requested = Arc::new(AtomicBool::new(false));
    let gpui_application = gpui_application().with_assets(AllAssets);
    let reopen_failures = Arc::clone(&run_failures);
    #[cfg(target_os = "macos")]
    let application_reopen_smoke = run_options.application_reopen_smoke;
    gpui_application.on_reopen(move |cx| {
        #[cfg(target_os = "macos")]
        if application_reopen_smoke
            && let Err(error) = write_smoke_status("application-reopen callback received")
        {
            record_failure(&reopen_failures, error.to_string());
        }
        if cx.has_global::<ProductCoordinator>() {
            match ensure_settings_window(cx) {
                Ok(_) => {
                    #[cfg(target_os = "macos")]
                    {
                        let coordinator = cx.global_mut::<ProductCoordinator>();
                        coordinator.application_reopens =
                            coordinator.application_reopens.saturating_add(1);
                    }
                }
                Err(error) => record_failure(&reopen_failures, error),
            }
        }
    });
    gpui_application.run(move |cx: &mut App| {
        // This is the first main-thread point at which AppKit's process appearance and
        // the overlay's native window can be ordered. Do this before creating the overlay;
        // its right-click menu must not depend on a settings window having existed first.
        if let Err(error) = bongocat_platform::apply_process_theme(initial_native_theme) {
            record_failure(&run_failures, format!("apply startup native theme: {error}"));
        }
        let overlay = match ProductOverlaySession::start_with_interaction_sinks(
            runtime_client,
            input_producer,
            cursor_producer,
            gamepad_axis_producer,
            render_consumer,
            overlay_options,
            OverlayInteractionSinks {
                context_menu_sender: Some(context_menu_sender),
            },
        ) {
            Ok(overlay) => overlay,
            Err(error) => {
                record_failure(&run_failures, error.to_string());
                if let Err(error) = application.shutdown() {
                    record_failure(&run_failures, error.to_string());
                }
                cx.quit();
                return;
            }
        };
        let settings_service =
            match bongocat_app::ApplicationSettingsService::start_with_product_capabilities(
                application,
                shortcut_receiver,
                shortcut_signals.clone(),
                status_icon,
                #[cfg(target_os = "windows")]
                taskbar_icon,
            ) {
                Ok(service) => service,
                Err(error) => {
                    record_failure(&run_failures, error.to_string());
                    let mut overlay = overlay;
                    if let Err(error) = overlay.stop_input() {
                        record_failure(&run_failures, error.to_string());
                    }
                    cx.quit();
                    return;
                }
            };
        let initial_menu_presentation = match settings_service.client().read_snapshot_blocking() {
            Ok(snapshot) => system_menu_presentation(&snapshot),
            Err(error) => {
                record_failure(&run_failures, error.to_string());
                let mut overlay = overlay;
                let _ = overlay.stop_input();
                let client = settings_service.client();
                let _ = client.shutdown_blocking();
                let _ = settings_service.join();
                let _ = overlay.finish_after_runtime_shutdown();
                cx.quit();
                return;
            }
        };
        let system_menu = match SystemMenu::start_with_presentation(
            initial_status_icon_visible,
            initial_menu_presentation,
        ) {
            Ok(system_menu) => system_menu,
            Err(error) => {
                record_failure(&run_failures, error.to_string());
                let mut overlay = overlay;
                if let Err(error) = overlay.stop_input() {
                    record_failure(&run_failures, error.to_string());
                }
                let client = settings_service.client();
                let _ = client.shutdown_blocking();
                if let Err(error) = settings_service.join() {
                    record_failure(&run_failures, error.to_string());
                }
                if let Err(error) = overlay.finish_after_runtime_shutdown() {
                    record_failure(&run_failures, error.to_string());
                }
                cx.quit();
                return;
            }
        };
        let settings_client = settings_service.client();

        // The update worker is independent of the settings worker: a check, a transfer
        // or an install must never queue behind a settings command, and the settings
        // service must not be blocked while an update is in flight.
        let update_service = match bongocat_app::ApplicationUpdateService::start(
            bongocat_app::BUILD_ENVIRONMENT,
            bongocat_app::PRODUCT_VERSION,
            update_diagnostics,
        ) {
            Ok(service) => service,
            Err(error) => {
                record_failure(&run_failures, error.to_string());
                let mut overlay = overlay;
                if let Err(error) = overlay.stop_input() {
                    record_failure(&run_failures, error.to_string());
                }
                let _ = settings_client.shutdown_blocking();
                if let Err(error) = settings_service.join() {
                    record_failure(&run_failures, error.to_string());
                }
                if let Err(error) = overlay.finish_after_runtime_shutdown() {
                    record_failure(&run_failures, error.to_string());
                }
                cx.quit();
                return;
            }
        };

        #[cfg(target_os = "windows")]
        let overlay = Rc::new(RefCell::new(Some(overlay)));
        #[cfg(target_os = "windows")]
        let frame_overlay = Rc::clone(&overlay);
        cx.set_global(ProductCoordinator {
            _core_log: core_log,
            #[cfg(target_os = "macos")]
            overlay: Some(overlay),
            #[cfg(target_os = "windows")]
            overlay,
            settings_service: Some(settings_service),
            settings_window: None,
            update_service: Some(update_service),
            update_window: None,
            update_language: bongocat_ui::SettingsLanguage::EnglishUnitedStates,
            update_appearance_theme: bongocat_ui::SettingsTheme::System,
            #[cfg(target_os = "macos")]
            update_installed_since: None,
            #[cfg(target_os = "macos")]
            update_restart_started: false,
            system_menu: Some(system_menu),
            #[cfg(target_os = "windows")]
            taskbar_icon_visible: initial_taskbar_icon_visible,
            #[cfg(target_os = "macos")]
            application_reopens: 0,
            #[cfg(target_os = "windows")]
            single_instance: Some(single_instance),
            #[cfg(target_os = "windows")]
            single_instance_wakes: 0,
            frame_source_running: true,
            frame_source_shutdown: frame_source_shutdown.clone(),
            shortcut_signals,
            shortcut_service,
            frame_ticks: 0,
            expect_visible_frame,
            failures: Arc::clone(&run_failures),
            #[cfg(target_os = "windows")]
            shutdown_requested: Arc::clone(&shutdown_requested),
            #[cfg(target_os = "windows")]
            shutdown_flush_complete: Arc::new(AtomicBool::new(false)),
        });

        // Startup permission check on its own worker (ADR-0032, amended 2026-09-15). The
        // overlay, settings service, system menu and update worker above are already
        // running, so a pending native prompt can no longer delay any product window.
        // The check is a read-only platform query; only a missing capability shows the
        // prompt, and the prompt outcome is neither persisted nor logged.
        //
        // Windows: the primary button promises "quit and go to settings". A successful
        // permission flow (the executable's folder was revealed) raises the same shutdown
        // flag the tray quit uses, so the product exits through the regular shutdown
        // coordinator while the user flips the compatibility flag. A failed reveal keeps
        // the product running, per the ADR rule that a failed flow only counts as
        // "later". macOS has no quit promise: its primary button only opens System
        // Settings.
        //
        // Lifecycle: the thread is deliberately detached. It owns only the resolved
        // language and the prompt strings, shares no locks with the product, and always
        // terminates - either the user answers the OS-owned dialog (a satisfied
        // capability returns immediately without any dialog), or the process exits and
        // the OS tears the dialog down with it. There is no cancellation channel for a
        // native dialog, so joining on quit would block shutdown on an unanswered
        // prompt, which is exactly the blocking behaviour this design removes.
        if permission_check_enabled {
            #[cfg(target_os = "windows")]
            let permission_flow_quit_requested = Arc::clone(&shutdown_requested);
            let spawn_result = std::thread::Builder::new()
                .name("bongocat-startup-permission".to_owned())
                .spawn(move || {
                    let status = bongocat_app::ensure_startup_permission(permission_language);
                    if let bongocat_platform::StartupPermissionStatus::PermissionFlowRequested(
                        true,
                    ) = status
                    {
                        #[cfg(target_os = "windows")]
                        permission_flow_quit_requested.store(true, Ordering::Release);
                    }
                });
            if let Err(error) = spawn_result {
                record_failure(&run_failures, error.to_string());
            }
        }

        cx.on_window_closed(|cx, _| {
            if !cx.has_global::<ProductCoordinator>() {
                return;
            }
            let settings_window = cx
                .global::<ProductCoordinator>()
                .settings_window
                .clone();
            if let Some(window_handle) = settings_window
                && window_handle.read(cx).is_err()
            {
                cx.global_mut::<ProductCoordinator>().settings_window = None;
            }
            // The update window can be closed at any time, including while a check or
            // a transfer is still running: the worker owns that work, not the window.
            let update_window = cx.global::<ProductCoordinator>().update_window.clone();
            if let Some(window_handle) = update_window
                && !window_handle.is_open()
            {
                cx.global_mut::<ProductCoordinator>().update_window = None;
            }
        })
        .detach();

        // The automatic check is driven from the GPUI side because the opt-in lives in
        // the user's configuration, which only the settings service can read. The
        // worker stays a plain command receiver; nothing about the schedule reaches it.
        let automatic_check_client = settings_client.clone();
        cx.spawn(async move |cx| {
            Timer::after(AUTOMATIC_UPDATE_CHECK_STARTUP_DELAY).await;
            loop {
                if !cx.update(|cx| cx.has_global::<ProductCoordinator>()) {
                    break;
                }
                let enabled = automatic_check_client
                    .read_snapshot()
                    .await
                    .is_ok_and(|snapshot| snapshot.check_for_updates_automatically);
                if enabled && cx.update(request_update_check) {
                    // Wait for the check to settle before deciding what to show. The
                    // bound keeps a worker that never reports back from parking this
                    // loop for the rest of the interval.
                    for _ in 0..AUTOMATIC_UPDATE_CHECK_SETTLE_ATTEMPTS {
                        Timer::after(AUTOMATIC_UPDATE_CHECK_SETTLE_INTERVAL).await;
                        match cx.update(published_update_phase) {
                            Some(bongocat_ui::UpdatePhase::Checking) => continue,
                            Some(bongocat_ui::UpdatePhase::Available { .. }) => {
                                // Surface the result rather than leaving it for the
                                // user to discover. The window is a singleton, so a
                                // second automatic check cannot stack one.
                                if !cx.update(update_window_is_open) {
                                    cx.update(show_update_window);
                                }
                                break;
                            }
                            _ => break,
                        }
                    }
                }
                Timer::after(AUTOMATIC_UPDATE_CHECK_INTERVAL).await;
            }
        })
        .detach();

        #[cfg(target_os = "macos")]
        let fail_on_smoke_failure = run_options.automated_verification;
        cx.on_app_quit(move |cx| {
            #[cfg(target_os = "macos")]
            if run_options.application_reopen_smoke
                && let Err(error) = write_smoke_status("application-reopen quit received")
                && let Some(coordinator) = cx.try_global::<ProductCoordinator>()
            {
                record_failure(&coordinator.failures, error.to_string());
            }
            let shutdown = cx
                .has_global::<ProductCoordinator>()
                .then(|| begin_product_shutdown(cx));
            #[cfg(target_os = "macos")]
            if fail_on_smoke_failure
                && let Some(shutdown) = shutdown.as_ref()
            {
                // AppKit can terminate before the async shutdown future reaches its final
                // instruction. Smoke failures are recorded before quit is requested, so inspect
                // the accumulator at this reachable boundary instead of relying on code after
                // `finish().await` (TODO P7-MACOS-SMOKE-EXIT-CODE).
                exit_after_automated_smoke(&shutdown.coordinator.failures);
            }
            async move {
                if let Some(shutdown) = shutdown {
                    #[cfg(target_os = "macos")]
                    let _ = fail_on_smoke_failure;
                    let failures = shutdown.finish().await;
                    let _ = failures;
                }
            }
        })
        .detach();

        let system_menu_snapshot_failures = Arc::clone(&run_failures);
        let system_menu_client = settings_client.clone();
        cx.spawn(async move |cx| {
            let mut last_menu_revision = None;
            loop {
                Timer::after(Duration::from_millis(50)).await;
                if !cx.update(|cx| cx.has_global::<ProductCoordinator>()) {
                    break;
                }
                // Ask for the revision first. This loop runs at 20 Hz for the whole
                // product lifetime, and a full snapshot also scans the model catalog —
                // work the menu cannot act on. The revision is a cheap comparison
                // against state the service already holds, and a menu update only needs
                // the snapshot once the revision has actually moved.
                let Ok(revision) = system_menu_client.read_snapshot_revision().await else {
                    continue;
                };
                if last_menu_revision == Some(revision) {
                    continue;
                }
                if let Ok(snapshot) = system_menu_client.read_snapshot().await {
                    let presentation = system_menu_presentation(&snapshot);
                    let language = snapshot.resolved_language;
                    let appearance_theme = snapshot.appearance_theme;
                    let result = cx.update(|cx| {
                        if !cx.has_global::<ProductCoordinator>() {
                            return Ok(());
                        }
                        let coordinator = cx.global_mut::<ProductCoordinator>();
                        coordinator.update_language = language;
                        coordinator.update_appearance_theme = appearance_theme;
                        coordinator
                            .system_menu
                            .as_mut()
                            .ok_or_else(|| "system menu owner is unavailable".to_owned())?
                            .set_presentation(presentation)
                            .map_err(|error| error.to_string())
                    });
                    match result {
                        Ok(()) => last_menu_revision = Some(snapshot.revision),
                        Err(error) => record_failure(&system_menu_snapshot_failures, error),
                    }
                }
            }
        })
        .detach();

        let system_menu_failures = Arc::clone(&run_failures);
        cx.spawn(async move |cx| {
            loop {
                Timer::after(Duration::from_millis(50)).await;
                if !cx.update(|cx| cx.has_global::<ProductCoordinator>()) {
                    break;
                }                while let Ok(request) = status_icon_receiver.try_recv() {
                    let result = cx.update(|cx| {
                        if !cx.has_global::<ProductCoordinator>() {
                            return Err(SettingsError::new(
                                SettingsErrorCode::StatusIconUpdateFailed,
                            ));
                        }
                        cx.global_mut::<ProductCoordinator>()
                            .system_menu
                            .as_mut()
                            .ok_or_else(|| {
                                SettingsError::new(SettingsErrorCode::StatusIconUpdateFailed)
                            })?
                            .set_visible(request.visible)
                            .map_err(|_| {
                                SettingsError::new(SettingsErrorCode::StatusIconUpdateFailed)
                            })
                    });
                    let _ = request.reply.send(result);
                }
                #[cfg(target_os = "windows")]
                while let Ok(request) = taskbar_icon_receiver.try_recv() {
                    let result = cx.update(|cx| apply_taskbar_icon_visibility(cx, request.visible));
                    let _ = request.reply.send(result);
                }
                // Only macOS observes a completed install: the Windows install path
                // hands the payload to the NSIS installer and exits the process before
                // returning, so the request is consumed and nothing else happens there.
                #[cfg(target_os = "macos")]
                if cx.update(poll_update_restart) {
                    break;
                }
                #[cfg(target_os = "windows")]
                let _ = cx.update(take_update_restart_request);
                let action = cx.update(|cx| {
                    cx.try_global::<ProductCoordinator>()
                        .and_then(|coordinator| coordinator.system_menu.as_ref())
                        .and_then(SystemMenu::try_recv)
                });
                let Some(action) = action else {
                    continue;
                };
                let handled = match action {
                    SystemMenuAction::OpenSettings => {
                        cx.update(|cx| ensure_settings_window(cx).map(|_| true))
                    }
                    SystemMenuAction::ToggleOverlayVisibility
                    | SystemMenuAction::ToggleClickThrough => {
                        let client = cx.update(|cx| {
                            cx.try_global::<ProductCoordinator>()
                                .and_then(|coordinator| coordinator.settings_service.as_ref())
                                .map(bongocat_app::ApplicationSettingsService::client)
                                .ok_or_else(|| {
                                    "settings service is unavailable for the system menu".to_owned()
                                })
                        });
                        match client {
                            Ok(client) => apply_system_menu_overlay_action(client, action).await,
                            Err(error) => Err(error),
                        }
                    }
                    SystemMenuAction::CheckForUpdates => cx.update(|cx| {
                        open_update_window_and_check(cx);
                        Ok(true)
                    }),
                    SystemMenuAction::OpenSource => bongocat_platform::open_external_url(
                        "https://github.com/ayangweb/BongoCat",
                    )
                    .map(|_| true)
                    .map_err(|error| error.to_string()),
                    SystemMenuAction::Restart => match restart_product() {
                        Ok(()) => cx.update(|cx| {
                            request_product_quit(cx);
                            Ok(false)
                        }),
                        Err(error) => Err(error),
                    },
                    SystemMenuAction::Quit => cx.update(|cx| {
                        request_product_quit(cx);
                        Ok(false)
                    }),
                };
                match handled {
                    Ok(true) => {}
                    Ok(false) => break,
                    Err(error) => record_failure(&system_menu_failures, error),
                }
            }
        })
        .detach();

        #[cfg(target_os = "windows")]
        let single_instance_failures = Arc::clone(&run_failures);
        #[cfg(target_os = "windows")]
        cx.spawn(async move |cx| {
            loop {
                Timer::after(Duration::from_millis(25)).await;
                if !cx.update(|cx| cx.has_global::<ProductCoordinator>()) {
                    break;
                }
                let action = cx.update(|cx| {
                    cx.try_global::<ProductCoordinator>()
                        .and_then(|coordinator| coordinator.single_instance.as_ref())
                        .and_then(SingleInstance::try_recv)
                });
                let Some(action) = action else {
                    continue;
                };
                let handled = cx.update(|cx| match action {
                    SingleInstanceAction::OpenSettings => {
                        ensure_settings_window(cx)?;
                        let coordinator = cx.global_mut::<ProductCoordinator>();
                        coordinator.single_instance_wakes =
                            coordinator.single_instance_wakes.saturating_add(1);
                        Ok::<_, String>(())
                    }
                });
                match handled {
                    Ok(()) => {}
                    Err(error) => record_failure(&single_instance_failures, error),
                }
            }
        })
        .detach();

        let initial_settings_window = if run_options.opens_settings_window_on_start() {
            match ensure_settings_window(cx) {
                Ok(window) => Some(window),
                Err(error) => {
                    record_failure(&run_failures, error);
                    request_product_quit(cx);
                    return;
                }
            }
        } else {
            None
        };

        #[cfg(target_os = "windows")]
        let frame_shutdown_requested = Arc::clone(&shutdown_requested);
        let frame_settings_client = settings_client.clone();
        let frame_source_guard = frame_source_shutdown.run_guard();
        cx.spawn(async move |cx| {
            let _frame_source_guard = frame_source_guard;
            #[cfg(target_os = "windows")]
            let mut frame_active = true;
            #[cfg(target_os = "windows")]
            let mut shutdown_flush_started = false;
            let mut last_overlay_bounds = None;
            let mut overlay_placement_debouncer = OverlayPlacementDebouncer::default();
            let mut retry_delay = None;
            loop {
                let runtime_snapshot = frame_runtime_client.snapshot();
                let frame_interval = bongocat_runtime::frame_interval_for_runtime(
                    runtime_snapshot.maximum_fps,
                    runtime_snapshot.overlay_visible,
                )
                .expect("runtime stores validated frame scheduling state");
                Timer::after(retry_delay.take().unwrap_or(frame_interval)).await;
                if frame_source_shutdown.stop_requested() {
                    break;
                }
                let context_menu_requested = context_menu_receiver.try_recv().is_ok();
                #[cfg(target_os = "macos")]
                let (keep_running, next_retry_delay) = cx.update(|cx| {
                    if !cx.has_global::<ProductCoordinator>() {
                        return (false, None);
                    }
                    handle_shortcut_toggle_settings(cx);
                    let (keep_running, failure, settings_window, failures, next_retry_delay) = {
                        let coordinator = cx.global_mut::<ProductCoordinator>();
                        if !coordinator.frame_source_running {
                            return (false, None);
                        }
                        let result = coordinator
                            .overlay
                            .as_mut()
                            .expect("product overlay owner is present")
                            .tick();
                        if context_menu_requested
                            && let Some(menu) = coordinator.system_menu.as_ref()
                            && let Some(overlay) = coordinator.overlay.as_ref()
                            && let Err(error) = menu.show_context_menu_for_window(overlay)
                        {
                            record_failure(&coordinator.failures, error.to_string());
                        }
                        match result {
                            Ok(outcome) => {
                                if let Ok(bounds) = coordinator
                                    .overlay
                                    .as_ref()
                                    .expect("product overlay owner is present")
                                    .window_bounds()
                                    && last_overlay_bounds != Some(bounds)
                                    && let Some(bounds) = overlay_placement_debouncer
                                        .observe(bounds, Instant::now())
                                    && {
                                        let sent = frame_settings_client
                                            .update_overlay_window_placement(
                                                bounds.x,
                                                bounds.y,
                                                bounds.width,
                                                bounds.height,
                                            )
                                            .is_ok();
                                        if sent {
                                            overlay_placement_debouncer.mark_sent(bounds);
                                        }
                                        sent
                                    }
                                {
                                    last_overlay_bounds = Some(bounds);
                                }
                                coordinator.frame_ticks = coordinator.frame_ticks.saturating_add(1);
                                (true, None, None, None, outcome.retry_after())
                            }
                            Err(error) => {
                                coordinator.frame_source_running = false;
                                (
                                    false,
                                    Some(error.to_string()),
                                    coordinator.settings_window.clone(),
                                    Some(Arc::clone(&coordinator.failures)),
                                    None,
                                )
                            }
                        }
                    };
                    if let (Some(failure), Some(settings_window), Some(failures)) =
                        (failure, settings_window, failures)
                    {
                        record_failure(&failures, failure);
                        let _ = settings_window.update(cx, |view, _, cx| {
                            view.report_service_error(
                                SettingsError::new(SettingsErrorCode::RuntimeUnavailable),
                                cx,
                            );
                        });
                    }
                    (keep_running, next_retry_delay)
                });
                #[cfg(target_os = "macos")]
                {
                    retry_delay = next_retry_delay;
                }
                #[cfg(target_os = "windows")]
                let (tick_result, system_termination_requested) = if frame_active {
                    let mut overlay = frame_overlay.borrow_mut();
                    let overlay = overlay
                        .as_mut()
                        .expect("product overlay owner is present while the frame source runs");
                    let result = overlay.tick();
                    if result.is_ok()
                        && let Ok(bounds) = overlay.window_bounds()
                        && last_overlay_bounds != Some(bounds)
                        && let Some(bounds) =
                            overlay_placement_debouncer.observe(bounds, Instant::now())
                        && {
                            let sent = frame_settings_client
                                .update_overlay_window_placement(
                                    bounds.x,
                                    bounds.y,
                                    bounds.width,
                                    bounds.height,
                                )
                                .is_ok();
                            if sent {
                                overlay_placement_debouncer.mark_sent(bounds);
                            }
                            sent
                        }
                    {
                        last_overlay_bounds = Some(bounds);
                    }
                    (Some(result), overlay.system_termination_requested())
                } else {
                    (None, false)
                };
                #[cfg(target_os = "windows")]
                if tick_result.as_ref().is_some_and(Result::is_err) {
                    frame_active = false;
                }
                #[cfg(target_os = "windows")]
                let next_retry_delay = tick_result
                    .as_ref()
                    .and_then(|result| result.as_ref().ok())
                    .and_then(|outcome| outcome.retry_after());
                #[cfg(target_os = "windows")]
                let mut tick_result = Some(tick_result);
                #[cfg(target_os = "windows")]
                let mut request_shutdown_flush = false;
                #[cfg(target_os = "windows")]
                let keep_running = cx.update(|cx| {
                    if !cx.has_global::<ProductCoordinator>() {
                        return false;
                    }
                    handle_shortcut_toggle_settings(cx);
                    if context_menu_requested
                        && let Some(coordinator) = cx.try_global::<ProductCoordinator>()
                        && let Some(menu) = coordinator.system_menu.as_ref()
                    {
                        let result = coordinator
                            .overlay
                            .borrow()
                            .as_ref()
                            .ok_or(bongocat_platform::SystemMenuError::WindowHandleUnavailable)
                            .and_then(|overlay| menu.show_context_menu_for_window(overlay));
                        if let Err(error) = result {
                            record_failure(&coordinator.failures, error.to_string());
                        }
                    }
                    let (failure, failures, settings_window) = {
                        let coordinator = cx.global_mut::<ProductCoordinator>();
                        match tick_result
                            .take()
                            .expect("a successful window update invokes the frame closure once")
                        {
                            None => (None, None, None),
                            Some(Ok(_)) => {
                                coordinator.frame_ticks = coordinator.frame_ticks.saturating_add(1);
                                (None, None, None)
                            }
                            Some(Err(error)) => {
                                coordinator.frame_source_running = false;
                                (
                                    Some(error.to_string()),
                                    Some(Arc::clone(&coordinator.failures)),
                                    coordinator.settings_window.clone(),
                                )
                            }
                        }
                    };
                    if let (Some(failure), Some(failures)) = (failure, failures) {
                        record_failure(&failures, failure);
                        if let Some(settings_window) = settings_window {
                            let _ = settings_window.update(cx, |view, _, cx| {
                                view.report_service_error(
                                    SettingsError::new(SettingsErrorCode::RuntimeUnavailable),
                                    cx,
                                );
                            });
                        }
                    }
                    if system_termination_requested {
                        frame_shutdown_requested.store(true, Ordering::Release);
                    }
                    if frame_shutdown_requested.load(Ordering::Acquire) {
                        let flush_complete = cx
                            .global::<ProductCoordinator>()
                            .shutdown_flush_complete
                            .load(Ordering::Acquire);
                        if flush_complete {
                            start_windows_product_shutdown(cx);
                            return false;
                        }
                        if !shutdown_flush_started {
                            shutdown_flush_started = true;
                            request_shutdown_flush = true;
                        }
                    }
                    true
                });
                #[cfg(target_os = "windows")]
                if request_shutdown_flush {
                    let flush_requested = cx.update(|cx| {
                        cx.try_global::<ProductCoordinator>()
                            .and_then(|coordinator| coordinator.settings_window.clone())
                            .is_some_and(|window| window.request_quit_after_flush(cx).is_ok())
                    });
                    if !flush_requested {
                        cx.update(|cx| {
                            cx.global::<ProductCoordinator>()
                                .shutdown_flush_complete
                                .store(true, Ordering::Release);
                        });
                    }
                }
                #[cfg(target_os = "windows")]
                {
                    retry_delay = next_retry_delay;
                }
                if !keep_running {
                    break;
                }
            }
            if let Some(bounds) = overlay_placement_debouncer.flush(Instant::now()) {
                let sent = frame_settings_client.update_overlay_window_placement(
                    bounds.x,
                    bounds.y,
                    bounds.width,
                    bounds.height,
                ).is_ok();
                if sent {
                    overlay_placement_debouncer.mark_sent(bounds);
                }
            }
        })
        .detach();

        if run_options.hidden_model_switch_smoke {
            let smoke_client = settings_client.clone();
            let smoke_failures = Arc::clone(&run_failures);
            #[cfg(target_os = "windows")]
            let smoke_shutdown_requested = Arc::clone(&shutdown_requested);
            cx.spawn(async move |cx| {
                Timer::after(Duration::from_millis(500)).await;
                let result = async {
                    let initial = smoke_client
                        .read_snapshot()
                        .await
                        .map_err(|error| format!("read initial settings snapshot: {error}"))?;
                    let initial_revision = initial.config_revision.ok_or_else(|| {
                        "initial configuration revision is unavailable".to_owned()
                    })?;
                    let initial_model = initial
                        .active_model
                        .clone()
                        .ok_or_else(|| "initial active model is unavailable".to_owned())?;
                    let replacement_model = initial
                        .model_catalog
                        .entries
                        .iter()
                        .find(|entry| {
                            entry.origin == SettingsModelOrigin::Preset
                                && (entry.id != initial_model.id
                                    || entry.origin != initial_model.origin)
                                && matches!(
                                    &entry.availability,
                                    SettingsModelAvailability::Ready { .. }
                                )
                        })
                        .map(|entry| SettingsModelKey {
                            id: entry.id.clone(),
                            origin: entry.origin,
                        })
                        .ok_or_else(|| "no alternate ready preset model is available".to_owned())?;
                    let (initial_generation, _) = cx.update(product_overlay_state)?;

                    let hidden = smoke_client
                        .set_overlay_visible(initial_revision, false)
                        .await
                        .map_err(|error| format!("hide overlay: {error}"))?;
                    if hidden.overlay_visible {
                        return Err("runtime did not hide the overlay".to_owned());
                    }
                    let hidden_revision = hidden
                        .config_revision
                        .ok_or_else(|| "hidden configuration revision is unavailable".to_owned())?;

                    let switched = smoke_client
                        .select_model(hidden_revision, replacement_model.clone())
                        .await
                        .map_err(|error| format!("switch hidden overlay model: {error}"))?;
                    if switched.overlay_visible
                        || switched.active_model.as_ref() != Some(&replacement_model)
                    {
                        return Err(
                            "hidden model switch did not project the committed model".to_owned()
                        );
                    }
                    let (switched_generation, visible) = cx.update(product_overlay_state)?;
                    if visible {
                        return Err("overlay became visible during hidden model switch".to_owned());
                    }
                    if switched_generation <= initial_generation {
                        return Err("hidden model switch did not advance GPU generation".to_owned());
                    }

                    let switched_revision = switched.config_revision.ok_or_else(|| {
                        "switched configuration revision is unavailable".to_owned()
                    })?;
                    let shown = smoke_client
                        .set_overlay_visible(switched_revision, true)
                        .await
                        .map_err(|error| format!("show switched overlay: {error}"))?;
                    let mut revealed = false;
                    for _ in 0..200 {
                        Timer::after(Duration::from_millis(10)).await;
                        let (generation, visible) = cx.update(product_overlay_state)?;
                        if visible && generation == switched_generation {
                            revealed = true;
                            break;
                        }
                    }
                    if !revealed {
                        return Err(
                            "switched overlay was not presented before becoming visible".to_owned()
                        );
                    }

                    let shown_revision = shown
                        .config_revision
                        .ok_or_else(|| "shown configuration revision is unavailable".to_owned())?;
                    let restored = smoke_client
                        .select_model(shown_revision, initial_model)
                        .await
                        .map_err(|error| format!("restore initial model: {error}"))?;
                    if !initial.overlay_visible {
                        let restored_revision = restored.config_revision.ok_or_else(|| {
                            "restored configuration revision is unavailable".to_owned()
                        })?;
                        smoke_client
                            .set_overlay_visible(restored_revision, false)
                            .await
                            .map_err(|error| format!("restore hidden overlay state: {error}"))?;
                    }
                    write_smoke_status("hidden model switch committed before reveal")
                        .map_err(|error| error.to_string())?;
                    Ok::<(), String>(())
                }
                .await;
                if let Err(error) = result {
                    record_failure(&smoke_failures, error);
                }
                #[cfg(target_os = "macos")]
                cx.update(request_product_quit);
                #[cfg(target_os = "windows")]
                request_windows_product_quit(&smoke_shutdown_requested);
            })
            .detach();
        }

        if run_options.settings_window_smoke {
            let smoke_failures = Arc::clone(&run_failures);
            let smoke_window = initial_settings_window
                .clone()
                .expect("settings window smoke requested its explicit settings window");
            #[cfg(target_os = "windows")]
            let smoke_shutdown_requested = Arc::clone(&shutdown_requested);
            cx.spawn(async move |cx| {
                Timer::after(Duration::from_millis(500)).await;
                // Wait for the first frame instead of assuming the delay covered it.
                // The general page asserts on `applied_theme`, which is only assigned
                // during a render; on a loaded machine 500ms was not always enough, so
                // the assertion failed and — because the page calls were chained with
                // `?` — the remaining pages were never exercised at all.
                let mut rendered = false;
                for _ in 0..SMOKE_FIRST_FRAME_WAIT_TICKS {
                    rendered = cx.update(|cx| {
                        smoke_window
                            .update(cx, |view, _, _| view.appearance_applied_for_smoke())
                            .unwrap_or(false)
                    });
                    if rendered {
                        break;
                    }
                    Timer::after(Duration::from_millis(50)).await;
                }
                if !rendered {
                    record_failure(
                        &smoke_failures,
                        "settings window did not render before the page smoke began".to_owned(),
                    );
                }
                // Every page runs even when an earlier one fails: chaining them with
                // `?` meant one failure hid the rest, and a single reported failure
                // looked like the whole smoke had been exercised.
                let settings_pages =
                    update_settings_window(cx, &smoke_window, |view, _, cx| {
                        view.run_page_smoke(cx)
                    })
                    .await;
                match settings_pages {
                    Ok(()) => {}
                    Err(error) => {
                        record_failure(&smoke_failures, error);
                        #[cfg(target_os = "macos")]
                        cx.update(request_product_quit);
                        #[cfg(target_os = "windows")]
                        request_windows_product_quit(&smoke_shutdown_requested);
                        return;
                    }
                }
                if run_options.models_page_smoke {
                    let models_page =
                        update_settings_window(cx, &smoke_window, |view, _, cx| {
                            view.show_models_page_for_smoke(cx)
                        })
                        .await;
                    match models_page {
                        Ok(()) => {
                            Timer::after(Duration::from_millis(250)).await;
                        }
                        Err(error) => {
                            record_failure(&smoke_failures, error);
                            #[cfg(target_os = "macos")]
                            cx.update(request_product_quit);
                            #[cfg(target_os = "windows")]
                            request_windows_product_quit(&smoke_shutdown_requested);
                            return;
                        }
                    }
                }
                let baseline = cx.update(|cx| -> Result<_, String> {
                    let (window_handle, frame_ticks) = {
                        let coordinator = cx.global::<ProductCoordinator>();
                        (
                            coordinator
                                .settings_window
                                .clone()
                                .ok_or_else(|| "settings window is not open".to_owned())?,
                            coordinator.frame_ticks,
                        )
                    };
                    cx.global::<ProductCoordinator>()
                        .shortcut_signals
                        .request_open_settings();
                    handle_shortcut_toggle_settings(cx);
                    Ok((frame_ticks, window_handle))
                });
                let (baseline_ticks, original_window) = match baseline {
                    Ok(baseline) => baseline,
                    Err(error) => {
                        record_failure(&smoke_failures, error);
                        #[cfg(target_os = "macos")]
                        cx.update(request_product_quit);
                        #[cfg(target_os = "windows")]
                        request_windows_product_quit(&smoke_shutdown_requested);
                        return;
                    }
                };

                let mut hidden = false;
                for _ in 0..60 {
                    Timer::after(Duration::from_millis(50)).await;
                    let observed = update_settings_window(cx, &original_window, |view, _, cx| {
                        Ok::<_, String>((view.window_hidden(), cx.windows().len()))
                    })
                    .await;
                    match observed {
                        Ok((true, 1)) => {
                            hidden = true;
                            break;
                        }
                        Ok((true, windows)) => {
                            record_failure(
                                &smoke_failures,
                                format!(
                                    "settings close left {windows} windows instead of the one \
                                     pre-rendered window"
                                ),
                            );
                            #[cfg(target_os = "macos")]
                            cx.update(request_product_quit);
                            #[cfg(target_os = "windows")]
                            request_windows_product_quit(&smoke_shutdown_requested);
                            return;
                        }
                        Ok((false, _)) => {}
                        Err(error) => {
                            record_failure(&smoke_failures, error);
                            #[cfg(target_os = "macos")]
                            cx.update(request_product_quit);
                            #[cfg(target_os = "windows")]
                            request_windows_product_quit(&smoke_shutdown_requested);
                            return;
                        }
                    }
                }
                if !hidden {
                    record_failure(&smoke_failures, "settings window did not hide");
                    #[cfg(target_os = "macos")]
                    cx.update(request_product_quit);
                    #[cfg(target_os = "windows")]
                    request_windows_product_quit(&smoke_shutdown_requested);
                    return;
                }

                Timer::after(Duration::from_millis(500)).await;
                let reopened = cx.update(|cx| -> Result<SettingsWindowHandle, String> {
                    if cx.global::<ProductCoordinator>().frame_ticks <= baseline_ticks {
                        return Err(
                            "frame source stopped while the settings window was closed".to_owned()
                        );
                    }
                    cx.global::<ProductCoordinator>()
                        .shortcut_signals
                        .request_open_settings();
                    handle_shortcut_toggle_settings(cx);
                    let reopened = cx
                        .global::<ProductCoordinator>()
                        .settings_window
                        .clone()
                        .ok_or_else(|| "settings shortcut did not restore the window".to_owned())?;
                    if cx.windows().len() != 1 {
                        return Err("settings reopen created more than one window".to_owned());
                    }
                    if reopened != original_window {
                        return Err(
                            "settings reopen replaced the pre-rendered window entity".to_owned()
                        );
                    }
                    Ok(reopened)
                });
                match reopened {
                    Ok(_) => {}
                    Err(error) => {
                        record_failure(&smoke_failures, error);
                        #[cfg(target_os = "macos")]
                        cx.update(request_product_quit);
                        #[cfg(target_os = "windows")]
                        request_windows_product_quit(&smoke_shutdown_requested);
                        return;
                    }
                }

                Timer::after(Duration::from_millis(500)).await;
                let restored = cx.update(|cx| -> Result<(), String> {
                    let window_handle =
                        cx.global::<ProductCoordinator>()
                            .settings_window
                            .clone()
                            .ok_or_else(|| "settings window was not retained".to_owned())?;
                    let revision = window_handle
                        .update(cx, |view, _, _| view.snapshot_revision())
                        .map_err(|error| error.to_string())?;
                    if revision.is_none() {
                        return Err(
                            "reopened settings window did not keep a runtime snapshot".to_owned(),
                        );
                    }
                    Ok(())
                });
                match restored {
                    Ok(()) => {
                        if let Err(error) = write_smoke_status(
                            "settings window hid and reopened from one pre-rendered entity",
                        ) {
                            record_failure(&smoke_failures, error.to_string());
                        }
                    }
                    Err(error) => {
                        record_failure(&smoke_failures, error);
                        #[cfg(target_os = "macos")]
                        cx.update(request_product_quit);
                        #[cfg(target_os = "windows")]
                        request_windows_product_quit(&smoke_shutdown_requested);
                    }
                }
                #[cfg(target_os = "macos")]
                cx.update(request_product_quit);
                #[cfg(target_os = "windows")]
                request_windows_product_quit(&smoke_shutdown_requested);
            })
            .detach();
        }

        if run_options.system_menu_smoke {
            let smoke_failures = Arc::clone(&run_failures);
            let smoke_client = settings_client.clone();
            cx.spawn(async move |cx| {
                Timer::after(Duration::from_millis(500)).await;
                let visibility_result = async {
                    let initial = smoke_client
                        .read_snapshot()
                        .await
                        .map_err(|error| format!("read status icon snapshot: {error}"))?;
                    let initial_visibility = initial.status_icon_visible;
                    let mut current = initial;
                    if !current.status_icon_visible {
                        current = smoke_client
                            .set_status_icon_visible(
                                current.config_revision.ok_or_else(|| {
                                    "status icon config revision is unavailable".to_owned()
                                })?,
                                true,
                            )
                            .await
                            .map_err(|error| format!("show status icon for smoke: {error}"))?;
                    }
                    for _ in 0..100 {
                        let visible = cx.update(|cx| {
                            cx.global::<ProductCoordinator>()
                                .system_menu
                                .as_ref()
                                .is_some_and(SystemMenu::is_visible)
                        });
                        if visible {
                            break;
                        }
                        Timer::after(Duration::from_millis(10)).await;
                    }
                    if !cx.update(|cx| {
                        cx.global::<ProductCoordinator>()
                            .system_menu
                            .as_ref()
                            .is_some_and(SystemMenu::is_visible)
                    }) {
                        return Err("status icon did not become visible".to_owned());
                    }

                    let hidden = smoke_client
                        .set_status_icon_visible(
                            current.config_revision.ok_or_else(|| {
                                "visible status icon revision is unavailable".to_owned()
                            })?,
                            false,
                        )
                        .await
                        .map_err(|error| format!("hide status icon: {error}"))?;
                    if hidden.status_icon_visible
                        || cx.update(|cx| {
                            cx.global::<ProductCoordinator>()
                                .system_menu
                                .as_ref()
                                .is_some_and(SystemMenu::is_visible)
                        })
                    {
                        return Err("status icon hide did not commit atomically".to_owned());
                    }

                    let shown = smoke_client
                        .set_status_icon_visible(
                            hidden.config_revision.ok_or_else(|| {
                                "hidden status icon revision is unavailable".to_owned()
                            })?,
                            true,
                        )
                        .await
                        .map_err(|error| format!("restore visible status icon: {error}"))?;
                    if !shown.status_icon_visible
                        || !cx.update(|cx| {
                            cx.global::<ProductCoordinator>()
                                .system_menu
                                .as_ref()
                                .is_some_and(SystemMenu::is_visible)
                        })
                    {
                        return Err("status icon show did not commit atomically".to_owned());
                    }
                    Ok::<_, String>((initial_visibility, shown))
                }
                .await;
                let (initial_visibility, shown) = match visibility_result {
                    Ok(result) => result,
                    Err(error) => {
                        record_failure(&smoke_failures, error);
                        cx.update(request_product_quit);
                        return;
                    }
                };
                if let Err(error) = write_smoke_status("status icon hidden and restored") {
                    record_failure(&smoke_failures, error.to_string());
                    cx.update(request_product_quit);
                    return;
                }
                let open_requested = cx.update(|cx| {
                    cx.global::<ProductCoordinator>()
                        .system_menu
                        .as_ref()
                        .ok_or_else(|| "system menu owner is unavailable".to_owned())?
                        .request_action_for_smoke(SystemMenuAction::OpenSettings)
                        .map_err(|error| error.to_string())
                });
                if let Err(error) = open_requested {
                    record_failure(&smoke_failures, error.to_string());
                    cx.update(request_product_quit);
                    return;
                }

                let mut open_verified =
                    Err("Open Settings did not restore a runtime snapshot".to_owned());
                for _ in 0..SMOKE_FIRST_FRAME_WAIT_TICKS {
                    Timer::after(Duration::from_millis(50)).await;
                    let verified = cx.update(|cx| -> Result<bool, String> {
                        if cx.windows().len() != 1 {
                            return Err("Open Settings created a duplicate GPUI window".to_owned());
                        }
                        let window = cx
                            .global::<ProductCoordinator>()
                            .settings_window
                            .clone()
                            .ok_or_else(|| {
                                "Open Settings did not retain a settings window".to_owned()
                            })?;
                        let (revision, hidden) = window
                            .update(cx, |view, _, _| {
                                (view.snapshot_revision(), view.window_hidden())
                            })
                            .map_err(|error| error.to_string())?;
                        if hidden {
                            return Err("Open Settings left the settings window hidden".to_owned());
                        }
                        Ok(revision.is_some())
                    });
                    match verified {
                        Ok(true) => {
                            open_verified = Ok(());
                            break;
                        }
                        Ok(false) => {}
                        Err(error) => {
                            open_verified = Err(error);
                            break;
                        }
                    }
                }
                if let Err(error) = open_verified {
                    record_failure(&smoke_failures, error.to_string());
                    cx.update(request_product_quit);
                    return;
                }

                let overlay_result = async {
                    let initial = smoke_client
                        .read_snapshot()
                        .await
                        .map_err(|error| format!("read overlay visibility snapshot: {error}"))?;
                    let initial_visibility = initial.overlay_visible;
                    cx.update(|cx| {
                        cx.global::<ProductCoordinator>()
                            .system_menu
                            .as_ref()
                            .ok_or_else(|| "system menu owner is unavailable".to_owned())?
                            .request_action_for_smoke(SystemMenuAction::ToggleOverlayVisibility)
                            .map_err(|error| error.to_string())
                    })?;

                    let mut changed = None;
                    for _ in 0..100 {
                        let snapshot = smoke_client
                            .read_snapshot()
                            .await
                            .map_err(|error| format!("read toggled overlay snapshot: {error}"))?;
                        if snapshot.overlay_visible != initial_visibility {
                            changed = Some(snapshot);
                            break;
                        }
                        Timer::after(Duration::from_millis(10)).await;
                    }
                    let changed = changed.ok_or_else(|| {
                        "system menu overlay action did not reach the runtime".to_owned()
                    })?;
                    if changed.config_revision == initial.config_revision {
                        return Err(
                            "system menu overlay action did not persist a new configuration revision"
                                .to_owned(),
                        );
                    }

                    cx.update(|cx| {
                        cx.global::<ProductCoordinator>()
                            .system_menu
                            .as_ref()
                            .ok_or_else(|| "system menu owner is unavailable".to_owned())?
                            .request_action_for_smoke(SystemMenuAction::ToggleOverlayVisibility)
                            .map_err(|error| error.to_string())
                    })?;
                    for _ in 0..100 {
                        let snapshot = smoke_client.read_snapshot().await.map_err(|error| {
                            format!("read restored overlay visibility snapshot: {error}")
                        })?;
                        if snapshot.overlay_visible == initial_visibility {
                            return Ok::<(), String>(());
                        }
                        Timer::after(Duration::from_millis(10)).await;
                    }
                    Err("system menu overlay action did not restore the runtime state".to_owned())
                }
                .await;
                if let Err(error) = overlay_result {
                    record_failure(&smoke_failures, error);
                    cx.update(request_product_quit);
                    return;
                }
                if let Err(error) = write_smoke_status("overlay visibility toggled and restored") {
                    record_failure(&smoke_failures, error.to_string());
                    cx.update(request_product_quit);
                    return;
                }

                if !initial_visibility {
                    let restored = smoke_client
                        .set_status_icon_visible(
                            shown
                                .config_revision
                                .expect("shown status icon has a config revision"),
                            false,
                        )
                        .await;
                    if let Err(error) = restored {
                        record_failure(
                            &smoke_failures,
                            format!("restore initial status icon visibility: {error}"),
                        );
                        cx.update(request_product_quit);
                        return;
                    }
                }

                #[cfg(target_os = "windows")]
                {
                    let taskbar_result = async {
                        let initial = smoke_client
                            .read_snapshot()
                            .await
                            .map_err(|error| format!("read taskbar icon snapshot: {error}"))?;
                        let initial_visibility = initial.taskbar_icon_visible;
                        let (native_initial_visibility, initially_hidden) =
                            cx.update(product_taskbar_icon_state)?;
                        if native_initial_visibility != initial_visibility || initially_hidden {
                            return Err(
                                "startup taskbar visibility diverged from the current snapshot"
                                    .to_owned(),
                            );
                        }
                        let changed = smoke_client
                            .set_taskbar_icon_visible(
                                initial.config_revision.ok_or_else(|| {
                                    "taskbar icon config revision is unavailable".to_owned()
                                })?,
                                !initial_visibility,
                            )
                            .await
                            .map_err(|error| format!("toggle taskbar icon: {error}"))?;
                        let (native_changed_visibility, window_hidden) =
                            cx.update(product_taskbar_icon_state)?;
                        if changed.taskbar_icon_visible == initial_visibility
                            || native_changed_visibility != changed.taskbar_icon_visible
                            || window_hidden
                        {
                            return Err(
                                "taskbar icon toggle did not preserve the visible settings window"
                                    .to_owned(),
                            );
                        }
                        let restored = smoke_client
                            .set_taskbar_icon_visible(
                                changed.config_revision.ok_or_else(|| {
                                    "changed taskbar icon revision is unavailable".to_owned()
                                })?,
                                initial_visibility,
                            )
                            .await
                            .map_err(|error| format!("restore taskbar icon: {error}"))?;
                        let (native_restored_visibility, window_hidden) =
                            cx.update(product_taskbar_icon_state)?;
                        if restored.taskbar_icon_visible != initial_visibility
                            || native_restored_visibility != initial_visibility
                            || window_hidden
                        {
                            return Err(
                                "taskbar icon visibility was not restored atomically".to_owned()
                            );
                        }
                        Ok::<(), String>(())
                    }
                    .await;
                    if let Err(error) = taskbar_result {
                        record_failure(&smoke_failures, error);
                        cx.update(request_product_quit);
                        return;
                    }
                    if let Err(error) = write_smoke_status("taskbar icon toggled and restored") {
                        record_failure(&smoke_failures, error.to_string());
                        cx.update(request_product_quit);
                        return;
                    }
                }

                let quit_requested = cx.update(|cx| {
                    cx.global::<ProductCoordinator>()
                        .system_menu
                        .as_ref()
                        .ok_or_else(|| "system menu owner is unavailable".to_owned())?
                        .request_action_for_smoke(SystemMenuAction::Quit)
                        .map_err(|error| error.to_string())
                });
                if let Err(error) = quit_requested {
                    record_failure(&smoke_failures, error.to_string());
                    cx.update(request_product_quit);
                }
            })
            .detach();
        }

        #[cfg(target_os = "macos")]
        if run_options.application_reopen_smoke {
            let smoke_failures = Arc::clone(&run_failures);
            cx.spawn(async move |cx| {
                Timer::after(Duration::from_millis(500)).await;
                let baseline = cx.update(|cx| -> Result<_, String> {
                    let coordinator = cx.global::<ProductCoordinator>();
                    let original_window = coordinator
                        .settings_window
                        .clone()
                        .ok_or_else(|| "settings window is not open".to_owned())?;
                    let frame_ticks = coordinator.frame_ticks;
                    let application_reopens = coordinator.application_reopens;
                    original_window
                        .update(cx, |view, window, cx| view.hide(window, cx))
                        .map_err(|error| error.to_string())??;
                    Ok((original_window, frame_ticks, application_reopens))
                });
                let (original_window, baseline_ticks, baseline_reopens) = match baseline {
                    Ok(baseline) => baseline,
                    Err(error) => {
                        record_failure(&smoke_failures, error);
                        cx.update(request_product_quit);
                        return;
                    }
                };

                // Hiding is the state the dock-icon reopen has to recover from: the window
                // stays pre-rendered while it is off screen, so this smoke proves the
                // reopen shows that same view instead of building a second one.
                let mut hidden = false;
                for _ in 0..60 {
                    Timer::after(Duration::from_millis(50)).await;
                    match update_settings_window(cx, &original_window, |view, _, _| {
                        Ok::<_, String>(view.window_hidden())
                    })
                    .await
                    {
                        Ok(true) => {
                            hidden = true;
                            break;
                        }
                        Ok(false) => {}
                        Err(error) => {
                            record_failure(&smoke_failures, error);
                            cx.update(request_product_quit);
                            return;
                        }
                    }
                }
                if !hidden {
                    let _ = write_smoke_status("application-reopen hide failed");
                    record_failure(
                        &smoke_failures,
                        "application-reopen smoke could not hide the settings window",
                    );
                    cx.update(request_product_quit);
                    return;
                }
                if let Err(error) = write_smoke_status("application-reopen primary ready") {
                    record_failure(&smoke_failures, error.to_string());
                    cx.update(request_product_quit);
                    return;
                }

                for _ in 0..100 {
                    Timer::after(Duration::from_millis(50)).await;
                    let restored = cx.update(|cx| -> Result<bool, String> {
                        let coordinator = cx.global::<ProductCoordinator>();
                        if coordinator.application_reopens <= baseline_reopens {
                            return Ok(false);
                        }
                        if coordinator.frame_ticks <= baseline_ticks {
                            return Ok(false);
                        }
                        let reopened = coordinator.settings_window.clone().ok_or_else(|| {
                            "application reopen did not retain a settings window".to_owned()
                        })?;
                        if reopened != original_window {
                            return Err(
                                "application reopen replaced the pre-rendered settings window"
                                    .to_owned(),
                            );
                        }
                        if cx.windows().len() != 1 {
                            return Err("application reopen created more than one settings window"
                                .to_owned());
                        }
                        if reopened
                            .update(cx, |view, _, _| view.window_hidden())
                            .map_err(|error| error.to_string())?
                        {
                            return Ok(false);
                        }
                        let revision = reopened
                            .update(cx, |view, _, _| view.snapshot_revision())
                            .map_err(|error| error.to_string())?;
                        if revision.is_none() {
                            return Ok(false);
                        }
                        Ok(true)
                    });
                    match restored {
                        Ok(true) => {
                            if let Err(error) = write_smoke_status(
                                "application reopen restored the settings window",
                            ) {
                                record_failure(&smoke_failures, error.to_string());
                                cx.update(request_product_quit);
                                return;
                            }
                            Timer::after(Duration::from_secs(1)).await;
                            cx.update(request_product_quit);
                            return;
                        }
                        Ok(false) => {}
                        Err(error) => {
                            let _ = write_smoke_status("application-reopen invariant failed");
                            record_failure(&smoke_failures, error);
                            cx.update(request_product_quit);
                            return;
                        }
                    }
                }
                record_failure(
                    &smoke_failures,
                    "running macOS application did not receive the LaunchServices reopen",
                );
                let _ = write_smoke_status("application-reopen timed out");
                cx.update(request_product_quit);
            })
            .detach();
        }

        #[cfg(target_os = "windows")]
        if run_options.single_instance_smoke {
            let single_instance_ready_file = run_options.single_instance_ready_file.clone();
            let single_instance_result_file = run_options.single_instance_result_file.clone();
            let smoke_failures = Arc::clone(&run_failures);
            let smoke_shutdown_requested = Arc::clone(&shutdown_requested);
            let settings_window = initial_settings_window
                .clone()
                .expect("single-instance smoke requested its explicit settings window");
            cx.spawn(async move |cx| {
                Timer::after(Duration::from_millis(500)).await;
                let baseline = update_settings_window(
                    cx,
                    &settings_window,
                    |_, window, cx| -> Result<u64, String> {
                        let frame_ticks = cx.global::<ProductCoordinator>().frame_ticks;
                        bongocat_platform::request_native_window_close(window)
                            .map_err(|error| error.to_string())?;
                        Ok(frame_ticks)
                    },
                )
                .await;
                let baseline_ticks = match baseline {
                    Ok(frame_ticks) => frame_ticks,
                    Err(error) => {
                        record_failure(&smoke_failures, error.to_string());
                        request_windows_product_quit(&smoke_shutdown_requested);
                        return;
                    }
                };

                let mut hidden = false;
                for _ in 0..60 {
                    Timer::after(Duration::from_millis(50)).await;
                    match update_settings_window(cx, &settings_window, |view, _, _| {
                        Ok(view.window_hidden())
                    })
                    .await
                    {
                        Ok(true) => {
                            hidden = true;
                            break;
                        }
                        Ok(false) => {}
                        Err(error) => {
                            record_failure(&smoke_failures, error);
                            request_windows_product_quit(&smoke_shutdown_requested);
                            return;
                        }
                    }
                }
                if !hidden {
                    record_failure(
                        &smoke_failures,
                        "single-instance smoke could not hide the settings window",
                    );
                    request_windows_product_quit(&smoke_shutdown_requested);
                    return;
                }
                if let Err(error) = write_smoke_status("single-instance primary ready") {
                    record_failure(&smoke_failures, error.to_string());
                    request_windows_product_quit(&smoke_shutdown_requested);
                    return;
                }
                if let Some(path) = single_instance_ready_file.as_deref()
                    && let Err(error) = write_smoke_marker(path, "single-instance primary ready")
                {
                    record_failure(&smoke_failures, error.to_string());
                    request_windows_product_quit(&smoke_shutdown_requested);
                    return;
                }

                for _ in 0..100 {
                    Timer::after(Duration::from_millis(50)).await;
                    let restored = update_settings_window(
                        cx,
                        &settings_window,
                        |view, _, cx| -> Result<bool, String> {
                            let coordinator = cx.global::<ProductCoordinator>();
                            if coordinator.single_instance_wakes == 0 {
                                return Ok(false);
                            }
                            if coordinator.frame_ticks <= baseline_ticks {
                                return Err(
                                    "frame source stopped while waiting for an instance wake"
                                        .to_owned(),
                                );
                            }
                            if cx.windows().len() != 1 {
                                return Err("instance wake created more than one settings window"
                                    .to_owned());
                            }
                            if view.window_hidden() {
                                return Err(
                                    "instance wake did not show the existing settings window"
                                        .to_owned(),
                                );
                            }
                            if view.snapshot_revision().is_none() {
                                return Err(
                                    "instance wake did not restore a runtime snapshot".to_owned()
                                );
                            }
                            Ok(true)
                        },
                    )
                    .await;
                    match restored {
                        Ok(true) => {
                            if let Err(error) = write_smoke_status(
                                "single-instance wake restored the settings window",
                            ) {
                                record_failure(&smoke_failures, error.to_string());
                            }
                            if let Some(path) = single_instance_result_file.as_deref()
                                && let Err(error) = write_smoke_marker(
                                    path,
                                    "single-instance wake restored the settings window",
                                )
                            {
                                record_failure(&smoke_failures, error.to_string());
                            }
                            request_windows_product_quit(&smoke_shutdown_requested);
                            return;
                        }
                        Ok(false) => {}
                        Err(error) => {
                            record_failure(&smoke_failures, error.to_string());
                            request_windows_product_quit(&smoke_shutdown_requested);
                            return;
                        }
                    }
                }
                record_failure(
                    &smoke_failures,
                    "primary instance did not receive the secondary wake",
                );
                request_windows_product_quit(&smoke_shutdown_requested);
            })
            .detach();
        }

        if !run_options.run_duration.is_zero() {
            #[cfg(target_os = "windows")]
            let quit_shutdown_requested = Arc::clone(&shutdown_requested);
            cx.spawn(async move |_cx| {
                Timer::after(run_options.run_duration).await;
                #[cfg(target_os = "macos")]
                _cx.update(request_product_quit);
                #[cfg(target_os = "windows")]
                request_windows_product_quit(&quit_shutdown_requested);
            })
            .detach();
        }
    });

    // Quitting through GPUI's last-window-closed path (for example closing the
    // update window when it is the last open window) returns from the run loop
    // while detached product watchers (system menu, single instance, smoke
    // probes) still hold their own clones of the accumulator. They observe the
    // app teardown only on their next wake, after the run loop has already
    // returned. Those clones are read-only at this point, so drain the
    // recorded failures through the shared reference instead of panicking.
    let failures = match Arc::try_unwrap(failures) {
        Ok(accumulated) => accumulated
            .into_inner()
            .unwrap_or_else(std::sync::PoisonError::into_inner),
        Err(shared) => shared
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone(),
    };
    if failures.is_empty() {
        Ok(())
    } else {
        Err(Box::new(ProductRunError { failures }))
    }
}

#[cfg(any(target_os = "macos", target_os = "windows"))]
fn preset_root() -> PathBuf {
    if let Ok(executable) = env::current_exe()
        && let Some(root) = bundled_preset_root(&executable)
        && root.is_dir()
    {
        return root;
    }
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .expect("repository root")
        .join("resources/models")
}

#[cfg(target_os = "macos")]
fn bundled_preset_root(executable: &Path) -> Option<PathBuf> {
    let macos = executable.parent()?;
    if macos.file_name()?.to_str()? != "MacOS" {
        return None;
    }
    let contents = macos.parent()?;
    if contents.file_name()?.to_str()? != "Contents" {
        return None;
    }
    Some(contents.join("Resources/models"))
}

#[cfg(target_os = "windows")]
fn bundled_preset_root(executable: &Path) -> Option<PathBuf> {
    executable_relative_preset_root(executable)
}

#[cfg(any(target_os = "windows", all(test, target_os = "macos")))]
fn executable_relative_preset_root(executable: &Path) -> Option<PathBuf> {
    Some(executable.parent()?.join("resources/models"))
}

#[cfg(not(any(target_os = "macos", target_os = "windows")))]
fn preset_root() -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .expect("repository root")
        .join("resources/models")
}

#[cfg(not(any(target_os = "macos", target_os = "windows")))]
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut application = bongocat_app::Application::start(preset_root())?;
    application.install_process_panic_hook();
    application.shutdown()?;
    Ok(())
}

#[cfg(all(test, any(target_os = "macos", target_os = "windows")))]
mod tests {
    use super::*;

    #[test]
    fn persisted_theme_resolves_for_process_startup_without_a_settings_window() {
        assert_eq!(
            native_theme_for_startup(bongocat_config::Theme::System),
            None
        );
        assert_eq!(
            native_theme_for_startup(bongocat_config::Theme::Light),
            Some(bongocat_platform::AppTheme::Light)
        );
        assert_eq!(
            native_theme_for_startup(bongocat_config::Theme::Dark),
            Some(bongocat_platform::AppTheme::Dark)
        );
    }

    #[test]
    fn overlay_placement_debouncer_coalesces_drag_updates_and_flushes_latest() {
        let origin = Instant::now();
        let first = OverlayWindowBounds::new(0, 0, 420, 560);
        let middle = OverlayWindowBounds::new(12, 8, 420, 560);
        let latest = OverlayWindowBounds::new(24, 16, 420, 560);
        let mut debouncer = OverlayPlacementDebouncer::default();

        assert_eq!(debouncer.observe(first, origin), Some(first));
        debouncer.mark_sent(first);
        assert_eq!(
            debouncer.observe(middle, origin + Duration::from_millis(50)),
            None
        );
        assert_eq!(
            debouncer.observe(latest, origin + Duration::from_millis(100)),
            None
        );
        assert_eq!(
            debouncer.observe(latest, origin + Duration::from_millis(150)),
            Some(latest)
        );
        debouncer.mark_sent(latest);
        assert_eq!(
            debouncer.flush(origin + Duration::from_millis(200)),
            None,
            "the stable update was already submitted"
        );
    }

    #[test]
    fn overlay_placement_debouncer_flushes_pending_update_on_shutdown() {
        let origin = Instant::now();
        let first = OverlayWindowBounds::new(0, 0, 420, 560);
        let latest = OverlayWindowBounds::new(24, 16, 420, 560);
        let mut debouncer = OverlayPlacementDebouncer::default();

        assert_eq!(debouncer.observe(first, origin), Some(first));
        debouncer.mark_sent(first);
        assert_eq!(
            debouncer.observe(latest, origin + Duration::from_millis(25)),
            None
        );
        assert_eq!(
            debouncer.flush(origin + Duration::from_millis(30)),
            Some(latest)
        );
        debouncer.mark_sent(latest);
        assert_eq!(debouncer.flush(origin + Duration::from_millis(31)), None);
    }

    #[test]
    fn overlay_placement_debouncer_keeps_unsent_value_for_retry() {
        let origin = Instant::now();
        let first = OverlayWindowBounds::new(0, 0, 420, 560);
        let latest = OverlayWindowBounds::new(24, 16, 420, 560);
        let mut debouncer = OverlayPlacementDebouncer::default();

        assert_eq!(debouncer.observe(first, origin), Some(first));
        // Simulate a full settings queue: the producer did not acknowledge either send.
        assert_eq!(
            debouncer.observe(latest, origin + Duration::from_millis(25)),
            None
        );
        assert_eq!(
            debouncer.flush(origin + Duration::from_millis(30)),
            Some(latest)
        );
        // A failed shutdown send must leave the latest bounds available for a retry.
        assert_eq!(
            debouncer.flush(origin + Duration::from_millis(31)),
            Some(latest)
        );
        debouncer.mark_sent(latest);
        assert_eq!(debouncer.flush(origin + Duration::from_millis(32)), None);
    }

    #[test]
    fn run_options_default_to_an_unbounded_product_lifetime() {
        let options = RunOptions::parse(Vec::new()).expect("default options");
        assert_eq!(
            options,
            RunOptions {
                run_duration: Duration::ZERO,
                automated_verification: false,
                #[cfg(target_os = "windows")]
                single_instance_ready_file: None,
                #[cfg(target_os = "windows")]
                single_instance_result_file: None,
                settings_window_smoke: false,
                settings_window_open_smoke: false,
                models_page_smoke: false,
                hidden_model_switch_smoke: false,
                #[cfg(feature = "storage-test-injection")]
                settings_window_state_smoke: false,
                #[cfg(feature = "storage-test-injection")]
                panic_diagnostics_smoke: false,
                #[cfg(feature = "storage-test-injection")]
                panic_diagnostics_smoke_child: false,
                #[cfg(feature = "storage-test-injection")]
                diagnostics_export_smoke: false,
                #[cfg(feature = "storage-test-injection")]
                diagnostics_export_failure_smoke: false,
                system_menu_smoke: false,
                startup_permission_smoke: false,
                #[cfg(target_os = "macos")]
                application_reopen_smoke: false,
                #[cfg(target_os = "macos")]
                startup_item_smoke: false,
                #[cfg(target_os = "windows")]
                single_instance_smoke: false,
            }
        );
        assert!(!options.opens_settings_window_on_start());
    }

    #[test]
    fn positive_seconds_select_a_bounded_diagnostic_run() {
        assert_eq!(
            RunOptions::parse(["--run-seconds".to_owned(), "30".to_owned()])
                .expect("bounded options")
                .run_duration,
            Duration::from_secs(30)
        );
    }

    #[test]
    fn zero_seconds_remains_an_explicit_unbounded_run() {
        assert_eq!(
            RunOptions::parse(["--run-seconds".to_owned(), "0".to_owned()])
                .expect("explicit unbounded options")
                .run_duration,
            Duration::ZERO
        );
    }

    #[test]
    fn settings_window_smoke_is_opt_in() {
        let options = RunOptions::parse([
            "--settings-window-smoke".to_owned(),
            "--run-seconds".to_owned(),
            "4".to_owned(),
        ])
        .expect("settings window smoke options");
        assert!(options.settings_window_smoke);
        assert!(!options.models_page_smoke);
        assert!(!options.hidden_model_switch_smoke);
        assert_eq!(options.run_duration, Duration::from_secs(4));
        assert!(options.opens_settings_window_on_start());
    }

    #[test]
    fn settings_window_open_smoke_only_opens_the_window() {
        let options = RunOptions::parse(["--settings-window-open-smoke".to_owned()])
            .expect("settings window open smoke options");
        assert!(options.settings_window_open_smoke);
        assert!(!options.settings_window_smoke);
        assert!(!options.models_page_smoke);
        assert!(!options.hidden_model_switch_smoke);
        assert!(options.opens_settings_window_on_start());
    }

    #[test]
    fn models_page_smoke_is_opt_in() {
        let options = RunOptions::parse(["--models-page-smoke".to_owned()])
            .expect("models page smoke options");
        assert!(options.models_page_smoke);
        assert!(options.settings_window_smoke);
        assert!(!options.hidden_model_switch_smoke);
        assert!(!options.system_menu_smoke);
        #[cfg(target_os = "macos")]
        assert!(!options.application_reopen_smoke);
        #[cfg(target_os = "macos")]
        assert!(!options.startup_item_smoke);
        #[cfg(target_os = "windows")]
        assert!(!options.single_instance_smoke);
    }

    #[test]
    fn hidden_model_switch_smoke_is_opt_in() {
        let options = RunOptions::parse(["--hidden-model-switch-smoke".to_owned()])
            .expect("hidden model switch smoke options");
        assert!(options.hidden_model_switch_smoke);
        assert!(!options.settings_window_smoke);
        assert!(!options.models_page_smoke);
    }

    #[test]
    fn frame_source_shutdown_acknowledges_only_after_the_run_guard_drops() {
        let shutdown = FrameSourceShutdown::default();
        let guard = shutdown.run_guard();

        shutdown.request_stop();
        assert!(shutdown.stop_requested());
        assert!(!shutdown.is_stopped());

        drop(guard);
        assert!(shutdown.is_stopped());
    }

    #[cfg(feature = "storage-test-injection")]
    #[test]
    fn settings_window_state_smoke_is_opt_in() {
        let options = RunOptions::parse(["--settings-window-state-smoke".to_owned()])
            .expect("settings window state smoke options");
        assert!(options.settings_window_state_smoke);
        assert!(!options.settings_window_smoke);
    }

    #[cfg(feature = "storage-test-injection")]
    #[test]
    fn panic_diagnostics_smoke_and_private_child_are_opt_in() {
        let options = RunOptions::parse(["--panic-diagnostics-smoke".to_owned()])
            .expect("panic diagnostics smoke options");
        assert!(options.panic_diagnostics_smoke);
        assert!(!options.panic_diagnostics_smoke_child);
        assert!(usage().contains("panic-diagnostics-smoke"));
        assert!(!usage().contains("panic-diagnostics-smoke-child"));

        let child = RunOptions::parse(["--panic-diagnostics-smoke-child".to_owned()])
            .expect("panic diagnostics child options");
        assert!(!child.panic_diagnostics_smoke);
        assert!(child.panic_diagnostics_smoke_child);
    }

    #[cfg(feature = "storage-test-injection")]
    #[test]
    fn diagnostics_export_smoke_is_opt_in() {
        let options = RunOptions::parse(["--diagnostics-export-smoke".to_owned()])
            .expect("diagnostics export smoke options");
        assert!(options.diagnostics_export_smoke);
        assert!(!options.settings_window_smoke);
        assert!(!options.panic_diagnostics_smoke);
        assert!(usage().contains("diagnostics-export-smoke"));
    }

    #[cfg(feature = "storage-test-injection")]
    #[test]
    fn diagnostics_export_failure_smoke_is_opt_in() {
        let options = RunOptions::parse(["--diagnostics-export-failure-smoke".to_owned()])
            .expect("diagnostics export failure smoke options");
        assert!(options.diagnostics_export_failure_smoke);
        assert!(!options.diagnostics_export_smoke);
        assert!(usage().contains("diagnostics-export-failure-smoke"));
    }

    #[test]
    fn startup_permission_smoke_is_opt_in_and_non_interactive() {
        let options = RunOptions::parse(["--startup-permission-smoke".to_owned()])
            .expect("startup permission smoke options");
        assert!(options.startup_permission_smoke);
        assert!(!options.settings_window_smoke);
        assert!(!options.opens_settings_window_on_start());
        assert!(options.automated_verification);
        assert!(usage().contains("startup-permission-smoke"));
    }

    #[test]
    fn only_the_bounded_run_duration_keeps_a_start_interactive() {
        let product = RunOptions::parse(["--run-seconds".to_owned(), "0".to_owned()])
            .expect("product run options");
        assert!(!product.automated_verification);
        assert!(!product.settings_window_smoke);

        // Every other accepted argument selects a harness, so the startup permission prompt stays
        // out of automated runs.
        for arguments in [
            vec!["--settings-window-smoke".to_owned()],
            vec!["--system-menu-smoke".to_owned()],
            vec![
                "--run-seconds".to_owned(),
                "4".to_owned(),
                "--settings-window-smoke".to_owned(),
            ],
        ] {
            assert!(
                RunOptions::parse(arguments.clone())
                    .expect("harness run options")
                    .automated_verification,
                "{arguments:?}"
            );
        }
    }

    #[cfg(not(feature = "storage-test-injection"))]
    #[test]
    fn product_options_reject_storage_test_injection() {
        let state_error = RunOptions::parse(["--settings-window-state-smoke".to_owned()])
            .expect_err("default product options must reject state storage injection");
        assert!(state_error.message.contains("unknown argument"));
        assert!(!usage().contains("settings-window-state-smoke"));
        let panic_error = RunOptions::parse(["--panic-diagnostics-smoke".to_owned()])
            .expect_err("default product options must reject panic storage injection");
        assert!(panic_error.message.contains("unknown argument"));
        assert!(!usage().contains("panic-diagnostics-smoke"));
        let diagnostics_error = RunOptions::parse(["--diagnostics-export-smoke".to_owned()])
            .expect_err("default product options must reject diagnostics storage injection");
        assert!(diagnostics_error.message.contains("unknown argument"));
        assert!(!usage().contains("diagnostics-export-smoke"));
        let child_error = RunOptions::parse(["--panic-diagnostics-smoke-child".to_owned()])
            .expect_err("default product options must reject panic child injection");
        assert!(child_error.message.contains("unknown argument"));
    }

    #[test]
    fn system_menu_smoke_is_opt_in() {
        let options = RunOptions::parse(["--system-menu-smoke".to_owned()])
            .expect("system menu smoke options");
        assert!(options.system_menu_smoke);
        assert!(!options.settings_window_smoke);
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn application_reopen_smoke_is_opt_in() {
        let options = RunOptions::parse(["--application-reopen-smoke".to_owned()])
            .expect("application-reopen smoke options");
        assert!(options.application_reopen_smoke);
        assert!(!options.settings_window_smoke);
        assert!(options.opens_settings_window_on_start());
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn startup_item_smoke_is_opt_in() {
        let options = RunOptions::parse(["--startup-item-smoke".to_owned()])
            .expect("startup-item smoke options");
        assert!(options.startup_item_smoke);
        assert!(!options.settings_window_smoke);
        assert!(!options.application_reopen_smoke);
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn bundled_preset_models_resolve_from_contents_resources() {
        assert_eq!(
            bundled_preset_root(Path::new(
                "/Applications/BongoCat.app/Contents/MacOS/bongocat-app"
            )),
            Some(PathBuf::from(
                "/Applications/BongoCat.app/Contents/Resources/models"
            ))
        );
        assert_eq!(
            bundled_preset_root(Path::new("/tmp/target/release/bongocat-app")),
            None
        );
    }

    #[test]
    fn executable_relative_preset_models_resolve_next_to_a_product_executable() {
        assert_eq!(
            executable_relative_preset_root(Path::new("/Applications/BongoCat/bongocat-app.exe")),
            Some(PathBuf::from("/Applications/BongoCat/resources/models"))
        );
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn single_instance_smoke_is_opt_in() {
        let options = RunOptions::parse(["--single-instance-smoke".to_owned()])
            .expect("single-instance smoke options");
        assert!(options.single_instance_smoke);
        assert!(!options.settings_window_smoke);
        assert!(options.opens_settings_window_on_start());
        assert_eq!(options.single_instance_ready_file, None);
        assert_eq!(options.single_instance_result_file, None);
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn single_instance_marker_options_are_parsed_with_the_smoke_flag() {
        let options = RunOptions::parse([
            "--single-instance-smoke".to_owned(),
            "--single-instance-ready-file".to_owned(),
            r"C:\runner\primary.ready".to_owned(),
            "--single-instance-result-file".to_owned(),
            r"C:\runner\primary.result".to_owned(),
        ])
        .expect("single-instance marker options");
        assert_eq!(
            options.single_instance_ready_file,
            Some(PathBuf::from(r"C:\runner\primary.ready"))
        );
        assert_eq!(
            options.single_instance_result_file,
            Some(PathBuf::from(r"C:\runner\primary.result"))
        );
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn single_instance_marker_options_require_the_smoke_flag() {
        for arguments in [
            vec![
                "--single-instance-ready-file".to_owned(),
                r"C:\runner\primary.ready".to_owned(),
            ],
            vec![
                "--single-instance-result-file".to_owned(),
                r"C:\runner\primary.result".to_owned(),
            ],
        ] {
            assert!(RunOptions::parse(arguments).is_err());
        }
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn single_instance_marker_options_require_non_empty_values() {
        for arguments in [
            vec![
                "--single-instance-smoke".to_owned(),
                "--single-instance-ready-file".to_owned(),
            ],
            vec![
                "--single-instance-smoke".to_owned(),
                "--single-instance-result-file".to_owned(),
            ],
            vec![
                "--single-instance-smoke".to_owned(),
                "--single-instance-ready-file".to_owned(),
                String::new(),
            ],
            vec![
                "--single-instance-smoke".to_owned(),
                "--single-instance-result-file".to_owned(),
                String::new(),
            ],
        ] {
            assert!(RunOptions::parse(arguments).is_err());
        }
    }

    /// The restart waits long enough to be seen, then happens whether or not the
    /// window is still open.
    #[cfg(target_os = "macos")]
    #[test]
    fn the_post_install_restart_waits_for_the_delay_then_fires() {
        let observed_at = Instant::now();
        assert!(!restart_delay_elapsed(observed_at, observed_at));
        assert!(!restart_delay_elapsed(
            observed_at,
            observed_at + UPDATE_RESTART_DELAY - Duration::from_millis(1)
        ));
        assert!(restart_delay_elapsed(
            observed_at,
            observed_at + UPDATE_RESTART_DELAY
        ));
        // A monotonic clock that appears to move backwards must not restart early.
        assert!(!restart_delay_elapsed(
            observed_at + UPDATE_RESTART_DELAY,
            observed_at
        ));
    }

    #[test]
    fn run_options_reject_missing_invalid_and_unknown_values() {
        for arguments in [
            vec!["--run-seconds".to_owned()],
            vec!["--run-seconds".to_owned(), "-1".to_owned()],
            vec!["--model".to_owned(), "standard".to_owned()],
            vec!["--environment".to_owned(), "production".to_owned()],
            vec!["--BONGOCAT_BUILD_ENV=production".to_owned()],
            vec!["--storage-root".to_owned(), "/production".to_owned()],
        ] {
            assert!(RunOptions::parse(arguments).is_err());
        }
    }
}
