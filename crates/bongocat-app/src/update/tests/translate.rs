//! Every source value has a UI value, and a reason is not collapsed.

use super::*;

/// The two code catalogs are a contract: a code that exists in one and not the
/// other would surface in a window as a missing translation.
#[test]
fn every_source_error_code_has_a_ui_code() {
    for code in SourceCode::ALL {
        let mapped = error_code(code);
        assert_eq!(mapped.as_str(), code.as_str());
    }
    assert_eq!(UpdateErrorCode::ALL.len(), SourceCode::ALL.len());
}

#[test]
fn every_source_stage_has_a_ui_stage() {
    let stages = [
        (UpdateStage::Check, UpdateFailureStage::Check),
        (UpdateStage::Download, UpdateFailureStage::Download),
        (UpdateStage::Verify, UpdateFailureStage::Verify),
        (UpdateStage::Install, UpdateFailureStage::Install),
    ];
    for (source, expected) in stages {
        assert_eq!(failure_stage(source), expected);
        assert_eq!(failure_stage(source).as_str(), source.as_str());
    }
}

#[test]
fn unavailability_reasons_are_preserved() {
    assert_eq!(
        unavailable_reason(UpdateUnavailability::DevelopmentChannel),
        UpdateUnavailableReason::DevelopmentBuild
    );
    assert_eq!(
        unavailable_reason(UpdateUnavailability::UnsupportedTarget),
        UpdateUnavailableReason::UnsupportedPlatform
    );
    assert_eq!(
        unavailable_reason(UpdateUnavailability::SigningKeyMissing),
        UpdateUnavailableReason::SigningKeyMissing
    );
}

#[test]
fn progress_is_carried_over_verbatim() {
    let mapped = progress_info(UpdateProgress {
        downloaded_bytes: 4096,
        total_bytes: Some(8192),
    });
    assert_eq!(mapped.downloaded_bytes, 4096);
    assert_eq!(mapped.total_bytes, Some(8192));
    assert_eq!(mapped.percent(), Some(50));
}

/// The restart requirement is a platform fact, not a runtime decision.
#[test]
fn restart_requirement_matches_the_platform() {
    assert_eq!(
        restart_required_after_install(),
        cfg!(target_os = "macos"),
        "macOS replaces the running bundle; Windows hands off to the installer"
    );
}

#[test]
fn an_installed_phase_only_offers_a_restart_where_one_is_needed() {
    let restarting = UpdatePhase::Installed {
        version: "1.1.0".to_owned(),
        restart_required: true,
    };
    assert!(restarting.offers_restart());
    let relaunching = UpdatePhase::Installed {
        version: "1.1.0".to_owned(),
        restart_required: false,
    };
    assert!(!relaunching.offers_restart());
}
