//! The parameter ids are stable and unique.

use super::*;

#[test]
fn product_parameter_ids_are_stable_and_unique() {
    let mut ids = ProductParameter::ALL
        .iter()
        .map(|parameter| parameter.id())
        .collect::<Vec<_>>();
    ids.sort_unstable();
    ids.dedup();
    assert_eq!(ids.len(), ProductParameter::ALL.len());
    assert_eq!(ProductParameter::LeftHandDown.id(), "CatParamLeftHandDown");
    assert_eq!(ProductParameter::StickRightY.id(), "CatParamStickRY");
}
