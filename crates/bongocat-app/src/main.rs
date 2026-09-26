// Hide the console window in packaged (release) builds so the product runs as a
// pure GUI application. Debug builds keep the console for developer logging.
#![cfg_attr(
    all(not(debug_assertions), target_os = "windows"),
    windows_subsystem = "windows"
)]
#![forbid(unsafe_code)]

//! The product entry point: start every subsystem, hand the windows to GPUI and
//! own the run loop.
//!
//! `main()` and the closure it hands `run` stay here on purpose. That closure is
//! where the shutdown ordering lives — block new frame ticks, stop the input
//! producers, confirm the frame source, stop the runtime, flush configuration,
//! stop audio, release the renderer, destroy the overlay, close GPUI — and it
//! repeats that sequence on every startup failure path, so it is only checkable
//! while it stays in one place. Everything around it is one module per subsystem:
//! `product_options` parses the run options, `product_shutdown` owns the quit
//! protocol, `product_windows` the product's windows, and the rest one concern
//! each.

#[cfg(test)]
mod binary_tests;
mod gamepad_observer;
mod model_cover;
mod overlay_placement;
mod preset_root;
mod product_icons;
mod product_options;
mod product_shutdown;
mod product_windows;
mod smoke;
mod smoke_status;
mod system_menu;
mod update_schedule;

use async_io::Timer;
use bongocat_app::{
    ApplicationLogCode, ApplicationLogContext, ApplicationLogEvent, application_shortcut_dispatcher,
};
use bongocat_live2d::CoreLogHandle;
use bongocat_overlay::{
    OverlayContextMenuRequest, OverlayInteractionSinks, OverlayResizeOutcome,
    OverlaySessionOptions, OverlayWindowBounds, ProductOverlaySession,
};
use bongocat_platform::GlobalShortcutService;
#[cfg(target_os = "windows")]
use bongocat_platform::{
    SingleInstance, SingleInstanceAction, SingleInstanceEnvironment, SingleInstanceStart,
};
use bongocat_platform::{SystemMenu, SystemMenuAction, SystemMenuPresentation};
use bongocat_runtime::hover_hide_delay_ms;
use bongocat_ui::{
    SettingsNavigationMemory, SettingsView, SettingsWindowHandle, SettingsWindowSeed,
    open_settings_window,
};
use bongocat_ui_protocol::{
    AutomaticUpdateSettings, SettingsClient, SettingsError, SettingsErrorCode,
    SettingsModelAvailability, SettingsModelKey, SettingsModelOrigin, SettingsOverlay,
    SettingsSnapshot,
};
use gpui_kit::{
    App, Application as GpuiApplication, Global, QuitMode, assets::AllAssets,
    platform::current_platform,
};
use gpui_kit::{AsyncApp, Context, Window};
use std::sync::atomic::{AtomicBool, Ordering};
#[cfg(target_os = "windows")]
use std::{cell::RefCell, rc::Rc};
use std::{
    env,
    io::{self, Write},
    path::Path,
    path::PathBuf,
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

use gamepad_observer::GamepadConnectionObserver;
use model_cover::{COVER_CAPTURE_POLL_INTERVAL_MS, capture_model_cover_without_blocking};
use overlay_placement::OverlayPlacementDebouncer;
use preset_root::preset_root;
use product_icons::{ProductStatusIcon, ProductTaskbarIcon};
use product_options::RunOptions;
#[cfg(target_os = "macos")]
use product_shutdown::exit_after_automated_smoke;
use product_shutdown::{
    FrameSourceShutdown, ProductRunError, begin_product_shutdown, native_theme_for_startup,
    quit_after_startup_failure, record_failure, request_product_quit, request_windows_product_quit,
    start_windows_product_shutdown,
};
use product_windows::{
    apply_taskbar_icon_visibility, ensure_settings_window, handle_shortcut_toggle_settings,
    open_update_window_and_check, product_overlay_state, product_taskbar_icon_state,
    publish_overlay_scale, published_update_phase, request_update_check, show_update_window,
    take_update_restart_request, update_settings_window, update_window_is_open,
};
#[cfg(target_os = "macos")]
use product_windows::{poll_update_restart, restart_after_update, restart_delay_elapsed};
use smoke_status::{SMOKE_FIRST_FRAME_WAIT_TICKS, write_smoke_marker, write_smoke_status};
use system_menu::{
    apply_system_menu_overlay_action, refresh_system_menu_presentation, system_menu_presentation,
};
use update_schedule::{
    AUTOMATIC_UPDATE_CHECK_SETTLE_ATTEMPTS, AUTOMATIC_UPDATE_CHECK_SETTLE_INTERVAL,
    AUTOMATIC_UPDATE_CHECK_STARTUP_DELAY, AUTOMATIC_UPDATE_SETTINGS_POLL_INTERVAL,
    AUTOMATIC_UPDATE_SETTINGS_RETRY_INTERVAL, automatic_update_schedule_delay,
};

fn gpui_application() -> GpuiApplication {
    // Quitting is owned by the product shutdown paths (tray menu, smoke recipes,
    // update restart), not by window bookkeeping: the product stays alive behind
    // the overlay and the status icon even when every product window is closed,
    // so GPUI must never auto-quit on last-window-closed.
    GpuiApplication::new_inaccessible(current_platform(false)).with_quit_mode(QuitMode::Explicit)
}

struct ProductCoordinator {
    _core_log: CoreLogHandle,
    #[cfg(target_os = "macos")]
    overlay: Option<ProductOverlaySession>,
    #[cfg(target_os = "windows")]
    overlay: Rc<RefCell<Option<ProductOverlaySession>>>,
    settings_service: Option<bongocat_app::ApplicationSettingsService>,
    /// The currently open settings window, if any; close destroys it.
    settings_window: Option<SettingsWindowHandle>,
    /// Process-local memory for the last settings sidebar page.
    settings_navigation_memory: SettingsNavigationMemory,
    /// The worker that owns the update pipeline.
    update_service: Option<bongocat_app::ApplicationUpdateService>,
    /// The open update window, if any; close destroys it.
    update_window: Option<bongocat_ui::UpdateWindowHandle>,
    /// The display language a product window opens with.
    ///
    /// Seeded from the startup snapshot and refreshed by the system menu loop, which
    /// already reads the settings snapshot every 50 ms. A window is created and shown
    /// before its own first snapshot arrives, so opening on the default would render
    /// one frame of it: the settings window used to redraw from English into the
    /// user's language as soon as the snapshot landed. Reading it here keeps the
    /// windows off a blocking read on the GPUI thread.
    product_language: bongocat_ui_protocol::SettingsLanguage,
    /// The appearance a product window opens with.
    ///
    /// The same reason as `product_language`: a window has to apply the product's
    /// theme on its first frame, and the settings snapshot only reaches it on the next
    /// poll. Opening on the default would let the update window clear an override the
    /// settings window has already installed (ADR-0048).
    product_appearance_theme: bongocat_ui_protocol::SettingsTheme,
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
    /// Work the settings worker has handed to this thread: the settings window a
    /// global shortcut asked for, and the cover captures of models it just
    /// imported.
    main_thread_signals: bongocat_app::ApplicationMainThreadSignals,
    shortcut_service: Option<bongocat_platform::GlobalShortcutService>,
    frame_ticks: u64,
    expect_visible_frame: bool,
    failures: Arc<Mutex<Vec<String>>>,
    #[cfg(target_os = "windows")]
    shutdown_requested: Arc<AtomicBool>,
    #[cfg(target_os = "windows")]
    shutdown_flush_complete: Arc<AtomicBool>,
}

impl Global for ProductCoordinator {}

#[cfg(target_os = "windows")]
fn build_single_instance_environment() -> SingleInstanceEnvironment {
    match bongocat_app::BUILD_ENVIRONMENT {
        bongocat_config::BuildEnvironment::Development => SingleInstanceEnvironment::Development,
        bongocat_config::BuildEnvironment::Production => SingleInstanceEnvironment::Production,
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let run_options = match RunOptions::parse(env::args().skip(1)) {
        Ok(options) => options,
        Err(error) if error.help => {
            // `clap` already rendered the help with its own trailing newline, and it is
            // the only thing this process should print on the way out.
            io::stdout().lock().write_all(error.message.as_bytes())?;
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
    // A verification mode replaces the product instead of instrumenting it: each one
    // answers for a subsystem the run loop below would otherwise own.
    #[cfg(feature = "storage-test-injection")]
    if run_options.settings_window_state_smoke {
        return smoke::run_settings_window_state_smoke();
    }
    #[cfg(feature = "storage-test-injection")]
    if run_options.panic_diagnostics_smoke_child {
        return smoke::run_panic_diagnostics_smoke_child();
    }
    #[cfg(feature = "storage-test-injection")]
    if run_options.panic_diagnostics_smoke {
        return smoke::run_panic_diagnostics_smoke();
    }
    #[cfg(feature = "storage-test-injection")]
    if run_options.diagnostics_export_smoke {
        return smoke::run_diagnostics_export_smoke();
    }
    #[cfg(feature = "storage-test-injection")]
    if run_options.diagnostics_export_failure_smoke {
        return smoke::run_diagnostics_export_failure_smoke();
    }
    #[cfg(target_os = "macos")]
    if run_options.startup_item_smoke {
        return smoke::run_startup_item_smoke();
    }
    if run_options.startup_permission_smoke {
        return smoke::run_startup_permission_smoke();
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
    let application_log = application.log_handle();
    application.install_process_panic_hook();
    let core_log = CoreLogHandle::install(
        application.logs_directory(),
        application.log_settings_controller(),
    )?;
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
    let permission_check_enabled = !run_options.automated_verification();

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
        maximum_fps: application.config().overlay.maximum_fps,
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
    // A right-button resize drag ends by reporting the scale it settled on. The
    // queue only has to hold the latest one: a drag is a single gesture, and a
    // second drag cannot start before the first has been released.
    let (resize_sender, resize_receiver) = std::sync::mpsc::sync_channel::<OverlayResizeOutcome>(1);
    let (status_icon_sender, status_icon_receiver) = std::sync::mpsc::sync_channel(4);
    let status_icon = Arc::new(ProductStatusIcon {
        sender: status_icon_sender,
    });
    let initial_status_icon_visible = application.config().system.show_status_icon;
    #[cfg(target_os = "windows")]
    let (taskbar_icon_sender, taskbar_icon_receiver) = std::sync::mpsc::sync_channel(4);
    #[cfg(target_os = "windows")]
    let taskbar_icon = Arc::new(ProductTaskbarIcon {
        sender: taskbar_icon_sender,
    });
    #[cfg(target_os = "windows")]
    let initial_taskbar_icon_visible = application.config().system.show_taskbar_icon;
    let main_thread_signals = bongocat_app::ApplicationMainThreadSignals::default();
    let input_producer = application.input_producer();
    let cursor_producer = application.cursor_producer();
    let gamepad_axis_producer = application.gamepad_axis_producer();
    let render_consumer = application.take_render_consumer()?;
    let expect_visible_frame = true;
    let frame_runtime_client = runtime_client.clone();
    let frame_source_shutdown = FrameSourceShutdown::default();
    let failures = Arc::new(Mutex::new(Vec::new()));
    let run_failures = Arc::clone(&failures);
    // Global shortcuts are OS registrations owned by a dedicated platform
    // service thread (ADR-0044); the input pipeline no longer matches edges.
    let shortcut_service = match GlobalShortcutService::start(
        application.shortcut_table(),
        application_shortcut_dispatcher(runtime_client.clone(), shortcut_sender),
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
                resize_sender: Some(resize_sender),
            },
        ) {
            Ok(overlay) => overlay,
            Err(error) => {
                record_failure(&run_failures, error.to_string());
                if let Err(error) = application.shutdown() {
                    record_failure(&run_failures, error.to_string());
                }
                quit_after_startup_failure(cx, &run_failures);
                return;
            }
        };
        let settings_service =
            match bongocat_app::ApplicationSettingsService::start_with_product_capabilities(
                application,
                shortcut_receiver,
                main_thread_signals.clone(),
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
                    quit_after_startup_failure(cx, &run_failures);
                    return;
                }
            };
        // One blocking read serves both the first system menu and the appearance every
        // product window opens with, so the language and theme a window is seeded with
        // cost no extra round trip and cannot disagree with the menu.
        let initial_settings_snapshot = match settings_service.client().read_snapshot_blocking() {
            Ok(snapshot) => snapshot,
            Err(error) => {
                record_failure(&run_failures, error.to_string());
                let mut overlay = overlay;
                let _ = overlay.stop_input();
                let client = settings_service.client();
                let _ = client.shutdown_blocking();
                let _ = settings_service.join();
                let _ = overlay.finish_after_runtime_shutdown();
                quit_after_startup_failure(cx, &run_failures);
                return;
            }
        };
        let initial_menu_presentation = system_menu_presentation(&initial_settings_snapshot);
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
                quit_after_startup_failure(cx, &run_failures);
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
            application_log.clone(),
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
                quit_after_startup_failure(cx, &run_failures);
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
            settings_navigation_memory: SettingsNavigationMemory::new(),
            update_service: Some(update_service),
            update_window: None,
            product_language: initial_settings_snapshot.resolved_language,
            product_appearance_theme: initial_settings_snapshot.appearance_theme,
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
            main_thread_signals: main_thread_signals.clone(),
            shortcut_service,
            frame_ticks: 0,
            expect_visible_frame,
            failures: Arc::clone(&run_failures),
            #[cfg(target_os = "windows")]
            shutdown_requested: Arc::clone(&shutdown_requested),
            #[cfg(target_os = "windows")]
            shutdown_flush_complete: Arc::new(AtomicBool::new(false)),
        });

        // Every model the settings worker installs is rendered into its own cover
        // here. The worker cannot do it — a capture creates a native window, and the
        // GPU thread that owns the product's windows is this one — so it queues the
        // model and this loop drains the queue, writes the result back through the
        // settings service, and drops the card's cached image so the next frame
        // reloads the file.
        //
        // The capture draws a few dozen frames while the model settles, but it does
        // not own this thread for that whole time: each frame is one step of a
        // `ModelCoverCaptureSession` and the task sleeps the session's frame
        // interval in between. Every sleep is an await on the foreground executor,
        // so the main loop keeps pumping while the capture settles — the settings
        // window keeps redrawing (the import card's spinner included) and the
        // overlay frame loop below keeps ticking (ADR-0055).
        //
        // A capture that fails abandons the import. The capture renders the model
        // through the same GPU path the overlay uses, so a model the product cannot
        // prepare for a cover is a model it cannot activate either: publishing it
        // would hand the user a card whose only outcome is "the selected model could
        // not be activated". The model is therefore removed from the store here, and
        // the settings window reports the import as failed instead of revealing it.
        // The model stays hidden from the grid throughout, because the import that
        // installed it is still withholding its cards until this capture reports.
        let cover_capture_client = settings_client.clone();
        let cover_capture_signals = main_thread_signals.clone();
        let cover_capture_log = application_log.clone();
        cx.spawn(async move |cx| {
            loop {
                Timer::after(Duration::from_millis(COVER_CAPTURE_POLL_INTERVAL_MS)).await;
                if !cx.update(|cx| cx.has_global::<ProductCoordinator>()) {
                    break;
                }
                for request in cover_capture_signals.take_model_cover_captures() {
                    let key = request.key().clone();
                    let captured = match
                        capture_model_cover_without_blocking(Arc::clone(request.model())).await
                    {
                        Ok(captured) => cover_capture_client
                            .replace_model_cover(key.clone(), captured.png().to_vec())
                            .await
                            .is_ok(),
                        Err(_) => false,
                    };
                    if !captured {
                        cover_capture_log.record(
                            ApplicationLogEvent::new(ApplicationLogCode::ModelOperationFailed)
                                .with_context(ApplicationLogContext::Operation("cover_capture"))
                                .with_context(ApplicationLogContext::Reason(
                                    "overlay_capture_failed",
                                )),
                        );
                        // Undo the import before reporting it, so the model is
                        // never revealed: the settings window reads the catalog
                        // this removal republishes, and the card for a model that
                        // is no longer installed cannot appear at all.
                        let _ = cover_capture_client.delete_model(key.clone()).await;
                    }
                    // The settings window keeps every model this import installed
                    // out of the grid until its capture reports back, so the
                    // report has to arrive either way: a failed capture removes
                    // the model rather than leaving it hidden behind a signal
                    // that never comes.
                    cx.update(|cx| {
                        let Some(window) = cx
                            .try_global::<ProductCoordinator>()
                            .and_then(|coordinator| coordinator.settings_window.clone())
                        else {
                            return;
                        };
                        let _ = window.update(cx, |view, _, cx| {
                            view.finish_model_cover_capture(&key, captured, cx)
                        });
                    });
                }
            }
        })
        .detach();

        // Startup permission check on its own worker (ADR-0032, amended 2026-09-15). The
        // overlay, settings service, system menu and update worker above are already
        // running, so a pending native prompt can no longer delay any product window.
        // The check is a read-only platform query; only a missing capability shows the
        // prompt, and the prompt outcome is neither persisted nor logged.
        //
        // Windows: the primary button promises "exit and open the program folder". A successful
        // permission flow (the executable's folder was revealed) raises the same shutdown
        // flag the tray quit uses, so the product exits through the regular shutdown
        // coordinator while the user flips the compatibility flag. A failed reveal keeps
        // the product running, per the ADR rule that a failed flow only counts as
        // "continue for now". macOS has no quit promise: its primary button only opens System
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
            // Both product windows are destroyed on close. The update worker keeps
            // running independently, including while its window is gone.
            let update_window = cx.global::<ProductCoordinator>().update_window.clone();
            if let Some(window_handle) = update_window
                && !window_handle.is_open()
            {
                cx.global_mut::<ProductCoordinator>().update_window = None;
            }
        })
        .detach();

        // The automatic check is driven from the GPUI side because the opt-in and
        // interval live in the user's configuration, which only the settings service
        // can read. The worker stays a plain command receiver; nothing about the
        // schedule reaches it. A cheap settings-only poll lets a changed interval or
        // switch re-arm this schedule without rebuilding the model-catalog snapshot.
        let automatic_check_client = settings_client.clone();
        cx.spawn(async move |cx| {
            Timer::after(AUTOMATIC_UPDATE_CHECK_STARTUP_DELAY).await;
            let mut settings = loop {
                if !cx.update(|cx| cx.has_global::<ProductCoordinator>()) {
                    return;
                }
                match automatic_check_client.read_automatic_update_settings().await {
                    Ok(settings) => break settings,
                    Err(_) => {
                        Timer::after(AUTOMATIC_UPDATE_SETTINGS_RETRY_INTERVAL).await;
                    }
                }
            };
            let mut last_dispatch = None;
            loop {
                if !cx.update(|cx| cx.has_global::<ProductCoordinator>()) {
                    break;
                }

                if settings.enabled {
                    let delay =
                        automatic_update_schedule_delay(settings, last_dispatch, Instant::now());
                    if delay.is_zero() {
                        if cx.update(request_update_check) {
                            // Measure the configured interval from the actual dispatch,
                            // not from the end of the settle window.
                            last_dispatch = Some(Instant::now());
                            // Wait for the check to settle before deciding what to show.
                            // The bound keeps a worker that never reports back from
                            // parking this loop indefinitely.
                            for _ in 0..AUTOMATIC_UPDATE_CHECK_SETTLE_ATTEMPTS {
                                Timer::after(AUTOMATIC_UPDATE_CHECK_SETTLE_INTERVAL).await;
                                match cx.update(published_update_phase) {
                                    Some(bongocat_ui_protocol::UpdatePhase::Checking) => continue,
                                    Some(bongocat_ui_protocol::UpdatePhase::Available { .. }) => {
                                        // Surface the result rather than leaving it for
                                        // the user to discover. The window is a singleton,
                                        // so a second automatic check cannot stack one.
                                        if !cx.update(update_window_is_open) {
                                            cx.update(show_update_window);
                                        }
                                        break;
                                    }
                                    _ => break,
                                }
                            }
                        } else {
                            Timer::after(AUTOMATIC_UPDATE_SETTINGS_POLL_INTERVAL).await;
                        }
                    } else {
                        Timer::after(delay).await;
                    }
                } else {
                    Timer::after(AUTOMATIC_UPDATE_SETTINGS_POLL_INTERVAL).await;
                }

                let next_settings =
                    match automatic_check_client.read_automatic_update_settings().await {
                        Ok(settings) => settings,
                        Err(_) => {
                            Timer::after(AUTOMATIC_UPDATE_SETTINGS_RETRY_INTERVAL).await;
                            continue;
                        }
                    };
                // Re-enabling automatic checks starts a fresh schedule. Changing only
                // the interval keeps the last dispatch as the anchor, so a shorter
                // interval can become due immediately without resetting a long wait.
                if next_settings.enabled && !settings.enabled {
                    last_dispatch = None;
                }
                settings = next_settings;
            }
        })
        .detach();

        #[cfg(target_os = "macos")]
        let fail_on_smoke_failure = run_options.automated_verification();
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
        let system_menu_action_client = system_menu_client.clone();
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
                        coordinator.product_language = language;
                        coordinator.product_appearance_theme = appearance_theme;
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
                    | SystemMenuAction::ToggleClickThrough
                    | SystemMenuAction::ToggleAlwaysOnTop
                    | SystemMenuAction::ToggleHideOnPointerHover => {
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
                    SystemMenuAction::Quit => cx.update(|cx| {
                        request_product_quit(cx);
                        Ok(false)
                    }),
                };
                match handled {
                    Ok(true) => {}
                    Ok(false) => break,
                    Err(error) => {
                        record_failure(&system_menu_failures, error);
                        if let Err(error) =
                            refresh_system_menu_presentation(&system_menu_action_client, cx).await
                        {
                            record_failure(&system_menu_failures, error);
                        }
                    }
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
        let frame_application_log = application_log.clone();
        let frame_source_guard = frame_source_shutdown.run_guard();
        cx.spawn(async move |cx| {
            let _frame_source_guard = frame_source_guard;
            #[cfg(target_os = "windows")]
            let mut frame_active = true;
            #[cfg(target_os = "windows")]
            let mut shutdown_flush_started = false;
            let mut last_overlay_bounds = None;
            let mut overlay_placement_debouncer = OverlayPlacementDebouncer::default();
            let mut gamepad_connections = GamepadConnectionObserver::default();
            let mut retry_delay = None;
            let mut frame_pacer: Option<bongocat_runtime::FramePacer> = None;
            loop {
                // Only the pacing inputs are read here. The overlay session reads
                // the full snapshot inside `tick`, so asking for a second copy per
                // frame just to learn the frame rate reallocated the active model's
                // name and behavior list once more for nothing.
                let scheduling = frame_runtime_client.frame_scheduling();
                let frame_interval = bongocat_runtime::frame_interval_for_runtime(
                    scheduling.maximum_fps,
                    scheduling.overlay_visible,
                )
                .expect("runtime stores validated frame scheduling state");
                // The wait is measured against the next frame deadline instead of
                // started once the previous frame was presented, so the window
                // keeps the configured `maximum_fps` rather than `interval + work`.
                // A retry backoff replaces the cadence for one iteration and the
                // grid re-anchors on the frame that follows it.
                let wait = if let Some(delay) = retry_delay.take() {
                    frame_pacer = None;
                    delay
                } else {
                    frame_pacer
                        .get_or_insert_with(|| {
                            bongocat_runtime::FramePacer::new(Instant::now(), frame_interval)
                        })
                        .wait(Instant::now(), frame_interval)
                };
                Timer::after(wait).await;
                if frame_source_shutdown.stop_requested() {
                    break;
                }
                let context_menu_requested = context_menu_receiver.try_recv().is_ok();
                let resize_outcome = resize_receiver.try_recv().ok();
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
                            frame_application_log.record(
                                ApplicationLogEvent::new(ApplicationLogCode::ServiceFailed)
                                    .with_context(ApplicationLogContext::Service("system_menu"))
                                    .with_context(ApplicationLogContext::Reason(
                                        "context_menu_failed",
                                    )),
                            );
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
                                frame_application_log.record(
                                    ApplicationLogEvent::new(ApplicationLogCode::ServiceFailed)
                                        .with_context(ApplicationLogContext::Service("overlay"))
                                        .with_context(ApplicationLogContext::Reason("tick_failed")),
                                );
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
                            frame_application_log.record(
                                ApplicationLogEvent::new(ApplicationLogCode::ServiceFailed)
                                    .with_context(ApplicationLogContext::Service("system_menu"))
                                    .with_context(ApplicationLogContext::Reason(
                                        "context_menu_failed",
                                    )),
                            );
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
                                frame_application_log.record(
                                    ApplicationLogEvent::new(ApplicationLogCode::ServiceFailed)
                                        .with_context(ApplicationLogContext::Service("overlay"))
                                        .with_context(ApplicationLogContext::Reason("tick_failed")),
                                );
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
                // A right-button resize drag reports the scale it settled on
                // after it has already resized the window, so this only brings
                // the stored configuration to the same number.
                if let Some(outcome) = resize_outcome {
                    publish_overlay_scale(&frame_settings_client, outcome.scale_percent).await;
                }
                // Gamepad connectivity is a product behaviour the settings
                // service owns, so the frame source only reports the transition.
                // The count is read in place: cloning the whole snapshot per
                // frame for one counter is exactly what the pacing read above
                // stopped doing. A frame that is already shutting down reports
                // nothing, because the automatic switch would write
                // configuration against the flush the shutdown is about to run.
                if keep_running {
                    gamepad_connections.observe(
                        frame_runtime_client.connected_gamepad_count(),
                        &frame_settings_client,
                    );
                }
                if let Some(pacer) = frame_pacer.as_mut() {
                    pacer.frame_produced(Instant::now(), frame_interval);
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
                            entry.origin == SettingsModelOrigin::BuiltIn
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
                // The appearance page asserts on `applied_theme`, which is only assigned
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
                            view.show_model_library_page_for_smoke(cx)
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
                    // The window is visible, so the existing toggle request is a
                    // close request. The coordinator must drop this handle when
                    // GPUI finishes destroying the window.
                    cx.global::<ProductCoordinator>()
                        .main_thread_signals
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

                let mut destroyed = false;
                for _ in 0..60 {
                    Timer::after(Duration::from_millis(50)).await;
                    let observed = cx.update(|cx| {
                        let coordinator = cx.global::<ProductCoordinator>();
                        (
                            coordinator.settings_window.is_none(),
                            original_window.read(cx).is_err(),
                        )
                    });
                    if observed == (true, true) {
                        destroyed = true;
                        break;
                    }
                }
                if !destroyed {
                    record_failure(
                        &smoke_failures,
                        "settings close did not destroy the window and clear its handle",
                    );
                    #[cfg(target_os = "macos")]
                    cx.update(request_product_quit);
                    #[cfg(target_os = "windows")]
                    request_windows_product_quit(&smoke_shutdown_requested);
                    return;
                }
                let remembered_page = 2;
                cx.update(|cx| {
                    cx.global_mut::<ProductCoordinator>()
                        .settings_navigation_memory
                        .set_page_index(remembered_page);
                });

                Timer::after(Duration::from_millis(500)).await;
                let reopened = cx.update(|cx| -> Result<SettingsWindowHandle, String> {
                    if cx.global::<ProductCoordinator>().frame_ticks <= baseline_ticks {
                        return Err(
                            "frame source stopped while the settings window was closed".to_owned()
                        );
                    }
                    cx.global::<ProductCoordinator>()
                        .main_thread_signals
                        .request_open_settings();
                    handle_shortcut_toggle_settings(cx);
                    let reopened = cx
                        .global::<ProductCoordinator>()
                        .settings_window
                        .clone()
                        .ok_or_else(|| "settings shortcut did not recreate the window".to_owned())?;
                    if cx.windows().len() != 1 {
                        return Err("settings reopen created more than one window".to_owned());
                    }
                    if reopened == original_window {
                        return Err(
                            "settings reopen reused the destroyed window handle".to_owned()
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
                            .ok_or_else(|| "settings window was not recreated".to_owned())?;
                    let revision = window_handle
                        .update(cx, |view, _, _| view.snapshot_revision())
                        .map_err(|error| error.to_string())?;
                    if revision.is_none() {
                        return Err(
                            "recreated settings window did not receive a runtime snapshot".to_owned(),
                        );
                    }
                    if cx.global::<ProductCoordinator>()
                        .settings_navigation_memory
                        .page_index()
                        != remembered_page
                    {
                        return Err("recreated settings window lost its sidebar page memory".to_owned());
                    }
                    Ok(())
                });
                match restored {
                    Ok(()) => {
                        if let Err(error) = write_smoke_status(
                            "settings window closed, was destroyed, and reopened with a fresh entity",
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
                    if changed.revision == initial.revision {
                        return Err(
                            "system menu overlay action did not advance the runtime snapshot"
                                .to_owned(),
                        );
                    }
                    if changed.config_revision != initial.config_revision {
                        return Err(
                            "system menu overlay visibility changed persisted configuration"
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
                        .update(cx, |view, window, cx| view.close(window, cx))
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

                // The close destroys the window. The next LaunchServices reopen
                // must create a new settings entity and restore its current page
                // from the process-local navigation memory.
                let mut destroyed = false;
                for _ in 0..60 {
                    Timer::after(Duration::from_millis(50)).await;
                    let observed = cx.update(|cx| {
                        let coordinator = cx.global::<ProductCoordinator>();
                        (
                            coordinator.settings_window.is_none(),
                            original_window.read(cx).is_err(),
                        )
                    });
                    if observed == (true, true) {
                        destroyed = true;
                        break;
                    }
                }
                if !destroyed {
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
                            "application reopen did not create a settings window".to_owned()
                        })?;
                        if reopened == original_window {
                            return Err(
                                "application reopen reused the destroyed settings window"
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

                let mut destroyed = false;
                for _ in 0..60 {
                    Timer::after(Duration::from_millis(50)).await;
                    let observed = cx.update(|cx| {
                        let coordinator = cx.global::<ProductCoordinator>();
                        (
                            coordinator.settings_window.is_none(),
                            settings_window.read(cx).is_err(),
                        )
                    });
                    if observed == (true, true) {
                        destroyed = true;
                        break;
                    }
                }
                if !destroyed {
                    record_failure(
                        &smoke_failures,
                        "single-instance smoke could not destroy the settings window",
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
                    let restored = cx.update(|cx| -> Result<bool, String> {
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
                        let reopened = coordinator.settings_window.clone().ok_or_else(|| {
                            "instance wake did not create a settings window".to_owned()
                        })?;
                        if cx.windows().len() != 1 {
                            return Err("instance wake created more than one settings window"
                                .to_owned());
                        }
                        if reopened == settings_window {
                            return Err(
                                "instance wake reused the destroyed settings window".to_owned(),
                            );
                        }
                        let hidden = reopened
                            .update(cx, |view, _, _| view.window_hidden())
                            .map_err(|error| error.to_string())?;
                        if hidden {
                            return Ok(false);
                        }
                        let revision = reopened
                            .update(cx, |view, _, _| view.snapshot_revision())
                            .map_err(|error| error.to_string())?;
                        if revision.is_none() {
                            return Err(
                                "instance wake did not restore a runtime snapshot".to_owned()
                            );
                        }
                        Ok(true)
                    });
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

        if !run_options.run_duration().is_zero() {
            #[cfg(target_os = "windows")]
            let quit_shutdown_requested = Arc::clone(&shutdown_requested);
            cx.spawn(async move |_cx| {
                Timer::after(run_options.run_duration()).await;
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
