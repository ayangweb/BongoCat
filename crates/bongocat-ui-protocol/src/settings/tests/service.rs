//! A closed service is reported, not hung on.

use super::*;

#[test]
fn a_closed_service_returns_a_stable_error() {
    let (client, endpoint) = SettingsClient::bounded(1);
    drop(endpoint);
    let result = client.read_snapshot_blocking();
    assert_eq!(
        result.expect_err("closed service").code(),
        SettingsErrorCode::ServiceUnavailable
    );
}
