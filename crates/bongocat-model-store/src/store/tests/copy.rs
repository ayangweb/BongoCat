//! What an import reports once the bytes are down.

use super::*;

#[test]
fn successful_import_reports_all_stages_and_final_copy_totals() {
    let data = tempdir().expect("data root");
    let store = model_store(data.path());
    let progress = RefCell::new(Vec::new());

    store
        .import_with_observer(
            ModelId::parse("observed").expect("model id"),
            fixture("非 ASCII 模型"),
            |update| progress.borrow_mut().push(update),
            || false,
        )
        .expect("observed import");

    let progress = progress.into_inner();
    assert_eq!(
        progress.first().map(|update| update.stage),
        Some(ModelImportStage::Preparing)
    );
    assert_eq!(
        progress.last().map(|update| update.stage),
        Some(ModelImportStage::Committing)
    );
    assert!(
        progress
            .iter()
            .any(|update| update.stage == ModelImportStage::Copying)
    );
    assert!(
        progress
            .iter()
            .any(|update| update.stage == ModelImportStage::Validating)
    );
    let final_progress = progress.last().expect("final progress");
    assert!(final_progress.files_copied > 0);
    assert!(final_progress.bytes_copied > 0);
    for updates in progress.windows(2) {
        assert!(updates[0].stage <= updates[1].stage);
        assert!(updates[0].files_copied <= updates[1].files_copied);
        assert!(updates[0].bytes_copied <= updates[1].bytes_copied);
    }
}
