//! The BongoCat project-level build, bundle and installer packaging entry point.
//!
//! `just build` runs this crate. Local developers and CI execute the exact same
//! code path, so there is a single place that decides how the product is
//! compiled and packaged:
//!
//! 1. compile `bongocat-app` for one release target, with the immutable
//!    `BONGOCAT_BUILD_ENV` environment compiled in,
//! 2. write the path-free build provenance record,
//! 3. hand the resulting executable to `cargo-packager`, which owns the bundle
//!    and installer layout: the macOS `.app` and the Windows NSIS `.exe`,
//! 4. wrap the finished `.app` in a `.dmg` with the macOS disk-image tooling.
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
    env, fmt, fs,
    path::{Path, PathBuf},
    process::Command,
};

use cargo_packager::{
    Config, PackageFormat,
    config::{Binary, MacOsConfig, NSISInstallerMode, NsisConfig, Resource},
};

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

/// Parsed command line.
struct Options {
    target: Option<ReleaseTarget>,
    environment: String,
    formats: Option<Vec<PackageFormat>>,
}

impl Options {
    const USAGE: &'static str = "\
usage: cargo run -p bongocat-packaging -- [options]

Builds the Production product and packages the host platform release artifacts.

options:
  --target <triple>        release target; defaults to the host target
  --environment <name>     development | production (default: production)
  --formats <list>         comma separated subset of the target's release
                           artifacts (app,dmg for macOS; nsis for Windows)
  --print-version          print the product version and exit
  -h, --help               print this help";

    fn parse(arguments: Vec<String>) -> Result<Self> {
        let mut options = Self {
            target: None,
            environment: "production".to_owned(),
            formats: None,
        };
        let mut arguments = arguments.into_iter();
        while let Some(argument) = arguments.next() {
            match argument.as_str() {
                "--target" => {
                    let triple = next_value(&mut arguments, "--target")?;
                    options.target = Some(ReleaseTarget::parse(&triple)?);
                }
                "--environment" => {
                    let environment = next_value(&mut arguments, "--environment")?;
                    if !BUILD_ENVIRONMENTS.contains(&environment.as_str()) {
                        return failure(format!(
                            "unknown build environment {environment}; expected one of {}",
                            BUILD_ENVIRONMENTS.join(", ")
                        ));
                    }
                    options.environment = environment;
                }
                "--formats" => {
                    let formats = next_value(&mut arguments, "--formats")?;
                    options.formats = Some(parse_formats(&formats)?);
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
                other => {
                    return failure(format!("unexpected argument {other}\n\n{}", Self::USAGE));
                }
            }
        }
        Ok(options)
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
    match run(env::args().skip(1).collect()) {
        Ok(artifacts) => {
            report(&artifacts);
            std::process::ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("bongocat-packaging: {error}");
            std::process::ExitCode::FAILURE
        }
    }
}

fn run(arguments: Vec<String>) -> Result<Vec<PathBuf>> {
    let options = Options::parse(arguments)?;
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
    let provenance = write_provenance(&workspace, target, &options.environment)?;

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

/// Compiles the product application for `target` with the environment compiled in.
///
/// The environment is passed to the child process directly instead of through a
/// shell hook, so Windows and macOS use identical quoting rules and an inherited
/// value can never leak into a release build.
fn build_application(workspace: &Path, target: ReleaseTarget, environment: &str) -> Result<()> {
    let cargo = env::var_os("CARGO").unwrap_or_else(|| "cargo".into());
    let status = Command::new(&cargo)
        .current_dir(workspace)
        .args([
            "build",
            "--locked",
            "--release",
            "--target",
            target.triple(),
            "-p",
            APPLICATION_BINARY,
        ])
        .env("BONGOCAT_BUILD_ENV", environment)
        .status()
        .map_err(|error| {
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
fn write_provenance(workspace: &Path, target: ReleaseTarget, environment: &str) -> Result<PathBuf> {
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
        .arg("default")
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

fn report(artifacts: &[PathBuf]) {
    println!();
    println!("Build completed successfully.");
    println!();
    println!("Artifacts:");
    for artifact in artifacts {
        println!("  {}", artifact.display());
    }
}

#[cfg(test)]
mod tests {
    use super::ReleaseTarget;

    /// The published name is product name, resolved version and architecture
    /// token, with no packaging suffix. `self_update` matches release assets on
    /// the target triple, so the installer name only has to be stable and
    /// self-describing.
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
}
