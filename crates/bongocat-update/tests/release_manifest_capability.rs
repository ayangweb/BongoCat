//! Capability contract for the release manifest, the Minisign payload signature and
//! the install step.
//!
//! Everything here runs against loopback HTTP servers, so it needs no published
//! release and no network. The payload is signed with `cargo-packager`'s own signer —
//! the tool the release pipeline uses — and verified by `cargo-packager-updater`,
//! which is what the application runs. That is the point of the file: it pins the
//! **signer/verifier pair** as a whole, so a change to either side that breaks the
//! other fails here rather than on a user's machine.
//!
//! Covered: the shared release manifest and its `<os>-<arch>` platform key lookup,
//! version comparison, the detached-signature check over the downloaded bytes,
//! rejection of a tampered payload, rejection of a payload signed by an unknown key,
//! refusal to treat an empty public key as "nothing to verify", and (on macOS)
//! replacement of the installed bundle with the archive's contents.
//!
//! Not covered: the real GitHub endpoint, Windows installer execution, process
//! restart, and anything requiring a published release.

use std::fs;
use std::io::{Read, Write};
use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::thread;

use cargo_packager::sign::{SigningConfig, generate_key, sign_file};
use cargo_packager_updater::semver::Version;
use cargo_packager_updater::url::Url;
use cargo_packager_updater::{Config, Error, Updater, UpdaterBuilder};

// The manifest asset name the runtime requests, taken from the crate rather than
// restated, so the fixture is served under exactly the name an update run asks for. The
// literal itself is pinned by
// `runtime::tests::the_manifest_endpoint_is_the_shared_release_manifest`.
use bongocat_update::RELEASE_MANIFEST_NAME as MANIFEST_NAME;

/// The version the fixture release announces.
///
/// Far above any real product version, so the running build is always offered the
/// update whatever the workspace version happens to be.
const RELEASE_VERSION: &str = "9999.0.0";

/// The version the updater under test believes it runs, so no fixture has to restate
/// the product version.
const CURRENT_VERSION: &str = env!("CARGO_PKG_VERSION");

/// Serves one fixed body per request path on an ephemeral loopback port.
///
/// Plain HTTP is accepted because the library only requires a URL it can parse; the
/// thread is detached and dies with the test process.
struct LocalServer {
    base_url: String,
}

impl LocalServer {
    fn serve(routes: Vec<(String, Vec<u8>)>) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind a loopback port");
        let port = listener
            .local_addr()
            .expect("read the bound address")
            .port();

        thread::spawn(move || {
            for stream in listener.incoming() {
                let Ok(mut stream) = stream else { continue };
                let mut request = [0u8; 4096];
                let read = stream.read(&mut request).unwrap_or(0);
                let request = String::from_utf8_lossy(&request[..read]);
                let path = request.split_whitespace().nth(1).unwrap_or("/").to_owned();

                match routes.iter().find(|(route, _)| *route == path) {
                    Some((_, body)) => {
                        let header = format!(
                            "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nContent-Type: application/octet-stream\r\nConnection: close\r\n\r\n",
                            body.len()
                        );
                        let _ = stream.write_all(header.as_bytes());
                        let _ = stream.write_all(body);
                    }
                    None => {
                        let _ = stream.write_all(
                            b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
                        );
                    }
                }
                let _ = stream.flush();
            }
        });

        Self {
            base_url: format!("http://127.0.0.1:{port}"),
        }
    }
}

/// The `<os>-<arch>` key the library looks the host up under.
///
/// Spelled out rather than borrowed from the library, and asserted against it in
/// `the_platform_key_matches_the_library`: a library that changed its spelling would
/// otherwise silently stop finding updates.
fn host_platform_key() -> String {
    let os = if cfg!(target_os = "macos") {
        "macos"
    } else if cfg!(target_os = "windows") {
        "windows"
    } else {
        "linux"
    };
    let arch = if cfg!(target_arch = "aarch64") {
        "aarch64"
    } else if cfg!(target_arch = "x86_64") {
        "x86_64"
    } else {
        "i686"
    };
    format!("{os}-{arch}")
}

fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("bongocat-manifest-capability-{name}"));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).expect("scratch directory");
    dir
}

#[cfg(target_os = "macos")]
fn write(path: &Path, contents: &str) {
    fs::create_dir_all(path.parent().expect("parent directory")).expect("parent directory");
    fs::write(path, contents).expect("write file");
}

fn read(path: &Path) -> String {
    fs::read_to_string(path).expect("read file")
}

/// Build a gzipped tar archive whose single root directory carries the entries.
///
/// One leading component on purpose: the macOS install path drops the archive's root
/// directory and renames what remains onto the bundle path.
fn write_archive(path: &Path, entries: &[(&str, &str)]) {
    let file = fs::File::create(path).expect("create the archive");
    let encoder = flate2::write::GzEncoder::new(file, flate2::Compression::default());
    let mut builder = tar::Builder::new(encoder);

    for (name, contents) in entries {
        let mut header = tar::Header::new_gnu();
        header.set_size(contents.len() as u64);
        header.set_mode(0o644);
        header.set_cksum();
        builder
            .append_data(&mut header, name, contents.as_bytes())
            .expect("append an archive entry");
    }

    builder
        .into_inner()
        .expect("finish the tar")
        .finish()
        .expect("finish the gzip");
}

/// Sign `payload` with a freshly generated Minisign key pair.
///
/// Returns the public key and the detached signature, both exactly as the release
/// pipeline publishes them.
fn sign_payload(payload: &Path) -> (String, String) {
    let keypair = generate_key(Some(String::new())).expect("generate a signing key");
    let signature_path = sign_file(
        &SigningConfig::new()
            .private_key(keypair.sk.clone())
            .password(String::new()),
        payload,
    )
    .expect("sign the payload");

    (keypair.pk, read(&signature_path))
}

/// Serve `payload` on its own port and return the URL it is served under.
fn serve_payload(payload: &Path) -> String {
    let name = payload
        .file_name()
        .expect("the payload has a file name")
        .to_string_lossy()
        .to_string();
    let server = LocalServer::serve(vec![(
        format!("/{name}"),
        fs::read(payload).expect("read the payload"),
    )]);
    format!("{}/{name}", server.base_url)
}

/// Serve the shared release manifest and return its own URL.
///
/// This is the shape `crates/bongocat-packaging` publishes as `latest.json`: one
/// document carrying a `platforms` entry per shipped target, keyed `<os>-<arch>`. It is
/// served under the name the runtime requests, so this helper mirrors the real endpoint
/// (`releases/latest/download/latest.json`) and the platform lookup the library performs
/// on it. The per-target fragments the packaging tool starts from are a different,
/// intermediate shape; `crates/bongocat-packaging` pins those against the library's
/// reader type, and `tools/tests/test_update_release_contract.py` pins the two names.
fn serve_release_manifest(
    download_url: &str,
    format: &str,
    signature: &str,
    platform_key: &str,
) -> String {
    let mut platforms = serde_json::Map::new();
    platforms.insert(
        platform_key.to_owned(),
        serde_json::json!({
            "url": download_url,
            "signature": signature,
            "format": format,
        }),
    );
    let manifest = serde_json::json!({
        "version": RELEASE_VERSION,
        "notes": "capability fixture",
        "pub_date": "2026-09-14T00:00:00Z",
        "platforms": serde_json::Value::Object(platforms),
    });

    let route = format!("/{MANIFEST_NAME}");
    let server = LocalServer::serve(vec![(
        route.clone(),
        serde_json::to_vec(&manifest).expect("serialize the manifest"),
    )]);
    format!("{}{route}", server.base_url)
}

fn updater_for(manifest_url: &str, public_key: &str, executable_path: Option<&Path>) -> Updater {
    let config = Config {
        endpoints: vec![Url::parse(manifest_url).expect("parse the manifest URL")],
        pubkey: public_key.to_owned(),
        ..Config::default()
    };

    let builder = UpdaterBuilder::new(
        Version::parse(CURRENT_VERSION).expect("parse the current version"),
        config,
    );
    match executable_path {
        Some(path) => builder.executable_path(path).build(),
        None => builder.build(),
    }
    .expect("build the updater")
}

/// A payload plus the release manifest announcing it, signed by one key pair.
struct Fixture {
    payload: PathBuf,
    public_key: String,
    signature: String,
    download_url: String,
    manifest_url: String,
}

/// Sign a bundle archive, then serve it and a manifest that announces it.
fn fixture(name: &str) -> Fixture {
    let root = scratch(name);
    let payload = root.join("BongoCat.app.tar.gz");
    write_archive(
        &payload,
        &[("BongoCat.app/Contents/MacOS/bongocat-app", "new-binary")],
    );

    let (public_key, signature) = sign_payload(&payload);
    let download_url = serve_payload(&payload);
    let manifest_url =
        serve_release_manifest(&download_url, "app", &signature, &host_platform_key());

    Fixture {
        payload,
        public_key,
        signature,
        download_url,
        manifest_url,
    }
}

/// The platform key used to look a host up must be the one the library derives.
#[test]
fn the_platform_key_matches_the_library() {
    assert_eq!(
        cargo_packager_updater::target(),
        Some(host_platform_key()),
        "the manifest key spelling must be the library's `<os>-<arch>` form"
    );
}

#[test]
fn a_signed_payload_is_offered_and_verifies() {
    let fixture = fixture("signed");
    let updater = updater_for(&fixture.manifest_url, &fixture.public_key, None);

    let update = updater
        .check()
        .expect("the manifest must parse")
        .expect("the fixture release is newer than this build");

    assert_eq!(update.version, RELEASE_VERSION);
    assert_eq!(update.download_url.as_str(), fixture.download_url);
    assert_eq!(update.format.to_string(), "app");

    let bytes = update
        .download()
        .expect("a correctly signed payload must verify");
    assert_eq!(
        bytes,
        fs::read(&fixture.payload).expect("read the payload"),
        "verification must not alter the downloaded bytes"
    );
}

/// The shared manifest has to be served under the name an update run requests.
///
/// `serve_release_manifest` derives the route from `RELEASE_MANIFEST_NAME`, so this
/// asserts the fixture really mirrors `releases/latest/download/<name>` instead of
/// quietly serving a manifest the runtime would never find.
#[test]
fn the_manifest_is_served_under_the_requested_name() {
    assert_eq!(MANIFEST_NAME, "latest.json");
    assert!(
        fixture("manifest-name")
            .manifest_url
            .ends_with("/latest.json"),
        "the fixture manifest must be reachable at the name the runtime requests"
    );
}

#[test]
fn a_payload_that_changed_after_signing_is_rejected() {
    let fixture = fixture("tampered");

    // Rewrite the payload after signing: the signature now describes other bytes.
    let mut tampered = fs::read(&fixture.payload).expect("read the payload");
    let last = tampered.len() - 1;
    tampered[last] ^= 0xff;
    fs::write(&fixture.payload, &tampered).expect("rewrite the payload");

    // Re-serve the rewritten bytes under the original manifest.
    let download_url = serve_payload(&fixture.payload);
    let manifest_url = serve_release_manifest(
        &download_url,
        "app",
        &fixture.signature,
        &host_platform_key(),
    );

    let update = updater_for(&manifest_url, &fixture.public_key, None)
        .check()
        .expect("the manifest must parse")
        .expect("the fixture release is newer than this build");

    let error = update
        .download()
        .expect_err("a payload that changed after signing must not verify");
    assert!(
        matches!(error, Error::Minisign(_)),
        "a failed signature check must be a signature error, got {error:?}"
    );
}

#[test]
fn a_payload_signed_by_another_key_is_rejected() {
    let fixture = fixture("wrong-key");
    let provisioned = generate_key(Some(String::new())).expect("generate the trusted key");

    // The manifest carries a signature made by a key the build does not know.
    let manifest_url = serve_release_manifest(
        &fixture.download_url,
        "app",
        &fixture.signature,
        &host_platform_key(),
    );

    let update = updater_for(&manifest_url, &provisioned.pk, None)
        .check()
        .expect("the manifest must parse")
        .expect("the fixture release is newer than this build");

    assert!(
        update.download().is_err(),
        "a payload signed by an unknown key must not verify"
    );
}

/// A release manifest that names other platforms than this host is a target miss.
///
/// The shared manifest is a per-platform map, so a release that omits this host's key
/// is a real failure mode — one a partially-published release can produce. `runtime.rs`
/// maps `Error::TargetNotFound` onto `UpdateErrorCode::NoMatchingAsset`, and this pins
/// the shape that produces it.
#[test]
fn a_manifest_without_this_platform_is_a_target_miss() {
    let fixture = fixture("missing-platform");

    let manifest_url = serve_release_manifest(
        &fixture.download_url,
        "app",
        &fixture.signature,
        "plan9-cris",
    );

    let error = updater_for(&manifest_url, &fixture.public_key, None)
        .check()
        .expect_err("a manifest that does not name this host must fail");
    assert!(
        matches!(error, Error::TargetNotFound(_)),
        "an absent platform entry must be a target miss, got {error:?}"
    );
}

/// An empty public key must fail verification rather than be read as "nothing to
/// verify against".
///
/// Note where the refusal lands: `check()` only parses the manifest, so an
/// unprovisioned key still produces an offer and the failure surfaces on the payload.
/// That is exactly why the runtime refuses before it builds an updater at all —
/// see `runtime::tests::a_missing_signing_key_fails_closed_before_any_request`.
#[test]
fn an_empty_public_key_cannot_verify_a_payload() {
    let fixture = fixture("empty-key");

    let update = updater_for(&fixture.manifest_url, "", None)
        .check()
        .expect("the manifest must parse")
        .expect("the fixture release is newer than this build");

    let error = update
        .download()
        .expect_err("an empty public key cannot authenticate anything");
    assert!(
        matches!(error, Error::Minisign(_)),
        "an empty key must fail decoding rather than skip verification, got {error:?}"
    );
}

/// The macOS install path replaces the whole bundle and drops what the previous one
/// carried, which is what lets a release ship changed resources.
#[cfg(target_os = "macos")]
#[test]
fn installing_replaces_the_configured_bundle() {
    let fixture = fixture("install");
    let root = fixture
        .payload
        .parent()
        .expect("the payload lives in the scratch directory")
        .to_path_buf();
    let bundle = root.join("Fake.app");
    let executable = bundle.join("Contents/MacOS/bongocat-app");
    write(&executable, "old-binary");
    write(
        &bundle.join("Contents/Resources/stale.txt"),
        "stale-resource",
    );

    let payload = root.join("replacement.app.tar.gz");
    write_archive(
        &payload,
        &[
            ("BongoCat.app/Contents/MacOS/bongocat-app", "new-binary"),
            ("BongoCat.app/Contents/Resources/models/a.moc3", "new-model"),
        ],
    );
    let (public_key, signature) = sign_payload(&payload);
    let download_url = serve_payload(&payload);
    let manifest_url =
        serve_release_manifest(&download_url, "app", &signature, &host_platform_key());

    let update = updater_for(&manifest_url, &public_key, Some(&executable))
        .check()
        .expect("the manifest must parse")
        .expect("the fixture release is newer than this build");
    update
        .download_and_install()
        .expect("a signed bundle must install");

    assert_eq!(
        read(&executable),
        "new-binary",
        "the bundle executable must be replaced"
    );
    assert_eq!(
        read(&bundle.join("Contents/Resources/models/a.moc3")),
        "new-model",
        "resources must arrive with the bundle"
    );
    assert!(
        !bundle.join("Contents/Resources/stale.txt").exists(),
        "a whole-bundle replacement must not leave files from the previous bundle"
    );
}
