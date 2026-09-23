//! Release channel, target identity, and the immutable build-time configuration
//! that binds one update run to a distribution channel.

use bongocat_config::BuildEnvironment;

/// The distribution channel an update run is bound to.
///
/// The channel is derived at compile time from [`BuildEnvironment`] and cannot be
/// switched by runtime input. `AGENTS.md` §10 requires the update channel to be
/// isolated per environment, so a Development build never installs a release
/// artifact even when it can reach the same endpoint.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ReleaseChannel {
    /// Development builds never fetch or install a release artifact.
    Development,
    /// Production builds update from the published release channel.
    Production,
}

impl ReleaseChannel {
    pub const fn from_environment(environment: BuildEnvironment) -> Self {
        match environment {
            BuildEnvironment::Development => Self::Development,
            BuildEnvironment::Production => Self::Production,
        }
    }

    /// Whether this channel is allowed to fetch and install releases.
    pub const fn is_enabled(self) -> bool {
        matches!(self, Self::Production)
    }

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Development => "development",
            Self::Production => "production",
        }
    }
}

/// The three release targets the Native Rewrite ships.
///
/// The list is closed on purpose: `AGENTS.md` §1 restricts the product to these
/// combinations, so an unrecognized host must refuse to update rather than fall
/// back to a guessed asset. Windows ARM64 is not shipped, because Cubism Native
/// R5 has no desktop ARM64 Core and Windows runs the x64 build under emulation.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum UpdateTargetTriple {
    Aarch64AppleDarwin,
    X86_64AppleDarwin,
    X86_64PcWindowsMsvc,
}

impl UpdateTargetTriple {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Aarch64AppleDarwin => "aarch64-apple-darwin",
            Self::X86_64AppleDarwin => "x86_64-apple-darwin",
            Self::X86_64PcWindowsMsvc => "x86_64-pc-windows-msvc",
        }
    }

    /// Whether this target installs a macOS `.app` bundle rather than a bare executable.
    pub const fn is_apple(self) -> bool {
        matches!(self, Self::Aarch64AppleDarwin | Self::X86_64AppleDarwin)
    }
}

/// Release-manifest identity.
///
/// This is deliberately a *second* `impl` block: `tools/tests/test_packaging_contract.py`
/// recovers the shipped target triples by regex-matching the first
/// `impl UpdateTargetTriple { .. }` block, and the platform keys below are a
/// different string set that must not be confused with the triples.
impl UpdateTargetTriple {
    /// The `<os>-<arch>` key this target is announced under in the release manifest.
    ///
    /// `cargo-packager-updater` derives the key it looks up from the host at runtime
    /// (`{get_updater_target()}-{get_updater_arch()}`), so the spelling has to match
    /// that exactly — notably `macos`, never `darwin`, and `aarch64`, never `arm64`.
    /// The packaging tool writes the same keys; the agreement is pinned by
    /// `tools/tests/test_update_release_contract.py`.
    pub const fn manifest_platform(self) -> &'static str {
        match self {
            Self::Aarch64AppleDarwin => "macos-aarch64",
            Self::X86_64AppleDarwin => "macos-x86_64",
            Self::X86_64PcWindowsMsvc => "windows-x86_64",
        }
    }
}

/// The release target this binary was built for.
#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
pub const HOST_TARGET_TRIPLE: UpdateTargetTriple = UpdateTargetTriple::Aarch64AppleDarwin;

#[cfg(all(target_os = "macos", target_arch = "x86_64"))]
pub const HOST_TARGET_TRIPLE: UpdateTargetTriple = UpdateTargetTriple::X86_64AppleDarwin;

#[cfg(all(target_os = "windows", target_arch = "x86_64"))]
pub const HOST_TARGET_TRIPLE: UpdateTargetTriple = UpdateTargetTriple::X86_64PcWindowsMsvc;

#[cfg(not(any(
    all(target_os = "macos", target_arch = "aarch64"),
    all(target_os = "macos", target_arch = "x86_64"),
    all(target_os = "windows", target_arch = "x86_64")
)))]
compile_error!("BongoCat Native Rewrite builds only for macOS and x86_64 Windows");

/// Immutable configuration for one update run.
///
/// Every field is compile-time constant: no user config, CLI flag or runtime
/// input can retarget an update at a different repository, channel or target.
///
/// `binary_name` and `bundle_name` describe the release identity rather than
/// driving the transport: `cargo-packager-updater` takes the payload location from
/// the release manifest and the install path from the running executable, so
/// neither name locates a file inside an archive any more.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ReleaseConfiguration {
    pub channel: ReleaseChannel,
    pub repository_owner: &'static str,
    pub repository_name: &'static str,
    pub binary_name: &'static str,
    /// Bundle directory name inside the release archive, on targets that install a bundle.
    pub bundle_name: Option<&'static str>,
    pub target: UpdateTargetTriple,
}

impl ReleaseConfiguration {
    /// The configuration for the current build.
    pub const fn for_current_build(
        environment: BuildEnvironment,
        repository_owner: &'static str,
        repository_name: &'static str,
        binary_name: &'static str,
        bundle_name: &'static str,
    ) -> Self {
        let target = HOST_TARGET_TRIPLE;
        Self {
            channel: ReleaseChannel::from_environment(environment),
            repository_owner,
            repository_name,
            binary_name,
            bundle_name: if target.is_apple() {
                Some(bundle_name)
            } else {
                None
            },
            target,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{HOST_TARGET_TRIPLE, ReleaseChannel, UpdateTargetTriple};
    use bongocat_config::BuildEnvironment;

    #[test]
    fn development_channel_is_disabled_and_production_is_enabled() {
        assert!(!ReleaseChannel::from_environment(BuildEnvironment::Development).is_enabled());
        assert!(ReleaseChannel::from_environment(BuildEnvironment::Production).is_enabled());
    }

    #[test]
    fn target_triples_match_the_shipped_set() {
        assert_eq!(
            UpdateTargetTriple::Aarch64AppleDarwin.as_str(),
            "aarch64-apple-darwin"
        );
        assert_eq!(
            UpdateTargetTriple::X86_64PcWindowsMsvc.as_str(),
            "x86_64-pc-windows-msvc"
        );
        assert!(UpdateTargetTriple::X86_64AppleDarwin.is_apple());
        assert!(!UpdateTargetTriple::X86_64PcWindowsMsvc.is_apple());
    }

    /// The manifest keys are what `cargo-packager-updater` looks up, so they must use
    /// the `<os>-<arch>` spelling it derives and not the Rust target triple.
    #[test]
    fn manifest_platforms_use_the_updater_spelling() {
        assert_eq!(
            UpdateTargetTriple::Aarch64AppleDarwin.manifest_platform(),
            "macos-aarch64"
        );
        assert_eq!(
            UpdateTargetTriple::X86_64AppleDarwin.manifest_platform(),
            "macos-x86_64"
        );
        assert_eq!(
            UpdateTargetTriple::X86_64PcWindowsMsvc.manifest_platform(),
            "windows-x86_64"
        );
        for target in [
            UpdateTargetTriple::Aarch64AppleDarwin,
            UpdateTargetTriple::X86_64AppleDarwin,
            UpdateTargetTriple::X86_64PcWindowsMsvc,
        ] {
            assert_ne!(
                target.manifest_platform(),
                target.as_str(),
                "a manifest key is not a target triple"
            );
        }
    }

    #[test]
    fn host_target_is_one_of_the_shipped_combinations() {
        assert!(matches!(
            HOST_TARGET_TRIPLE,
            UpdateTargetTriple::Aarch64AppleDarwin
                | UpdateTargetTriple::X86_64AppleDarwin
                | UpdateTargetTriple::X86_64PcWindowsMsvc
        ));
    }
}
