//! The codes are stable, unique, and name the resource.

use super::*;

#[test]
fn live2d_error_codes_are_stable_and_unique() {
    let mut codes = Live2dErrorCode::ALL
        .iter()
        .map(|code| code.as_str())
        .collect::<Vec<_>>();
    assert!(codes.iter().all(|code| !code.is_empty()));
    codes.sort_unstable();
    codes.dedup();
    assert_eq!(codes.len(), Live2dErrorCode::ALL.len());
    assert_eq!(Live2dErrorCode::MotionInvalid.to_string(), "motion_invalid");
    let error = Live2dError::new(Live2dErrorCode::ResourceIo, "/private/model.moc3");
    assert!(error.to_string().starts_with("resource_io: "));
}
