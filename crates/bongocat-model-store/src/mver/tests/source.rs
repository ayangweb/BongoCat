//! A source is read without following anything out of it.

use super::*;

#[test]
fn source_walk_enforces_the_existing_directory_depth_limit() {
    let root = tempdir().expect("root");
    fs::create_dir_all(root.path().join("one/two")).expect("nested source");
    let limits = ModelPackageLimits {
        maximum_directory_depth: 1,
        ..ModelPackageLimits::default()
    };
    let mut files = Vec::new();

    let error = collect_source_files(root.path(), "", limits, &mut files)
        .expect_err("directory beyond the source limit");
    assert_eq!(error.code, ModelStoreDiagnostic::SourceConversionFailed);
    assert!(
        error
            .detail
            .contains("legacy source is nested deeper than the package limit allows")
    );
}

#[cfg(unix)]
#[test]
fn a_symbolic_link_inside_the_source_is_rejected_rather_than_followed() {
    use std::os::unix::fs::symlink;

    let root = tempdir().expect("root");
    let outside = tempdir().expect("outside");
    write(
        outside.path(),
        "secret.png",
        &encode_png(2, 2, |_, _| [1, 2, 3, 255]),
    );
    legacy_source(
        root.path(),
        &[(
            MverInputMode::Standard,
            r#"{"hand":[[65]],"keyboard":[[65]]}"#,
        )],
        true,
    );
    fs::remove_file(root.path().join("img/standard/hand/0.png")).expect("remove hand image");
    symlink(
        outside.path().join("secret.png"),
        root.path().join("img/standard/hand/0.png"),
    )
    .expect("symlink hand image");

    let error = inspect(
        &MverSource::directory(root.path()).expect("open legacy source"),
        ModelPackageLimits::default(),
    )
    .expect_err("symlinked legacy source");
    assert_eq!(error.code, ModelStoreDiagnostic::SourceSymlinkUnsupported);
}

#[test]
fn conversion_refuses_resources_beyond_the_legacy_read_bound() {
    let root = tempdir().expect("root");
    legacy_source(
        root.path(),
        &[(
            MverInputMode::Standard,
            r#"{"hand":[[65]],"keyboard":[[65]]}"#,
        )],
        true,
    );
    // A sparse file, so the test never allocates the bytes it is bounding.
    let oversized = root.path().join("img/standard/cat_model/oversized.bin");
    File::create(&oversized)
        .expect("create oversized resource")
        .set_len(LEGACY_RESOURCE_MAXIMUM_BYTES + 1)
        .expect("size oversized resource");

    let plan = inspect_directory(root.path()).expect("legacy plan");
    let staging = tempdir().expect("staging");
    let error = convert(
        root.path(),
        &plan_mode(&plan, MverInputMode::Standard),
        staging.path(),
    )
    .expect_err("oversized resource");
    assert_eq!(error.code, ModelStoreDiagnostic::SourceConversionFailed);
    assert_eq!(
        error.resource.as_deref(),
        Some("img/standard/cat_model/oversized.bin")
    );
}
