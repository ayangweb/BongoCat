#![forbid(unsafe_code)]

#[cfg(any(target_os = "macos", target_os = "windows"))]
use bongocat_overlay::{OverlaySessionOptions, ProductOverlaySession};
#[cfg(any(target_os = "macos", target_os = "windows"))]
use bongocat_ui::{SettingsError, SettingsErrorCode, SettingsView, open_settings_window};
#[cfg(any(target_os = "macos", target_os = "windows"))]
use gpui::{App, Application as GpuiApplication, Global, Timer, WindowHandle};
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
const DEFAULT_RUN_SECONDS: u64 = 30;

#[cfg(any(target_os = "macos", target_os = "windows"))]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct RunOptions {
    run_duration: Duration,
}

#[cfg(any(target_os = "macos", target_os = "windows"))]
impl RunOptions {
    fn parse(arguments: impl IntoIterator<Item = String>) -> Result<Self, RunOptionsError> {
        let mut arguments = arguments.into_iter();
        let mut run_seconds = DEFAULT_RUN_SECONDS;
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
    "Usage: bongocat-app [--run-seconds <seconds>]\n\nA value of 0 keeps the overlay running until its window is closed."
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
    overlay: Option<ProductOverlaySession>,
    settings_service: Option<bongocat_app::ApplicationSettingsService>,
    settings_window: WindowHandle<SettingsView>,
    frame_source_running: bool,
    expect_visible_frame: bool,
    failures: Arc<Mutex<Vec<String>>>,
}

#[cfg(any(target_os = "macos", target_os = "windows"))]
impl Global for ProductCoordinator {}

#[cfg(any(target_os = "macos", target_os = "windows"))]
fn record_failure(failures: &Arc<Mutex<Vec<String>>>, failure: impl Into<String>) {
    failures
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .push(failure.into());
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
    let mut application = bongocat_app::Application::start()?;

    let model_id = application
        .config()
        .model
        .selected_model_id
        .clone()
        .unwrap_or_else(|| "standard".to_owned());
    let overlay_options = OverlaySessionOptions {
        click_through: application.config().overlay.click_through,
        always_on_top: application.config().overlay.always_on_top,
        scale_percent: application.config().overlay.scale_percent,
        opacity_percent: application.config().overlay.opacity_percent,
        maximum_fps: application.config().model.maximum_fps,
    };
    application.prepare_preset_model(development_preset_root(), model_id)?;
    let runtime_client = application.runtime_client();
    let input_producer = application.input_producer();
    let cursor_producer = application.cursor_producer();
    let render_consumer = application.take_render_consumer()?;
    let expect_visible_frame = application.config().overlay.visible;
    let frame_interval = Duration::from_secs_f64(1.0 / f64::from(overlay_options.maximum_fps));
    let failures = Arc::new(Mutex::new(Vec::new()));
    let run_failures = Arc::clone(&failures);

    GpuiApplication::new().run(move |cx: &mut App| {
        let overlay = match ProductOverlaySession::start(
            runtime_client,
            input_producer,
            cursor_producer,
            render_consumer,
            overlay_options,
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
        let settings_service = match bongocat_app::ApplicationSettingsService::start(application) {
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
        let settings_client = settings_service.client();
        let settings_window = match open_settings_window(settings_client, cx) {
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

        cx.set_global(ProductCoordinator {
            overlay: Some(overlay),
            settings_service: Some(settings_service),
            settings_window,
            frame_source_running: true,
            expect_visible_frame,
            failures: Arc::clone(&run_failures),
        });

        cx.on_app_quit(|cx| {
            let mut coordinator = cx.remove_global::<ProductCoordinator>();
            coordinator.frame_source_running = false;
            let mut overlay = coordinator
                .overlay
                .take()
                .expect("product overlay owner is present");
            if let Err(error) = overlay.stop_input() {
                record_failure(&coordinator.failures, error.to_string());
            }
            let settings_service = coordinator
                .settings_service
                .take()
                .expect("settings service owner is present");
            let settings_client = settings_service.client();
            async move {
                if let Err(error) = settings_client.shutdown().await {
                    record_failure(&coordinator.failures, error.to_string());
                }
                if let Err(error) = settings_service.join() {
                    record_failure(&coordinator.failures, error.to_string());
                }
                match overlay.finish_after_runtime_shutdown() {
                    Ok(report)
                        if coordinator.expect_visible_frame && report.frames_presented == 0 =>
                    {
                        record_failure(
                            &coordinator.failures,
                            "product overlay presented no frames",
                        );
                    }
                    Ok(_) => {}
                    Err(error) => record_failure(&coordinator.failures, error.to_string()),
                }
            }
        })
        .detach();

        cx.spawn(async move |cx| {
            loop {
                Timer::after(frame_interval).await;
                let keep_running = cx
                    .update(|cx| {
                        let (keep_running, failure, settings_window, failures) = {
                            let coordinator = cx.global_mut::<ProductCoordinator>();
                            if !coordinator.frame_source_running {
                                return false;
                            }
                            let result = coordinator
                                .overlay
                                .as_mut()
                                .expect("product overlay owner is present")
                                .tick();
                            match result {
                                Ok(_) => (true, None, None, None),
                                Err(error) => {
                                    coordinator.frame_source_running = false;
                                    (
                                        false,
                                        Some(error.to_string()),
                                        Some(coordinator.settings_window),
                                        Some(Arc::clone(&coordinator.failures)),
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
                        keep_running
                    })
                    .unwrap_or(false);
                if !keep_running {
                    break;
                }
            }
        })
        .detach();

        if !run_options.run_duration.is_zero() {
            cx.spawn(async move |cx| {
                Timer::after(run_options.run_duration).await;
                let _ = cx.update(|cx| cx.quit());
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
fn development_preset_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(3)
        .expect("repository root")
        .join("native/resources/models")
}

#[cfg(not(any(target_os = "macos", target_os = "windows")))]
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let application = bongocat_app::Application::start()?;
    application.shutdown()?;
    Ok(())
}

#[cfg(all(test, any(target_os = "macos", target_os = "windows")))]
mod tests {
    use super::*;

    #[test]
    fn run_options_default_to_a_bounded_preview() {
        assert_eq!(
            RunOptions::parse(Vec::new()).expect("default options"),
            RunOptions {
                run_duration: Duration::from_secs(30)
            }
        );
    }

    #[test]
    fn zero_seconds_selects_an_unbounded_run() {
        assert_eq!(
            RunOptions::parse(["--run-seconds".to_owned(), "0".to_owned()])
                .expect("unbounded options")
                .run_duration,
            Duration::ZERO
        );
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
