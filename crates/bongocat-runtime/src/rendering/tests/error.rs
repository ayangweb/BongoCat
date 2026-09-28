//! A vendor failure becomes a stable runtime category.

use super::*;

#[test]
fn live2d_errors_map_to_stable_runtime_categories() {
    for code in Live2dErrorCode::ALL {
        let error = Live2dError {
            code,
            detail: String::new(),
        };
        let mapped = map_live2d_error(error, RuntimeRenderErrorCode::ModelEvaluationFailed);
        assert_eq!(
            mapped,
            RuntimeRenderErrorCode::ModelEvaluationFailed,
            "unexpected mapping for {code}"
        );
    }
}
