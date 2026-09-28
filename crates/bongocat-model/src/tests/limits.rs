//! The names a model may take.

use super::*;

#[test]
fn model_ids_are_portable_store_keys() {
    assert!(ModelId::parse("keyboard-v2_1").is_ok());
    assert_eq!(
        ModelId::parse("a".repeat(65))
            .expect_err("overlong model id")
            .code,
        ModelDiagnostic::InvalidModelId
    );
    for invalid in [
        "",
        "..",
        ".hidden",
        "trailing.",
        "CON",
        "nul.custom",
        "Com1",
        "LPT9.backup",
        "cat/model",
        "猫",
        "model id",
    ] {
        assert_eq!(
            ModelId::parse(invalid).expect_err("invalid id").code,
            ModelDiagnostic::InvalidModelId
        );
    }
}

proptest! {


    #[test]
    fn accepted_model_ids_exactly_match_the_portable_store_key_contract(value in any::<String>()) {
        let expected = !value.is_empty()
            && value.len() <= 64
            && !value.starts_with('.')
            && !value.ends_with('.')
            && value.bytes().all(|byte| {
                byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.')
            })
            && !is_windows_reserved_name(&value);
        prop_assert_eq!(ModelId::parse(value).is_ok(), expected);
    }
}
