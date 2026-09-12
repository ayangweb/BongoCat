use crate::{
    StartupItemEnvironment, StartupItemError, StartupItemState, StartupItemUnsupportedReason,
};
use objc2::runtime::AnyClass;
use objc2_service_management::{SMAppService, SMAppServiceStatus};

pub(super) fn state(
    environment: StartupItemEnvironment,
) -> Result<StartupItemState, StartupItemError> {
    if environment == StartupItemEnvironment::Development {
        return Ok(StartupItemState::Unsupported(
            StartupItemUnsupportedReason::BuildEnvironment,
        ));
    }
    let Some(service) = main_app_service() else {
        return Ok(StartupItemState::Unsupported(
            StartupItemUnsupportedReason::OperatingSystem,
        ));
    };
    // SAFETY: main_app_service first proves the SMAppService class exists at runtime. The retained
    // service remains alive for this Objective-C message and status returns a value type.
    Ok(map_status(unsafe { service.status() }))
}

pub(super) fn set_enabled(
    environment: StartupItemEnvironment,
    enabled: bool,
) -> Result<StartupItemState, StartupItemError> {
    let current = state(environment)?;
    if matches!(current, StartupItemState::Unsupported(_)) {
        return Ok(current);
    }
    if (enabled
        && matches!(
            current,
            StartupItemState::Enabled | StartupItemState::RequiresApproval
        ))
        || (!enabled && current == StartupItemState::Disabled)
    {
        return Ok(current);
    }
    let Some(service) = main_app_service() else {
        return Err(StartupItemError::BackendUnavailable);
    };
    if enabled {
        // SAFETY: class availability was checked before constructing the retained service. The
        // generated binding owns the NSError and does not let it escape this stable adapter.
        unsafe { service.registerAndReturnError() }.map_err(|_| StartupItemError::EnableFailed)?;
    } else {
        // SAFETY: the same class/retained-owner invariant as registration applies. NSError details
        // are intentionally discarded so paths and platform error objects never cross the boundary.
        unsafe { service.unregisterAndReturnError() }
            .map_err(|_| StartupItemError::DisableFailed)?;
    }
    // SAFETY: service remains retained and class availability was established above.
    Ok(map_status(unsafe { service.status() }))
}

fn main_app_service() -> Option<objc2::rc::Retained<SMAppService>> {
    AnyClass::get(c"SMAppService")?;
    // SAFETY: AnyClass::get proves the macOS 13+ class exists before the generated binding resolves
    // and sends +mainAppService. The method returns a retained object managed by objc2.
    Some(unsafe { SMAppService::mainAppService() })
}

fn map_status(status: SMAppServiceStatus) -> StartupItemState {
    if status == SMAppServiceStatus::NotRegistered {
        StartupItemState::Disabled
    } else if status == SMAppServiceStatus::Enabled {
        StartupItemState::Enabled
    } else if status == SMAppServiceStatus::RequiresApproval {
        StartupItemState::RequiresApproval
    } else {
        StartupItemState::NotFound
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn service_management_statuses_map_to_stable_states() {
        assert_eq!(
            map_status(SMAppServiceStatus::NotRegistered),
            StartupItemState::Disabled
        );
        assert_eq!(
            map_status(SMAppServiceStatus::Enabled),
            StartupItemState::Enabled
        );
        assert_eq!(
            map_status(SMAppServiceStatus::RequiresApproval),
            StartupItemState::RequiresApproval
        );
        assert_eq!(
            map_status(SMAppServiceStatus::NotFound),
            StartupItemState::NotFound
        );
        assert_eq!(
            map_status(SMAppServiceStatus(99)),
            StartupItemState::NotFound
        );
    }

    #[test]
    fn development_is_unsupported_without_resolving_the_production_service() {
        assert_eq!(
            state(StartupItemEnvironment::Development),
            Ok(StartupItemState::Unsupported(
                StartupItemUnsupportedReason::BuildEnvironment
            ))
        );
        assert_eq!(
            set_enabled(StartupItemEnvironment::Development, true),
            Ok(StartupItemState::Unsupported(
                StartupItemUnsupportedReason::BuildEnvironment
            ))
        );
    }
}
