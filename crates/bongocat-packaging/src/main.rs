//! The BongoCat project-level build, bundle and installer packaging entry point.
//!
//! `just build` runs this crate. Local developers and CI execute the exact same
//! code path, so there is a single place that decides how the product is
//! compiled and packaged:
//!
//! 1. compile `bongocat-app` for one release target, with the immutable
//!    build-environment Cargo feature selected,
//! 2. write the path-free build provenance record,
//! 3. hand the resulting executable to `cargo-packager`, which owns the bundle
//!    and installer layout: the macOS `.app` and the Windows NSIS `.exe`,
//! 4. wrap the finished `.app` in a `.dmg` with the macOS disk-image tooling,
//! 5. when the release pipeline provisioned a signing key, build the updater payload
//!    (the macOS bundle as a `.tar.gz`; the Windows installer as published), sign it
//!    with Minisign, and write this target's fragment of the release manifest.
//!
//! `--merge-manifests` is the second entry point. The updater reads **one** shared
//! manifest, but each target is built in its own job and can only announce the payload
//! it produced, so the fragments have to be combined before publication. That merge
//! lives here rather than in the pipeline for the same reason the rest of the packaging
//! does: the manifest shape stays owned by one place, and the release workflow only
//! calls the tool.
//!
//! `--generate-signing-key` is the third: a one-time, offline provisioning step that
//! creates the Minisign key pair signing is done with. It lives here so that provisioning
//! a key uses the same pinned toolchain as signing it, instead of asking a maintainer to
//! `cargo install` a matching global binary.
//!
//! Only product-specific facts live here: which targets ship, where the runtime
//! expects its bundled resources, and what the macOS bundle declares. Bundle
//! contents, `Info.plist` generation and the NSIS installer are owned by
//! `cargo-packager` and must not be re-implemented in this repository.
//!
//! # Why step 4 is not also delegated to `cargo-packager`
//!
//! `cargo-packager` 0.11.8 builds its DMG with `create-dmg` pinned to the 2022
//! commit `28867ba`, and that script cannot build a DMG on current macOS. It
//! reads the attach output as
//!
//! ```text
//! hdiutil attach ... | grep -E '^/dev/' | sed 1q | awk '{print $1}'
//! ```
//!
//! which closes `hdiutil`'s stdout after the first line. The resulting SIGPIPE
//! leaves the mount unfinished, and the script's own `hdiutil detach <device>`
//! then fails with `detach failed - No such file or directory`. Reproduced
//! deterministically on macOS 26.5 (invoking `create-dmg` with the exact
//! argument list `cargo-packager` uses): three of three runs fail, while
//! consuming the full attach output and detaching succeeds three of three.
//! Pinning a newer `create-dmg` is not available either — the URL and revision
//! are compile-time constants in `cargo-packager`, and its current `main` still
//! pins the same revision.
//!
//! Building `.dmg` files is therefore left to the operating system's own
//! disk-image tooling, which is also what `create-dmg` wraps. This is the
//! smallest possible step — stage the finished bundle, add the `/Applications`
//! drop link, compress, sign — and it never touches bundle or installer layout.
//!
//! Exit condition: restore `PackageFormat::Dmg` once `cargo-packager` ships a
//! `create-dmg` revision that works on the current macOS. See
//! `docs/adr/0033-build-packaging-and-release-toolchain.md`.

#![allow(clippy::print_stdout, clippy::print_stderr)]

use std::{
    collections::BTreeMap,
    env, fmt, fs,
    path::{Path, PathBuf},
    process::Command,
};

use cargo_packager::{
    Config, PackageFormat,
    config::{Binary, MacOsConfig, NSISInstallerMode, NsisConfig, Resource},
    sign::SigningConfig,
};
use serde::{Deserialize, Serialize};

/// Product name. Determines `BongoCat.app` and the installer product name.
const PRODUCT_NAME: &str = "BongoCat";
/// The fixed Native Rewrite bundle identifier.
const BUNDLE_IDENTIFIER: &str = "com.ayangweb.bongo-cat";
/// The oldest macOS release the product supports.
const MACOS_MINIMUM_SYSTEM_VERSION: &str = "12.0";
/// The product executable. `bongocat-update` pins this name for release archives.
const APPLICATION_BINARY: &str = "bongocat-app";
/// Preset models that must ship inside every packaged artifact.
const PRESET_MODELS: [&str; 3] = ["standard", "keyboard", "gamepad"];
/// Repository-relative directory holding icons and preset models.
const RESOURCE_DIRECTORY: &str = "resources";
/// Repository-relative directory holding the three preset models.
const MODEL_DIRECTORY: &str = "models";
/// Repository-relative directory holding the macOS `Info.plist` overlay.
const MACOS_INFO_PLIST: &str = "macos/Info.plist";
/// Repository-relative build provenance generator.
const PROVENANCE_GENERATOR: &str = "tools/record-native-provenance.py";
/// Build provenance file name inside the packaged resources.
const PROVENANCE_FILE: &str = "build-provenance.json";
/// Package output directory, relative to the workspace root.
const OUTPUT_DIRECTORY: &str = "target/package";
/// Staging directory for generated packaging inputs, relative to the output directory.
const STAGING_DIRECTORY: &str = "provenance";
/// Staging directory for the disk image contents, relative to the output directory.
///
/// Only the macOS disk image builder reads it, so it is Unix-only: a Windows
/// build would otherwise carry a constant no code path can reach, which the
/// workspace's `-D warnings` gate rejects.
#[cfg(unix)]
const DISK_IMAGE_STAGING_DIRECTORY: &str = "dmg-stage";
/// Ad-hoc signature used when no distribution identity is provisioned.
///
/// It keeps the bundle and disk image integrity verifiable locally and in CI.
/// Distribution signing, hardened runtime and notarization stay separate release
/// gates.
const ADHOC_SIGNING_IDENTITY: &str = "-";
/// Overrides the macOS signing identity for release builds.
///
/// Signing certificates cannot be committed, so the release pipeline injects the
/// real identity through this variable. It is a credential injection point, not
/// a hidden build step: an unset or empty value keeps the ad-hoc identity.
const MACOS_SIGNING_IDENTITY_VARIABLE: &str = "BONGOCAT_MACOS_SIGNING_IDENTITY";
/// Selected through `--environment`; `production` is the release default.
const BUILD_ENVIRONMENTS: [&str; 2] = ["development", "production"];
/// Cargo feature that turns the default Development build into Production.
const PRODUCTION_FEATURE: &str = "production";
/// Carries the Minisign private key that signs update payloads.
///
/// Release signing keys cannot be committed, so the release pipeline injects the
/// key through this variable — the same credential-injection shape as
/// [`MACOS_SIGNING_IDENTITY_VARIABLE`]. An unset or empty value means the build
/// produces bundle and installer artifacts but no update assets, which is what a
/// local development build wants.
const SIGNING_PRIVATE_KEY_VARIABLE: &str = "SIGNING_PRIVATE_KEY";
/// Password of the private key in [`SIGNING_PRIVATE_KEY_VARIABLE`].
///
/// An empty value is meaningful: it is the "encrypted with an empty password" case
/// that `cargo-packager`'s signer produces when asked to skip the interactive prompt.
const SIGNING_PRIVATE_KEY_PASSWORD_VARIABLE: &str = "SIGNING_PRIVATE_KEY_PASSWORD";
/// Name of the shared release manifest the updater requests.
///
/// `bongocat-update` reads this asset from the repository's *latest* release, so it has
/// to be one file describing every target. `tools/tests/test_update_release_contract.py`
/// pins the name against the runtime's own constant.
const UPDATE_MANIFEST_NAME: &str = "latest.json";
/// Suffix of the per-target manifest fragment a build writes.
///
/// The fragment is named after the `<os>-<arch>` platform key, because that key is what
/// the merge writes into the shared manifest and what the updater looks this host up
/// under. A release job can therefore only announce the payload it produced itself.
const UPDATE_FRAGMENT_SUFFIX: &str = ".json";
/// The repository that publishes releases; the manifest links its assets from here.
const RELEASE_REPOSITORY_URL: &str = "https://github.com/ayangweb/BongoCat";

type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

/// A message-only failure with no wrapped source error.
#[derive(Debug)]
struct Failure(String);

impl fmt::Display for Failure {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl std::error::Error for Failure {}

fn failure<T>(message: impl Into<String>) -> Result<T> {
    Err(Box::new(Failure(message.into())))
}

/// Every target BongoCat publishes artifacts for.
///
/// `bongocat-update::UpdateTargetTriple` declares the same three combinations
/// for release-asset matching; `tools/tests/test_update_release_contract.py`
/// fails the build if the two lists ever drift apart.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ReleaseTarget {
    MacosAarch64,
    MacosX86_64,
    WindowsX86_64,
}

impl ReleaseTarget {
    const ALL: [Self; 3] = [Self::WindowsX86_64, Self::MacosX86_64, Self::MacosAarch64];

    const fn triple(self) -> &'static str {
        match self {
            Self::MacosAarch64 => "aarch64-apple-darwin",
            Self::MacosX86_64 => "x86_64-apple-darwin",
            Self::WindowsX86_64 => "x86_64-pc-windows-msvc",
        }
    }

    /// Short architecture token used in release artifact file names.
    const fn architecture(self) -> &'static str {
        match self {
            Self::MacosAarch64 => "arm64",
            Self::MacosX86_64 => "x64",
            Self::WindowsX86_64 => "x64",
        }
    }

    /// The file name the release publishes this target's installer under.
    ///
    /// `None` for the Apple targets: their `.app` and `.dmg` are already named by
    /// this crate. The Windows installer is named by `cargo-packager` instead,
    /// which hard-codes `{main binary name}_{version}_{arch}-setup.exe` with no
    /// option to configure it, so it is renamed to this name after packaging.
    /// Version and architecture come from the same sources as every other
    /// artifact, so neither is hard-coded here.
    fn installer_file_name(self) -> Option<String> {
        match self {
            Self::WindowsX86_64 => Some(format!(
                "{PRODUCT_NAME}_{}_{}.exe",
                env!("CARGO_PKG_VERSION"),
                self.architecture()
            )),
            Self::MacosAarch64 | Self::MacosX86_64 => None,
        }
    }

    fn parse(triple: &str) -> Result<Self> {
        Self::ALL
            .into_iter()
            .find(|target| target.triple() == triple)
            .ok_or_else(|| {
                Box::new(Failure(format!(
                    "unsupported target {triple}; BongoCat ships {}",
                    Self::ALL.map(Self::triple).join(", ")
                ))) as Box<dyn std::error::Error>
            })
    }

    const fn is_apple(self) -> bool {
        matches!(self, Self::MacosAarch64 | Self::MacosX86_64)
    }

    /// The release artifacts this target publishes.
    const fn release_formats(self) -> &'static [PackageFormat] {
        match self {
            Self::MacosAarch64 | Self::MacosX86_64 => &[PackageFormat::App, PackageFormat::Dmg],
            Self::WindowsX86_64 => &[PackageFormat::Nsis],
        }
    }

    /// The `<os>-<arch>` key this target is announced under in the release manifest.
    ///
    /// `bongocat-update` declares the same keys in
    /// `UpdateTargetTriple::manifest_platform`; `tools/tests/test_update_release_contract.py`
    /// fails the build if the two ever drift apart.
    const fn manifest_platform(self) -> &'static str {
        match self {
            Self::MacosAarch64 => "macos-aarch64",
            Self::MacosX86_64 => "macos-x86_64",
            Self::WindowsX86_64 => "windows-x86_64",
        }
    }

    /// The payload format the release manifest announces for this target.
    ///
    /// The updater installs an `app` payload by replacing the bundle and an `nsis`
    /// payload by running the installer it downloaded.
    const fn update_format(self) -> &'static str {
        match self {
            Self::MacosAarch64 | Self::MacosX86_64 => "app",
            Self::WindowsX86_64 => "nsis",
        }
    }

    /// The file name this target's update payload is published under.
    ///
    /// The updater takes the payload from the manifest rather than matching an asset
    /// name, so these names only have to be stable and self-describing. Windows
    /// reuses the installer it already publishes; macOS needs the bundle wrapped in
    /// an archive, because the updater installs a directory.
    fn update_payload_name(self) -> String {
        match self.installer_file_name() {
            Some(name) => name,
            None => format!(
                "{PRODUCT_NAME}-{}-{}.app.tar.gz",
                env!("CARGO_PKG_VERSION"),
                self.triple()
            ),
        }
    }

    /// The target this process runs on.
    fn host() -> Result<Self> {
        match (env::consts::OS, env::consts::ARCH) {
            ("macos", "aarch64") => Ok(Self::MacosAarch64),
            ("macos", "x86_64") => Ok(Self::MacosX86_64),
            ("windows", "x86_64") => Ok(Self::WindowsX86_64),
            (os, arch) => failure(format!(
                "{os}/{arch} is not a BongoCat release host; packaging must run on one of {}",
                Self::ALL.map(Self::triple).join(", ")
            )),
        }
    }
}

/// One parsed invocation of the packaging tool.
enum Invocation {
    /// Build and package one release target.
    Package(Options),
    /// Merge the per-target fragments into the shared release manifest.
    MergeManifest {
        directory: PathBuf,
        fragments: Vec<PathBuf>,
        /// The release's changelog, read from a file, announced to the updater.
        release_notes: Option<PathBuf>,
    },
    /// Compose this version's release notes from the bilingual changelog.
    ExtractReleaseNotes(PathBuf),
    /// Generate the Minisign key pair that signs update payloads.
    GenerateSigningKey(PathBuf),
}

/// Options for one packaging run.
struct Options {
    target: Option<ReleaseTarget>,
    environment: String,
    formats: Option<Vec<PackageFormat>>,
}

impl Invocation {
    const USAGE: &'static str = "\
usage: cargo run -p bongocat-packaging -- [options]

Builds the Production product and packages the host platform release artifacts.

options:
  --target <triple>        release target; defaults to the host target
  --environment <name>     development | production (default: production)
  --formats <list>         comma separated subset of the target's release
                           artifacts (app,dmg for macOS; nsis for Windows)
  --merge-manifests <dir>  merge the per-target fragments that follow into the
                           shared release manifest, written to <dir>/latest.json,
                           instead of packaging; takes no other option except
                           --release-notes
  --release-notes <file>   read the release changelog from <file> and announce it in
                           the merged manifest, so the update window can show what
                           changed; only valid with --merge-manifests
  --extract-release-notes <file>
                           compose this version's release notes from CHANGELOG.md and
                           CHANGELOG.zh-CN.md and write them to <file>, instead of
                           packaging; takes no other option
  --generate-signing-key <file>
                           generate a new Minisign key pair for signing update
                           payloads, written to <file> and <file>.pub, instead of
                           packaging; takes no other option
  --print-version          print the product version and exit
  -h, --help               print this help

environment:
  SIGNING_PRIVATE_KEY_PASSWORD  passphrase for the generated private key,
                           and the passphrase used to unlock it when signing";

    fn parse(arguments: Vec<String>) -> Result<Self> {
        let mut target = None;
        let mut environment = None;
        let mut formats = None;
        let mut merge_directory: Option<PathBuf> = None;
        let mut release_notes: Option<PathBuf> = None;
        let mut extract_notes: Option<PathBuf> = None;
        let mut key_output: Option<PathBuf> = None;
        let mut fragments = Vec::new();

        let mut arguments = arguments.into_iter();
        while let Some(argument) = arguments.next() {
            match argument.as_str() {
                "--target" => {
                    let triple = next_value(&mut arguments, "--target")?;
                    target = Some(ReleaseTarget::parse(&triple)?);
                }
                "--environment" => {
                    let value = next_value(&mut arguments, "--environment")?;
                    if !BUILD_ENVIRONMENTS.contains(&value.as_str()) {
                        return failure(format!(
                            "unknown build environment {value}; expected one of {}",
                            BUILD_ENVIRONMENTS.join(", ")
                        ));
                    }
                    environment = Some(value);
                }
                "--formats" => {
                    let value = next_value(&mut arguments, "--formats")?;
                    formats = Some(parse_formats(&value)?);
                }
                "--merge-manifests" => {
                    let directory = next_value(&mut arguments, "--merge-manifests")?;
                    merge_directory = Some(PathBuf::from(directory));
                }
                "--release-notes" => {
                    let file = next_value(&mut arguments, "--release-notes")?;
                    release_notes = Some(PathBuf::from(file));
                }
                "--extract-release-notes" => {
                    let file = next_value(&mut arguments, "--extract-release-notes")?;
                    extract_notes = Some(PathBuf::from(file));
                }
                "--generate-signing-key" => {
                    let file = next_value(&mut arguments, "--generate-signing-key")?;
                    key_output = Some(PathBuf::from(file));
                }
                "--print-version" => {
                    // Cargo resolved the single product version source before this
                    // process started, so the release pipeline never re-parses it.
                    println!("{}", env!("CARGO_PKG_VERSION"));
                    std::process::exit(0);
                }
                "-h" | "--help" => {
                    println!("{}", Self::USAGE);
                    std::process::exit(0);
                }
                // Everything that is not an option is a fragment path, and only the
                // merge takes fragments. Rejecting them anywhere else keeps a
                // mistyped argument from being silently ignored.
                other if merge_directory.is_some() && !other.starts_with('-') => {
                    fragments.push(PathBuf::from(other));
                }
                other => {
                    return failure(format!("unexpected argument {other}\n\n{}", Self::USAGE));
                }
            }
        }

        // Neither alternative mode builds anything, so an option that only affects a
        // build is a mistake rather than a no-op, and the two alternatives are mutually
        // exclusive.
        let build_option = target.is_some() || environment.is_some() || formats.is_some();
        if let Some(file) = key_output {
            if build_option || merge_directory.is_some() || release_notes.is_some() {
                return failure(
                    "--generate-signing-key cannot be combined with --target, --environment, \
                     --formats, --merge-manifests or --release-notes",
                );
            }
            return Ok(Self::GenerateSigningKey(file));
        }

        // Composing the notes writes a file and builds nothing, so every option that
        // only affects a build or a merge would be silently ignored if it were allowed
        // alongside. This mode produces the input `--release-notes` consumes, and the
        // pipeline runs the two as separate steps.
        if let Some(file) = extract_notes {
            if build_option || merge_directory.is_some() || release_notes.is_some() {
                return failure(
                    "--extract-release-notes cannot be combined with --target, --environment, \
                     --formats, --merge-manifests or --release-notes",
                );
            }
            return Ok(Self::ExtractReleaseNotes(file));
        }

        if release_notes.is_some() && merge_directory.is_none() {
            return failure("--release-notes is only valid with --merge-manifests");
        }

        let Some(directory) = merge_directory else {
            return Ok(Self::Package(Options {
                target,
                environment: environment.unwrap_or_else(|| "production".to_owned()),
                formats,
            }));
        };
        if build_option {
            return failure(
                "--merge-manifests cannot be combined with --target, --environment or --formats",
            );
        }
        Ok(Self::MergeManifest {
            directory,
            fragments,
            release_notes,
        })
    }
}

fn next_value(arguments: &mut impl Iterator<Item = String>, name: &str) -> Result<String> {
    arguments.next().ok_or_else(|| {
        Box::new(Failure(format!("{name} requires a value"))) as Box<dyn std::error::Error>
    })
}

fn parse_formats(value: &str) -> Result<Vec<PackageFormat>> {
    let mut formats = Vec::new();
    for name in value
        .split(',')
        .map(str::trim)
        .filter(|name| !name.is_empty())
    {
        let format = PackageFormat::from_short_name(name)
            .ok_or_else(|| Box::new(Failure(format!("unknown package format {name}"))))?;
        if !formats.contains(&format) {
            formats.push(format);
        }
    }
    if formats.is_empty() {
        return failure("--formats requires at least one format");
    }
    Ok(formats)
}

fn main() -> std::process::ExitCode {
    match Invocation::parse(env::args().skip(1).collect()).and_then(execute) {
        Ok((summary, artifacts)) => {
            report(summary, &artifacts);
            std::process::ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("bongocat-packaging: {error}");
            std::process::ExitCode::FAILURE
        }
    }
}

/// Runs one parsed invocation, returning its closing line and the files it produced.
fn execute(invocation: Invocation) -> Result<(&'static str, Vec<PathBuf>)> {
    match invocation {
        Invocation::Package(options) => {
            package(options).map(|artifacts| ("Build completed successfully.", artifacts))
        }
        Invocation::MergeManifest {
            directory,
            fragments,
            release_notes,
        } => merge_manifest(&directory, &fragments, release_notes.as_deref())
            .map(|artifacts| ("Release manifest merged successfully.", artifacts)),
        Invocation::ExtractReleaseNotes(output) => extract_release_notes(&output)
            .map(|artifacts| ("Release notes composed successfully.", artifacts)),
        Invocation::GenerateSigningKey(path) => generate_signing_key(&path)
            .map(|artifacts| ("Signing key generated successfully.", artifacts)),
    }
}

fn package(options: Options) -> Result<Vec<PathBuf>> {
    let workspace = workspace_root()?;
    let target = match options.target {
        Some(target) => target,
        None => ReleaseTarget::host()?,
    };
    let requested = match &options.formats {
        Some(formats) => {
            for format in formats {
                if !target.release_formats().contains(format) {
                    return failure(format!(
                        "{} does not produce the {} artifact",
                        target.triple(),
                        format.short_name()
                    ));
                }
            }
            formats.clone()
        }
        None => target.release_formats().to_vec(),
    };

    println!(
        "Packaging {PRODUCT_NAME} {} for {} ({}, {})",
        env!("CARGO_PKG_VERSION"),
        target.triple(),
        options.environment,
        requested
            .iter()
            .map(|format| format.short_name())
            .collect::<Vec<_>>()
            .join(",")
    );

    build_application(&workspace, target, &options.environment)?;
    let provenance = write_provenance(
        &workspace,
        target,
        &options.environment,
        environment_features(&options.environment),
    )?;

    let config = packaging_config(
        &workspace,
        target,
        &provenance,
        &packager_formats(&requested),
    )?;
    let packages = cargo_packager::package(&config)?;
    let mut artifacts = collect_artifacts(&packages);
    rename_windows_installer(target, &mut artifacts)?;

    if requested.contains(&PackageFormat::Dmg) {
        let bundle = artifacts
            .iter()
            .find(|path| path.is_dir())
            .ok_or_else(|| {
                Box::new(Failure(
                    "the disk image needs a packaged .app bundle".into(),
                )) as Box<dyn std::error::Error>
            })?
            .clone();
        artifacts.push(build_disk_image(
            &workspace,
            target,
            &bundle,
            &macos_signing_identity(),
        )?);
    }

    artifacts.sort();
    if update_signing_configured() {
        let update_assets =
            publish_update_assets(&workspace, target, &artifacts, &signing_material())?;
        artifacts.extend(update_assets);
        artifacts.sort();
    }
    verify_artifacts(target, &artifacts)?;
    Ok(artifacts)
}

/// The formats handed to `cargo-packager`.
///
/// A `.dmg` is built from the finished bundle instead, so requesting it only
/// requires the `.app` here. See the module documentation for why.
fn packager_formats(requested: &[PackageFormat]) -> Vec<PackageFormat> {
    let mut formats: Vec<PackageFormat> = requested
        .iter()
        .copied()
        .filter(|format| *format != PackageFormat::Dmg)
        .collect();
    if requested.contains(&PackageFormat::Dmg) && !formats.contains(&PackageFormat::App) {
        formats.push(PackageFormat::App);
    }
    formats
}

/// Resolves the workspace root from this crate's manifest directory.
fn workspace_root() -> Result<PathBuf> {
    let manifest_directory = Path::new(env!("CARGO_MANIFEST_DIR"));
    let root = manifest_directory
        .ancestors()
        .nth(2)
        .ok_or_else(|| Box::new(Failure("crate is not inside a workspace".into())))?
        .to_path_buf();
    if !root.join("Cargo.toml").is_file() {
        return failure(format!(
            "{} does not look like the BongoCat workspace root",
            root.display()
        ));
    }
    Ok(root)
}

/// Returns the feature set that represents `environment` in the child build.
fn environment_features(environment: &str) -> &'static str {
    if environment == "production" {
        PRODUCTION_FEATURE
    } else {
        "default"
    }
}

/// Compiles the product application for `target` with the environment compiled in.
///
/// The environment is expressed to Cargo as a feature instead of an environment
/// variable, so Cargo owns feature parsing and build-script rerun semantics.
fn build_application(workspace: &Path, target: ReleaseTarget, environment: &str) -> Result<()> {
    let cargo = env::var_os("CARGO").unwrap_or_else(|| "cargo".into());
    let mut command = Command::new(&cargo);
    command.current_dir(workspace).args([
        "build",
        "--locked",
        "--release",
        "--target",
        target.triple(),
        "-p",
        APPLICATION_BINARY,
    ]);
    let features = environment_features(environment);
    if features != "default" {
        command.args(["--features", features]);
    }
    let status = command.status().map_err(|error| {
        Box::new(Failure(format!(
            "could not run {}: {error}",
            Path::new(&cargo).display()
        ))) as Box<dyn std::error::Error>
    })?;
    if !status.success() {
        return failure(format!(
            "cargo build for {} failed with {status}",
            target.triple()
        ));
    }
    Ok(())
}

/// Writes the path-free build provenance record into the packaging staging area.
fn write_provenance(
    workspace: &Path,
    target: ReleaseTarget,
    environment: &str,
    features: &str,
) -> Result<PathBuf> {
    let generator = workspace.join(PROVENANCE_GENERATOR);
    if !generator.is_file() {
        return failure(format!("missing {}", generator.display()));
    }
    let output = workspace
        .join(OUTPUT_DIRECTORY)
        .join(STAGING_DIRECTORY)
        .join(PROVENANCE_FILE);
    if let Some(directory) = output.parent() {
        fs::create_dir_all(directory)?;
    }

    // `tools/` is committed Python that the Phase 0 fixture gates already require.
    let python = if cfg!(windows) { "python" } else { "python3" };
    let mut command = Command::new(python);
    command
        .current_dir(workspace)
        .arg(&generator)
        .arg("--workspace")
        .arg(workspace)
        .arg("--output")
        .arg(&output)
        .arg("--target")
        .arg(target.triple())
        .arg("--profile")
        .arg("release")
        .arg("--features")
        .arg(features)
        .arg("--environment")
        .arg(environment);
    run_command(python, &mut command).map_err(|error| {
        Box::new(Failure(format!(
            "build provenance needs a Python 3 interpreter: {error}"
        ))) as Box<dyn std::error::Error>
    })?;
    if !output.is_file() {
        return failure(format!(
            "build provenance was not written to {}",
            output.display()
        ));
    }
    Ok(output)
}

/// Assembles the `cargo-packager` configuration for one release target.
fn packaging_config(
    workspace: &Path,
    target: ReleaseTarget,
    provenance: &Path,
    formats: &[PackageFormat],
) -> Result<Config> {
    let mut config = Config::default();
    config.product_name = PRODUCT_NAME.to_owned();
    // The packaging crate inherits `[workspace.package].version`, so Cargo itself
    // resolves the single product version source before this code runs.
    config.version = env!("CARGO_PKG_VERSION").to_owned();
    config.identifier = Some(BUNDLE_IDENTIFIER.to_owned());
    config.description =
        Some("A desktop pet that reacts to your keyboard, mouse and gamepad input.".to_owned());
    config.homepage = Some("https://github.com/ayangweb/BongoCat".to_owned());
    config.authors = Some(vec!["ayangweb".to_owned()]);
    // No `license_file`: cargo-packager uses it only to add an end-user EULA page
    // to the NSIS installer, which the product does not ask for. The MIT licence
    // stays in the repository.
    config.icons = Some(vec![
        workspace
            .join(RESOURCE_DIRECTORY)
            .join("icons/logo-macos.icns")
            .display()
            .to_string(),
        workspace
            .join(RESOURCE_DIRECTORY)
            .join("icons/logo-windows.ico")
            .display()
            .to_string(),
    ]);
    config.binaries = vec![Binary::new(APPLICATION_BINARY).main(true)];
    config.binaries_dir = Some(
        workspace
            .join("target")
            .join(target.triple())
            .join("release"),
    );
    config.out_dir = workspace.join(OUTPUT_DIRECTORY);
    config.target_triple = Some(target.triple().to_owned());
    config.formats = Some(formats.to_vec());
    config.resources = Some(resources(workspace, target, provenance));

    if target.is_apple() {
        let mut macos = MacOsConfig::new();
        macos.minimum_system_version = Some(MACOS_MINIMUM_SYSTEM_VERSION.to_owned());
        // `cargo-packager` generates the bundle `Info.plist` from this configuration
        // and then merges the repository overlay over it, so product-specific keys
        // such as `LSMultipleInstancesProhibited` have exactly one source.
        macos.info_plist_path = Some(workspace.join(MACOS_INFO_PLIST));
        macos.signing_identity = Some(macos_signing_identity());
        config.macos = Some(macos);
    }

    if target == ReleaseTarget::WindowsX86_64 {
        let mut nsis = NsisConfig::new();
        // Per-user install: no administrator prompt, no machine-level registry keys.
        nsis.install_mode = NSISInstallerMode::CurrentUser;
        config.nsis = Some(nsis);
    }

    Ok(config)
}

/// Maps the bundled resources to the locations the application resolves at runtime.
///
/// macOS reads them from `Contents/Resources`; Windows reads them from a
/// `resources/` directory beside the executable. The prefixes differ because
/// `cargo-packager` resolves resource targets relative to each platform's own
/// resource root, which is exactly the layout `bongocat-app::preset_root` expects.
fn resources(workspace: &Path, target: ReleaseTarget, provenance: &Path) -> Vec<Resource> {
    let prefix = if target.is_apple() {
        String::new()
    } else {
        format!("{RESOURCE_DIRECTORY}/")
    };
    vec![
        Resource::Mapped {
            src: workspace
                .join(RESOURCE_DIRECTORY)
                .join(MODEL_DIRECTORY)
                .display()
                .to_string(),
            target: PathBuf::from(format!("{prefix}{MODEL_DIRECTORY}")),
        },
        Resource::Mapped {
            src: provenance.display().to_string(),
            target: PathBuf::from(format!("{prefix}{PROVENANCE_FILE}")),
        },
    ]
}

/// The identity used to sign the macOS bundle, preferring an injected credential.
fn macos_signing_identity() -> String {
    match env::var(MACOS_SIGNING_IDENTITY_VARIABLE) {
        Ok(identity) if !identity.trim().is_empty() => identity,
        _ => ADHOC_SIGNING_IDENTITY.to_owned(),
    }
}

/// Whether the release pipeline provisioned a key for signing update payloads.
///
/// A local build has none, so it produces bundle and installer artifacts and no
/// update assets. Release jobs set the variable, and assert afterwards that the
/// assets exist, so a release can never ship unsigned update material.
fn update_signing_configured() -> bool {
    env::var(SIGNING_PRIVATE_KEY_VARIABLE).is_ok_and(|key| !key.trim().is_empty())
}

/// The signing material for this build, read from the pipeline's environment.
///
/// `cargo-packager` reads *its own* variables (`TAURI_SIGNING_PRIVATE_KEY` and
/// friends) only in its CLI; used as a library it takes the key explicitly, which
/// keeps a stray variable in a developer's shell from signing a local build.
fn signing_material() -> SigningConfig {
    SigningConfig::new()
        .private_key(env::var(SIGNING_PRIVATE_KEY_VARIABLE).unwrap_or_default())
        .password(env::var(SIGNING_PRIVATE_KEY_PASSWORD_VARIABLE).unwrap_or_default())
}

/// The fragment file this target's release job writes.
///
/// Named after the platform key plus [`UPDATE_FRAGMENT_SUFFIX`], so the merge can read
/// the `<os>-<arch>` key the updater looks this host up under straight off the file
/// name. `tools/tests/test_update_release_contract.py` pins the key set against
/// `bongocat-update::UpdateTargetTriple::manifest_platform`.
fn fragment_file_name(target: ReleaseTarget) -> String {
    format!("{}{UPDATE_FRAGMENT_SUFFIX}", target.manifest_platform())
}

/// Sign this target's update payload and write this target's manifest fragment.
///
/// Signing is the last step for a reason: a Minisign signature covers the exact
/// published bytes, so anything that rewrites or renames the payload afterwards
/// invalidates it.
fn publish_update_assets(
    workspace: &Path,
    target: ReleaseTarget,
    artifacts: &[PathBuf],
    signing: &SigningConfig,
) -> Result<Vec<PathBuf>> {
    let output_directory = workspace.join(OUTPUT_DIRECTORY);
    let payload = update_payload(target, artifacts, &output_directory)?;

    let signature_path = cargo_packager::sign::sign_file(signing, &payload).map_err(|error| {
        Box::new(Failure(format!(
            "could not sign {}: {error}",
            payload.display()
        ))) as Box<dyn std::error::Error>
    })?;
    let signature = fs::read_to_string(&signature_path)?.trim().to_owned();

    let payload_name = payload
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .ok_or_else(|| {
            Box::new(Failure(format!("invalid payload {}", payload.display())))
                as Box<dyn std::error::Error>
        })?;
    let asset_url = format!(
        "{RELEASE_REPOSITORY_URL}/releases/download/v{version}/{payload_name}",
        version = env!("CARGO_PKG_VERSION"),
    );

    // One fragment per target: this job can only announce the payload it produced, and
    // the platform key comes from the file name. The release merges the fragments into
    // the single manifest the updater requests. See `--merge-manifests`.
    //
    // No `pub_date`: the manifest treats it as optional, the updater decides freshness
    // by version, and the packaging tool carries no date formatter.
    let fragment = ManifestFragment {
        version: env!("CARGO_PKG_VERSION").to_owned(),
        entry: ManifestEntry {
            url: asset_url,
            signature,
            format: target.update_format().to_owned(),
        },
    };

    let fragment_path = output_directory.join(fragment_file_name(target));
    fs::write(&fragment_path, serde_json::to_vec_pretty(&fragment)?)?;

    let mut produced = vec![signature_path, fragment_path];
    if !artifacts.contains(&payload) {
        produced.push(payload);
    }
    Ok(produced)
}

/// The per-target manifest fragment a release job writes and the merge reads back.
///
/// The field set is the update library's per-platform entry plus the product version,
/// which the merge checks for agreement: every fragment has to describe the same
/// release, or the merged manifest would claim a version its entries do not belong to.
#[derive(Deserialize, Serialize)]
struct ManifestFragment {
    version: String,
    #[serde(flatten)]
    entry: ManifestEntry,
}

/// One platform's entry inside the shared release manifest.
#[derive(Deserialize, Serialize)]
struct ManifestEntry {
    url: String,
    signature: String,
    format: String,
}

/// The shared release manifest the updater requests.
///
/// The updater looks this host's entry up by the `<os>-<arch>` key it derives at
/// runtime, so the map keys are the platform keys and the version is the release's, not
/// a per-entry field. `notes` is the release changelog the update window shows; it is
/// optional because the updater treats it as optional and a release published without
/// one is still installable.
#[derive(Serialize)]
struct ReleaseManifest {
    version: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    notes: Option<String>,
    platforms: BTreeMap<String, ManifestEntry>,
}

/// The authored changelogs this version's release notes are read from.
///
/// The notes are not derived from commit history: the repository keeps the bilingual
/// changelog as the record of what changed and why, and the release page, the in-app
/// update window and the shared manifest all show that same text. Both languages are
/// required, so a release whose entry was written in one file only fails here instead of
/// publishing a half-translated changelog.
const RELEASE_CHANGELOG_NAME: &str = "CHANGELOG.md";
const RELEASE_CHANGELOG_ZH_NAME: &str = "CHANGELOG.zh-CN.md";

/// What separates the two languages inside the composed notes.
///
/// A Markdown thematic break: the two halves describe one release twice, and a rule is
/// how that has always been spelled on this project's release pages. Both consumers draw
/// it — GitHub as a horizontal rule, and `bongocat-ui::update_markdown` as its own rule
/// block, which is why the separator is a plain `---` and not a heading neither changelog
/// has.
const RELEASE_NOTES_LANGUAGE_SEPARATOR: &str = "---";

/// Upper bound on the announced changelog.
///
/// The manifest is fetched and parsed on every check, so it must not grow with the
/// length of a release's commit history. A longer changelog is truncated at a character
/// boundary with a visible marker instead of failing the release: the release is still
/// valid, only the in-app summary is shortened.
const MAXIMUM_RELEASE_NOTES_BYTES: usize = 32 * 1024;
const RELEASE_NOTES_TRUNCATION_MARKER: &str = "\n\n…";

/// Read the announced changelog, if the release has one.
fn read_release_notes(path: Option<&Path>) -> Result<Option<String>> {
    let Some(path) = path else {
        return Ok(None);
    };
    let notes = fs::read_to_string(path).map_err(|error| {
        Box::new(Failure(format!(
            "could not read the release notes from {}: {error}",
            path.display()
        ))) as Box<dyn std::error::Error>
    })?;
    let notes = notes.trim();
    if notes.is_empty() {
        return Ok(None);
    }
    Ok(Some(truncate_release_notes(notes)))
}

/// Shorten a changelog to the announced bound without splitting a character.
fn truncate_release_notes(notes: &str) -> String {
    if notes.len() <= MAXIMUM_RELEASE_NOTES_BYTES {
        return notes.to_owned();
    }
    let mut boundary = MAXIMUM_RELEASE_NOTES_BYTES;
    while boundary > 0 && !notes.is_char_boundary(boundary) {
        boundary -= 1;
    }
    format!("{}{RELEASE_NOTES_TRUNCATION_MARKER}", &notes[..boundary])
}

/// Compose this version's release notes from the bilingual changelog.
///
/// The release notes are the release's own changelog entry rather than a summary of the
/// commits between two tags. `CHANGELOG.md` and `CHANGELOG.zh-CN.md` are the authored
/// record of what changed, so they are the source, and the two languages are joined by a
/// thematic break into the one document the release page and the update window both show.
///
/// The version is this tool's own — the value `--print-version` reports and the one the
/// release pipeline has already matched the tag against — so a tag whose changelog entry
/// was never written fails the release here instead of publishing notes that describe
/// some other version.
fn extract_release_notes(output: &Path) -> Result<Vec<PathBuf>> {
    let workspace = workspace_root()?;
    let version = env!("CARGO_PKG_VERSION");

    let english = read_changelog_section(&workspace.join(RELEASE_CHANGELOG_NAME), version)?;
    let chinese = read_changelog_section(&workspace.join(RELEASE_CHANGELOG_ZH_NAME), version)?;

    fs::write(output, compose_release_notes(&english, &chinese))?;
    Ok(vec![output.to_path_buf()])
}

/// The published release-notes document, from the two languages' entries.
///
/// Split out from the file handling so the published shape — which is a contract with
/// both the release page and the update window — is testable without a workspace.
fn compose_release_notes(english: &str, chinese: &str) -> String {
    format!("{english}\n\n{RELEASE_NOTES_LANGUAGE_SEPARATOR}\n\n{chinese}\n")
}

/// Read one changelog's entry for `version`.
///
/// A missing entry stops the release: publishing notes for a version the changelog never
/// documented would announce a changelog that does not exist. The error names the
/// versions the file does document, because the usual cause is a version that was bumped
/// in one place and not the other.
fn read_changelog_section(path: &Path, version: &str) -> Result<String> {
    let text = fs::read_to_string(path).map_err(|error| {
        Box::new(Failure(format!(
            "could not read the changelog {}: {error}",
            path.display()
        ))) as Box<dyn std::error::Error>
    })?;

    changelog_section(&text, version).ok_or_else(|| {
        let documented = documented_versions(&text);
        let documented = if documented.is_empty() {
            "no versions".to_owned()
        } else {
            documented.join(", ")
        };
        Box::new(Failure(format!(
            "{} has no release notes for {version}; it documents {documented}",
            path.display()
        ))) as Box<dyn std::error::Error>
    })
}

/// The body of `version`'s entry in a changelog.
///
/// A changelog is Markdown, so an entry is opened by a heading and not by the version
/// appearing as text. Walking the headings is what makes a version mentioned in prose, a
/// `###` subheading and a heading inside a fenced example all harmless: only a
/// second-level heading whose own first token is the version opens an entry, and only the
/// next second-level heading closes it. The returned body keeps the entry's own headings
/// and list markup verbatim, because the emoji section headings are part of how these
/// notes read.
///
/// `None` means the version has no entry at all, which is different from an empty one.
fn changelog_section(markdown: &str, version: &str) -> Option<String> {
    let mut body: Vec<&str> = Vec::new();
    let mut open = false;

    for (line, opens_entry) in entry_lines(markdown) {
        if opens_entry {
            if open {
                // The entry ends where the next one begins; that heading announces its
                // own release, not this one.
                break;
            }
            // The version heading itself is dropped: the release already carries its
            // version, and keeping it would print it once per language.
            open = level_two_heading(line)
                .and_then(|heading| strip_version(heading, version))
                .is_some();
            continue;
        }
        if open {
            body.push(line);
        }
    }

    if !open {
        return None;
    }
    let body = body.join("\n");
    let body = body.trim();
    if body.is_empty() {
        return None;
    }
    Some(body.to_owned())
}

/// The versions a changelog documents, in the order it lists them.
///
/// Only used to explain a missing entry, but it has to read the file the same way the
/// lookup does: a version named in prose is not a documented release.
fn documented_versions(markdown: &str) -> Vec<&str> {
    entry_lines(markdown)
        .filter(|(_, opens_entry)| *opens_entry)
        .filter_map(|(line, _)| level_two_heading(line))
        .filter_map(|heading| heading.split_whitespace().next())
        // Keep a Changelog spells an entry `## [<version>] - <date>`.
        .map(|token| token.trim_matches(['[', ']']))
        .collect()
}

/// Walk a changelog's lines, flagging the `##` headings that open an entry.
///
/// A heading inside a fenced code block is an example, so it neither opens nor closes an
/// entry; both the lookup and the diagnostic need exactly that walk, and doing it twice
/// would let the two disagree about what a documented version is.
fn entry_lines(markdown: &str) -> impl Iterator<Item = (&str, bool)> {
    let mut fence: Option<char> = None;
    markdown.lines().map(move |line| {
        // A changelog checked out on Windows is CRLF, and a stray `\r` would end up
        // inside the published notes.
        let line = line.trim_end_matches('\r');
        let Some(marker) = fence_marker(line) else {
            return (line, fence.is_none() && level_two_heading(line).is_some());
        };
        // Only the marker that opened the block closes it, so a `~~~` inside a ``` block
        // is content rather than the end of it.
        match fence {
            Some(open) if open == marker => fence = None,
            Some(_) => {}
            None => fence = Some(marker),
        }
        (line, false)
    })
}

/// The fence marker a line opens or closes a code block with, if it is one.
fn fence_marker(line: &str) -> Option<char> {
    let trimmed = line.trim_start_matches(' ');
    // CommonMark allows at most three leading spaces; more is an indented code block, and
    // this changelog has no use for one.
    if line.len() - trimmed.len() > 3 {
        return None;
    }
    match trimmed.as_bytes().first() {
        Some(b'`') if trimmed.starts_with("```") => Some('`'),
        Some(b'~') if trimmed.starts_with("~~~") => Some('~'),
        _ => None,
    }
}

/// The text of a second-level ATX heading, if the line is one.
fn level_two_heading(line: &str) -> Option<&str> {
    let trimmed = line.trim_start_matches(' ');
    let rest = trimmed.strip_prefix("##")?;
    // `###` opens a section inside an entry, not an entry.
    if rest.starts_with('#') {
        return None;
    }
    // `##x` is not a heading at all: ATX requires a space or the end of the line.
    if !rest.is_empty() && !rest.starts_with([' ', '\t']) {
        return None;
    }
    Some(rest.trim())
}

/// The text after `version`, if the heading names it as its own entry.
///
/// The version has to be a whole token, so `## <version> - <date>` and `## [<version>]`
/// name the release while `## <version>-rc.1` and `## <version>.1` do not. A leading `v`
/// is accepted because the release pipeline tags `v<version>`.
fn strip_version<'a>(heading: &'a str, version: &str) -> Option<&'a str> {
    let heading = heading.trim();
    let rest = match heading.strip_prefix('[') {
        Some(bracketed) => {
            let (inside, after) = bracketed.split_once(']')?;
            if inside.trim() != version {
                return None;
            }
            after
        }
        None => heading
            .strip_prefix(['v', 'V'])
            .unwrap_or(heading)
            .strip_prefix(version)?,
    };
    let continues = rest.starts_with(|character: char| {
        character.is_alphanumeric() || character == '.' || character == '-'
    });
    (!continues).then_some(rest)
}

/// Merge the per-target fragments into the manifest the updater requests.
///
/// The release builds each target in its own job, so a job can only announce the payload
/// it produced itself; the updater, however, reads one shared manifest and picks its own
/// entry out of it. Combining them is a packaging concern — the manifest shape and the
/// asset name must stay owned by one place — so the pipeline calls this instead of
/// assembling JSON itself. The output name is not an argument for the same reason.
fn merge_manifest(
    directory: &Path,
    fragments: &[PathBuf],
    release_notes: Option<&Path>,
) -> Result<Vec<PathBuf>> {
    let version = env!("CARGO_PKG_VERSION");
    let mut platforms: BTreeMap<String, ManifestEntry> = BTreeMap::new();

    for path in fragments {
        let key = fragment_target(path)?.manifest_platform().to_owned();
        let fragment: ManifestFragment =
            serde_json::from_slice(&fs::read(path)?).map_err(|error| {
                Box::new(Failure(format!(
                    "{} is not a release manifest fragment: {error}",
                    path.display()
                ))) as Box<dyn std::error::Error>
            })?;

        // Every fragment has to describe this release. The manifest carries one version
        // for all platforms, so a fragment from another build would make it announce a
        // version its entries do not belong to.
        if fragment.version != version {
            return failure(format!(
                "{} announces version {}, but this release is {version}",
                path.display(),
                fragment.version
            ));
        }

        if platforms.insert(key.clone(), fragment.entry).is_some() {
            return failure(format!("two fragments announce the platform {key}"));
        }
    }

    if platforms.is_empty() {
        return failure("--merge-manifests needs at least one fragment");
    }

    // The version is this tool's own, which Cargo resolved from the single product
    // version source, so the manifest can never disagree with the artifacts.
    let manifest = ReleaseManifest {
        version: version.to_owned(),
        notes: read_release_notes(release_notes)?,
        platforms,
    };

    fs::create_dir_all(directory)?;
    let output = directory.join(UPDATE_MANIFEST_NAME);
    fs::write(&output, serde_json::to_vec_pretty(&manifest)?)?;
    Ok(vec![output])
}

/// The release target a fragment's file name declares.
///
/// The name is the `<os>-<arch>` key the updater looks this host up under, so it has to
/// be one of the keys this tool publishes. Validating it here turns a misnamed or
/// mistyped fragment into a failed release rather than a manifest entry no host can
/// ever match.
fn fragment_target(path: &Path) -> Result<ReleaseTarget> {
    let name = path
        .file_stem()
        .map(|stem| stem.to_string_lossy().into_owned());
    ReleaseTarget::ALL
        .into_iter()
        .find(|target| name.as_deref() == Some(target.manifest_platform()))
        .ok_or_else(|| {
            Box::new(Failure(format!(
                "{} is not named after a shipped platform; expected one of {}",
                path.display(),
                ReleaseTarget::ALL
                    .map(ReleaseTarget::manifest_platform)
                    .join(", ")
            ))) as Box<dyn std::error::Error>
        })
}

/// Generate the Minisign key pair that signs update payloads.
///
/// A one-time, offline operation run by the maintainer, not by a build: the private half
/// is a release credential that must never be committed, built into an artifact or
/// logged. It goes through the same pinned `cargo-packager` this crate signs with, so
/// provisioning a key needs no global `cargo install`.
///
/// The passphrase comes from [`SIGNING_PRIVATE_KEY_PASSWORD_VARIABLE`] — the same variable
/// the signing path unlocks the key with — so a key and the way it is used cannot drift
/// apart. An unset passphrase produces a key stored in the clear, which is reported
/// rather than refused: it is a supported choice, just not the default a release wants.
fn generate_signing_key(path: &Path) -> Result<Vec<PathBuf>> {
    let public_path = PathBuf::from(format!("{}.pub", path.display()));
    // `save_keypair` overwrites an existing private key only when asked, but it deletes
    // an existing public key unconditionally, so both are checked before anything is
    // written. Losing a release key pair means every installed copy stops being able to
    // update, so this refuses rather than trusting the caller.
    for existing in [path, public_path.as_path()] {
        if existing.exists() {
            return failure(format!(
                "{} already exists; refusing to overwrite a signing key",
                existing.display()
            ));
        }
    }
    if let Some(directory) = path.parent()
        && !directory.as_os_str().is_empty()
    {
        fs::create_dir_all(directory)?;
    }

    let passphrase = env::var(SIGNING_PRIVATE_KEY_PASSWORD_VARIABLE).unwrap_or_default();
    let keypair =
        cargo_packager::sign::generate_key(Some(passphrase.clone())).map_err(|error| {
            Box::new(Failure(format!(
                "could not generate a signing key: {error}"
            ))) as Box<dyn std::error::Error>
        })?;
    let (private, public) =
        cargo_packager::sign::save_keypair(&keypair, path, false).map_err(|error| {
            Box::new(Failure(format!(
                "could not save the signing key to {}: {error}",
                path.display()
            ))) as Box<dyn std::error::Error>
        })?;
    restrict_private_key(&private)?;

    println!();
    println!("Private key: {}", private.display());
    println!("  Keep it offline and never commit it. In CI it is the value of");
    println!("  the {SIGNING_PRIVATE_KEY_VARIABLE} secret.");
    println!("Public key:  {}", public.display());
    println!("  Not a secret. Paste this single line into RELEASE_SIGNING_KEY in");
    println!("  crates/bongocat-update/src/runtime.rs:");
    println!();
    println!("{}", keypair.pk);
    if passphrase.is_empty() {
        println!();
        println!(
            "warning: {SIGNING_PRIVATE_KEY_PASSWORD_VARIABLE} was not set, so the private key is \
             stored unencrypted."
        );
        println!("Set it and generate a new pair if the key should be passphrase-protected.");
    }

    Ok(vec![private, public])
}

/// Restrict the private key to its owner.
///
/// `cargo-packager` writes the key with the process umask, which on a default macOS or
/// Linux account leaves it world-readable. Windows has no equivalent mode, so there the
/// file inherits the account's ACL.
#[cfg(unix)]
fn restrict_private_key(path: &Path) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;

    fs::set_permissions(path, fs::Permissions::from_mode(0o600))?;
    Ok(())
}

#[cfg(not(unix))]
fn restrict_private_key(_path: &Path) -> Result<()> {
    Ok(())
}

/// The artifact the updater installs for this target.
///
/// macOS installs a directory, so the finished bundle is wrapped in a `.tar.gz` whose
/// single root entry is the bundle itself — the updater drops that root entry and
/// installs the rest on the bundle path. Windows installs the installer it downloaded,
/// so the published NSIS `.exe` is used unchanged.
fn update_payload(
    target: ReleaseTarget,
    artifacts: &[PathBuf],
    output_directory: &Path,
) -> Result<PathBuf> {
    if target.is_apple() {
        let bundle = artifacts.iter().find(|path| path.is_dir()).ok_or_else(|| {
            Box::new(Failure(format!(
                "{} needs a packaged .app bundle to build its update payload",
                target.triple()
            ))) as Box<dyn std::error::Error>
        })?;
        let archive = output_directory.join(target.update_payload_name());
        write_bundle_archive(bundle, &archive)?;
        return Ok(archive);
    }

    artifacts
        .iter()
        .find(|path| {
            path.is_file()
                && path
                    .extension()
                    .is_some_and(|extension| extension.eq_ignore_ascii_case("exe"))
        })
        .cloned()
        .ok_or_else(|| {
            Box::new(Failure(format!(
                "{} must produce an installer to publish as its update payload",
                target.triple()
            ))) as Box<dyn std::error::Error>
        })
}

/// Write `bundle` into a gzipped tar archive rooted at the bundle's own name.
///
/// Symlinks are stored as symlinks, which is what a `.app` bundle's internal links
/// need; dereferencing them would duplicate frameworks into the archive.
fn write_bundle_archive(bundle: &Path, archive: &Path) -> Result<()> {
    let root = bundle.file_name().map(PathBuf::from).ok_or_else(|| {
        Box::new(Failure(format!("invalid bundle {}", bundle.display())))
            as Box<dyn std::error::Error>
    })?;

    let file = fs::File::create(archive)?;
    let encoder = flate2::write::GzEncoder::new(file, flate2::Compression::default());
    let mut builder = tar::Builder::new(encoder);
    builder.follow_symlinks(false);
    builder.append_dir_all(root, bundle)?;
    builder.into_inner()?.finish()?;
    Ok(())
}

fn collect_artifacts(packages: &[cargo_packager::PackageOutput]) -> Vec<PathBuf> {
    packages
        .iter()
        .flat_map(|package| package.paths.iter().cloned())
        .collect()
}

/// Renames the packaged Windows installer to the name the release publishes.
///
/// `cargo-packager` owns the NSIS installer and names it after the main binary
/// with a `-setup` suffix; the product publishes
/// [`ReleaseTarget::installer_file_name`] instead. Only the finished file is
/// renamed here, so installer contents and bundle layout stay owned by
/// `cargo-packager`.
fn rename_windows_installer(target: ReleaseTarget, artifacts: &mut [PathBuf]) -> Result<()> {
    let Some(name) = target.installer_file_name() else {
        return Ok(());
    };
    let installer = artifacts
        .iter()
        .position(|artifact| {
            artifact.is_file()
                && artifact
                    .extension()
                    .is_some_and(|extension| extension.eq_ignore_ascii_case("exe"))
        })
        .ok_or_else(|| {
            Box::new(Failure(format!(
                "{} must produce an installer, but packaging produced none",
                target.triple()
            ))) as Box<dyn std::error::Error>
        })?;

    let renamed = artifacts[installer].with_file_name(&name);
    if renamed == artifacts[installer] {
        return Ok(());
    }
    if renamed.exists() {
        fs::remove_file(&renamed)?;
    }
    fs::rename(&artifacts[installer], &renamed)?;
    artifacts[installer] = renamed;
    Ok(())
}

/// Runs a tool, mapping a missing executable and a non-zero exit onto a failure.
fn run_command(program: &str, command: &mut Command) -> Result<()> {
    let status = command.status().map_err(|error| {
        Box::new(Failure(format!("could not run {program}: {error}"))) as Box<dyn std::error::Error>
    })?;
    if !status.success() {
        return failure(format!("{program} failed with {status}"));
    }
    Ok(())
}

/// Builds the macOS installer disk image from a finished `.app` bundle.
#[cfg(unix)]
fn build_disk_image(
    workspace: &Path,
    target: ReleaseTarget,
    bundle: &Path,
    identity: &str,
) -> Result<PathBuf> {
    if !target.is_apple() {
        return failure("disk images are a macOS artifact");
    }
    let output_directory = workspace.join(OUTPUT_DIRECTORY);
    let image = output_directory.join(format!(
        "{PRODUCT_NAME}-{}-{}.dmg",
        env!("CARGO_PKG_VERSION"),
        target.architecture()
    ));
    let staging = output_directory.join(DISK_IMAGE_STAGING_DIRECTORY);
    if staging.exists() {
        fs::remove_dir_all(&staging)?;
    }
    fs::create_dir_all(&staging)?;

    let bundle_name = bundle
        .file_name()
        .ok_or_else(|| Box::new(Failure(format!("invalid bundle {}", bundle.display()))))?;
    let staged_bundle = staging.join(bundle_name);

    // `ditto` copies the bundle with its extended attributes and signature intact.
    let mut copy = Command::new("ditto");
    copy.arg(bundle).arg(&staged_bundle);
    run_command("ditto", &mut copy)?;

    // The drop link is what makes the mounted image a drag-to-install installer.
    std::os::unix::fs::symlink("/Applications", staging.join("Applications"))?;

    if image.exists() {
        fs::remove_file(&image)?;
    }
    let mut create = Command::new("hdiutil");
    create
        .args(["create", "-volname", PRODUCT_NAME, "-srcfolder"])
        .arg(&staging)
        .args(["-ov", "-format", "UDZO"])
        .arg(&image);
    run_command("hdiutil", &mut create)?;

    let mut sign = Command::new("codesign");
    sign.args(["--force", "-s", identity]).arg(&image);
    run_command("codesign", &mut sign)?;

    fs::remove_dir_all(&staging)?;

    if !image.is_file() {
        return failure(format!("disk image was not created: {}", image.display()));
    }
    Ok(image)
}

#[cfg(not(unix))]
fn build_disk_image(
    _workspace: &Path,
    _target: ReleaseTarget,
    _bundle: &Path,
    _identity: &str,
) -> Result<PathBuf> {
    failure("a macOS disk image can only be built on macOS")
}

/// Rejects a package whose resources did not land where the product reads them.
///
/// `cargo-packager` resolves resource targets differently per platform, so a
/// configuration mistake silently ships a bundle without preset models. This
/// check turns that into a failed build.
fn verify_artifacts(target: ReleaseTarget, artifacts: &[PathBuf]) -> Result<()> {
    if artifacts.is_empty() {
        return failure("packaging produced no artifacts");
    }
    for artifact in artifacts {
        if !artifact.exists() {
            return failure(format!("missing artifact {}", artifact.display()));
        }
        if artifact.is_file() && fs::metadata(artifact)?.len() == 0 {
            return failure(format!("empty artifact {}", artifact.display()));
        }
        if artifact.is_dir() {
            verify_app_bundle(target, artifact)?;
        }
    }
    Ok(())
}

fn verify_app_bundle(target: ReleaseTarget, bundle: &Path) -> Result<()> {
    if !target.is_apple() {
        return Ok(());
    }
    let contents = bundle.join("Contents");
    let expected = [
        contents.join("Info.plist"),
        contents.join("MacOS").join(APPLICATION_BINARY),
        contents.join("Resources").join(PROVENANCE_FILE),
    ];
    for path in expected {
        if !path.is_file() {
            return failure(format!(
                "{} is missing inside {}",
                path.strip_prefix(bundle)
                    .unwrap_or(path.as_path())
                    .display(),
                bundle.display()
            ));
        }
    }
    for model in PRESET_MODELS {
        let directory = contents.join("Resources").join(MODEL_DIRECTORY).join(model);
        if !directory.is_dir() {
            return failure(format!(
                "preset model {model} is missing inside {}",
                bundle.display()
            ));
        }
    }
    Ok(())
}

fn report(summary: &str, artifacts: &[PathBuf]) {
    println!();
    println!("{summary}");
    println!();
    println!("Artifacts:");
    for artifact in artifacts {
        println!("  {}", artifact.display());
    }
}

#[cfg(test)]
mod tests {
    use super::ReleaseTarget;

    /// The published name is product name, resolved version and architecture token,
    /// with no packaging suffix. The updater takes its payload from the release
    /// manifest rather than matching an asset name, so the installer name only has to
    /// be stable and self-describing — but it is also the update payload name on
    /// Windows, so it is what users see in both places.
    #[test]
    fn windows_installer_is_published_under_the_product_release_name() {
        let name = ReleaseTarget::WindowsX86_64
            .installer_file_name()
            .expect("the Windows target publishes an installer name");

        assert_eq!(
            name,
            format!("BongoCat_{}_x64.exe", env!("CARGO_PKG_VERSION"))
        );
        assert!(
            !name.contains("-setup"),
            "the published installer must not keep the packaging suffix: {name}"
        );
    }

    #[test]
    fn apple_targets_keep_the_names_this_crate_already_builds() {
        assert!(ReleaseTarget::MacosAarch64.installer_file_name().is_none());
        assert!(ReleaseTarget::MacosX86_64.installer_file_name().is_none());
    }

    /// The updater installs the installer it downloads, so the update payload and the
    /// published installer are the same file under the same name.
    #[test]
    fn the_windows_update_payload_is_the_published_installer() {
        let target = ReleaseTarget::WindowsX86_64;
        assert_eq!(
            target.update_payload_name(),
            target
                .installer_file_name()
                .expect("the Windows target publishes an installer")
        );
    }

    /// macOS installs a directory, so its payload is the bundle in an archive.
    #[test]
    fn the_apple_update_payload_is_a_bundle_archive() {
        for target in [ReleaseTarget::MacosAarch64, ReleaseTarget::MacosX86_64] {
            let name = target.update_payload_name();
            assert!(
                name.ends_with(".app.tar.gz"),
                "the macOS payload must be an archive, got {name}"
            );
            assert!(
                name.contains(target.triple()),
                "the payload name must stay self-describing, got {name}"
            );
        }
    }

    /// The manifest keys must use the updater's `<os>-<arch>` spelling, which is also
    /// what `bongocat-update::UpdateTargetTriple::manifest_platform` returns.
    #[test]
    fn manifest_platforms_use_the_updater_spelling() {
        assert_eq!(
            ReleaseTarget::MacosAarch64.manifest_platform(),
            "macos-aarch64"
        );
        assert_eq!(
            ReleaseTarget::MacosX86_64.manifest_platform(),
            "macos-x86_64"
        );
        assert_eq!(
            ReleaseTarget::WindowsX86_64.manifest_platform(),
            "windows-x86_64"
        );
        assert_eq!(ReleaseTarget::MacosAarch64.update_format(), "app");
        assert_eq!(ReleaseTarget::WindowsX86_64.update_format(), "nsis");
    }

    /// A fragment is named after the platform key, because the merge reads the key the
    /// updater looks this host up under straight off the file name.
    #[test]
    fn the_fragment_is_named_after_the_platform_key() {
        assert_eq!(
            super::fragment_file_name(ReleaseTarget::MacosAarch64),
            "macos-aarch64.json"
        );
        assert_eq!(
            super::fragment_file_name(ReleaseTarget::MacosX86_64),
            "macos-x86_64.json"
        );
        assert_eq!(
            super::fragment_file_name(ReleaseTarget::WindowsX86_64),
            "windows-x86_64.json"
        );
    }

    /// The shared manifest name has to be the asset an update run asks for.
    ///
    /// Restated as a literal on purpose: this is the published asset name, so changing
    /// it must be an intentional edit here rather than a silent consequence of a
    /// constant. `bongocat-update` pins the same literal on the requesting side.
    #[test]
    fn the_shared_manifest_name_is_the_requested_asset() {
        assert_eq!(super::UPDATE_MANIFEST_NAME, "latest.json");
    }

    /// The merged manifest must be readable by the library the runtime runs.
    ///
    /// This is the reason `cargo-packager-updater` is a dev-dependency: the test feeds
    /// the produced manifest to the update library's own reader type, so a shape the
    /// runtime cannot read fails here rather than on a user's machine.
    #[test]
    fn merging_fragments_produces_a_manifest_the_updater_can_read() {
        let root = std::env::temp_dir().join("bongocat-packaging-merge");
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).expect("scratch directory");

        let version = env!("CARGO_PKG_VERSION");
        let mut fragments = Vec::new();
        for (key, format, extension) in [
            ("macos-aarch64", "app", "app.tar.gz"),
            ("macos-x86_64", "app", "app.tar.gz"),
            ("windows-x86_64", "nsis", "exe"),
        ] {
            let path = root.join(format!("{key}.json"));
            std::fs::write(
                &path,
                serde_json::to_vec_pretty(&super::ManifestFragment {
                    version: version.to_owned(),
                    entry: super::ManifestEntry {
                        url: format!(
                            "https://github.com/ayangweb/BongoCat/releases/download/v{version}/BongoCat-{key}.{extension}"
                        ),
                        signature: format!("signature-for-{key}"),
                        format: format.to_owned(),
                    },
                })
                .expect("serialize a fragment"),
            )
            .expect("write a fragment");
            fragments.push(path);
        }

        let merged = super::merge_manifest(&root, &fragments, None).expect("merge the fragments");
        assert_eq!(
            merged,
            vec![root.join(super::UPDATE_MANIFEST_NAME)],
            "the merge writes the shared manifest under its published name"
        );
        let output = &merged[0];

        let merged: serde_json::Value =
            serde_json::from_slice(&std::fs::read(output).expect("read the manifest"))
                .expect("the manifest must be JSON");
        assert_eq!(merged["version"], version);
        let platforms = merged["platforms"]
            .as_object()
            .expect("the manifest must carry a platform map");
        assert_eq!(platforms.len(), 3, "every fragment must be announced");

        // The reader is what the application runs, so its view of the manifest is the
        // contract: the version, the three keys, and a per-platform entry it can decode.
        assert!(
            merged.get("notes").is_none(),
            "a release published without notes must not announce an empty changelog"
        );
        let release: cargo_packager_updater::RemoteReleaseData =
            serde_json::from_value(merged.clone()).expect("the updater must be able to read it");
        let cargo_packager_updater::RemoteReleaseData::Static { platforms } = release else {
            panic!("a shared manifest must read as the updater's static shape");
        };
        assert!(
            merged.get("notes").is_none(),
            "a release published without notes must not announce an empty changelog"
        );
        assert_eq!(
            platforms
                .keys()
                .cloned()
                .collect::<std::collections::BTreeSet<_>>(),
            std::collections::BTreeSet::from([
                "macos-aarch64".to_owned(),
                "macos-x86_64".to_owned(),
                "windows-x86_64".to_owned(),
            ])
        );
        assert_eq!(platforms["windows-x86_64"].format.to_string(), "nsis");
        assert_eq!(platforms["macos-aarch64"].format.to_string(), "app");
    }

    /// The changelog the update window shows comes from this manifest, so it has to
    /// survive the merge and stay readable by the update library.
    #[test]
    fn merged_manifests_carry_the_release_notes_the_updater_reads() {
        let root = std::env::temp_dir().join("bongocat-packaging-merge-notes");
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).expect("scratch directory");

        let version = env!("CARGO_PKG_VERSION");
        let fragment = root.join("macos-aarch64.json");
        std::fs::write(
            &fragment,
            serde_json::to_vec_pretty(&super::ManifestFragment {
                version: version.to_owned(),
                entry: super::ManifestEntry {
                    url: format!("https://github.com/ayangweb/BongoCat/releases/download/v{version}/x.app.tar.gz"),
                    signature: "signature".to_owned(),
                    format: "app".to_owned(),
                },
            })
            .expect("serialize a fragment"),
        )
        .expect("write a fragment");

        let notes_path = root.join("notes.md");
        std::fs::write(&notes_path, "## What's new\n\n- fixed the thing\n")
            .expect("write the release notes");

        let merged = super::merge_manifest(&root, &[fragment], Some(&notes_path))
            .expect("merge with release notes");
        let manifest: serde_json::Value =
            serde_json::from_slice(&std::fs::read(&merged[0]).expect("read the manifest"))
                .expect("the manifest must be JSON");
        assert_eq!(manifest["notes"], "## What's new\n\n- fixed the thing");

        let release: cargo_packager_updater::RemoteRelease =
            serde_json::from_value(manifest).expect("the updater must read the notes");
        assert_eq!(
            release.notes.as_deref(),
            Some("## What's new\n\n- fixed the thing")
        );
    }

    /// An empty notes file is not a changelog, and the manifest must not pretend it is.
    #[test]
    fn blank_release_notes_are_not_announced() {
        let root = std::env::temp_dir().join("bongocat-packaging-merge-blank-notes");
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).expect("scratch directory");
        let notes_path = root.join("notes.md");
        std::fs::write(&notes_path, "   \n\t\n").expect("write the release notes");
        assert_eq!(
            super::read_release_notes(Some(&notes_path)).expect("read blank notes"),
            None
        );
        assert_eq!(
            super::read_release_notes(None).expect("read no notes"),
            None
        );
    }

    /// The manifest is fetched on every check, so the announced changelog is bounded.
    #[test]
    fn an_over_long_changelog_is_truncated_at_a_character_boundary() {
        let notes = "é".repeat(super::MAXIMUM_RELEASE_NOTES_BYTES);
        let truncated = super::truncate_release_notes(&notes);
        assert!(
            truncated.len()
                <= super::MAXIMUM_RELEASE_NOTES_BYTES
                    + super::RELEASE_NOTES_TRUNCATION_MARKER.len()
        );
        assert!(truncated.ends_with(super::RELEASE_NOTES_TRUNCATION_MARKER));
        // A truncated changelog is still valid UTF-8, which is what a byte-wise cut
        // would break.
        assert!(std::str::from_utf8(truncated.as_bytes()).is_ok());

        let short = "## What's new";
        assert_eq!(super::truncate_release_notes(short), short);
    }

    /// Fragments from different builds would produce a manifest that lies about which
    /// version its entries belong to, so the merge refuses them.
    #[test]
    fn merging_rejects_a_fragment_from_another_release() {
        let root = std::env::temp_dir().join("bongocat-packaging-merge-version");
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).expect("scratch directory");

        let mut fragments = Vec::new();
        for (key, version) in [
            ("macos-aarch64", env!("CARGO_PKG_VERSION")),
            ("windows-x86_64", "0.0.1"),
        ] {
            let path = root.join(format!("{key}.json"));
            std::fs::write(
                &path,
                serde_json::to_vec_pretty(&super::ManifestFragment {
                    version: version.to_owned(),
                    entry: super::ManifestEntry {
                        url: "https://github.com/ayangweb/BongoCat/releases/download/v0.0.1/x"
                            .to_owned(),
                        signature: "signature".to_owned(),
                        format: "app".to_owned(),
                    },
                })
                .expect("serialize a fragment"),
            )
            .expect("write a fragment");
            fragments.push(path);
        }

        let error = super::merge_manifest(&root, &fragments, None)
            .expect_err("fragments from different releases must be rejected");
        assert!(
            error.to_string().contains(&format!(
                "but this release is {}",
                env!("CARGO_PKG_VERSION")
            )),
            "unexpected error: {error}"
        );
    }

    /// A fragment whose name is not a shipped platform would publish a manifest entry
    /// no host can ever match, so the merge refuses it.
    #[test]
    fn merging_rejects_a_fragment_that_is_not_a_shipped_platform() {
        let root = std::env::temp_dir().join("bongocat-packaging-merge-keyless");
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).expect("scratch directory");
        let path = root.join("plan9-cris.json");
        std::fs::write(&path, b"{}").expect("write a fragment");

        let error = super::merge_manifest(&root, &[path], None)
            .expect_err("a fragment for an unshipped platform must be rejected");
        assert!(
            error
                .to_string()
                .contains("not named after a shipped platform"),
            "unexpected error: {error}"
        );
    }

    /// The provisioning step has to produce exactly what the release consumes.
    ///
    /// `--generate-signing-key` exists so a maintainer never has to hand-assemble the CI
    /// secret or the compiled-in public key. This test runs it and then uses the files it
    /// wrote the way a release does: the private file's contents as
    /// `SIGNING_PRIVATE_KEY`, unlocked with the same passphrase variable the
    /// signing path reads.
    #[test]
    fn generating_a_signing_key_produces_the_files_the_release_consumes() {
        let root = std::env::temp_dir().join("bongocat-packaging-keygen");
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).expect("scratch directory");

        let produced = super::generate_signing_key(&root.join("bongocat.key"))
            .expect("generate a signing key");
        let [private, public] = produced.as_slice() else {
            panic!("generating a key must produce exactly two files, got {produced:?}");
        };
        assert_eq!(private.file_name().unwrap(), "bongocat.key");
        assert_eq!(public.file_name().unwrap(), "bongocat.key.pub");

        let private_contents = std::fs::read_to_string(private).expect("read the private key");
        let public_contents = std::fs::read_to_string(public).expect("read the public key");
        for (label, contents) in [("private", &private_contents), ("public", &public_contents)] {
            assert!(
                !contents.trim().is_empty() && !contents.trim().contains('\n'),
                "the {label} key must be one line of text, so it can be pasted into a CI \
                 secret or a Rust string literal without an encoding step, got {contents:?}"
            );
        }

        // `cargo-packager` writes with the process umask, so without this the private key
        // would be world-readable on a default account.
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;

            let mode = std::fs::metadata(private)
                .expect("private key metadata")
                .permissions()
                .mode()
                & 0o777;
            assert_eq!(mode, 0o600, "the private key must not be world-readable");
        }

        let payload = root.join("payload.bin");
        std::fs::write(&payload, b"update payload").expect("write a payload");
        let signing = cargo_packager::sign::SigningConfig::new()
            .private_key(private_contents.trim())
            .password(
                std::env::var(super::SIGNING_PRIVATE_KEY_PASSWORD_VARIABLE).unwrap_or_default(),
            );
        let signature = cargo_packager::sign::sign_file(&signing, &payload)
            .expect("the saved private key must sign when unlocked the way the release unlocks it");
        assert!(
            std::fs::metadata(&signature)
                .expect("signature metadata")
                .len()
                > 0,
            "signing must produce a signature file"
        );

        let error = super::generate_signing_key(private)
            .expect_err("an existing key pair must never be overwritten");
        assert!(
            error.to_string().contains("refusing to overwrite"),
            "unexpected error: {error}"
        );
    }

    /// A manifest with no platform entries would be published and then read by every
    /// installed copy, so an empty merge is a failure rather than an empty file.
    #[test]
    fn merging_needs_at_least_one_fragment() {
        let root = std::env::temp_dir().join("bongocat-packaging-merge-empty");
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).expect("scratch directory");

        let error =
            super::merge_manifest(&root, &[], None).expect_err("an empty merge must be rejected");
        assert!(
            error.to_string().contains("needs at least one fragment"),
            "unexpected error: {error}"
        );
    }

    /// The updater drops the archive's root entry and installs what remains on the
    /// bundle path, so the root has to be the bundle directory and the bundle's
    /// contents have to sit under it.
    #[test]
    fn the_bundle_archive_roots_at_the_bundle_directory() {
        let root = std::env::temp_dir().join("bongocat-packaging-bundle-archive");
        let _ = std::fs::remove_dir_all(&root);
        let bundle = root.join("BongoCat.app");
        std::fs::create_dir_all(bundle.join("Contents/MacOS")).expect("bundle directory");
        std::fs::write(bundle.join("Contents/MacOS/bongocat-app"), "binary").expect("write file");

        let archive = root.join("BongoCat.app.tar.gz");
        super::write_bundle_archive(&bundle, &archive).expect("archive the bundle");

        let file = std::fs::File::open(&archive).expect("open the archive");
        let entries = tar::Archive::new(flate2::read::GzDecoder::new(file))
            .entries()
            .expect("read the archive")
            .map(|entry| {
                entry
                    .expect("archive entry")
                    .path()
                    .expect("entry path")
                    .display()
                    .to_string()
            })
            .collect::<Vec<_>>();

        // Directory entries carry a trailing separator; only the root of the archive
        // matters here, and it must be the bundle directory itself.
        assert!(
            entries
                .iter()
                .any(|entry| entry.trim_end_matches('/') == "BongoCat.app"),
            "the archive must root at the bundle directory, got {entries:?}"
        );
        assert!(
            entries
                .iter()
                .any(|entry| entry == "BongoCat.app/Contents/MacOS/bongocat-app"),
            "the bundle contents must sit under that root, got {entries:?}"
        );
    }

    /// The published notes are one document made of two languages, and the shape of it is
    /// a contract with both the release page and the update window.
    #[test]
    fn the_release_notes_join_both_languages_around_a_rule() {
        let notes = super::compose_release_notes(
            "### ✨ Features\n\n- did a thing",
            "### ✨ 新功能\n\n- 做了件事",
        );

        assert_eq!(
            notes,
            "### ✨ Features\n\n- did a thing\n\n---\n\n### ✨ 新功能\n\n- 做了件事\n"
        );
        // The separator has to be a thematic break and nothing else, or the renderer on
        // the other side draws a paragraph instead of a rule.
        assert!(notes.lines().any(|line| line == "---"));
    }

    /// An entry is opened by a heading whose own first token is the version, so every
    /// other way a version can appear in a changelog has to be ignored.
    ///
    /// The sample versions are deliberately synthetic: `tools/tests/test_product_version_contract.py`
    /// fails if a shipped version is restated anywhere in this file, and a fixture that
    /// spelled one out would trip it.
    #[test]
    fn a_changelog_entry_is_found_by_its_heading() {
        let markdown = "\
# Changelog

See 9.9.8 for the previous notes.

```markdown
## 9.9.8 - 2000-01-01

- a documented example, not an entry
```

## 9.9.9 - 2026-09-16

### ✨ Features

- the real entry

### 9.9.9

- a section that happens to be named after the version

## 9.9.7

- the previous release
";

        assert_eq!(
            super::changelog_section(markdown, "9.9.9").as_deref(),
            Some(
                "### ✨ Features\n\n- the real entry\n\n### 9.9.9\n\n- a section that happens to be named after the version"
            )
        );
        // The entry stops at the next version heading rather than running to the end of
        // the file, and it never picks up the fenced example.
        assert_eq!(
            super::changelog_section(markdown, "9.9.8").as_deref(),
            None,
            "a heading inside a fenced block is an example, not an entry"
        );
        assert_eq!(
            super::changelog_section(markdown, "9.9.7").as_deref(),
            Some("- the previous release")
        );
        assert_eq!(super::changelog_section(markdown, "9.9.6"), None);
        // A version that only appears in prose is not a documented release.
        assert_eq!(
            super::documented_versions(markdown),
            vec!["9.9.9", "9.9.7"],
            "the diagnostic must read the file the way the lookup does"
        );
    }

    /// Both changelog conventions in use have to resolve, and a version has to be a whole
    /// token so a pre-release of it is not mistaken for it.
    #[test]
    fn a_version_heading_may_be_bracketed_or_tagged_but_not_a_prefix() {
        let section =
            |heading: &str| super::changelog_section(&format!("{heading}\n\n- notes\n"), "9.9.9");

        for heading in [
            "## 9.9.9",
            "## 9.9.9 - 2026-09-16",
            "## [9.9.9] - 2026-09-16",
            "## v9.9.9",
            "## 9.9.9  ",
        ] {
            assert_eq!(
                section(heading).as_deref(),
                Some("- notes"),
                "{heading} names the release"
            );
        }

        for heading in [
            "## 9.9.9-rc.1",
            "## 9.9.9.1",
            "## 99.9.9",
            "# 9.9.9",
            "##9.9.9",
        ] {
            assert_eq!(
                section(heading),
                None,
                "{heading} does not name the release"
            );
        }
    }

    /// A changelog checked out with CRLF line endings is the same changelog, and the
    /// published notes must not carry the carriage returns into the manifest.
    #[test]
    fn carriage_returns_never_reach_the_published_notes() {
        let markdown = "## 9.9.9\r\n\r\n### ✨ Features\r\n\r\n- a thing\r\n";

        let section = super::changelog_section(markdown, "9.9.9").expect("the entry");
        assert_eq!(section, "### ✨ Features\n\n- a thing");
        assert!(!section.contains('\r'));
    }

    /// A release whose entry was never written must stop the pipeline, and the error has
    /// to say what the file does document: the usual cause is a version bumped in one
    /// place only.
    #[test]
    fn a_missing_changelog_entry_names_the_documented_versions() {
        let root = std::env::temp_dir().join("bongocat-packaging-changelog-missing");
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).expect("scratch directory");
        let path = root.join(super::RELEASE_CHANGELOG_NAME);
        std::fs::write(&path, "## 9.9.9 - 2026-09-16\n\n- notes\n").expect("write a changelog");

        let error = super::read_changelog_section(&path, "9.9.8")
            .expect_err("an undocumented version must not produce notes");
        let message = error.to_string();
        assert!(message.contains("9.9.8"), "unexpected error: {message}");
        assert!(message.contains("9.9.9"), "unexpected error: {message}");

        // An entry with a heading and no content is not a changelog either.
        std::fs::write(&path, "## 9.9.8\n\n## 9.9.7\n\n- notes\n").expect("write a changelog");
        assert_eq!(
            super::changelog_section("## 9.9.8\n\n## 9.9.7\n", "9.9.8"),
            None
        );

        let error = super::read_changelog_section(&root.join("absent.md"), "9.9.8")
            .expect_err("an absent changelog must not produce notes");
        assert!(
            error.to_string().contains("could not read the changelog"),
            "unexpected error: {error}"
        );
    }

    /// Composing the notes writes a file and builds nothing, so pairing it with an option
    /// that only affects a build or a merge would silently ignore that option.
    #[test]
    fn composing_the_notes_is_its_own_mode() {
        let parse = |arguments: &[&str]| {
            super::Invocation::parse(arguments.iter().map(|value| value.to_string()).collect())
        };

        assert!(matches!(
            parse(&["--extract-release-notes", "notes.md"]),
            Ok(super::Invocation::ExtractReleaseNotes(_))
        ));
        for conflicting in [
            vec!["--environment", "development"],
            vec!["--merge-manifests", "target/package"],
            vec!["--release-notes", "notes.md"],
        ] {
            let mut arguments = vec!["--extract-release-notes", "notes.md"];
            arguments.extend_from_slice(&conflicting);
            assert!(
                parse(&arguments).is_err(),
                "{conflicting:?} must not be accepted alongside --extract-release-notes"
            );
        }
    }

    /// Both changelogs are read for the same release, so they have to document the same
    /// versions: an entry written in one language only would ship a release whose notes
    /// describe it twice, once in a language the reader may not have.
    #[test]
    fn the_repository_changelogs_document_the_same_versions() {
        let root = super::workspace_root().expect("the workspace root");
        let read = |name: &str| {
            std::fs::read_to_string(root.join(name)).unwrap_or_else(|error| {
                panic!("{name} must be readable, because a release reads it: {error}")
            })
        };

        let english = read(super::RELEASE_CHANGELOG_NAME);
        let chinese = read(super::RELEASE_CHANGELOG_ZH_NAME);
        let english = super::documented_versions(&english);
        assert!(
            !english.is_empty(),
            "the changelog must document at least one release"
        );
        assert_eq!(
            english,
            super::documented_versions(&chinese),
            "the two changelogs must document the same versions in the same order"
        );
    }
}
