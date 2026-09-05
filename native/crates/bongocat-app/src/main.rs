#![forbid(unsafe_code)]

#[cfg(any(target_os = "macos", target_os = "windows"))]
use async_io::Timer;
#[cfg(any(target_os = "macos", target_os = "windows"))]
use bongocat_live2d::CoreLogHandle;
#[cfg(any(target_os = "macos", target_os = "windows"))]
use bongocat_overlay::{OverlaySessionOptions, OverlayWindowBounds, ProductOverlaySession};
#[cfg(any(target_os = "macos", target_os = "windows"))]
use bongocat_platform::ShortcutDispatcher;
#[cfg(target_os = "windows")]
use bongocat_platform::{
    SingleInstance, SingleInstanceAction, SingleInstanceEnvironment, SingleInstanceStart,
};
#[cfg(any(target_os = "macos", target_os = "windows"))]
use bongocat_platform::{SystemMenu, SystemMenuAction};
#[cfg(target_os = "windows")]
use bongocat_ui::SettingsView;
#[cfg(any(target_os = "macos", target_os = "windows"))]
use bongocat_ui::{
    SettingsError, SettingsErrorCode, SettingsModelAvailability, SettingsModelKey,
    SettingsModelOrigin, SettingsWindowHandle, open_settings_window,
};
#[cfg(any(target_os = "macos", target_os = "windows"))]
use gpui_kit::{
    App, Application as GpuiApplication, Global, assets::Assets, platform::current_platform,
};
#[cfg(target_os = "windows")]
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
    sync::{Arc, Mutex},
    time::Duration,
};

#[cfg(any(target_os = "macos", target_os = "windows"))]
struct StatusIconRequest {
    visible: bool,
    reply: std::sync::mpsc::SyncSender<Result<(), SettingsError>>,
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
    // SettingsAccessibilityBridge owns the window's AccessKit adapter. Current GPUI also
    // installs one by default, but two adapters cannot subclass the same native view.
    GpuiApplication::new_inaccessible(current_platform(false))
}

#[cfg(any(target_os = "macos", target_os = "windows"))]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct RunOptions {
    run_duration: Duration,
    settings_window_smoke: bool,
    models_page_smoke: bool,
    hidden_model_switch_smoke: bool,
    #[cfg(feature = "storage-test-injection")]
    configuration_recovery_smoke: bool,
    #[cfg(feature = "storage-test-injection")]
    settings_window_state_smoke: bool,
    #[cfg(feature = "storage-test-injection")]
    panic_diagnostics_smoke: bool,
    #[cfg(feature = "storage-test-injection")]
    panic_diagnostics_smoke_child: bool,
    system_menu_smoke: bool,
    #[cfg(target_os = "macos")]
    application_reopen_smoke: bool,
    #[cfg(target_os = "macos")]
    startup_item_smoke: bool,
    #[cfg(target_os = "windows")]
    single_instance_smoke: bool,
}

#[cfg(any(target_os = "macos", target_os = "windows"))]
impl RunOptions {
    fn parse(arguments: impl IntoIterator<Item = String>) -> Result<Self, RunOptionsError> {
        let mut arguments = arguments.into_iter();
        let mut run_seconds = DEFAULT_RUN_SECONDS;
        let mut settings_window_smoke = false;
        let mut models_page_smoke = false;
        let mut hidden_model_switch_smoke = false;
        #[cfg(feature = "storage-test-injection")]
        let mut configuration_recovery_smoke = false;
        #[cfg(feature = "storage-test-injection")]
        let mut settings_window_state_smoke = false;
        #[cfg(feature = "storage-test-injection")]
        let mut panic_diagnostics_smoke = false;
        #[cfg(feature = "storage-test-injection")]
        let mut panic_diagnostics_smoke_child = false;
        let mut system_menu_smoke = false;
        #[cfg(target_os = "macos")]
        let mut application_reopen_smoke = false;
        #[cfg(target_os = "macos")]
        let mut startup_item_smoke = false;
        #[cfg(target_os = "windows")]
        let mut single_instance_smoke = false;
        while let Some(argument) = arguments.next() {
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
                "--models-page-smoke" => {
                    models_page_smoke = true;
                    settings_window_smoke = true;
                }
                "--hidden-model-switch-smoke" => hidden_model_switch_smoke = true,
                #[cfg(feature = "storage-test-injection")]
                "--configuration-recovery-smoke" => configuration_recovery_smoke = true,
                #[cfg(feature = "storage-test-injection")]
                "--settings-window-state-smoke" => settings_window_state_smoke = true,
                #[cfg(feature = "storage-test-injection")]
                "--panic-diagnostics-smoke" => panic_diagnostics_smoke = true,
                #[cfg(feature = "storage-test-injection")]
                "--panic-diagnostics-smoke-child" => panic_diagnostics_smoke_child = true,
                "--system-menu-smoke" => system_menu_smoke = true,
                #[cfg(target_os = "macos")]
                "--application-reopen-smoke" => application_reopen_smoke = true,
                #[cfg(target_os = "macos")]
                "--startup-item-smoke" => startup_item_smoke = true,
                #[cfg(target_os = "windows")]
                "--single-instance-smoke" => single_instance_smoke = true,
                "--help" | "-h" => return Err(RunOptionsError::help()),
                _ => {
                    return Err(RunOptionsError::new(format!(
                        "unknown argument {argument:?}"
                    )));
                }
            }
        }
        Ok(Self {
            run_duration: Duration::from_secs(run_seconds),
            settings_window_smoke,
            models_page_smoke,
            hidden_model_switch_smoke,
            #[cfg(feature = "storage-test-injection")]
            configuration_recovery_smoke,
            #[cfg(feature = "storage-test-injection")]
            settings_window_state_smoke,
            #[cfg(feature = "storage-test-injection")]
            panic_diagnostics_smoke,
            #[cfg(feature = "storage-test-injection")]
            panic_diagnostics_smoke_child,
            system_menu_smoke,
            #[cfg(target_os = "macos")]
            application_reopen_smoke,
            #[cfg(target_os = "macos")]
            startup_item_smoke,
            #[cfg(target_os = "windows")]
            single_instance_smoke,
        })
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
    return "Usage: bongocat-app [--run-seconds <seconds>] [--settings-window-smoke] [--models-page-smoke] [--hidden-model-switch-smoke] [--configuration-recovery-smoke] [--settings-window-state-smoke] [--panic-diagnostics-smoke] [--system-menu-smoke] [--single-instance-smoke]\n\nThe application runs until it is explicitly quit by default. A positive value enables a bounded diagnostic run.";

    #[cfg(all(target_os = "windows", not(feature = "storage-test-injection")))]
    return "Usage: bongocat-app [--run-seconds <seconds>] [--settings-window-smoke] [--models-page-smoke] [--hidden-model-switch-smoke] [--system-menu-smoke] [--single-instance-smoke]\n\nThe application runs until it is explicitly quit by default. A positive value enables a bounded diagnostic run.";

    #[cfg(all(target_os = "macos", feature = "storage-test-injection"))]
    return "Usage: bongocat-app [--run-seconds <seconds>] [--settings-window-smoke] [--models-page-smoke] [--hidden-model-switch-smoke] [--configuration-recovery-smoke] [--settings-window-state-smoke] [--panic-diagnostics-smoke] [--system-menu-smoke] [--application-reopen-smoke] [--startup-item-smoke]\n\nThe application runs until it is explicitly quit by default. A positive value enables a bounded diagnostic run.";

    #[cfg(all(target_os = "macos", not(feature = "storage-test-injection")))]
    "Usage: bongocat-app [--run-seconds <seconds>] [--settings-window-smoke] [--models-page-smoke] [--hidden-model-switch-smoke] [--system-menu-smoke] [--application-reopen-smoke] [--startup-item-smoke]\n\nThe application runs until it is explicitly quit by default. A positive value enables a bounded diagnostic run."
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
    frame_ticks: u64,
    expect_visible_frame: bool,
    failures: Arc<Mutex<Vec<String>>>,
    #[cfg(target_os = "windows")]
    shutdown_requested: Arc<AtomicBool>,
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

#[cfg(target_os = "windows")]
async fn update_windows_settings<R>(
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
fn request_product_quit(cx: &mut App) {
    #[cfg(target_os = "macos")]
    cx.quit();

    #[cfg(target_os = "windows")]
    {
        if let Some(coordinator) = cx.try_global::<ProductCoordinator>() {
            coordinator
                .shutdown_requested
                .store(true, Ordering::Release);
        }
    }
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
}

#[cfg(any(target_os = "macos", target_os = "windows"))]
impl ProductShutdown {
    async fn finish(self) -> Arc<Mutex<Vec<String>>> {
        let failures = Arc::clone(&self.coordinator.failures);
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
            Ok(report) if !report.work_area_constraint_satisfied => {
                record_failure(
                    &failures,
                    "product overlay escaped the configured work area",
                );
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
    if let Err(error) = overlay.stop_input() {
        record_failure(&coordinator.failures, error.to_string());
    }
    let settings_service = coordinator
        .settings_service
        .take()
        .expect("settings service owner is present");
    ProductShutdown {
        coordinator,
        overlay,
        settings_service,
    }
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
        request_product_quit,
        cx,
    )?;
    cx.global_mut::<ProductCoordinator>().settings_window = Some(window_handle.clone());
    Ok(window_handle)
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
fn handle_shortcut_open_settings(cx: &mut App) {
    let requested = cx
        .try_global::<ProductCoordinator>()
        .is_some_and(|coordinator| coordinator.shortcut_signals.take_open_settings_request());
    if !requested {
        return;
    }
    if let Err(error) = ensure_settings_window(cx)
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

#[cfg(any(target_os = "macos", target_os = "windows"))]
fn run_configuration_recovery_mode(
    application: bongocat_app::Application,
) -> Result<(), Box<dyn std::error::Error>> {
    let settings_service = bongocat_app::ApplicationSettingsService::start(application)?;
    let settings_client = settings_service.client();
    let window_state = settings_service.window_state();
    let gpui_application = gpui_application().with_assets(Assets);
    gpui_application.run(move |cx| {
        if let Err(error) = open_settings_window(
            settings_client.clone(),
            window_state.clone(),
            true,
            |cx| cx.quit(),
            cx,
        ) {
            let mut stderr = io::stderr().lock();
            let _ = writeln!(stderr, "configuration recovery window failed: {error}");
            let _ = stderr.flush();
            cx.quit();
        }
    });
    let client = settings_service.client();
    let _ = client.shutdown_blocking();
    settings_service.join()?;
    Ok(())
}

#[cfg(all(
    feature = "storage-test-injection",
    any(target_os = "macos", target_os = "windows")
))]
struct RecoverySmokeRoot(PathBuf);

#[cfg(all(
    feature = "storage-test-injection",
    any(target_os = "macos", target_os = "windows")
))]
impl RecoverySmokeRoot {
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
impl Drop for RecoverySmokeRoot {
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
fn run_configuration_recovery_smoke() -> Result<(), Box<dyn std::error::Error>> {
    use bongocat_config::{BuildEnvironment, ConfigStore, StorageLayout};

    let root = env::temp_dir().join(format!(
        "bongocat-recovery-window-smoke-{}",
        std::process::id()
    ));
    if root.exists() {
        std::fs::remove_dir_all(&root)?;
    }
    let root = RecoverySmokeRoot(root);
    let layout = StorageLayout::under(&root.0, BuildEnvironment::Development);
    let _store = ConfigStore::new(layout.clone())?;
    std::fs::write(&layout.config, b"corrupt-current-without-backups")?;
    let application =
        bongocat_app::Application::start_with_layout_for_smoke(layout, preset_root())?;
    if application.is_operational() {
        return Err("recovery smoke unexpectedly started an operational application".into());
    }
    write_smoke_status("configuration recovery required")?;
    let service = bongocat_app::ApplicationSettingsService::start(application)?;
    let client = service.client();
    let window_state = service.window_state();
    let snapshot = client.read_snapshot_blocking()?;
    if !matches!(
        snapshot.configuration_status,
        bongocat_ui::SettingsConfigurationStatus::RecoveryRequired { checked_backups: 0 }
    ) {
        return Err("recovery smoke did not project the expected recovery snapshot".into());
    }
    let gpui_application = gpui_application().with_assets(Assets);
    let smoke_client = client.clone();
    gpui_application.run(move |cx| {
        let window =
            match open_settings_window(smoke_client, window_state, true, |cx| cx.quit(), cx) {
                Ok(window) => window,
                Err(error) => {
                    let _ = write_smoke_status(&format!("recovery window failed: {error}"));
                    cx.quit();
                    return;
                }
            };
        let _ = write_smoke_status("recovery window opened");
        cx.spawn(async move |cx| {
            let mut diagnostics_verified = false;
            for _ in 0..200 {
                let diagnostics =
                    window.update(cx, |view, _, cx| view.show_diagnostics_page_for_smoke(cx));
                if matches!(diagnostics, Ok(Ok(()))) {
                    diagnostics_verified = true;
                    break;
                }
                Timer::after(Duration::from_millis(10)).await;
            }
            if diagnostics_verified {
                let _ = write_smoke_status("recovery diagnostics verified");
            }
            Timer::after(Duration::from_millis(1000)).await;
            let shutdown = client.shutdown().await;
            let joined = service.join();
            let cleanup = root.cleanup();
            if shutdown.is_ok() && joined.is_ok() && cleanup.is_ok() {
                let _ = write_smoke_status("recovery service stopped");
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
    let root = RecoverySmokeRoot(root);
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
    let gpui_application = gpui_application().with_assets(Assets);
    gpui_application.run(move |cx| {
        let window =
            match open_settings_window(
                client.clone(),
                window_state.clone(),
                true,
                |cx| cx.quit(),
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
                let mut diagnostics_verified = false;
                let mut last_diagnostics_error = None;
                for _ in 0..200 {
                    let diagnostics = window.update(cx, |view, _, cx| {
                        view.show_diagnostics_page_for_smoke(cx)
                    });
                    match diagnostics {
                        Ok(Ok(())) => {
                            diagnostics_verified = true;
                            break;
                        }
                        Ok(Err(error)) => last_diagnostics_error = Some(error),
                        Err(error) => last_diagnostics_error = Some(error.to_string()),
                    }
                    Timer::after(Duration::from_millis(10)).await;
                }
                if !diagnostics_verified {
                    let detail = last_diagnostics_error
                        .unwrap_or_else(|| "settings view was unavailable".to_owned());
                    return Err(io::Error::other(format!(
                        "settings window did not apply Diagnostics localization: {detail}"
                    ))
                    .into());
                }
                write_smoke_status("Chinese Diagnostics localization verified")?;
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
    let root = RecoverySmokeRoot(root);
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
    #[cfg(feature = "storage-test-injection")]
    if run_options.configuration_recovery_smoke {
        return run_configuration_recovery_smoke();
    }
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
    #[cfg(target_os = "macos")]
    if run_options.startup_item_smoke {
        return run_startup_item_smoke();
    }
    #[cfg(target_os = "windows")]
    let single_instance = match SingleInstance::acquire(build_single_instance_environment())? {
        SingleInstanceStart::Primary(single_instance) => single_instance,
        SingleInstanceStart::SecondaryNotified => {
            write_smoke_status("secondary instance notified primary")?;
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
        }
    });
    if !application.is_operational() {
        return run_configuration_recovery_mode(application);
    }

    let (model_origin, model_id) = match (
        application.config().model.selected_model_origin,
        application.config().model.selected_model_id.clone(),
    ) {
        (Some(bongocat_config::SelectedModelOrigin::Preset), Some(id)) => {
            (bongocat_model::ModelOrigin::Preset, id)
        }
        (Some(bongocat_config::SelectedModelOrigin::Installed), Some(id)) => {
            (bongocat_model::ModelOrigin::Installed, id)
        }
        (None, None) => (bongocat_model::ModelOrigin::Preset, "standard".to_owned()),
        _ => unreachable!("validated model selection is paired"),
    };
    let overlay_options = OverlaySessionOptions {
        click_through: application.config().overlay.click_through,
        always_on_top: application.config().overlay.always_on_top,
        scale_percent: application.config().overlay.scale_percent,
        opacity_percent: application.config().overlay.opacity_percent,
        keep_inside_work_area: application.config().overlay.keep_inside_work_area,
        maximum_fps: application.config().model.maximum_fps,
        window_bounds: application.overlay_window_placement().map(|placement| {
            OverlayWindowBounds::new(placement.x, placement.y, placement.width, placement.height)
        }),
    };
    application.prepare_model(model_origin, model_id)?;
    let runtime_client = application.runtime_client();
    let (shortcut_sender, shortcut_receiver) = std::sync::mpsc::sync_channel(64);
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
    let initial_taskbar_icon_visible = application.config().application.show_taskbar_icon;
    let shortcut_signals = bongocat_app::ApplicationShortcutSignals::default();
    let shortcut_dispatcher = Some(ShortcutDispatcher::with_application_sink(
        application.shortcut_table(),
        runtime_client.clone(),
        shortcut_sender,
    ));
    let input_producer = application.input_producer();
    let cursor_producer = application.cursor_producer();
    let gamepad_axis_producer = application.gamepad_axis_producer();
    let render_consumer = application.take_render_consumer()?;
    let expect_visible_frame = application.config().overlay.visible;
    let frame_runtime_client = runtime_client.clone();
    let frame_source_shutdown = FrameSourceShutdown::default();
    let failures = Arc::new(Mutex::new(Vec::new()));
    let run_failures = Arc::clone(&failures);
    #[cfg(target_os = "windows")]
    let shutdown_requested = Arc::new(AtomicBool::new(false));
    let gpui_application = gpui_application().with_assets(Assets);
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
        let overlay = match ProductOverlaySession::start_with_shortcuts(
            runtime_client,
            input_producer,
            cursor_producer,
            gamepad_axis_producer,
            render_consumer,
            overlay_options,
            shortcut_dispatcher,
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
        let system_menu = match SystemMenu::start_with_visibility(initial_status_icon_visible) {
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
        let window_state = settings_service.window_state();
        let settings_window = match open_settings_window(
            settings_client.clone(),
            window_state,
            initial_taskbar_icon_visible,
            request_product_quit,
            cx,
        ) {
            Ok(window) => window,
            Err(error) => {
                record_failure(&run_failures, error);
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
            settings_window: Some(settings_window.clone()),
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
            frame_ticks: 0,
            expect_visible_frame,
            failures: Arc::clone(&run_failures),
            #[cfg(target_os = "windows")]
            shutdown_requested: Arc::clone(&shutdown_requested),
        });

        cx.on_window_closed(|cx, _| {
            let Some(window_handle) = cx
                .try_global::<ProductCoordinator>()
                .and_then(|coordinator| coordinator.settings_window.clone())
            else {
                return;
            };
            if window_handle.read(cx).is_err() {
                cx.global_mut::<ProductCoordinator>().settings_window = None;
            }
        })
        .detach();

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
            async move {
                if let Some(shutdown) = shutdown {
                    let _ = shutdown.finish().await;
                }
            }
        })
        .detach();

        let system_menu_failures = Arc::clone(&run_failures);
        cx.spawn(async move |cx| {
            loop {
                Timer::after(Duration::from_millis(50)).await;
                while let Ok(request) = status_icon_receiver.try_recv() {
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
                    SystemMenuAction::ToggleOverlayVisibility => {
                        let client = cx.update(|cx| {
                            cx.try_global::<ProductCoordinator>()
                                .and_then(|coordinator| coordinator.settings_service.as_ref())
                                .map(bongocat_app::ApplicationSettingsService::client)
                                .ok_or_else(|| {
                                    "settings service is unavailable for the system menu".to_owned()
                                })
                        });
                        match client {
                            Ok(client) => async {
                                let snapshot = client
                                    .read_snapshot()
                                    .await
                                    .map_err(|error| error.to_string())?;
                                let revision = snapshot.config_revision.ok_or_else(|| {
                                    "system menu cannot update configuration during recovery".to_owned()
                                })?;
                                client
                                    .set_overlay_visible(revision, !snapshot.overlay_visible)
                                    .await
                                    .map(|_| true)
                                    .map_err(|error| error.to_string())
                            }
                            .await,
                            Err(error) => Err(error),
                        }
                    }
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

        #[cfg(target_os = "windows")]
        let frame_window = settings_window.clone();
        #[cfg(target_os = "windows")]
        let frame_failures = Arc::clone(&run_failures);
        #[cfg(target_os = "windows")]
        let frame_shutdown_requested = Arc::clone(&shutdown_requested);
        let frame_settings_client = settings_client.clone();
        let frame_source_guard = frame_source_shutdown.run_guard();
        cx.spawn(async move |cx| {
            let _frame_source_guard = frame_source_guard;
            #[cfg(target_os = "windows")]
            let mut frame_active = true;
            let mut last_overlay_bounds = None;
            let mut retry_delay = None;
            #[cfg(target_os = "windows")]
            let mut update_failure_reported = false;
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
                #[cfg(target_os = "macos")]
                let (keep_running, next_retry_delay) = cx.update(|cx| {
                    if !cx.has_global::<ProductCoordinator>() {
                        return (false, None);
                    }
                    handle_shortcut_open_settings(cx);
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
                        match result {
                            Ok(outcome) => {
                                if let Ok(bounds) = coordinator
                                    .overlay
                                    .as_ref()
                                    .expect("product overlay owner is present")
                                    .window_bounds()
                                    && last_overlay_bounds != Some(bounds)
                                    && frame_settings_client
                                        .update_overlay_window_placement(
                                            bounds.x,
                                            bounds.y,
                                            bounds.width,
                                            bounds.height,
                                        )
                                        .is_ok()
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
                        && frame_settings_client
                            .update_overlay_window_placement(
                                bounds.x,
                                bounds.y,
                                bounds.width,
                                bounds.height,
                            )
                            .is_ok()
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
                let keep_running = update_windows_settings(cx, &frame_window, |view, _, cx| {
                    if !cx.has_global::<ProductCoordinator>() {
                        return Ok(false);
                    }
                    handle_shortcut_open_settings(cx);
                    let (failure, failures) = {
                        let coordinator = cx.global_mut::<ProductCoordinator>();
                        match tick_result
                            .take()
                            .expect("a successful window update invokes the frame closure once")
                        {
                            None => (None, None),
                            Some(Ok(_)) => {
                                coordinator.frame_ticks = coordinator.frame_ticks.saturating_add(1);
                                (None, None)
                            }
                            Some(Err(error)) => {
                                coordinator.frame_source_running = false;
                                (
                                    Some(error.to_string()),
                                    Some(Arc::clone(&coordinator.failures)),
                                )
                            }
                        }
                    };
                    if let (Some(failure), Some(failures)) = (failure, failures) {
                        record_failure(&failures, failure);
                        view.report_service_error(
                            SettingsError::new(SettingsErrorCode::RuntimeUnavailable),
                            cx,
                        );
                    }
                    if frame_shutdown_requested.load(Ordering::Acquire)
                        || system_termination_requested
                    {
                        start_windows_product_shutdown(cx);
                        Ok(false)
                    } else {
                        Ok(true)
                    }
                })
                .await;
                #[cfg(target_os = "windows")]
                let keep_running = match keep_running {
                    Ok(keep_running) => {
                        update_failure_reported = false;
                        keep_running
                    }
                    Err(error) => {
                        if !update_failure_reported {
                            record_failure(&frame_failures, error);
                            update_failure_reported = true;
                        }
                        true
                    }
                };
                #[cfg(target_os = "windows")]
                {
                    retry_delay = next_retry_delay;
                }
                if !keep_running {
                    break;
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
            let smoke_window = settings_window.clone();
            #[cfg(target_os = "windows")]
            let smoke_shutdown_requested = Arc::clone(&shutdown_requested);
            cx.spawn(async move |cx| {
                Timer::after(Duration::from_millis(500)).await;
                #[cfg(target_os = "macos")]
                let settings_pages = cx.update(|cx| -> Result<(), String> {
                    smoke_window
                        .update(cx, |view, _, cx| {
                            view.show_general_page_for_smoke(cx)?;
                            view.show_diagnostics_page_for_smoke(cx)?;
                            view.show_about_page_for_smoke(cx)
                        })
                        .map_err(|error| error.to_string())?
                });
                #[cfg(target_os = "windows")]
                let settings_pages = update_windows_settings(cx, &smoke_window, |view, _, cx| {
                    view.show_general_page_for_smoke(cx)?;
                    view.show_diagnostics_page_for_smoke(cx)?;
                    view.show_about_page_for_smoke(cx)
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
                    #[cfg(target_os = "macos")]
                    let models_page = cx.update(|cx| -> Result<(), String> {
                        smoke_window
                            .update(cx, |view, _, cx| view.show_models_page_for_smoke(cx))
                            .map_err(|error| error.to_string())?
                    });
                    #[cfg(target_os = "windows")]
                    let models_page = update_windows_settings(cx, &smoke_window, |view, _, cx| {
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
                #[cfg(target_os = "macos")]
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
                    #[cfg(target_os = "macos")]
                    window_handle
                        .update(cx, |_, window, _| window.remove_window())
                        .map_err(|error| error.to_string())?;
                    #[cfg(target_os = "windows")]
                    window_handle
                        .update(cx, |_, window, _| {
                            bongocat_platform::request_native_window_close(window)
                        })
                        .map_err(|error| error.to_string())?
                        .map_err(|error| error.to_string())?;
                    Ok((frame_ticks, window_handle))
                });
                #[cfg(target_os = "windows")]
                let baseline = update_windows_settings(
                    cx,
                    &smoke_window,
                    |_, window, cx| -> Result<_, String> {
                        let frame_ticks = cx.global::<ProductCoordinator>().frame_ticks;
                        bongocat_platform::request_native_window_close(window)
                            .map_err(|error| error.to_string())?;
                        Ok((frame_ticks, smoke_window.clone()))
                    },
                )
                .await;
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

                let mut window_unavailable = false;
                for _ in 0..60 {
                    Timer::after(Duration::from_millis(50)).await;
                    #[cfg(target_os = "macos")]
                    let hidden =
                        cx.update(|cx| -> Result<bool, String> { Ok(cx.windows().is_empty()) });
                    #[cfg(target_os = "windows")]
                    let hidden = update_windows_settings(cx, &original_window, |view, _, _| {
                        Ok::<_, String>(view.window_hidden())
                    })
                    .await;
                    match hidden {
                        Ok(true) => {
                            window_unavailable = true;
                            break;
                        }
                        Ok(false) => {}
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
                if !window_unavailable {
                    record_failure(&smoke_failures, "settings window did not close or hide");
                    #[cfg(target_os = "macos")]
                    cx.update(request_product_quit);
                    #[cfg(target_os = "windows")]
                    request_windows_product_quit(&smoke_shutdown_requested);
                    return;
                }

                Timer::after(Duration::from_millis(500)).await;
                #[cfg(target_os = "macos")]
                let reopened = cx.update(|cx| -> Result<SettingsWindowHandle, String> {
                    if cx.global::<ProductCoordinator>().frame_ticks <= baseline_ticks {
                        return Err(
                            "frame source stopped while the settings window was closed".to_owned()
                        );
                    }
                    let reopened = ensure_settings_window(cx)?;
                    if cx.windows().len() != 1 {
                        return Err("settings reopen created more than one window".to_owned());
                    }
                    #[cfg(target_os = "macos")]
                    if reopened == original_window {
                        return Err("settings reopen retained the closed macOS entity".to_owned());
                    }
                    #[cfg(target_os = "windows")]
                    if reopened != original_window {
                        return Err("settings reopen replaced the hidden Windows entity".to_owned());
                    }
                    Ok(reopened)
                });
                #[cfg(target_os = "windows")]
                let reopened = update_windows_settings(
                    cx,
                    &original_window,
                    |view, window, cx| -> Result<SettingsWindowHandle, String> {
                        if cx.global::<ProductCoordinator>().frame_ticks <= baseline_ticks {
                            return Err(
                                "frame source stopped while the settings window was closed"
                                    .to_owned(),
                            );
                        }
                        view.reopen(window, cx)?;
                        if cx.windows().len() != 1 {
                            return Err("settings reopen created more than one window".to_owned());
                        }
                        Ok(original_window.clone())
                    },
                )
                .await;
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
                #[cfg(target_os = "macos")]
                let restored = cx.update(|cx| -> Result<(), String> {
                    let window_handle =
                        cx.global::<ProductCoordinator>()
                            .settings_window
                            .clone()
                            .ok_or_else(|| "settings window was not recreated".to_owned())?;
                    let revision = window_handle
                        .update(cx, |view, _, _| view.snapshot_revision())
                        .map_err(|error| error.to_string())?;
                    if revision.is_none() {
                        return Err(
                            "recreated settings window did not restore a runtime snapshot"
                                .to_owned(),
                        );
                    }
                    Ok(())
                });
                #[cfg(target_os = "windows")]
                let restored = update_windows_settings(
                    cx,
                    &original_window,
                    |view, _, _| -> Result<(), String> {
                        if view.snapshot_revision().is_none() {
                            return Err(
                                "recreated settings window did not restore a runtime snapshot"
                                    .to_owned(),
                            );
                        }
                        Ok(())
                    },
                )
                .await;
                match restored {
                    Ok(()) => {}
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

                Timer::after(Duration::from_millis(250)).await;
                let open_verified = cx.update(|cx| -> Result<(), String> {
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
                    let revision = window
                        .update(cx, |view, _, _| view.snapshot_revision())
                        .map_err(|error| error.to_string())?;
                    if revision.is_none() {
                        return Err("Open Settings did not restore a runtime snapshot".to_owned());
                    }
                    Ok(())
                });
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
                        .update(cx, |_, window, _| window.remove_window())
                        .map_err(|error| error.to_string())?;
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

                let mut closed = false;
                for _ in 0..60 {
                    Timer::after(Duration::from_millis(50)).await;
                    if cx.update(|cx| {
                        cx.windows().is_empty()
                            && cx.global::<ProductCoordinator>().settings_window.is_none()
                    }) {
                        closed = true;
                        break;
                    }
                }
                if !closed {
                    let _ = write_smoke_status("application-reopen close failed");
                    record_failure(
                        &smoke_failures,
                        "application-reopen smoke could not destroy the settings window",
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
                        if reopened == original_window {
                            return Err(
                                "application reopen retained the destroyed macOS Entity".to_owned()
                            );
                        }
                        if cx.windows().len() != 1 {
                            return Err("application reopen created more than one settings window"
                                .to_owned());
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
            let smoke_failures = Arc::clone(&run_failures);
            let smoke_shutdown_requested = Arc::clone(&shutdown_requested);
            cx.spawn(async move |cx| {
                Timer::after(Duration::from_millis(500)).await;
                let baseline = update_windows_settings(
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
                    match update_windows_settings(cx, &settings_window, |view, _, _| {
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

                for _ in 0..100 {
                    Timer::after(Duration::from_millis(50)).await;
                    let restored = update_windows_settings(
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

    let failures = Arc::try_unwrap(failures)
        .unwrap_or_else(|_| panic!("product failure accumulator is still shared"))
        .into_inner()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
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
        .nth(3)
        .expect("repository root")
        .join("native/resources/models")
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

#[cfg(any(target_os = "windows", test))]
fn executable_relative_preset_root(executable: &Path) -> Option<PathBuf> {
    Some(executable.parent()?.join("resources/models"))
}

#[cfg(not(any(target_os = "macos", target_os = "windows")))]
fn preset_root() -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(3)
        .expect("repository root")
        .join("native/resources/models")
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
    fn run_options_default_to_an_unbounded_product_lifetime() {
        assert_eq!(
            RunOptions::parse(Vec::new()).expect("default options"),
            RunOptions {
                run_duration: Duration::ZERO,
                settings_window_smoke: false,
                models_page_smoke: false,
                hidden_model_switch_smoke: false,
                #[cfg(feature = "storage-test-injection")]
                configuration_recovery_smoke: false,
                #[cfg(feature = "storage-test-injection")]
                settings_window_state_smoke: false,
                #[cfg(feature = "storage-test-injection")]
                panic_diagnostics_smoke: false,
                #[cfg(feature = "storage-test-injection")]
                panic_diagnostics_smoke_child: false,
                system_menu_smoke: false,
                #[cfg(target_os = "macos")]
                application_reopen_smoke: false,
                #[cfg(target_os = "macos")]
                startup_item_smoke: false,
                #[cfg(target_os = "windows")]
                single_instance_smoke: false,
            }
        );
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
    fn configuration_recovery_smoke_is_opt_in() {
        let options = RunOptions::parse(["--configuration-recovery-smoke".to_owned()])
            .expect("configuration recovery smoke options");
        assert!(options.configuration_recovery_smoke);
        assert!(!options.settings_window_state_smoke);
        assert!(!options.settings_window_smoke);
        assert!(!options.models_page_smoke);
    }

    #[cfg(feature = "storage-test-injection")]
    #[test]
    fn settings_window_state_smoke_is_opt_in() {
        let options = RunOptions::parse(["--settings-window-state-smoke".to_owned()])
            .expect("settings window state smoke options");
        assert!(options.settings_window_state_smoke);
        assert!(!options.configuration_recovery_smoke);
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

    #[cfg(not(feature = "storage-test-injection"))]
    #[test]
    fn product_options_reject_storage_test_injection() {
        let error = RunOptions::parse(["--configuration-recovery-smoke".to_owned()])
            .expect_err("default product options must reject storage injection");
        assert!(error.message.contains("unknown argument"));
        assert!(!usage().contains("configuration-recovery-smoke"));
        let state_error = RunOptions::parse(["--settings-window-state-smoke".to_owned()])
            .expect_err("default product options must reject state storage injection");
        assert!(state_error.message.contains("unknown argument"));
        assert!(!usage().contains("settings-window-state-smoke"));
        let panic_error = RunOptions::parse(["--panic-diagnostics-smoke".to_owned()])
            .expect_err("default product options must reject panic storage injection");
        assert!(panic_error.message.contains("unknown argument"));
        assert!(!usage().contains("panic-diagnostics-smoke"));
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
            bundled_preset_root(Path::new("/tmp/native/target/release/bongocat-app")),
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
    }

    #[test]
    fn run_options_reject_missing_invalid_and_unknown_values() {
        for arguments in [
            vec!["--run-seconds".to_owned()],
            vec!["--run-seconds".to_owned(), "-1".to_owned()],
            vec!["--model".to_owned(), "standard".to_owned()],
        ] {
            assert!(RunOptions::parse(arguments).is_err());
        }
    }
}
