//! References that try to leave the package.

use super::*;

#[test]
fn absolute_and_platform_prefixed_references_are_rejected() {
    for reference in ["/tmp/model.moc3", r"C:\models\model.moc3", "../model.moc3"] {
        assert_eq!(
            normalize_reference(reference)
                .expect_err("escaping reference")
                .code,
            ModelDiagnostic::ModelReferenceEscapesRoot
        );
    }
}

proptest! {


    #[test]
    fn normalized_references_are_idempotent_relative_paths(reference in any::<String>()) {
        if let Ok(normalized) = normalize_reference(&reference) {
            let renormalized = normalize_reference(&normalized);
            prop_assert_eq!(renormalized.as_deref(), Ok(normalized.as_str()));
            prop_assert!(!normalized.chars().any(|character| matches!(character, '\\' | '\0' | ':')));
            prop_assert!(!Path::new(&normalized).is_absolute());
            prop_assert!(normalized.split('/').all(|part| !part.is_empty() && part != "." && part != ".."));
            prop_assert!(path_from_reference(&normalized)
                .components()
                .all(|component| matches!(component, Component::Normal(_))));
        }
    }
}
