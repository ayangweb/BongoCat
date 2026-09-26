//! The catalogue round-trips, and owns its severity and message.

use super::*;
#[test]
fn code_catalog_round_trips_and_owns_its_severity_and_message() {
    let mut codes = BTreeSet::new();
    let mut messages = BTreeSet::new();
    for code in ApplicationLogCode::ALL {
        assert!(codes.insert(code.as_str()));
        assert!(messages.insert(code.message()));
        assert_eq!(ApplicationLogCode::parse(code.as_str()), Some(*code));
        let record = ApplicationLogEvent::new(*code).to_record(SystemTime::UNIX_EPOCH);
        assert_eq!(record.level, code.level());
        assert_eq!(record.code, code.as_str());
        assert_eq!(record.module, code.component().as_str());
        assert_eq!(record.message, code.message());
    }
    assert_eq!(ApplicationLogCode::parse("unknown/event"), None);
}
