//! Local Linux installation archive; deliberately excluded from updater publishing.
use super::*;
pub(super) fn package(
    workspace: &Path,
    environment: &str,
    target: ReleaseTarget,
) -> Result<Vec<PathBuf>> {
    if env::consts::OS != "linux" {
        return failure("Linux packaging requires a Linux host");
    }
    let core = workspace.join("vendor/cubism/5-r.5/Core/lib/linux/x86_64/libLive2DCubismCore.a");
    if !core.is_file() {
        return failure(format!(
            "Missing {}. Supply the pinned official Cubism Native SDK Linux x86_64 archive.",
            core.display()
        ));
    }
    build_application(workspace, target, environment)?;
    let provenance = write_provenance(
        workspace,
        target,
        environment,
        environment_features(environment),
    )?;
    let staging = tempfile::tempdir_in(workspace.join("target"))?;
    let root = staging.path();
    let copy = |from: PathBuf, to: &str| -> Result<()> {
        let to = root.join(to);
        fs::create_dir_all(to.parent().expect("staging parent"))?;
        fs::copy(from, to)?;
        Ok(())
    };
    let binaries = workspace
        .join("target")
        .join(target.triple())
        .join("release");
    copy(binaries.join(APPLICATION_BINARY), "usr/bin/bongocat-app")?;
    for (source, dest) in [
        (
            "resources/linux/com.ayangweb.bongo-cat.desktop",
            "usr/share/applications/com.ayangweb.bongo-cat.desktop",
        ),
        ("resources/linux/install.sh", "install.sh"),
    ] {
        copy(workspace.join(source), dest)?;
    }
    copy(provenance, "usr/share/bongocat/build-provenance.json")?;
    let output = workspace
        .join(OUTPUT_DIRECTORY)
        .join(target.download_asset());
    fs::create_dir_all(output.parent().unwrap())?;
    let encoder =
        flate2::write::GzEncoder::new(fs::File::create(&output)?, flate2::Compression::default());
    let mut archive = tar::Builder::new(encoder);
    archive.append_dir_all(".", root)?;
    for name in ["LICENSE.md", "NOTICE.md"] {
        archive.append_path_with_name(
            workspace.join("vendor/cubism/5-r.5").join(name),
            format!("usr/share/bongocat/licenses/cubism/{name}"),
        )?;
    }
    archive.into_inner()?.finish()?;
    let executable = output.with_file_name(format!(
        "{}.bin",
        target
            .download_asset()
            .strip_suffix(".tar.gz")
            .expect("Linux archive suffix"),
    ));
    // Replacing the directory entry also works while the previous binary is running.
    let staged_executable = tempfile::NamedTempFile::new_in(executable.parent().unwrap())?;
    fs::copy(binaries.join(APPLICATION_BINARY), staged_executable.path())?;
    staged_executable.persist(&executable)?;
    Ok(vec![executable, output])
}
