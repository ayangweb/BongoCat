use crate::{StartupItemEnvironment, StartupItemError, StartupItemState};
use auto_launch::{AutoLaunch, AutoLaunchBuilder, MacOSLaunchMode, WindowsEnableMode};

const PRODUCT_ARGUMENTS: [&str; 2] = ["--run-seconds", "0"];
const PRODUCT_BUNDLE_IDENTIFIER: &str = "com.ayangweb.bongo-cat";

pub(super) fn state(
    environment: StartupItemEnvironment,
) -> Result<StartupItemState, StartupItemError> {
    let backend = backend(environment)?;
    match backend.is_enabled() {
        Ok(true) => Ok(StartupItemState::Enabled),
        Ok(false) => Ok(StartupItemState::Disabled),
        Err(_) => Err(StartupItemError::StateReadFailed),
    }
}

pub(super) fn set_enabled(
    environment: StartupItemEnvironment,
    enabled: bool,
) -> Result<StartupItemState, StartupItemError> {
    let backend = backend(environment)?;
    let current = backend
        .is_enabled()
        .map_err(|_| StartupItemError::StateReadFailed)?;
    if current == enabled {
        return state(environment);
    }
    if enabled {
        backend.enable().map_err(enable_error)?;
    } else {
        backend
            .disable()
            .map_err(|_| StartupItemError::DisableFailed)?;
    }
    state(environment)
}

fn enable_error(error: auto_launch::Error) -> StartupItemError {
    match error {
        auto_launch::Error::AppPathDoesntExist(_) | auto_launch::Error::AppPathIsNotAbsolute(_) => {
            StartupItemError::InvalidExecutablePath
        }
        _ => StartupItemError::EnableFailed,
    }
}

fn backend(environment: StartupItemEnvironment) -> Result<AutoLaunch, StartupItemError> {
    let executable =
        std::env::current_exe().map_err(|_| StartupItemError::CurrentExecutableUnavailable)?;
    if !executable.is_absolute() {
        return Err(StartupItemError::InvalidExecutablePath);
    }
    let executable = executable
        .to_str()
        .ok_or(StartupItemError::InvalidExecutablePath)?;
    // The macOS-only and Windows-only setters are cross-platform no-ops in auto-launch, so a
    // single constructor keeps both backends on the same explicitly selected modes.
    let mut builder = AutoLaunchBuilder::new();
    builder
        .set_app_name(app_name(environment))
        .set_app_path(executable)
        .set_args(&PRODUCT_ARGUMENTS)
        .set_macos_launch_mode(MacOSLaunchMode::LaunchAgent)
        .set_bundle_identifiers(&[PRODUCT_BUNDLE_IDENTIFIER])
        .set_windows_enable_mode(WindowsEnableMode::CurrentUser);
    builder
        .build()
        .map_err(|_| StartupItemError::BackendUnavailable)
}

const fn app_name(environment: StartupItemEnvironment) -> &'static str {
    match environment {
        StartupItemEnvironment::Development => "BongoCat Development",
        StartupItemEnvironment::Production => "BongoCat Production",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn environments_use_distinct_stable_app_names() {
        assert_eq!(
            app_name(StartupItemEnvironment::Development),
            "BongoCat Development"
        );
        assert_eq!(
            app_name(StartupItemEnvironment::Production),
            "BongoCat Production"
        );
    }

    #[test]
    fn backend_builds_for_both_environments() {
        for environment in [
            StartupItemEnvironment::Development,
            StartupItemEnvironment::Production,
        ] {
            assert!(backend(environment).is_ok());
        }
    }

    #[test]
    #[ignore = "mutates and restores the current user's Development startup item"]
    fn startup_item_lifecycle_smoke_restores_original_state() {
        let environment = StartupItemEnvironment::Development;
        let original = state(environment).unwrap();
        let production_before = state(StartupItemEnvironment::Production).unwrap();

        set_enabled(environment, false).unwrap();
        assert_eq!(state(environment), Ok(StartupItemState::Disabled));

        assert_eq!(
            set_enabled(environment, true),
            Ok(StartupItemState::Enabled)
        );
        assert_eq!(state(environment), Ok(StartupItemState::Enabled));
        #[cfg(target_os = "macos")]
        assert!(launch_agent_plist_path(app_name(environment)).exists());

        assert_eq!(
            set_enabled(environment, false),
            Ok(StartupItemState::Disabled)
        );
        #[cfg(target_os = "macos")]
        assert!(!launch_agent_plist_path(app_name(environment)).exists());

        if matches!(original, StartupItemState::Enabled) {
            set_enabled(environment, true).unwrap();
        }
        assert_eq!(state(environment), Ok(original));
        assert_eq!(
            state(StartupItemEnvironment::Production),
            Ok(production_before)
        );
    }

    #[cfg(target_os = "macos")]
    fn launch_agent_plist_path(app_name: &str) -> std::path::PathBuf {
        let home = std::env::var("HOME").expect("HOME environment variable");
        std::path::Path::new(&home)
            .join("Library")
            .join("LaunchAgents")
            .join(format!("{app_name}.plist"))
    }
}
