#![forbid(unsafe_code)]

#[cfg(any(target_os = "macos", target_os = "windows"))]
use bongocat_overlay::{OverlaySessionOptions, ProductOverlaySession};
#[cfg(any(target_os = "macos", target_os = "windows"))]
use std::{
    env, fmt,
    io::{self, Write},
    path::Path,
    path::PathBuf,
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
    if !application.config().overlay.visible {
        application.shutdown()?;
        return Ok(());
    }

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

    let session = ProductOverlaySession::start(
        application.runtime_client(),
        application.input_producer(),
        application.cursor_producer(),
        application.take_render_consumer()?,
        overlay_options,
    );
    let mut session = match session {
        Ok(session) => session,
        Err(error) => {
            let mut failures = vec![error.to_string()];
            if let Err(error) = application.shutdown() {
                failures.push(error.to_string());
            }
            return Err(Box::new(ProductRunError { failures }));
        }
    };

    let mut failures = Vec::new();
    if let Err(error) = session.run_for(run_options.run_duration) {
        failures.push(error.to_string());
    }
    if let Err(error) = session.stop_input() {
        failures.push(error.to_string());
    }
    if let Err(error) = application.shutdown() {
        failures.push(error.to_string());
    }
    match session.finish_after_runtime_shutdown() {
        Ok(report) if report.frames_presented == 0 => {
            failures.push("product overlay presented no frames".to_owned());
        }
        Ok(_) => {}
        Err(error) => failures.push(error.to_string()),
    }

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
