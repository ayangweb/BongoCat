//! Rendering an imported model's own cover without owning the main thread.
//!
//! A capture needs a native window, so it runs on the thread that owns the
//! product's windows — but each step draws only one frame, and the sleep between
//! steps is an await on the foreground executor, so the settings window keeps
//! redrawing and the overlay keeps ticking while the capture's model settles.

use async_io::Timer;
use std::sync::Arc;

/// How often the GPUI thread looks for a cover capture the settings worker queued.
///
/// The capture itself is a render of a few dozen frames; this only bounds how long a
/// newly imported model shows the cover its source shipped before the captured one
/// replaces it.
pub(crate) const COVER_CAPTURE_POLL_INTERVAL_MS: u64 = 50;

/// Capture a model cover without owning the main thread for the whole capture.
///
/// The capture needs a native window, so its steps run here, on the thread that
/// owns the product's windows — but each `step` draws only one frame. Between
/// steps the task sleeps the session's frame interval, and that sleep is an await
/// on the foreground executor: the main loop keeps pumping, so the settings
/// window keeps redrawing (the import card's spinner keeps turning) and the
/// overlay frame loop keeps ticking while the capture's model settles (ADR-0055).
pub(crate) async fn capture_model_cover_without_blocking(
    model: Arc<bongocat_model::CommittedModel>,
) -> Result<bongocat_overlay::ModelCoverCapture, bongocat_overlay::OverlayError> {
    let mut session = bongocat_overlay::ModelCoverCaptureSession::start(model)?;
    while session.step()? {
        Timer::after(session.frame_interval()).await;
    }
    session.finish()
}

/// Commit a captured cover, or remove the unusable import before exposing its card.
pub(crate) async fn capture_imported_model_cover(
    client: &bongocat_ui_protocol::SettingsClient,
    request: bongocat_app::CoverCaptureRequest,
    log: &bongocat_app::ApplicationLogHandle,
) -> (bongocat_ui_protocol::SettingsModelKey, bool) {
    let key = request.key().clone();
    let captured = match capture_model_cover_without_blocking(Arc::clone(request.model())).await {
        Ok(cover) => client
            .replace_model_cover(key.clone(), cover.png().to_vec())
            .await
            .is_ok(),
        Err(_) => false,
    };
    if !captured {
        log.record(
            bongocat_app::ApplicationLogEvent::new(
                bongocat_app::ApplicationLogCode::ModelOperationFailed,
            )
            .with_context(bongocat_app::ApplicationLogContext::Operation(
                "cover_capture",
            ))
            .with_context(bongocat_app::ApplicationLogContext::Reason(
                "overlay_capture_failed",
            )),
        );
        let _ = client.delete_model(key.clone()).await;
    }
    (key, captured)
}
