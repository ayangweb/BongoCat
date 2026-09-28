//! A preview never regresses, and a superseded one is not overwritten.

use super::*;

#[test]
fn model_generation_may_skip_rejected_candidates_but_never_regress() {
    validate_model_generation_advance(4, 7).expect("rejected generations may be skipped");
    assert!(validate_model_generation_advance(4, 4).is_err());
    assert!(validate_model_generation_advance(4, 3).is_err());
}
