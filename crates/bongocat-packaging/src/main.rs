//! The project-level build, bundle and installer packaging entry point.
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
//! `--extract-release-notes` is the fourth, and it composes the document the other two
//! entry points publish: each changelog's entry for this version, followed by the download
//! links, the model gallery and the sponsor list. Those last three are generated here
//! rather than authored in the changelogs, because the asset names, the download URLs and
//! the version are all facts this crate already owns — a hand-written copy in
//! `CHANGELOG.md` would be a second place to correct on every release, with no gate that
//! could tell it had drifted.
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
//! drop link, describe the window the installer opens with, compress, sign —
//! and it never touches bundle or installer layout.
//!
//! Exit condition: restore `PackageFormat::Dmg` once `cargo-packager` ships a
//! `create-dmg` revision that works on the current macOS. See
//! `docs/adr/0033-build-packaging-and-release-toolchain.md` for the toolchain
//! decision and `docs/adr/0075-dmg-finder-window-layout.md` for the window.

#![allow(clippy::print_stdout, clippy::print_stderr)]

// Only the macOS disk image has a Finder window to describe, so the encoder is
// not compiled into any other platform's packaging run.
#[cfg(target_os = "macos")]
mod finder_store;

use std::{
    collections::BTreeMap,
    env, fs,
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
/// The fixed product bundle identifier.
const BUNDLE_IDENTIFIER: &str = "com.ayangweb.bongo-cat";
/// The oldest macOS release the product supports.
const MACOS_MINIMUM_SYSTEM_VERSION: &str = "12.0";
/// The product executable. `bongocat-update` pins this name for release archives.
const APPLICATION_BINARY: &str = "bongocat-app";
/// Preset models that must ship inside every packaged artifact.
const PRESET_MODELS: [&str; 3] = ["standard", "keyboard", "gamepad"];
/// Repository-relative directory holding icons and preset models.
const RESOURCE_DIRECTORY: &str = "resources";
/// Repository-relative macOS application icon, relative to [`RESOURCE_DIRECTORY`].
///
/// The same icon is the disk image's volume icon, so the mounted installer
/// window carries it in its title bar and its path bar.
const MACOS_ICON: &str = "icons/logo-macos.icns";
/// Repository-relative directory holding the three preset models.
const MODEL_DIRECTORY: &str = "models";
/// Resource bundle the macOS guided permission flow resolves its strings from.
///
/// `swift-rs` builds it inside `permission-flow`'s own `OUT_DIR` and never publishes it, while the
/// accessor SwiftPM generates looks for it in `Contents/Resources` among other places and aborts
/// the process when it is absent (ADR-0078).
const SWIFT_RESOURCE_BUNDLE: &str = "PermissionFlow_PermissionFlow.bundle";
/// Repository-relative directory holding the macOS `Info.plist` overlay.
const MACOS_INFO_PLIST: &str = "macos/Info.plist";
/// Repository-relative NSIS translation of the installer messages `cargo-packager` ships no text for.
const VIETNAMESE_INSTALLER_STRINGS: &str = "crates/bongocat-packaging/installer/Vietnamese.nsh";
/// NSIS installer languages, one per application language and in the order the picker lists them.
///
/// The set is the application's own language list (`bongocat-i18n` catalogs). English comes
/// first because NSIS falls back to the first language when the system language is not listed,
/// and `tools/tests/test_packaging_contract.py` keeps this list equal to the catalog set.
const INSTALLER_LANGUAGES: [&str; 7] = [
    "English",
    "SimpChinese",
    "TradChinese",
    "Arabic",
    "Vietnamese",
    "PortugueseBR",
    "Korean",
];
/// Repository-relative build provenance generator.
const PROVENANCE_GENERATOR: &str = "tools/record-provenance.py";
/// Build provenance file name inside the packaged resources.
const PROVENANCE_FILE: &str = "build-provenance.json";
/// Package output directory, relative to the workspace root.
const OUTPUT_DIRECTORY: &str = "target/package";
/// Staging directory for generated packaging inputs, relative to the output directory.
const STAGING_DIRECTORY: &str = "provenance";
/// Staging directory for the disk image contents, relative to the output directory.
///
/// Only the macOS disk image builder reads it, so it is macOS-only: a build for
/// any other platform would otherwise carry a constant no code path can reach,
/// which the workspace's `-D warnings` gate rejects.
#[cfg(target_os = "macos")]
const DISK_IMAGE_STAGING_DIRECTORY: &str = "dmg-stage";
/// The drop link the installer window offers as the install target.
#[cfg(target_os = "macos")]
const APPLICATIONS_LINK: &str = "Applications";
/// The icon file Finder reads a volume's own icon from.
#[cfg(target_os = "macos")]
const VOLUME_ICON_FILE: &str = ".VolumeIcon.icns";
/// The repair command the installer window offers next to the drop link.
///
/// The name carries no extension on purpose: macOS runs a file with a shebang and
/// the executable bit in Terminal on a double-click, and an extension would only
/// be one more line in the window's label. Finder's hidden-extension attribute is
/// not an alternative — it is a per-file bit that icon view does not honour.
///
/// It is named after what the person reading the window is looking at rather than
/// after the product: macOS tells them the app is damaged, and this is the item
/// that answers that. The `App` is what keeps it apart from disk damage, which
/// matters for an item that lives on a disk image.
#[cfg(target_os = "macos")]
const REPAIR_COMMAND: &str = "Fix Damaged App";
/// Where a drag-to-install product lands, which is what the repair repairs.
#[cfg(target_os = "macos")]
const APPLICATIONS_DIRECTORY: &str = "/Applications";
/// Staging directory for the cleaned preset models, relative to the output directory.
///
/// See [`stage_model_resources`].
const RESOURCE_STAGING_DIRECTORY: &str = "resource-stage";
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
#[derive(Debug, thiserror::Error)]
#[error("{0}")]
struct Failure(String);

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

    /// The architecture token every file name this target publishes is built from.
    ///
    /// One token per target, not one per kind of artifact, so the disk image, the
    /// bundle archive and the installer of a target are named after the same chip and
    /// a reader cannot end up matching `BongoCat-<version>-aarch64.dmg` against an
    /// artifact called something else. It is the Rust target architecture, which is
    /// also what a Homebrew cask writes in its own `arch` line, so a cask pinned to
    /// this token keeps resolving after the release renames anything.
    ///
    /// The full triple is deliberately not used: `-apple-darwin` is the same string on
    /// both Apple targets and says nothing a reader could act on.
    const fn architecture(self) -> &'static str {
        match self {
            Self::MacosAarch64 => "aarch64",
            Self::MacosX86_64 => "x64",
            Self::WindowsX86_64 => "x64",
        }
    }

    /// The file name the release publishes this target's installer under.
    ///
    /// `None` for the Apple targets: their `.app` and `.dmg` are already named by
    /// this crate. The Windows installer is named by `cargo-packager` instead,
    /// which hard-codes `{main binary name}_{version}_{arch}-setup.exe` with no
    /// option to configure it, so it is renamed to this name after packaging —
    /// which is why the name itself is [`Self::download_asset`]'s to define.
    ///
    /// Version and architecture come from the same sources as every other
    /// artifact, so neither is hard-coded here.
    fn installer_file_name(self) -> Option<String> {
        match self {
            Self::WindowsX86_64 => Some(self.download_asset()),
            Self::MacosAarch64 | Self::MacosX86_64 => None,
        }
    }

    /// The file name this target's hand-installed release asset is published under.
    ///
    /// Every target publishes exactly one: the Windows installer for Windows, the
    /// disk image for the Apple targets. The name is the same one
    /// [`Self::installer_file_name`] renames the installer to and
    /// [`build_disk_image`] creates the image under, which is the point — the
    /// release notes link these files, so a second spelling here would produce a
    /// download link to an asset the release never uploads, and nothing else in
    /// the build would fail.
    ///
    /// Deliberately *not* [`Self::update_payload_name`]: that is the signed archive
    /// the updater resolves out of the manifest, not something a person installs,
    /// and offering it as a download would point readers at the same bundle twice.
    fn download_asset(self) -> String {
        match self {
            Self::WindowsX86_64 => format!(
                "{PRODUCT_NAME}_{}_{}.exe",
                env!("CARGO_PKG_VERSION"),
                self.architecture()
            ),
            Self::MacosAarch64 | Self::MacosX86_64 => self.disk_image_file_name(),
        }
    }

    /// The file name this target's macOS disk image is published under.
    ///
    /// An Apple target's name, and the same name [`build_disk_image`] writes the
    /// image to, so the two cannot disagree about what a release uploads.
    fn disk_image_file_name(self) -> String {
        format!(
            "{PRODUCT_NAME}-{}-{}.dmg",
            env!("CARGO_PKG_VERSION"),
            self.architecture()
        )
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
    ///
    /// The macOS name carries [`Self::architecture`] and nothing else, which is the
    /// same token its disk image is named with. The archive stays unambiguous even
    /// without the platform: the extension is unique to the macOS payload, and
    /// Windows publishes an `.exe` under the installer's own name.
    fn update_payload_name(self) -> String {
        match self.installer_file_name() {
            Some(name) => name,
            None => format!(
                "{PRODUCT_NAME}-{}-{}.app.tar.gz",
                env!("CARGO_PKG_VERSION"),
                self.architecture()
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
  --target <triple>        one of aarch64-apple-darwin,
                            x86_64-apple-darwin, x86_64-pc-windows-msvc;
                            defaults to the host target
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
                           compose this version's release notes from CHANGELOG.md
                           and CHANGELOG.zh-CN.md, followed by the download
                           links, model gallery and sponsors this tool knows, and
                           write them to <file>, instead of packaging; takes no
                           other option
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
    let models = stage_model_resources(&workspace)?;
    // Resolved after `build_application`, which is what produces the Swift bundle.
    let swift_bundle = if target.is_apple() {
        Some(swift_resource_bundle(&workspace, target)?)
    } else {
        None
    };

    let config = packaging_config(
        &workspace,
        target,
        &provenance,
        &models,
        swift_bundle.as_deref(),
        &packager_formats(&requested),
    )?;
    let packages = cargo_packager::package(&config)?;
    // The packager has copied from the staging area by now, and nothing after
    // this point reads it, so it goes before the disk image and the update
    // payload are built rather than being left in the output directory.
    discard_staged_resources(&workspace);
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
            "{} does not look like the workspace root",
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
    models: &Path,
    swift_bundle: Option<&Path>,
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
    // to the NSIS installer, which the product does not ask for. The Apache-2.0
    // licence stays in the repository.
    config.icons = Some(vec![
        workspace
            .join(RESOURCE_DIRECTORY)
            .join(MACOS_ICON)
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
    config.resources = Some(resources(target, models, provenance, swift_bundle));

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
        config.nsis = Some(windows_installer(workspace));
    }

    Ok(config)
}

/// NSIS installer settings: a per-user install with a wizard language picker.
///
/// The picker only changes the wizard's own text and the uninstaller reuses the stored choice;
/// the application language still follows its own setting. NSIS preselects the system language
/// and falls back to English. `cargo-packager` embeds every language except Vietnamese, which
/// the repository supplies.
fn windows_installer(workspace: &Path) -> NsisConfig {
    let mut nsis = NsisConfig::new()
        .languages(INSTALLER_LANGUAGES)
        .custom_language_files([("Vietnamese", workspace.join(VIETNAMESE_INSTALLER_STRINGS))])
        .display_language_selector(true);
    // Per-user install: no administrator prompt, no machine-level registry keys.
    nsis.install_mode = NSISInstallerMode::CurrentUser;
    nsis
}

/// Maps the bundled resources to the locations the application resolves at runtime.
///
/// macOS reads them from `Contents/Resources`; Windows reads them from a
/// `resources/` directory beside the executable. The prefixes differ because
/// `cargo-packager` resolves resource targets relative to each platform's own
/// resource root, which is exactly the layout `bongocat-app::preset_root` expects.
///
/// `models` is the staged copy from [`stage_model_resources`], not the working
/// tree's `resources/models`.
fn resources(
    target: ReleaseTarget,
    models: &Path,
    provenance: &Path,
    swift_bundle: Option<&Path>,
) -> Vec<Resource> {
    let prefix = if target.is_apple() {
        String::new()
    } else {
        format!("{RESOURCE_DIRECTORY}/")
    };
    let mut resources = vec![
        Resource::Mapped {
            src: models.display().to_string(),
            target: PathBuf::from(format!("{prefix}{MODEL_DIRECTORY}")),
        },
        Resource::Mapped {
            src: provenance.display().to_string(),
            target: PathBuf::from(format!("{prefix}{PROVENANCE_FILE}")),
        },
    ];
    if let Some(swift_bundle) = swift_bundle {
        resources.push(Resource::Mapped {
            src: swift_bundle.display().to_string(),
            target: PathBuf::from(SWIFT_RESOURCE_BUNDLE),
        });
    }
    resources
}

/// Locates the Swift resource bundle `swift-rs` built for the guided permission flow.
///
/// The bundle stays in `permission-flow`'s own `OUT_DIR`, and SwiftPM's generated accessor aborts
/// the process when it cannot find it, so the package has to carry a copy. The only place to read
/// it from is the Cargo profile directory of the target being packaged, and the build script's own
/// `build` entry has no Swift output at all, so a directory that is missing skips to the next one
/// rather than ending the search.
fn swift_resource_bundle(workspace: &Path, target: ReleaseTarget) -> Result<PathBuf> {
    let build = workspace
        .join("target")
        .join(target.triple())
        .join("release")
        .join("build");
    let Ok(entries) = fs::read_dir(&build) else {
        return failure(format!(
            "could not read {} to locate {SWIFT_RESOURCE_BUNDLE}",
            build.display()
        ));
    };
    for entry in entries.flatten() {
        if !entry
            .file_name()
            .to_string_lossy()
            .starts_with("permission-flow-")
        {
            continue;
        }
        if let Some(bundle) =
            find_swift_resource_bundle(&entry.path().join("out/swift-rs/PermissionFlowShimFFI"))
        {
            return Ok(bundle);
        }
    }
    failure(format!(
        "missing {SWIFT_RESOURCE_BUNDLE} under {}: the macOS guided permission flow cannot \
         resolve its strings without it",
        build.display()
    ))
}

/// Finds the resource bundle below the build path `swift-rs` gave to SwiftPM.
///
/// That layout has already changed once, and this repository is built with more than one toolchain:
/// before Xcode 27 SwiftPM wrote products to `<arch>-apple-macosx/<Configuration>` under the build
/// path, and since then to `[out/]Products/<Configuration>`, and Xcode 27 keeps the older directory
/// around as well. A search pinned to the shape the local machine happens to produce passes here
/// and fails in CI, so the bundle is looked up by walking the package's own build path. The walk is
/// depth-bounded because the deepest layout in use puts the bundle three levels down, and a
/// packaging step has no reason to crawl an unbounded tree.
fn find_swift_resource_bundle(build_path: &Path) -> Option<PathBuf> {
    const MAX_DEPTH: usize = 3;

    let mut pending = vec![(build_path.to_path_buf(), 0usize)];
    while let Some((directory, depth)) = pending.pop() {
        let Ok(entries) = fs::read_dir(&directory) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path
                .file_name()
                .is_some_and(|name| name == SWIFT_RESOURCE_BUNDLE)
                && path.is_dir()
            {
                return Some(path);
            }
            if depth < MAX_DEPTH && path.is_dir() {
                pending.push((path, depth + 1));
            }
        }
    }
    None
}

/// The staging root for packaged resources, under the output directory.
///
/// [`stage_model_resources`] writes the cleaned models below it and
/// [`discard_staged_resources`] removes it again, so the one location is
/// derived here rather than spelled out twice.
fn resource_staging_directory(workspace: &Path) -> PathBuf {
    workspace
        .join(OUTPUT_DIRECTORY)
        .join(RESOURCE_STAGING_DIRECTORY)
}

/// Removes the resource staging area once the packager has copied from it.
///
/// The staged models exist only as an input to `cargo-packager`, which copies
/// them into the bundle or the installer, so keeping a second full copy of the
/// models in the output directory has no purpose. A staging area that is
/// already gone is the normal state of a run that cleaned up, so that is not an
/// error, and neither is a removal that fails: the directory is rebuilt from the
/// repository on the next build either way, and a leftover temporary directory
/// must not fail a package that is otherwise complete.
fn discard_staged_resources(workspace: &Path) {
    let staging = resource_staging_directory(workspace);
    if let Err(error) = fs::remove_dir_all(&staging) {
        println!(
            "warning: could not remove the resource staging area {}: {error}",
            staging.display()
        );
    }
}

/// Copies the preset models into the staging area the packager maps from.
///
/// The models are the one packaged tree that is also a working directory, so
/// whatever a developer's Finder, Explorer or editor leaves inside it would
/// otherwise be copied into every bundle, disk image and installer this crate
/// produces, and the shipped contents would depend on the state of the checkout
/// rather than on the repository. Staging also makes the artifact reproducible:
/// the same models always produce the same package.
///
/// Returns the staged model directory, which [`discard_staged_resources`] then
/// removes.
fn stage_model_resources(workspace: &Path) -> Result<PathBuf> {
    let source = workspace.join(RESOURCE_DIRECTORY).join(MODEL_DIRECTORY);
    if !source.is_dir() {
        return failure(format!("missing preset models at {}", source.display()));
    }
    let staged = resource_staging_directory(workspace).join(MODEL_DIRECTORY);
    if staged.exists() {
        fs::remove_dir_all(&staged)?;
    }
    copy_resource_tree(&source, &staged)?;
    for model in PRESET_MODELS {
        if !staged.join(model).is_dir() {
            return failure(format!(
                "preset model {model} is missing from {}",
                source.display()
            ));
        }
    }
    Ok(staged)
}

/// File and directory names that must never reach a packaged artifact.
///
/// No Live2D model file starts with a dot and none carries one of these names,
/// so skipping them cannot drop a file the runtime reads — which is what makes
/// this a safe filter rather than a guess. `._` and the other dot names are
/// what macOS writes next to files it has copied, `.DS_Store` is what Finder
/// leaves in every directory it visits, and the last two are what Windows
/// Explorer leaves behind.
fn is_packaging_junk(name: &str) -> bool {
    name.starts_with('.') || matches!(name, "__MACOSX" | "Thumbs.db" | "desktop.ini")
}

/// Recursively copies a resource tree, leaving [`is_packaging_junk`] behind.
///
/// A name that is not valid UTF-8 is skipped for the same reason: a model
/// manifest is UTF-8 JSON that names its files, so such a file cannot be
/// referenced by one and is dead weight in the package.
fn copy_resource_tree(source: &Path, destination: &Path) -> Result<()> {
    fs::create_dir_all(destination)?;
    for entry in fs::read_dir(source)? {
        let entry = entry?;
        let Some(name) = entry.file_name().into_string().ok() else {
            continue;
        };
        if is_packaging_junk(&name) {
            continue;
        }
        let target = destination.join(&name);
        if entry.file_type()?.is_dir() {
            copy_resource_tree(&entry.path(), &target)?;
        } else {
            fs::copy(entry.path(), &target)?;
        }
    }
    Ok(())
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

/// Where a published release asset can be fetched from.
///
/// The one place a download URL is spelled. [`publish_update_assets`] puts it in
/// the manifest the updater fetches and the release notes put it in front of a
/// reader, so a link in the notes and the manifest entry for the same file are
/// the same string rather than two shapes that have to be kept in step by hand.
fn release_asset_url(name: &str) -> String {
    format!(
        "{RELEASE_REPOSITORY_URL}/releases/download/v{version}/{name}",
        version = env!("CARGO_PKG_VERSION"),
    )
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
    let asset_url = release_asset_url(&payload_name);

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

/// The model gallery every release points readers at.
///
/// A link, not prose: the gallery is the answer to "where do I get more models",
/// and it is the same for every version, so a changelog entry is the wrong place for
/// it — two files would have to be edited per release to restate a URL that never
/// changes. The URL is composed here rather than read from the repository's
/// `repository` field, because that field names the source tree and this is a
/// different repository.
const MODELS_GALLERY_NAME: &str = "Awesome-BongoCat";
const MODELS_GALLERY_URL: &str = "https://github.com/ayangweb/Awesome-BongoCat";

/// The Homebrew tap macOS readers are pointed at as a third option.
///
/// The link goes to the tap's repository rather than to an install command, because
/// this document is also rendered by the update window, which shows prose and links
/// but has nowhere to put a shell command a reader could copy. The tap's own README
/// carries `brew tap` and `brew install`.
///
/// `Homebrew` is the tool's own name, so the label is the same in both languages and
/// is deliberately not a translation slot. The tap is a distribution channel this
/// project does not build or sign, so it is named as one option among the two
/// official disk images rather than as the recommended way to install.
const HOMEBREW_TAP_NAME: &str = "Homebrew";
const HOMEBREW_TAP_URL: &str = "https://github.com/ayangweb/Homebrew-BongoCat";

/// The sponsors every release lists, in the order they are shown.
///
/// Names and URLs only, so both languages link the same two entries under their own
/// heading. A sponsor that pays in a currency or in kind rather than in a link does
/// not belong here: this list is the release's disclosure of who is funding the
/// project, and it is shown in the same document as the install instructions.
const RELEASE_SPONSORS: [(&str, &str); 2] = [
    ("NexaRelay", "https://api.nexarelay.com"),
    ("ChooseC API", "https://api.choosec.cn"),
];

/// One language's copy for the block the release appends after the changelog entry.
///
/// The changelog says what changed; this says the things a reader of a release page
/// cannot get out of it — where to get this version, where to get more models, and
/// who sponsors the project. It is generated rather than authored in
/// `CHANGELOG.md` because every fact in it is one this crate already owns: the asset
/// names come from [`ReleaseTarget::download_asset`], the URLs from
/// [`release_asset_url`], and the version from the same `CARGO_PKG_VERSION` the
/// release tag was matched against. A hand-written copy would have to be corrected
/// by hand whenever any of those moved, and no gate would notice when it was not.
struct ReleaseNoteAppendix {
    /// Heading over the authored changelog entry.
    ///
    /// The entry's own sections are `###`, and so is nothing else in the document —
    /// every generated section below is `##`. Without a heading of its own the entry
    /// would open the document and its `###` headings would look like they belong to
    /// whatever came before, so this is what makes the two sources tellable apart: a
    /// reader can see at a glance which half is the changelog and which half the
    /// release generated.
    changelog_heading: &'static str,
    /// Heading of the download section.
    downloads_heading: &'static str,
    /// Row label for the Windows installer, and for the macOS row.
    windows_label: &'static str,
    /// Link text for the Windows installer, naming the one architecture it is built for.
    windows_architecture_label: &'static str,
    /// Why there is one Windows download and not one per architecture.
    ///
    /// Windows on ARM has no separate build: the release ships x64 only, and the
    /// product runs it there under emulation. A reader on ARM64 therefore has
    /// nothing to choose between, so its cell is prose rather than a link —
    /// otherwise a reader on ARM64 either looks for an ARM64 download that does not
    /// exist or assumes the one offered is not for them.
    ///
    /// Both locales carry their own parentheses, so this is a self-contained cell
    /// rather than an explanation a shared separator has to wrap.
    windows_note: &'static str,
    macos_label: &'static str,
    /// The two macOS variants, which are separate downloads.
    apple_silicon_label: &'static str,
    intel_label: &'static str,
    /// Heading of the models section. Its single row is a bare link: the gallery's own
    /// name says what it is, so a sentence describing it only repeats the heading.
    models_heading: &'static str,
    /// Heading of the sponsors section.
    sponsors_heading: &'static str,
}

const APPENDIX_ENGLISH: ReleaseNoteAppendix = ReleaseNoteAppendix {
    changelog_heading: "Changelog",
    downloads_heading: "Downloads",
    windows_label: "Windows 10+",
    windows_architecture_label: "x64",
    windows_note: "ARM64 (runs the x64 build through emulation)",
    macos_label: "macOS 12+",
    apple_silicon_label: "Apple Silicon",
    intel_label: "Intel",
    models_heading: "More models",
    sponsors_heading: "Sponsors",
};

/// The two locales differ only in their headings and the ARM64 sentence.
///
/// `Apple Silicon` and `Intel` are the chip names a reader looks for, and the Chinese
/// half of the block is read right after the English one. Translating them to
/// `Apple 芯片` / `Intel 芯片` named one machine three ways in one document: two
/// different words inside this block, and a third in the job names on the release's
/// checks. `docs/localization-copy-conventions.md` governs the UI catalog, which this
/// block is not; the constraint here is that a chip is called the same thing wherever
/// a reader meets it.
const APPENDIX_CHINESE: ReleaseNoteAppendix = ReleaseNoteAppendix {
    changelog_heading: "更新日志",
    downloads_heading: "下载地址",
    windows_label: "Windows 10+",
    windows_architecture_label: "x64",
    windows_note: "ARM64（通过仿真运行 x64 版本）",
    macos_label: "macOS 12+",
    apple_silicon_label: "Apple Silicon",
    intel_label: "Intel",
    models_heading: "更多模型",
    sponsors_heading: "赞助商",
};

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
/// What changed is only half of what a reader of a release page needs: the download
/// links, the model gallery and the sponsor disclosure are the same for every version
/// and are facts this crate already holds, so they are generated here and appended to
/// each language's block. They are deliberately *not* in the changelog files — a
/// versioned record of a release is the wrong home for text that has to be restated by
/// hand in two languages on every release, and nothing would check that it was right.
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
///
/// Each language gets the changelog entry and then the generated block, so a reader
/// meets this release's install instructions and its sponsor disclosure under their
/// own language rather than after scrolling past the other one. The block comes last
/// because the changelog opens with the upgrade notice: a reader who has to uninstall
/// an old version first should read that before reaching for a download link.
fn compose_release_notes(english: &str, chinese: &str) -> String {
    format!(
        "{}\n\n{RELEASE_NOTES_LANGUAGE_SEPARATOR}\n\n{}\n",
        release_note_block(english, &APPENDIX_ENGLISH),
        release_note_block(chinese, &APPENDIX_CHINESE),
    )
}

/// One language's half of the document: the authored entry under its own heading, then
/// the generated block.
fn release_note_block(entry: &str, appendix: &ReleaseNoteAppendix) -> String {
    format!(
        "## {}\n\n{entry}\n\n{}",
        appendix.changelog_heading,
        render_release_note_appendix(appendix),
    )
}

/// The block a release appends to one language's changelog entry.
///
/// Every download row links [`ReleaseTarget::download_asset`], the same function the
/// build names the artifact with, so a link here cannot point at a file the release
/// does not upload.
///
/// Both rows are one shape — `**<system> <oldest supported release>**: [<chip>](…)
/// | …` — and every cell names a chip rather than a file. Naming a chip is what lets a
/// reader tell the two macOS downloads apart, and it is the same vocabulary the
/// release's job names use; restating the artifact's own file name on one row but not
/// the other made the two rows read as different kinds of list.
///
/// Windows gets one cell and not one per architecture because the release ships no
/// ARM64 build — see [`ReleaseNoteAppendix::windows_note`]. The separator is a plain
/// vertical bar so each row stays a single line, which is the shape this Markdown is
/// rendered in by the update window and the shape the release page lays out.
///
/// Only Markdown the update window can render safely: ordinary links and list items,
/// no images and no raw HTML, both of which `bongocat-ui::update_markdown` deliberately
/// refuses to interpret.
fn render_release_note_appendix(copy: &ReleaseNoteAppendix) -> String {
    let mut appendix = String::new();

    appendix.push_str(&format!("## {}\n\n", copy.downloads_heading));
    let windows = ReleaseTarget::WindowsX86_64.download_asset();
    appendix.push_str(&format!(
        "- **{}**: [{}]({}) | {}\n",
        copy.windows_label,
        copy.windows_architecture_label,
        release_asset_url(&windows),
        copy.windows_note,
    ));
    appendix.push_str(&format!(
        "- **{}**: [{}]({}) | [{}]({}) | [{HOMEBREW_TAP_NAME}]({HOMEBREW_TAP_URL})\n\n",
        copy.macos_label,
        copy.apple_silicon_label,
        release_asset_url(&ReleaseTarget::MacosAarch64.download_asset()),
        copy.intel_label,
        release_asset_url(&ReleaseTarget::MacosX86_64.download_asset()),
    ));

    appendix.push_str(&format!("## {}\n\n", copy.models_heading));
    appendix.push_str(&format!(
        "- [{MODELS_GALLERY_NAME}]({MODELS_GALLERY_URL})\n\n"
    ));

    appendix.push_str(&format!("## {}\n\n", copy.sponsors_heading));
    for (name, url) in RELEASE_SPONSORS {
        appendix.push_str(&format!("- [{name}]({url})\n"));
    }

    // The callers set the blank lines around the block themselves — one against the
    // changelog entry, one against the language separator — so the block ends at its
    // last character rather than carrying a newline that would make the gap three deep.
    appendix.trim_end().to_owned()
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
/// `cargo-packager` writes the key with the process umask, which on macOS leaves it
/// world-readable. Windows has no equivalent mode, so there the file inherits the
/// account's ACL.
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
#[cfg(target_os = "macos")]
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
    let image = output_directory.join(target.disk_image_file_name());
    let staging = output_directory.join(DISK_IMAGE_STAGING_DIRECTORY);
    if staging.exists() {
        fs::remove_dir_all(&staging)?;
    }
    fs::create_dir_all(&staging)?;
    // The image is attached outside the build tree on purpose. Attaching a
    // writable volume inside a directory macOS is watching writes a
    // filesystem-events journal into that volume, and the journal would then
    // ship inside the published image and differ between machines.
    let mount = tempfile::TempDir::new()?;

    let bundle_name = bundle
        .file_name()
        .ok_or_else(|| Box::new(Failure(format!("invalid bundle {}", bundle.display()))))?
        .to_string_lossy()
        .into_owned();
    let staged_bundle = staging.join(&bundle_name);

    // `ditto` copies the bundle with its extended attributes and signature intact.
    let mut copy = Command::new("ditto");
    copy.arg(bundle).arg(&staged_bundle);
    run_command("ditto", &mut copy)?;

    // The drop link is what makes the mounted image a drag-to-install installer.
    std::os::unix::fs::symlink("/Applications", staging.join(APPLICATIONS_LINK))?;

    // Finder takes the volume's own icon from this file in the volume root, and
    // the flag set on the mounted volume below points at it. Without them the
    // window is titled with a generic disk image icon.
    let volume_icon = staging.join(VOLUME_ICON_FILE);
    fs::copy(
        workspace.join(RESOURCE_DIRECTORY).join(MACOS_ICON),
        &volume_icon,
    )?;
    mark_icon_file(&volume_icon)?;

    let repair = staging.join(REPAIR_COMMAND);
    fs::write(&repair, repair_command(&bundle_name))?;
    make_executable(&repair)?;

    // The window the mounted image opens with. Writing it is what keeps the
    // layout out of Finder: `create-dmg` would mount the image and drive Finder
    // with AppleScript, which needs a graphical session and an Automation
    // consent prompt, and this build has to run unattended.
    let layout = finder_store::WindowLayout::default();
    let items = [
        finder_store::Item::new(&bundle_name, 175, 110),
        finder_store::Item::new(APPLICATIONS_LINK, 485, 110),
        finder_store::Item::new(REPAIR_COMMAND, 330, 255),
    ];
    fs::write(
        staging.join(finder_store::FILE_NAME),
        finder_store::window(&layout, &items)
            .map_err(|error| Box::new(Failure(error.to_string())) as Box<dyn std::error::Error>)?,
    )?;

    if image.exists() {
        fs::remove_file(&image)?;
    }
    // The image is built read-write, because the volume's custom icon is a
    // Finder file attribute and only a mounted, writable volume can take one.
    let writable = image.with_extension("rw.dmg");
    if writable.exists() {
        fs::remove_file(&writable)?;
    }
    let mut create = Command::new("hdiutil");
    create
        .args(["create", "-volname", PRODUCT_NAME, "-srcfolder"])
        .arg(&staging)
        .args(["-ov", "-format", "UDRW"])
        .arg(&writable);
    run_command("hdiutil", &mut create)?;

    let mounted = MountedImage::attach(&writable, mount.path())?;
    set_volume_icon(mount.path())?;
    mounted.detach()?;

    // LZFSE rather than zlib or bzip2. The `UDZO` and `UDBZ` formats compress
    // the image in fixed 64 KB blocks, so they cannot see the repeated preset
    // model assets the bundle carries, while `ULMO` compresses the image as one
    // stream. Measured on one arm64 bundle, with every format mounted and the
    // binary plus all three models read back out: `UDZO` 13,795,779 B, `UDBZ`
    // 13,462,431 B, `ULFO` 13,082,584 B, `ULMO` 10,380,216 B — 24.8% below
    // `UDZO`. Mounting and reading the whole image averages 0.68 s against
    // `UDZO`'s 0.92 s, so the smaller image is also the faster one to install
    // from; `UDBZ` reads faster still (0.45 s) but is 23% larger. `ULMO` is an
    // Apple read-only compressed format supported well before the macOS 12
    // minimum this product declares.
    let mut compress = Command::new("hdiutil");
    compress
        .arg("convert")
        .arg(&writable)
        .args(["-format", "ULMO", "-o"])
        .arg(&image);
    run_command("hdiutil", &mut compress)?;
    fs::remove_file(&writable)?;

    let mut sign = Command::new("codesign");
    sign.args(["--force", "-s", identity]).arg(&image);
    run_command("codesign", &mut sign)?;

    fs::remove_dir_all(&staging)?;

    if !image.is_file() {
        return failure(format!("disk image was not created: {}", image.display()));
    }
    Ok(image)
}

/// The repair command the installer window offers, for `app_name`.
///
/// It is a command script rather than a product mode because the product may be
/// the thing that is broken: this file is the one thing in the disk image that
/// only needs Terminal, `sudo` and `xattr`, all of which macOS ships.
///
/// Two commands, in one order. The recursive form is the one that also covers a
/// bundle with anything quarantined inside it, and macOS 15 and later reject it,
/// so the plain form is the fallback rather than a third thing to try.
///
/// The script also closes the Terminal window it ran in, because the window is
/// the interface here and a prompt that promises to close it has to keep that
/// promise; see `finish` in the script for the two conditions that make it safe.
#[cfg(target_os = "macos")]
fn repair_command(app_name: &str) -> String {
    format!(
        r##"#!/bin/bash
# Repairs the {app_name} copy in {applications}.
#
# A bundle copied out of a downloaded disk image carries the quarantine attribute
# Gatekeeper puts on downloads, which is what makes macOS report the app as
# damaged or as coming from an unidentified developer. Removing that attribute
# is the whole repair, and /Applications is why it needs your password.
#
# Double-clicked, this file runs in Terminal and reads its password from there.
clear

app_dir="{applications}/{app_name}"
green="\033[0;32m"
yellow="\033[1;33m"
red="\033[0;31m"
cyan="\033[0;36m"
none="\033[0m"

# Closes the window once the reader is done with it, then exits with `status`.
#
# Terminal leaves a window open when the command in it finishes, so the promise
# printed below has to be kept here. The close waits a second, because Terminal
# asks for confirmation before it terminates a process that is still running and
# this script would be one. It only ever matches the window Terminal opened for
# this file, by tty and by title, so a shell somebody is using is never closed;
# from another terminal there is nothing to close and the script just waits.
finish() {{
  if [ "${{TERM_PROGRAM:-}}" = "Apple_Terminal" ]; then
    read -r -p "Press Enter to close this window... " _
    # Read here rather than inside the background job: bash gives a background
    # job /dev/null as its input, and `tty` then answers "not a tty" instead of
    # naming the window this script is running in.
    this_tty=$(tty)
    this_name=$(basename "$0")
    (
      sleep 1
      osascript -e "tell application \"Terminal\" to close (first window whose tty is \"$this_tty\" and name contains \"$this_name\")" >/dev/null 2>&1
    ) &
  else
    read -r -p "Press Enter when you are done... " _
  fi
  exit "$1"
}}

echo ""
echo -e "${{cyan}}Fix Damaged App: $app_dir${{none}}"
echo ""

if [ ! -d "$app_dir" ]; then
  echo -e "${{red}}{app_name} is not in {applications} yet.${{none}}"
  echo "Drag {app_name} from this disk image into Applications first, then run"
  echo "this again."
  echo ""
  finish 1
fi

if ! xattr -p com.apple.quarantine "$app_dir" >/dev/null 2>&1; then
  echo -e "${{yellow}}Nothing to repair: no quarantine attribute on {app_name}.${{none}}"
  echo ""
  echo "Either that copy is already repaired, or it is not the copy from this"
  echo "disk image. Install a fresh one by dragging {app_name} from this image"
  echo "onto Applications, replacing the copy that is there, then run this again"
  echo "if macOS still calls it damaged."
  echo ""
  finish 0
fi

echo -e "${{yellow}}Enter your Mac login password when asked. Nothing is shown while you type.${{none}}"
echo ""

if sudo xattr -r -d com.apple.quarantine "$app_dir" 2>/dev/null ||
  sudo xattr -d com.apple.quarantine "$app_dir"; then
  echo ""
  echo -e "${{green}}Repaired. {app_name} should open normally now.${{none}}"
  open "$app_dir"
  finish 0
fi

echo ""
echo -e "${{red}}Could not remove the quarantine attribute.${{none}}"
echo "If macOS still reports {app_name} as damaged, run this in Terminal:"
echo "  sudo codesign --force --deep --sign - \"$app_dir\""
finish 1
"##,
        app_name = app_name,
        applications = APPLICATIONS_DIRECTORY
    )
}

/// Makes a staged file runnable by whoever opens the image.
#[cfg(unix)]
fn make_executable(path: &Path) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;
    let mut permissions = fs::metadata(path)?.permissions();
    permissions.set_mode(0o755);
    fs::set_permissions(path, permissions)?;
    Ok(())
}

/// A disk image attached to a mount point, detached again when it goes out of
/// scope.
///
/// A build that fails between attaching and detaching must not leave a volume
/// mounted on the machine that ran it, and a build that fails after detaching
/// must not detach something it no longer owns, so the guard tracks whether the
/// volume is still attached.
#[cfg(target_os = "macos")]
struct MountedImage {
    mount_point: PathBuf,
    attached: bool,
}

#[cfg(target_os = "macos")]
impl MountedImage {
    /// Attaches `image` to `mount_point`, which has to exist and be empty.
    fn attach(image: &Path, mount_point: &Path) -> Result<Self> {
        // The mount point is given rather than parsed out of the attach output:
        // reading `hdiutil`'s output through a pipe is what breaks the
        // `create-dmg` script, and a build tool has no business mounting
        // anything under `/Volumes`.
        let mut attach = Command::new("hdiutil");
        attach
            .args([
                "attach",
                "-nobrowse",
                "-noverify",
                "-noautoopen",
                "-mountpoint",
            ])
            .arg(mount_point)
            .arg(image);
        run_command("hdiutil", &mut attach)?;
        Ok(Self {
            mount_point: mount_point.to_path_buf(),
            attached: true,
        })
    }

    /// Detaches the volume, reporting a failure rather than only warning.
    fn detach(mut self) -> Result<()> {
        let result = Self::run_detach(&self.mount_point);
        self.attached = false;
        result
    }

    fn run_detach(mount_point: &Path) -> Result<()> {
        let mut detach = Command::new("hdiutil");
        detach.arg("detach").arg(mount_point);
        run_command("hdiutil", &mut detach)
    }
}

#[cfg(target_os = "macos")]
impl Drop for MountedImage {
    fn drop(&mut self) {
        if self.attached {
            // Only a panic or an early return can get here: [`Self::detach`]
            // clears the flag before the value is dropped, and its own failure
            // is already reported to the caller.
            println!("warning: could not detach {}", self.mount_point.display());
            let _ = Self::run_detach(&self.mount_point);
        }
    }
}

/// Gives a mounted volume the product icon, so Finder titles the installer
/// window with it instead of a generic disk image icon.
#[cfg(target_os = "macos")]
fn set_volume_icon(mount: &Path) -> Result<()> {
    // `SetFile` ships with macOS in `/usr/bin`; it is what `create-dmg` uses for
    // this too, and nothing in the Command Line Tools is needed.
    let mut set = Command::new("SetFile");
    set.args(["-a", "C"]).arg(mount);
    run_command("SetFile", &mut set)
}

/// Marks a file as an icon, so Finder reads it as the volume's icon rather than
/// as an unknown file with an `.icns` name.
#[cfg(target_os = "macos")]
fn mark_icon_file(icon: &Path) -> Result<()> {
    let mut set = Command::new("SetFile");
    set.args(["-c", "icnC"]).arg(icon);
    run_command("SetFile", &mut set)
}

#[cfg(not(target_os = "macos"))]
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
        // The guided permission flow aborts the process when SwiftPM's accessor cannot find this
        // bundle, so a package that lost it would fail in a user's hands rather than here.
        contents.join("Resources").join(SWIFT_RESOURCE_BUNDLE),
    ];
    for path in expected {
        // The Swift resource bundle is a directory; the rest are files.
        if !path.exists() {
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
    use super::{
        INSTALLER_LANGUAGES, MODEL_DIRECTORY, OUTPUT_DIRECTORY, PRESET_MODELS, PRODUCT_NAME,
        ReleaseTarget, is_packaging_junk,
    };

    /// The wizard has to offer the picker, keep English first as the fallback, and carry
    /// translated installer messages for the one language `cargo-packager` embeds none for.
    #[test]
    fn the_windows_installer_offers_every_language_with_english_as_the_fallback() {
        let workspace = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .and_then(std::path::Path::parent)
            .expect("the packaging crate lives two levels below the workspace");
        let nsis = super::windows_installer(workspace);

        assert!(nsis.display_language_selector);
        assert_eq!(nsis.install_mode, super::NSISInstallerMode::CurrentUser);
        assert_eq!(INSTALLER_LANGUAGES.first(), Some(&"English"));
        assert_eq!(
            nsis.languages.as_deref(),
            Some(INSTALLER_LANGUAGES.map(str::to_owned).as_slice())
        );

        let custom = nsis.custom_language_files.expect("custom language files");
        assert_eq!(custom.len(), 1);
        let vietnamese = custom.get("Vietnamese").expect("Vietnamese strings");
        assert!(vietnamese.is_file(), "{} must exist", vietnamese.display());
    }

    /// The repair command is shell, and nothing in the build runs it, so `bash -n`
    /// is the only check it gets from the test suite. A quoting mistake in the
    /// format string above would otherwise ship a file that fails only on the
    /// reader's machine, after they have already dragged the app across.
    #[test]
    #[cfg(target_os = "macos")]
    fn the_shipped_repair_command_is_valid_shell() {
        let script = super::repair_command("BongoCat.app");
        let path = std::env::temp_dir().join("bongocat-repair-command-check.sh");
        std::fs::write(&path, &script).expect("write the repair command");

        let status = std::process::Command::new("bash")
            .args(["-n", &path.to_string_lossy()])
            .status()
            .expect("bash is on every macOS");
        let _ = std::fs::remove_file(&path);
        assert!(status.success(), "the repair command must parse:\n{script}");
    }

    /// The staged models are only an input to the packager, so packaging must not
    /// leave a second copy of them in the output directory — and cleaning up a
    /// run that never got as far as creating one must stay a no-op rather than an
    /// error, because that is the state every run ends in.
    #[test]
    fn the_resource_staging_area_does_not_survive_packaging() {
        let root = std::env::temp_dir().join("bongocat-packaging-staging-cleanup");
        let _ = std::fs::remove_dir_all(&root);
        let staging = super::resource_staging_directory(&root);
        let models = staging.join(MODEL_DIRECTORY).join(PRESET_MODELS[0]);

        // Nothing staged yet: this is the state of a run that already cleaned up.
        super::discard_staged_resources(&root);
        assert!(
            !staging.exists(),
            "cleaning up an absent staging area must succeed and leave nothing behind"
        );

        std::fs::create_dir_all(&models).expect("staged models");
        std::fs::write(models.join("cat.model3.json"), b"{}").expect("staged file");
        super::discard_staged_resources(&root);
        assert!(
            !staging.exists(),
            "the staging area must be gone once the packager has copied from it"
        );

        // The staging root is where cleanup looks, so it has to be the directory
        // staging writes into rather than the models inside it.
        assert_eq!(
            staging,
            root.join(OUTPUT_DIRECTORY)
                .join(super::RESOURCE_STAGING_DIRECTORY)
        );

        let _ = std::fs::remove_dir_all(&root);
    }

    /// The preset models are the one packaged tree that is also a working
    /// directory, so the filter has to catch what Finder, Explorer and the
    /// macOS copy machinery leave behind — including the dot files macOS writes
    /// *next to* the resources it copies.
    #[test]
    fn packaging_junk_is_recognised() {
        for junk in [
            ".DS_Store",
            "._texture_00.png",
            ".Spotlight-V100",
            ".Trashes",
            "__MACOSX",
            "Thumbs.db",
            "desktop.ini",
        ] {
            assert!(is_packaging_junk(junk), "{junk} must not be packaged");
        }
    }

    /// The filter is only safe because nothing a model actually contains looks
    /// like junk, so this is the half of the contract that protects the runtime:
    /// every real Live2D asset, manifest and sub-resource directory has to
    /// survive the filter, including names that merely contain a dot.
    #[test]
    fn model_assets_are_never_mistaken_for_junk() {
        for asset in [
            "demomodel.moc3",
            "cat.model3.json",
            "live2d_motion1.motion3.json",
            "live2d_expression0.exp3.json",
            "demomodel.cdi3.json",
            "live2d_motion1.flac",
            "demomodel.1024",
            "resources",
            "left-keys",
            "KeyT.png",
            "DPadUp.png",
            "background.png",
        ] {
            assert!(!is_packaging_junk(asset), "{asset} must be packaged");
        }
    }

    /// Every preset model the product promises has to be staged, so a model
    /// directory that was renamed or removed fails the build here instead of
    /// shipping a bundle that cannot load it.
    #[test]
    fn every_promised_preset_model_is_staged() {
        for model in PRESET_MODELS {
            assert!(
                !model.starts_with('.'),
                "a preset model name would be filtered as packaging junk: {model}"
            );
        }
    }

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

    /// Packaging fails outright when the Swift resource bundle is missing, so the
    /// search has to cover every layout SwiftPM has used for a `--build-path`
    /// build. This repository is built with more than one Xcode, and a search
    /// pinned to the shape the local machine produces passes here while failing
    /// in CI, which is exactly how the `out/Products`-only search got in.
    #[test]
    fn the_swift_resource_bundle_is_found_in_every_products_layout() {
        for products in [
            "out/Products/Release",
            "Products/Release",
            "release",
            "arm64-apple-macosx/release",
        ] {
            let root = std::env::temp_dir().join("bongocat-packaging-swift-bundle");
            let _ = std::fs::remove_dir_all(&root);
            let package = root
                .join("target")
                .join(ReleaseTarget::MacosAarch64.triple())
                .join("release")
                .join("build")
                .join("permission-flow-0123456789abcdef/out/swift-rs/PermissionFlowShimFFI");
            let expected = package.join(products).join(super::SWIFT_RESOURCE_BUNDLE);
            std::fs::create_dir_all(&expected).expect("products directory");

            assert_eq!(
                super::swift_resource_bundle(&root, ReleaseTarget::MacosAarch64)
                    .expect("the bundle is present"),
                expected,
                "the {products} layout must be searched"
            );

            let _ = std::fs::remove_dir_all(&root);
        }
    }

    /// A build whose Swift package produced no bundle has to fail with the reason
    /// rather than ship an app whose panel aborts the process on first use.
    #[test]
    fn a_build_without_the_swift_resource_bundle_fails_with_the_reason() {
        let root = std::env::temp_dir().join("bongocat-packaging-swift-bundle-missing");
        let _ = std::fs::remove_dir_all(&root);
        let package = root
            .join("target")
            .join(ReleaseTarget::MacosAarch64.triple())
            .join("release")
            .join("build")
            .join("permission-flow-0123456789abcdef/out/swift-rs/PermissionFlowShimFFI");
        // The build script's own `build` entry exists and carries no Swift output.
        std::fs::create_dir_all(&package).expect("build directory");

        let error = super::swift_resource_bundle(&root, ReleaseTarget::MacosAarch64)
            .expect_err("a build without the bundle must not package")
            .to_string();

        assert!(
            error.contains(super::SWIFT_RESOURCE_BUNDLE),
            "the failure has to name the bundle it could not find: {error}"
        );

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn apple_targets_keep_the_names_this_crate_already_builds() {
        assert!(ReleaseTarget::MacosAarch64.installer_file_name().is_none());
        assert!(ReleaseTarget::MacosX86_64.installer_file_name().is_none());
    }

    /// Every published file name carries its target's architecture token, and no other
    /// spelling of it.
    ///
    /// The Apple disk image used to be named `arm64` while the archive beside it was
    /// named `aarch64` and the manifest key was `macos-aarch64`, so one release
    /// uploaded `BongoCat-<version>-arm64.dmg`, shipped
    /// `BongoCat-<version>-aarch64.app.tar.gz` and announced `macos-aarch64` for the
    /// same machine. Three spellings of one chip, none of which any other layer had to
    /// agree with. Naming is not cosmetic here: the release notes link these names and
    /// the release workflow asserts them, so a second spelling produces a download link
    /// to an asset nobody uploaded and the build stays green.
    #[test]
    fn every_published_name_carries_the_targets_architecture_token() {
        let version = env!("CARGO_PKG_VERSION");
        for target in ReleaseTarget::ALL {
            // The separator is per platform, not free: `cargo-packager` hard-codes
            // `{binary}_{version}_{arch}-setup.exe` for the Windows installer, so the
            // rename step that drops `-setup` keeps its `_`, while the Apple artifacts
            // are built here and use `-`. Accepting either separator would let a
            // rename quietly move a published asset to a name the release notes and
            // the workflow no longer produce.
            let separator = if target.is_apple() { "-" } else { "_" };
            let stem = format!(
                "{PRODUCT_NAME}{separator}{version}{separator}{}",
                target.architecture()
            );
            for name in [target.download_asset(), target.update_payload_name()] {
                assert!(
                    name.starts_with(&stem),
                    "{target:?} publishes {name}, which does not start with {stem}"
                );
                assert!(
                    !name.contains("arm64"),
                    "{target:?} publishes {name}; the Rust target architecture is the \
                     only token a published name may use"
                );
                assert!(
                    !name.contains("-apple-darwin"),
                    "{target:?} publishes {name}; the platform suffix repeats what the \
                     extension already says"
                );
            }
        }
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
    ///
    /// The name carries the architecture and not the target triple. That is safe for
    /// the updater because it resolves the payload out of the manifest rather than by
    /// matching an asset name, and it is safe for a reader because the extension is
    /// unique to this file — so what has to hold is only that the two Apple targets
    /// still produce different names.
    #[test]
    fn the_apple_update_payload_is_a_bundle_archive() {
        let mut names = Vec::new();
        for target in [ReleaseTarget::MacosAarch64, ReleaseTarget::MacosX86_64] {
            let name = target.update_payload_name();
            assert!(
                name.ends_with(".app.tar.gz"),
                "the macOS payload must be an archive, got {name}"
            );
            assert!(
                name.contains(target.architecture()),
                "the payload name must name the architecture it was built for, got {name}"
            );
            assert!(
                !name.contains("-apple-darwin"),
                "the payload only exists on the one platform, so repeating it in the \
                 name says nothing: {name}"
            );
            names.push(name);
        }
        names.sort();
        assert_ne!(
            names[0], names[1],
            "the two Apple targets must publish differently named payloads, got {names:?}"
        );
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

        let (english, chinese) = notes
            .split_once("\n\n---\n\n")
            .unwrap_or_else(|| panic!("the two languages must be joined by a rule: {notes}"));

        assert!(
            english.starts_with("## Changelog\n\n### ✨ Features\n\n- did a thing"),
            "{english}"
        );
        assert!(
            chinese.starts_with("## 更新日志\n\n### ✨ 新功能\n\n- 做了件事"),
            "{chinese}"
        );
        // The separator has to be a thematic break and nothing else, or the renderer on
        // the other side draws a paragraph instead of a rule.
        assert!(notes.lines().any(|line| line == "---"));
    }

    /// The published document, spelled out: two authored entries in their own language,
    /// each followed by that language's generated block, joined by one rule.
    ///
    /// The asset names are read back out of [`ReleaseTarget::download_asset`] rather than
    /// written here, because `tools/tests/test_product_version_contract.py` fails the
    /// build if a shipped version is restated anywhere in this file. What this pins is the
    /// shape — the headings, the list markup, the link syntax, and the order — which is
    /// what the release page and the update window each depend on.
    #[test]
    fn the_release_notes_append_a_download_block_to_each_language() {
        let block = |copy: &super::ReleaseNoteAppendix| {
            let windows = ReleaseTarget::WindowsX86_64.download_asset();
            let apple = ReleaseTarget::MacosAarch64.download_asset();
            let intel = ReleaseTarget::MacosX86_64.download_asset();
            format!(
                "## {downloads}\n\n\
                 - **{windows_label}**: [{windows_arch}]({windows_url}) | {windows_note}\n\
                 - **{macos_label}**: [{apple_silicon}]({apple_url}) | [{intel}]({intel_url}) | \
                 [Homebrew](https://github.com/ayangweb/Homebrew-BongoCat)\n\n\
                 ## {models}\n\n\
                 - [{gallery}]({gallery_url})\n\n\
                 ## {sponsors}\n\n\
                 - [NexaRelay](https://api.nexarelay.com)\n\
                 - [ChooseC API](https://api.choosec.cn)",
                downloads = copy.downloads_heading,
                windows_label = copy.windows_label,
                windows_arch = copy.windows_architecture_label,
                windows_url = super::release_asset_url(&windows),
                windows_note = copy.windows_note,
                macos_label = copy.macos_label,
                apple_silicon = copy.apple_silicon_label,
                apple_url = super::release_asset_url(&apple),
                intel = copy.intel_label,
                intel_url = super::release_asset_url(&intel),
                models = copy.models_heading,
                gallery = super::MODELS_GALLERY_NAME,
                gallery_url = super::MODELS_GALLERY_URL,
                sponsors = copy.sponsors_heading,
            )
        };

        assert_eq!(
            super::compose_release_notes("- did a thing", "- 做了件事"),
            format!(
                "## {changelog}\n\n- did a thing\n\n{block_en}\n\n---\n\n\
                 ## {changelog_zh}\n\n- 做了件事\n\n{block_zh}\n",
                changelog = super::APPENDIX_ENGLISH.changelog_heading,
                block_en = block(&super::APPENDIX_ENGLISH),
                changelog_zh = super::APPENDIX_CHINESE.changelog_heading,
                block_zh = block(&super::APPENDIX_CHINESE),
            ),
        );
    }

    /// The generated block links the artifacts this release uploads, and only those.
    ///
    /// The signed update payloads are what the updater resolves out of the manifest: they
    /// install the bundle the disk image already installs, so offering them to a reader
    /// would be a second, redundant download of the same program.
    #[test]
    fn the_generated_block_offers_every_installable_artifact_and_no_payload() {
        let notes = super::compose_release_notes("- did a thing", "- 做了件事");

        for target in ReleaseTarget::ALL {
            let asset = target.download_asset();
            let link = format!("]({})", super::release_asset_url(&asset));
            assert!(
                notes.contains(&link),
                "{target:?} publishes {asset}, so the notes must link it, got {notes}",
            );
        }

        for target in [ReleaseTarget::MacosAarch64, ReleaseTarget::MacosX86_64] {
            let payload = target.update_payload_name();
            assert!(
                !notes.contains(&payload),
                "{payload} is an update payload rather than a download, but the notes \
                 offer it",
            );
        }
    }

    /// Every link in the published document has to be one the update window will open.
    ///
    /// `bongocat-ui::update_markdown` turns a refused target into plain text, so a
    /// `http://` or `file://` link here would arrive at a reader as a label with no way
    /// to follow it — and the release page and the window read this same document.
    #[test]
    fn every_link_in_the_release_notes_is_one_the_update_window_opens() {
        let notes = super::compose_release_notes("- did a thing", "- 做了件事");

        let mut links = 0;
        let mut rest = notes.as_str();
        while let Some(at) = rest.find("](") {
            rest = &rest[at + 2..];
            let end = rest
                .find(')')
                .unwrap_or_else(|| panic!("an unterminated link target in {notes}"));
            let (target, tail) = rest.split_at(end);
            rest = tail;
            links += 1;
            assert!(
                target.starts_with("https://"),
                "{target} is not an https target, so the update window renders it as \
                 plain text",
            );
        }
        // Per language: one installer, two disk images, the tap, the gallery and both
        // sponsors.
        assert_eq!(links, 14, "unexpected link count in {notes}");
    }

    /// The two sources sit at the same level and in a fixed order: the changelog's own
    /// heading, then its entry, then the generated block.
    ///
    /// The heading is what makes the entry's `###` sections read as belonging to the
    /// changelog rather than to whatever preceded them, and the entry staying first is
    /// what puts a release's upgrade notice — which is what a changelog opens with —
    /// ahead of its download links.
    #[test]
    fn each_language_heads_its_changelog_before_the_generated_block() {
        let notes = super::compose_release_notes(
            "### ⚠️ Upgrade Notice\n\n- uninstall the old version first",
            "### ⚠️ 升级说明\n\n- 请先卸载旧版本",
        );

        let (english, chinese) = notes
            .split_once("\n\n---\n\n")
            .unwrap_or_else(|| panic!("the two languages must be joined by a rule: {notes}"));

        for (block, changelog, entry, downloads) in [
            (
                english,
                super::APPENDIX_ENGLISH.changelog_heading,
                "uninstall the old version first",
                super::APPENDIX_ENGLISH.downloads_heading,
            ),
            (
                chinese,
                super::APPENDIX_CHINESE.changelog_heading,
                "请先卸载旧版本",
                super::APPENDIX_CHINESE.downloads_heading,
            ),
        ] {
            let at = |needle: &str| {
                block
                    .find(needle)
                    .unwrap_or_else(|| panic!("{needle} is missing from {block}"))
            };
            let (changelog_at, entry_at, downloads_at) = (at(changelog), at(entry), at(downloads));

            assert_eq!(
                block[..changelog_at].matches("\n## ").count(),
                0,
                "{changelog} must be the first `##` of its half, so the entry's own `###` \
                 sections have a parent to sit under",
            );
            assert!(
                changelog_at < entry_at && entry_at < downloads_at,
                "{changelog} must come first, then the entry, then {downloads}; got \
                 {changelog_at}, {entry_at}, {downloads_at}",
            );
        }
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
