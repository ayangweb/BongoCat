//! The update runtime's tests.
//!
//! The names they use are the crate's own: the runtime re-exports the public half
//! of every module below it, and the block after this is what the original test
//! module imported.

use super::{
    GITHUB_PROXY_PREFIXES, ManifestFetch, ManifestSource, RELEASE_BINARY_NAME, RELEASE_BUNDLE_NAME,
    RELEASE_MANIFEST_NAME, RELEASE_REPOSITORY_NAME, RELEASE_REPOSITORY_OWNER, RELEASE_SIGNING_KEY,
    UPDATE_MANIFEST_REQUEST_TIMEOUT, UPDATE_REQUEST_TIMEOUT, UpdateError, UpdateErrorCode,
    UpdateOutcome, UpdateProgress, UpdateRelease, UpdateRuntime, UpdateStage, UpdateUnavailability,
    configured_signing_key,
};
use crate::diagnostics::UpdateDiagnosticsTracker;
use crate::release::{ReleaseChannel, ReleaseConfiguration, UpdateTargetTriple};
use cargo_packager_updater::url::Url;

/// A fixed release configuration.
///
/// The gating tests must not go through `for_current_build`: a missing
/// configuration would turn the channel and error-code assertions below into
/// no-ops instead of real checks.
fn configuration(channel: ReleaseChannel) -> ReleaseConfiguration {
    ReleaseConfiguration {
        channel,
        repository_owner: "ayangweb",
        repository_name: "BongoCat",
        binary_name: "bongocat-app",
        bundle_name: Some("BongoCat.app"),
        target: UpdateTargetTriple::Aarch64AppleDarwin,
    }
}

fn runtime_for(channel: ReleaseChannel) -> UpdateRuntime {
    UpdateRuntime::new(
        configuration(channel),
        env!("CARGO_PKG_VERSION"),
        UpdateDiagnosticsTracker::default(),
    )
}

fn development_runtime() -> UpdateRuntime {
    runtime_for(ReleaseChannel::Development)
}

/// A manifest source for the selection tests: a stable URL and the proxy that
/// produced it.
fn test_source(proxy: Option<&'static str>, tail: &str) -> ManifestSource {
    ManifestSource {
        proxy,
        endpoint: Url::parse(&format!("https://source.invalid/{tail}"))
            .expect("a test endpoint parses"),
    }
}

/// A source is usable only when its request succeeds and the manifest parses;

#[test]
fn development_builds_never_reach_the_network() {
    let runtime = development_runtime();
    assert_eq!(runtime.channel().as_str(), "development");

    let error = runtime
        .check()
        .expect_err("development channel is disabled");
    assert_eq!(error.code(), UpdateErrorCode::EnvironmentDisabled);

    let snapshot = runtime.diagnostics().snapshot();
    assert_eq!(snapshot.checks_started, 1);
    assert_eq!(snapshot.checks_failed, 1);
    assert_eq!(
        snapshot.last_error_code,
        Some("update_environment_disabled")
    );
}

#[test]
fn the_release_signing_key_is_provisioned() {
    assert_eq!(
        RELEASE_SIGNING_KEY,
        Some(
            "dW50cnVzdGVkIGNvbW1lbnQ6IG1pbmlzaWduIHB1YmxpYyBrZXk6IERGNUUyQzlEMjU1REQ4NUUKUldSZTJGMGxuU3hlMzIyMHdwUldMNTRvRStMb1hJZlI3T2w0TEdFRlI3YXhsa3k0NldGUW5EN20K"
        )
    );
    assert!(configured_signing_key(RELEASE_SIGNING_KEY).is_some());
}

#[test]
fn absent_or_blank_signing_keys_fail_closed() {
    assert!(configured_signing_key(None).is_none());
    assert!(configured_signing_key(Some("")).is_none());
    assert!(configured_signing_key(Some("   \n\t")).is_none());
}

/// The endpoint is the repository's shared release manifest.
///
/// The literal URL is restated on purpose: it is the address the product is
/// expected to check, so a change to the repository identity or the asset name has
/// to be an intentional edit here rather than a silent consequence of a constant.
#[test]
fn the_manifest_endpoint_is_the_shared_release_manifest() {
    let endpoint = UpdateRuntime::manifest_endpoint(configuration(ReleaseChannel::Production))
        .expect("the release identity produces a valid URL");

    assert_eq!(
        endpoint.as_str(),
        format!(
            "https://github.com/{RELEASE_REPOSITORY_OWNER}/{RELEASE_REPOSITORY_NAME}/releases/latest/download/{RELEASE_MANIFEST_NAME}"
        )
    );
    assert_eq!(
        endpoint.as_str(),
        "https://github.com/ayangweb/BongoCat/releases/latest/download/latest.json"
    );
    assert_eq!(endpoint.scheme(), "https");

    // One shared manifest serves every target, so the build's triple must not
    // change the endpoint. The library still needs it to pick this host's entry out
    // of the manifest's `platforms` map.
    for target in [
        UpdateTargetTriple::Aarch64AppleDarwin,
        UpdateTargetTriple::X86_64AppleDarwin,
        UpdateTargetTriple::X86_64PcWindowsMsvc,
    ] {
        let mut configuration = configuration(ReleaseChannel::Production);
        configuration.target = target;
        assert_eq!(
            UpdateRuntime::manifest_endpoint(configuration)
                .expect("every shipped target produces a valid URL"),
            endpoint,
            "{} must request the shared manifest",
            target.as_str()
        );
    }
}

#[test]
fn error_codes_are_stable_strings() {
    let error = UpdateError::new(UpdateErrorCode::SignatureKeyMissing);
    assert_eq!(error.code_str(), "update_signature_key_missing");
    assert_eq!(error.to_string(), "update_signature_key_missing");
}

/// The transport has no timeout of its own, so the bound has to exist and has to
/// stay generous enough for a whole payload on a slow link.
#[test]
fn a_transfer_is_bounded_but_not_tight() {
    assert!(UPDATE_REQUEST_TIMEOUT >= std::time::Duration::from_secs(600));
    assert!(UPDATE_REQUEST_TIMEOUT <= std::time::Duration::from_secs(3600));
}

#[test]
fn outcome_variants_are_distinguishable() {
    assert_ne!(
        UpdateOutcome::UpToDate,
        UpdateOutcome::Available {
            release: UpdateRelease {
                version: "1.0.0".to_owned(),
                notes: None,
            },
        }
    );
}

#[test]
fn an_available_release_carries_its_changelog() {
    let outcome = UpdateOutcome::Available {
        release: UpdateRelease {
            version: "1.2.0".to_owned(),
            notes: Some("- fixed the thing".to_owned()),
        },
    };
    assert_eq!(
        outcome.release(),
        Some(UpdateRelease {
            version: "1.2.0".to_owned(),
            notes: Some("- fixed the thing".to_owned()),
        })
    );
    assert_eq!(UpdateOutcome::UpToDate.release(), None);
}

/// The proxy list is the product's ordered fallback policy, so its content and
/// order are pinned the way the official endpoint's URL is: the literals are
/// restated on purpose, so a change to either is an intentional edit.
#[test]
fn the_proxy_prefixes_are_the_ordered_fallback_policy() {
    assert_eq!(
        GITHUB_PROXY_PREFIXES,
        &[
            "https://cdn.gh-proxy.org",
            "https://v6.gh-proxy.org",
            "https://axisnow.gh-proxy.org",
            "https://v4.gh-proxy.org",
            "https://gh-proxy.org",
        ]
    );
    for prefix in GITHUB_PROXY_PREFIXES {
        let url = Url::parse(prefix).expect("a proxy prefix parses as a URL");
        assert_eq!(url.scheme(), "https", "{prefix} must be HTTPS");
        assert!(
            url.host_str().is_some_and(|host| !host.is_empty()),
            "{prefix} must name a host"
        );
        assert!(
            !prefix.ends_with('/'),
            "{prefix} must not end with a slash; prefixing adds its own"
        );
    }
}

/// The official URL prefixed by the first proxy is exactly the address the
/// product expects a proxy check to hit.
#[test]
fn the_proxied_manifest_endpoint_prefixes_the_official_url() {
    let official = UpdateRuntime::manifest_endpoint(configuration(ReleaseChannel::Production))
        .expect("the release identity produces a valid URL");

    let proxied = UpdateRuntime::proxied_manifest_endpoint(
        GITHUB_PROXY_PREFIXES
            .first()
            .expect("the list is non-empty"),
        &official,
    )
    .expect("a proxy prefix and the official URL produce a valid endpoint");

    assert_eq!(
        proxied.as_str(),
        "https://cdn.gh-proxy.org/https://github.com/ayangweb/BongoCat/releases/latest/download/latest.json"
    );
}

/// The source order is the fallback order: every proxy prefixed, official last.
#[test]
fn manifest_sources_try_proxies_in_order_then_the_official_endpoint() {
    let sources = UpdateRuntime::manifest_sources(configuration(ReleaseChannel::Production))
        .expect("every source endpoint parses");

    assert_eq!(sources.len(), GITHUB_PROXY_PREFIXES.len() + 1);
    for (source, prefix) in sources.iter().zip(GITHUB_PROXY_PREFIXES) {
        assert_eq!(source.proxy, Some(*prefix));
        assert_eq!(
            source.endpoint.as_str(),
            format!(
                "{prefix}/https://github.com/{RELEASE_REPOSITORY_OWNER}/{RELEASE_REPOSITORY_NAME}/releases/latest/download/{RELEASE_MANIFEST_NAME}"
            )
        );
    }
    let official = sources.last().expect("the official source is last");
    assert_eq!(
        official.proxy, None,
        "the official endpoint needs no prefix"
    );
    assert_eq!(
        official.endpoint.as_str(),
        "https://github.com/ayangweb/BongoCat/releases/latest/download/latest.json"
    );
}

/// anything else moves on to the next source, and the first usable one wins.
#[test]
fn the_first_usable_source_wins_and_later_sources_are_not_consulted() {
    let sources = vec![
        test_source(Some("https://first.invalid"), "a"),
        test_source(Some("https://second.invalid"), "b"),
        test_source(None, "official"),
    ];

    let mut consulted = Vec::new();
    let fetch = UpdateRuntime::select_manifest_source(&sources, |source| {
        consulted.push(source.endpoint.as_str().to_owned());
        if source.proxy == Some("https://second.invalid") {
            Ok(ManifestFetch::Offered("second"))
        } else {
            Err(UpdateError::new(UpdateErrorCode::ReleaseFetchFailed))
        }
    })
    .expect("the second source is usable");

    assert_eq!(
        consulted,
        vec![
            "https://source.invalid/a".to_owned(),
            "https://source.invalid/b".to_owned(),
        ],
        "the official endpoint must not be requested once a proxy succeeded"
    );
    let ManifestFetch::Offered(update) = fetch else {
        panic!("expected an offered update, got {fetch:?}");
    };
    assert_eq!(update, "second");
}

/// Every proxy failing hands the run to the official endpoint.
#[test]
fn all_proxies_failing_falls_through_to_the_official_endpoint() {
    let sources = vec![
        test_source(Some("https://proxy.invalid"), "a"),
        test_source(None, "official"),
    ];

    let mut consulted = Vec::new();
    let fetch = UpdateRuntime::select_manifest_source(&sources, |source| {
        consulted.push(source.proxy);
        match source.proxy {
            Some(_) => Err(UpdateError::new(UpdateErrorCode::ReleaseFetchFailed)),
            None => Ok(ManifestFetch::Offered("official")),
        }
    })
    .expect("the official endpoint is usable");

    assert_eq!(consulted, vec![Some("https://proxy.invalid"), None]);
    let ManifestFetch::Offered(update) = fetch else {
        panic!("expected an offered update, got {fetch:?}");
    };
    assert_eq!(update, "official");
}

/// A run where every source fails reports the last source's error.
#[test]
fn all_sources_failing_reports_the_last_error() {
    let sources = vec![
        test_source(Some("https://first.invalid"), "a"),
        test_source(None, "official"),
    ];

    let error = UpdateRuntime::select_manifest_source::<&str>(&sources, |source| {
        Err(UpdateError::new(if source.proxy.is_none() {
            UpdateErrorCode::ReleaseManifestInvalid
        } else {
            UpdateErrorCode::ReleaseFetchFailed
        }))
    })
    .expect_err("no source is usable");

    assert_eq!(error.code(), UpdateErrorCode::ReleaseManifestInvalid);
}

/// A manifest that parses but announces no asset for this host stops the
/// fallback: every source serves the same release asset, so another proxy
/// cannot change the answer and the diagnostic must survive.
#[test]
fn a_manifest_without_this_platform_stops_the_source_fallback() {
    let sources = vec![
        test_source(Some("https://first.invalid"), "a"),
        test_source(None, "official"),
    ];

    let mut consulted = Vec::new();
    let error = UpdateRuntime::select_manifest_source::<&str>(&sources, |source| {
        consulted.push(source.proxy);
        Err(UpdateError::new(UpdateErrorCode::NoMatchingAsset))
    })
    .expect_err("no source can offer this host an asset");

    assert_eq!(error.code(), UpdateErrorCode::NoMatchingAsset);
    assert_eq!(consulted.len(), 1, "later sources must not be consulted");
}

/// The download-URL conversion rewrites exactly the official GitHub URLs, once.
#[test]
fn download_urls_are_proxied_only_when_official_github() {
    let proxy = Some("https://cdn.gh-proxy.org");
    let github = Url::parse(
        "https://github.com/ayangweb/BongoCat/releases/download/v0.0.0-test/BongoCat_x64-setup.exe",
    )
    .expect("the announced GitHub URL parses");
    assert_eq!(
        UpdateRuntime::proxied_download_url(proxy, &github).as_str(),
        "https://cdn.gh-proxy.org/https://github.com/ayangweb/BongoCat/releases/download/v0.0.0-test/BongoCat_x64-setup.exe"
    );

    // The official run keeps the announced URL untouched.
    assert_eq!(UpdateRuntime::proxied_download_url(None, &github), github);

    // An already-proxied URL is not prefixed again: its host is the proxy, not
    // github.com, so the conversion is idempotent.
    let already_proxied = UpdateRuntime::proxied_download_url(proxy, &github);
    assert_eq!(
        UpdateRuntime::proxied_download_url(proxy, &already_proxied),
        already_proxied
    );

    // Any other host is left alone.
    let elsewhere = Url::parse("https://example.invalid/BongoCat_x64-setup.exe")
        .expect("the foreign URL parses");
    assert_eq!(
        UpdateRuntime::proxied_download_url(proxy, &elsewhere),
        elsewhere
    );

    // And so is a GitHub URL that is not HTTPS.
    let insecure =
        Url::parse("http://github.com/ayangweb/BongoCat/releases/download/v0.0.0-test/a")
            .expect("the insecure URL parses");
    assert_eq!(
        UpdateRuntime::proxied_download_url(proxy, &insecure),
        insecure
    );
}

/// The per-source manifest bound has to be real (a dead source is retired, not
/// waited out) and has to stay below the payload transfer bound.
#[test]
fn a_manifest_request_is_bounded_below_a_transfer() {
    assert!(UPDATE_MANIFEST_REQUEST_TIMEOUT >= std::time::Duration::from_secs(10));
    assert!(UPDATE_MANIFEST_REQUEST_TIMEOUT < UPDATE_REQUEST_TIMEOUT);
    assert!(UPDATE_MANIFEST_REQUEST_TIMEOUT <= std::time::Duration::from_secs(300));
}

/// A failed update has to say which step it failed in, because the UI reports
/// "could not download" and "could not install" as different problems.
#[test]
fn failures_carry_the_stage_they_happened_in() {
    assert_eq!(
        UpdateError::new(UpdateErrorCode::EnvironmentDisabled).stage(),
        UpdateStage::Check
    );
    assert_eq!(
        UpdateError::at(UpdateStage::Verify, UpdateErrorCode::SignatureInvalid).stage(),
        UpdateStage::Verify
    );
    assert_eq!(
        UpdateError::at(UpdateStage::Install, UpdateErrorCode::InstallFailed).stage(),
        UpdateStage::Install
    );
    assert_eq!(UpdateStage::Download.as_str(), "download");
}

/// The library reads and verifies the payload in one call, so the error code is
/// what separates a transfer failure from an authentication failure.
#[test]
fn download_failures_are_split_by_their_code() {
    assert_eq!(
        UpdateError::download_stage(UpdateErrorCode::DownloadTransportFailed),
        UpdateStage::Download
    );
    assert_eq!(
        UpdateError::download_stage(UpdateErrorCode::SignatureInvalid),
        UpdateStage::Verify
    );
    assert_eq!(
        UpdateError::download_stage(UpdateErrorCode::Internal),
        UpdateStage::Download,
        "an unrecognized failure must not claim the payload was authenticated"
    );
}

/// A manifest that never arrived and one that arrived unreadable are different
/// problems, and the diagnostics code has to say which happened.
#[test]
fn a_missing_manifest_is_not_an_unreadable_one() {
    assert_eq!(
        UpdateError::from_library(cargo_packager_updater::Error::ReleaseNotFound).code(),
        UpdateErrorCode::ReleaseFetchFailed
    );
    let unreadable = cargo_packager_updater::Error::Serialization(
        serde_json::from_str::<serde_json::Value>("not json").expect_err("invalid JSON"),
    );
    assert_eq!(
        UpdateError::from_library(unreadable).code(),
        UpdateErrorCode::ReleaseManifestInvalid
    );
    // This build's own version is parsed before any request, so a semver failure
    // out of the library can only be the manifest's `version` field.
    let bad_version = cargo_packager_updater::Error::Semver(
        cargo_packager_updater::semver::Version::parse("not a version")
            .expect_err("invalid version"),
    );
    assert_eq!(
        UpdateError::from_library(bad_version).code(),
        UpdateErrorCode::ReleaseManifestInvalid
    );
}

#[test]
fn download_progress_reports_a_fraction_only_when_the_size_is_known() {
    assert_eq!(
        UpdateProgress {
            downloaded_bytes: 512,
            total_bytes: Some(1024),
        }
        .fraction(),
        Some(0.5)
    );
    assert_eq!(
        UpdateProgress {
            downloaded_bytes: 512,
            total_bytes: None,
        }
        .fraction(),
        None
    );
    assert_eq!(
        UpdateProgress {
            downloaded_bytes: 2048,
            total_bytes: Some(1024),
        }
        .fraction(),
        Some(1.0),
        "a server that under-reports the length must not exceed a full bar"
    );
}

#[test]
fn the_release_page_url_follows_the_release_identity() {
    let runtime = runtime_for(ReleaseChannel::Production);
    assert_eq!(
        runtime.release_page_url("1.2.3").as_deref(),
        Some("https://github.com/ayangweb/BongoCat/releases/tag/v1.2.3")
    );
    assert_eq!(
        runtime.release_page_url("v1.2.3").as_deref(),
        Some("https://github.com/ayangweb/BongoCat/releases/tag/v1.2.3")
    );
}

#[test]
fn availability_requires_a_production_channel_and_a_signing_key() {
    assert!(
        !development_runtime().is_available(),
        "the development channel is disabled"
    );
    assert!(
        runtime_for(ReleaseChannel::Production).is_available(),
        "the provisioned signing key makes production updates available"
    );
}

/// The reason an entry point is missing has to be reportable, not just "no".
#[test]
fn unavailability_names_the_gate_that_closed() {
    assert_eq!(
        development_runtime().unavailability(),
        Some(UpdateUnavailability::DevelopmentChannel)
    );
    assert_eq!(
        runtime_for(ReleaseChannel::Production).unavailability(),
        None
    );
}

/// The release identity constants describe what the packaging pipeline ships.
#[test]
fn the_release_identity_matches_the_packaging_conventions() {
    assert_eq!(RELEASE_BINARY_NAME, "bongocat-app");
    assert_eq!(RELEASE_BUNDLE_NAME, "BongoCat.app");
}
