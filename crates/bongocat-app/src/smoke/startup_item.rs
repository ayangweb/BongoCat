//! The login item: a platform that can have one gets it registered, and the
//! marker it leaves is what the check reads.
//!
//! `SmokeRoot` is a directory the scenario owns and removes on the way out, which
//! is why it has a `Drop`: a smoke that fails halfway must not leave a login item
//! registered on the machine it ran on.

use crate::write_smoke_status;
#[cfg(feature = "storage-test-injection")]
use std::io;
#[cfg(feature = "storage-test-injection")]
use std::path::PathBuf;

/// Reports the startup permission state the product would act on, without showing any prompt.
///
/// This is the repeatable acceptance path for both platforms: it is run once while the capability
/// is missing and once while it is granted, and it never writes product state.
pub(crate) fn run_startup_permission_smoke() -> Result<(), Box<dyn std::error::Error>> {
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
pub(crate) fn run_startup_item_smoke() -> Result<(), Box<dyn std::error::Error>> {
    use bongocat_platform::{
        StartupItemEnvironment, StartupItemState, set_startup_item_enabled, startup_item_state,
    };
    // This mode is gated on the platform, not on `storage-test-injection`, so it
    // cannot rely on the module's feature-gated `std` import block.
    use std::io;

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

#[cfg(feature = "storage-test-injection")]
pub(crate) struct SmokeRoot(pub(crate) PathBuf);

#[cfg(feature = "storage-test-injection")]
impl SmokeRoot {
    pub(crate) fn cleanup(mut self) -> io::Result<()> {
        let result = std::fs::remove_dir_all(&self.0);
        self.0 = PathBuf::new();
        result
    }
}

#[cfg(feature = "storage-test-injection")]
impl Drop for SmokeRoot {
    fn drop(&mut self) {
        if !self.0.as_os_str().is_empty() {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }
}
