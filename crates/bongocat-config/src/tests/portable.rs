//! Where a portable copy keeps its data.
//!
//! The decision is [`portable_application_root`]: given the path of the
//! executable, does this copy keep its settings next to itself? That function
//! is compiled and tested on both supported platforms even though only Windows
//! consults it, because a rule that is reachable on exactly one platform is a
//! rule nothing checks.

use super::*;
use std::fs;
use tempfile::tempdir;

/// An executable in a directory with no marker is not portable.
///
/// This is the case that decides whether an existing installation keeps its
/// settings: portable has to be something the user opts into, never something a
/// build turns on by accident.
#[test]
fn a_directory_without_the_marker_is_not_portable() {
    let base = tempdir().expect("temp directory");
    let executable = base.path().join("bongocat-app.exe");
    fs::write(&executable, b"").expect("write executable");
    assert_eq!(portable_application_root(&executable), None);
}

/// A marker next to the executable makes the copy portable.
#[test]
fn a_marker_file_next_to_the_executable_makes_the_copy_portable() {
    let base = tempdir().expect("temp directory");
    let executable = base.path().join("bongocat-app.exe");
    fs::write(&executable, b"").expect("write executable");
    // The contents are never read, so an empty file is a complete marker. A
    // user creating one with a text editor is the expected path.
    fs::write(base.path().join(PORTABLE_MARKER_FILE_NAME), b"").expect("write marker");
    assert_eq!(
        portable_application_root(&executable),
        Some(base.path().into())
    );
}

/// A *directory* named like the marker is not a request.
///
/// Silently moving a user's data next to their executable is a bad enough
/// outcome that a packaging accident — an empty `portable.txt/` shipped by
/// accident — must not be able to cause it.
#[test]
fn a_directory_named_like_the_marker_is_not_a_request() {
    let base = tempdir().expect("temp directory");
    let executable = base.path().join("bongocat-app.exe");
    fs::write(&executable, b"").expect("write executable");
    fs::create_dir(base.path().join(PORTABLE_MARKER_FILE_NAME)).expect("create marker directory");
    assert_eq!(portable_application_root(&executable), None);
}

/// A marker somewhere else in the tree does not make this copy portable.
///
/// The marker is read next to the executable and nowhere else, so a copy the
/// user extracted into a subdirectory of a portable one is its own copy rather
/// than a silent second writer of the same settings.
#[test]
fn a_marker_outside_the_executable_directory_is_ignored() {
    let base = tempdir().expect("temp directory");
    fs::write(base.path().join(PORTABLE_MARKER_FILE_NAME), b"").expect("write marker");
    let nested = base.path().join("copy-2");
    fs::create_dir(&nested).expect("create nested directory");
    let executable = nested.join("bongocat-app.exe");
    fs::write(&executable, b"").expect("write executable");
    assert_eq!(portable_application_root(&executable), None);
}

/// A portable layout keeps the environment separation an installed one has.
///
/// The marker relocates the root; it does not collapse it. A portable
/// Production copy and a portable Development copy in one folder stay two
/// copies, which is the property the environment directory exists to guarantee.
#[test]
fn a_portable_layout_still_separates_environments() {
    let base = tempdir().expect("temp directory");
    let development = StorageLayout::portable(base.path(), BuildEnvironment::Development);
    let production = StorageLayout::portable(base.path(), BuildEnvironment::Production);

    assert!(development.portable);
    assert!(production.portable);
    assert_eq!(development.root, base.path().join("development"));
    assert_eq!(production.root, base.path().join("production"));
    assert_ne!(development.root, production.root);
    // Every path a portable layout hands out lives under its own root, so
    // nothing escapes into the executable's directory.
    for layout in [&development, &production] {
        for path in [
            &layout.config,
            &layout.window_state,
            &layout.models,
            &layout.model_overrides,
            &layout.backups,
            &layout.logs,
            &layout.updates,
            &layout.locks,
        ] {
            assert!(
                path.starts_with(&layout.root),
                "{} escaped the portable root {}",
                path.display(),
                layout.root.display()
            );
        }
    }
}

/// An installed layout is not portable, and says so.
#[test]
fn an_installed_layout_reports_that_it_is_not_portable() {
    let base = tempdir().expect("temp directory");
    assert!(
        !StorageLayout::under(base.path(), BuildEnvironment::Production).portable,
        "a layout under the platform data directory is not portable"
    );
    assert!(
        !StorageLayout::under_application_root(base.path(), BuildEnvironment::Production).portable,
        "naming an application root does not make it portable"
    );
}

/// A portable layout has the same shape as an installed one.
///
/// A portable copy is the same product with a different root, so the file names
/// inside it have to be the ones an upgrade or a backup already knows.
#[test]
fn a_portable_layout_has_the_same_shape_as_an_installed_one() {
    let base = tempdir().expect("temp directory");
    let installed = StorageLayout::under(base.path(), BuildEnvironment::Production);
    let portable = StorageLayout::portable(base.path(), BuildEnvironment::Production);

    let shape = |layout: &StorageLayout| {
        [
            layout.config.clone(),
            layout.window_state.clone(),
            layout.models.clone(),
            layout.model_overrides.clone(),
            layout.backups.clone(),
            layout.logs.clone(),
            layout.updates.clone(),
            layout.locks.clone(),
        ]
    };
    let relative = |layout: &StorageLayout| {
        shape(layout)
            .iter()
            .map(|path| {
                path.strip_prefix(&layout.root)
                    .expect("every path is under the root")
                    .to_path_buf()
            })
            .collect::<Vec<_>>()
    };
    assert_eq!(relative(&installed), relative(&portable));
    assert_ne!(installed.root, portable.root);
}

/// The installed path is unchanged, which is what makes portable opt-in.
///
/// This is the case every existing installation is in, so it is the one that
/// decides whether an upgrade moves anybody's settings. It asserts against the
/// platform data directory rather than a temp directory because that is the
/// root an installed copy actually resolves to.
#[test]
fn a_copy_with_no_marker_keeps_the_platform_data_directory() {
    let base = tempdir().expect("temp directory");
    let executable = base.path().join("bongocat-app.exe");
    fs::write(&executable, b"").expect("write executable");

    let layout =
        layout_for_executable(&executable, BuildEnvironment::Production).expect("installed layout");
    assert!(!layout.portable);
    assert!(layout.root.ends_with("production"));
    assert!(
        layout.root.starts_with(
            dirs::data_dir()
                .expect("a platform data directory on a supported platform")
                .join(BUNDLE_ID)
        ),
        "an installed copy keeps its settings in the platform data directory, got {}",
        layout.root.display()
    );
}

/// A marker makes the resolved layout portable, with the marker directory as
/// the application root.
///
/// This is the composition the Windows entry point performs, exercised here
/// against a real directory so the whole decision — not only the marker rule —
/// is covered on a platform that cannot use it.
#[test]
fn a_copy_with_a_marker_resolves_to_a_portable_layout() {
    let base = tempdir().expect("temp directory");
    let executable = base.path().join("bongocat-app.exe");
    fs::write(&executable, b"").expect("write executable");
    fs::write(base.path().join(PORTABLE_MARKER_FILE_NAME), b"").expect("write marker");

    let layout =
        layout_for_executable(&executable, BuildEnvironment::Production).expect("portable layout");
    assert!(layout.portable);
    assert_eq!(layout.root, base.path().join("production"));
    // Nothing the layout hands out may land outside the executable's directory:
    // a portable copy that still wrote anything to the data directory would
    // leave part of its state behind when the folder is moved.
    for path in [
        &layout.config,
        &layout.window_state,
        &layout.models,
        &layout.model_overrides,
        &layout.backups,
        &layout.logs,
        &layout.updates,
        &layout.locks,
    ] {
        assert!(
            path.starts_with(base.path()),
            "{} escaped the portable folder {}",
            path.display(),
            base.path().display()
        );
    }
}
