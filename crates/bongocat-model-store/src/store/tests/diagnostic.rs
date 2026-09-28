//! The codes a refusal can carry, and the chain behind them.

use super::*;

#[test]
fn package_errors_keep_their_model_source_chain() {
    let source = ModelError {
        code: bongocat_model::ModelDiagnostic::ModelJsonInvalid,
        resource: Some("model3.json".to_owned()),
        detail: "invalid JSON".to_owned(),
    };
    let expected = source.to_string();
    let error = ModelStoreError::package(source);

    assert_eq!(
        std::error::Error::source(&error).map(ToString::to_string),
        Some(expected)
    );
}

#[test]
fn model_store_diagnostic_codes_are_stable_and_unique() {
    let mut codes = ModelStoreDiagnostic::ALL
        .iter()
        .map(|code| code.as_str())
        .collect::<Vec<_>>();
    assert!(codes.iter().all(|code| code.starts_with("model_store_")));
    codes.sort_unstable();
    codes.dedup();
    assert_eq!(codes.len(), ModelStoreDiagnostic::ALL.len());
    assert_eq!(
        ModelStoreDiagnostic::SourceChanged.as_str(),
        "model_store_source_changed"
    );
}
