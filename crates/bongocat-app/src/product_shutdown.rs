//! The product's quit protocol.
//!
//! Shutdown is a sequence, not a flag: block new frame ticks, stop the input
//! producers, confirm the frame source has actually stopped, stop the runtime,
//! flush configuration, stop audio, release the renderer, destroy the overlay,
//! and only then let the window close. The guard types here are what make a
//! skipped step or a thread that outlives its owner visible instead of silent.

use super::*;

#[derive(Debug, thiserror::Error)]
#[error("product run failed: {}", .failures.join("; "))]
pub(crate) struct ProductRunError {
    pub(crate) failures: Vec<String>,
}

#[derive(Clone, Debug, Default)]
pub(crate) struct FrameSourceShutdown {
    pub(crate) stop_requested: Arc<AtomicBool>,
    pub(crate) stopped: Arc<AtomicBool>,
}

impl FrameSourceShutdown {
    pub(crate) fn request_stop(&self) {
        self.stop_requested.store(true, Ordering::Release);
    }

    pub(crate) fn stop_requested(&self) -> bool {
        self.stop_requested.load(Ordering::Acquire)
    }

    pub(crate) fn is_stopped(&self) -> bool {
        self.stopped.load(Ordering::Acquire)
    }

    pub(crate) fn run_guard(&self) -> FrameSourceRunGuard {
        FrameSourceRunGuard {
            stopped: Arc::clone(&self.stopped),
        }
    }

    pub(crate) async fn wait_for_stop(&self) -> bool {
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

pub(crate) struct FrameSourceRunGuard {
    pub(crate) stopped: Arc<AtomicBool>,
}

impl Drop for FrameSourceRunGuard {
    fn drop(&mut self) {
        self.stopped.store(true, Ordering::Release);
    }
}

/// Turn the final shared failure list into a process exit code, printing any
/// failures once.
pub(crate) fn product_failures_exit_code(failures: &Arc<Mutex<Vec<String>>>) -> i32 {
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

/// Leave the GPUI loop after a startup failure that happens before a
/// `ProductCoordinator` exists, so `finish_product_quit` cannot take over shutdown.
///
/// An automated run must still report startup failure: a bare `cx.quit()` could
/// terminate with status 0. Exit directly when anything was recorded; otherwise
/// quit normally.
pub(crate) fn quit_after_startup_failure(cx: &mut App, failures: &Arc<Mutex<Vec<String>>>) {
    let exit_code = product_failures_exit_code(failures);
    if exit_code != 0 {
        std::process::exit(exit_code);
    }
    cx.quit();
}

pub(crate) fn record_failure(failures: &Arc<Mutex<Vec<String>>>, failure: impl Into<String>) {
    failures
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .push(failure.into());
}

pub(crate) const fn native_theme_for_startup(
    theme: bongocat_config::Theme,
) -> Option<bongocat_platform::AppTheme> {
    match theme {
        bongocat_config::Theme::System => None,
        bongocat_config::Theme::Light => Some(bongocat_platform::AppTheme::Light),
        bongocat_config::Theme::Dark => Some(bongocat_platform::AppTheme::Dark),
    }
}

#[cfg(target_os = "windows")]
pub(crate) fn request_windows_product_quit(shutdown_requested: &AtomicBool) {
    shutdown_requested.store(true, Ordering::Release);
}

pub(crate) fn finish_product_quit(cx: &mut App) {
    #[cfg(target_os = "macos")]
    {
        // The application-owned quit must own its exit boundary: AppKit can terminate
        // the process after `on_app_quit` completes without returning from
        // `NSApplication::run()`, so the fallback `exit_after_automated_smoke` sees
        // only the failures that existed at that point. Await the whole shutdown
        // here, then make the exit code from the final list (TODO
        // P7-MACOS-SMOKE-EXIT-CODE).
        if cx.has_global::<ProductCoordinator>() {
            let shutdown = begin_product_shutdown(cx);
            cx.spawn(async move |_| {
                let failures = shutdown.finish().await;
                std::process::exit(product_failures_exit_code(&failures));
            })
            .detach();
            return;
        }
        cx.quit();
    }

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

pub(crate) fn request_product_quit(cx: &mut App) {
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
pub(crate) fn start_windows_product_shutdown(cx: &mut App) {
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

pub(crate) struct ProductShutdown {
    pub(crate) coordinator: ProductCoordinator,
    pub(crate) overlay: ProductOverlaySession,
    pub(crate) settings_service: bongocat_app::ApplicationSettingsService,
    pub(crate) update_service: Option<bongocat_app::ApplicationUpdateService>,
}

impl ProductShutdown {
    pub(crate) async fn finish(self) -> Arc<Mutex<Vec<String>>> {
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

pub(crate) fn begin_product_shutdown(cx: &mut App) -> ProductShutdown {
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
pub(crate) fn exit_after_automated_smoke(failures: &Arc<Mutex<Vec<String>>>) {
    let failures = failures
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    if failures.is_empty() {
        return;
    }
    let mut stderr = io::stderr().lock();
    let _ = writeln!(stderr, "product run failed: {}", failures.join("; "));
    let _ = stderr.flush();
    // The fallback for OS-initiated termination on macOS. An application-owned quit
    // (`finish_product_quit`) awaits the full shutdown and exits from the final list;
    // this helper only covers paths where AppKit starts termination itself and the
    // shutdown future never gets to run to completion. Normal product quits never call
    // it: they do not set `automated_verification` (TODO P7-MACOS-SMOKE-EXIT-CODE).
    std::process::exit(1);
}

#[cfg(target_os = "windows")]
pub(crate) fn windows_product_exit_code(failures: &Arc<Mutex<Vec<String>>>) -> i32 {
    product_failures_exit_code(failures)
}
