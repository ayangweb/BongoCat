//! Frame-source acknowledgement and ordered service shutdown shared by platform hosts.
use async_io::Timer;
use bongocat_overlay::ProductOverlaySession;
use std::{
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};

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
#[cfg(not(target_os = "linux"))]
pub(crate) fn product_failures_exit_code(failures: &Arc<Mutex<Vec<String>>>) -> i32 {
    let failures = failures
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    if failures.is_empty() {
        return 0;
    }
    use std::io::{self, Write};
    let mut stderr = io::stderr().lock();
    let _ = writeln!(stderr, "product run failed: {}", failures.join("; "));
    let _ = stderr.flush();
    1
}

pub(crate) fn record_failure(failures: &Arc<Mutex<Vec<String>>>, failure: impl Into<String>) {
    failures
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .push(failure.into());
}

pub(crate) async fn finish_product_services(
    overlay: ProductOverlaySession,
    settings_service: bongocat_app::ApplicationSettingsService,
    frame_source: &FrameSourceShutdown,
    expect_visible_frame: bool,
    require_visible_placement: bool,
    failures: &Arc<Mutex<Vec<String>>>,
) {
    if !frame_source.wait_for_stop().await {
        record_failure(
            failures,
            "product frame source did not stop before runtime shutdown",
        );
    }
    let settings_client = settings_service.client();
    if let Ok(bounds) = overlay.window_bounds() {
        for _ in 0..20 {
            if settings_client
                .update_overlay_window_placement(bounds.x, bounds.y, bounds.width, bounds.height)
                .is_ok()
            {
                break;
            }
            async_io::Timer::after(Duration::from_millis(10)).await;
        }
    }
    if let Err(error) = settings_client.shutdown().await {
        record_failure(failures, error.to_string());
    }
    if let Err(error) = settings_service.join() {
        record_failure(failures, error.to_string());
    }
    match overlay.finish_after_runtime_shutdown() {
        Ok(report) if expect_visible_frame && report.frames_presented == 0 => {
            record_failure(failures, "product overlay presented no frames");
        }
        Ok(report) if require_visible_placement && !report.placement_fully_visible => {
            record_failure(failures, "product overlay left the display bounds");
        }
        Ok(_) => {}
        Err(error) => record_failure(failures, error.to_string()),
    }
}
