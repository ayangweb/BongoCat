use bongocat_platform::{DirectoryPickerOutcome, pick_model_directory};
use std::{env, error::Error, io, path::PathBuf};

#[cfg(target_os = "macos")]
fn prepare_native_application() {
    use objc2::MainThreadMarker;
    use objc2_app_kit::{NSApplication, NSApplicationActivationPolicy};

    let mtm = MainThreadMarker::new().expect("picker smoke must run on the AppKit main thread");
    let application = NSApplication::sharedApplication(mtm);
    let _ = application.setActivationPolicy(NSApplicationActivationPolicy::Regular);
    application.activate();
}

#[cfg(not(target_os = "macos"))]
fn prepare_native_application() {}

enum ExpectedOutcome {
    Cancelled,
    Selected(PathBuf),
}

fn expected_outcome() -> Result<ExpectedOutcome, io::Error> {
    let mut arguments = env::args().skip(1);
    let expected = match arguments.next().as_deref() {
        Some("--expect-cancel") => ExpectedOutcome::Cancelled,
        Some("--expect-selected") => {
            let path = arguments
                .next()
                .ok_or_else(|| io::Error::other("--expect-selected requires a path"))?;
            ExpectedOutcome::Selected(PathBuf::from(path).canonicalize()?)
        }
        _ => {
            return Err(io::Error::other(
                "expected --expect-cancel or --expect-selected <path>",
            ));
        }
    };
    if arguments.next().is_some() {
        return Err(io::Error::other("unexpected picker smoke argument"));
    }
    Ok(expected)
}

fn main() -> Result<(), Box<dyn Error>> {
    let expected = expected_outcome()?;
    prepare_native_application();
    match (expected, pick_model_directory()?) {
        (ExpectedOutcome::Cancelled, DirectoryPickerOutcome::Cancelled) => Ok(()),
        (ExpectedOutcome::Selected(expected), DirectoryPickerOutcome::Selected(actual))
            if actual == expected =>
        {
            Ok(())
        }
        _ => Err(io::Error::other("directory picker returned an unexpected outcome").into()),
    }
}
