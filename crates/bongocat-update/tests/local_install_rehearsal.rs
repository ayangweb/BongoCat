//! Local rehearsal of the full install pipeline, without GitHub and without a signing key.
//!
//! `github::Update` cannot be pointed at a local endpoint, but `backends::custom` (always available,
//! not feature-gated) takes any [`ReleaseSource`]. That is enough to drive the real
//! download -> verify -> extract -> install chain against an archive this test builds, using the
//! same configuration constants the production updater uses.
//!
//! This is the closest thing to an end-to-end update that can run without a published release. It
//! covers what the archive-layout and move-primitive tests cover only in pieces: asset selection by
//! target triple, the download, extension-based archive detection, extraction of the configured
//! path, and the install itself.
//!
//! It deliberately does **not** cover signature verification (no key is provisioned), the GitHub
//! backend, or restarting the process.

use std::fs;
use std::io::{Read, Write};
use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::thread;

use self_update::backends::custom;
use self_update::{Release, ReleaseAsset, ReleaseSource};
use zip::write::SimpleFileOptions;
use zip::{CompressionMethod, ZipWriter};

/// A fixed triple so the rehearsal runs on every CI platform, including the Linux runner that is
/// not one of the four shipped targets. The target-to-policy coupling is pinned separately by
/// `tools/tests/test_update_release_contract.py`.
const TEST_TARGET: &str = "x86_64-unknown-linux-gnu";
const RELEASE_VERSION: &str = "9.9.9";
const CURRENT_VERSION: &str = env!("CARGO_PKG_VERSION");

/// Serves one fixed body over plain HTTP on an ephemeral loopback port.
///
/// The library's `https_only` knob does not exist in 1.3.0, so plain HTTP is accepted; the thread is
/// left detached and dies with the test process.
struct LocalServer {
    base_url: String,
}

impl LocalServer {
    fn serve(body: Vec<u8>) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind a loopback port");
        let port = listener
            .local_addr()
            .expect("read the bound address")
            .port();

        thread::spawn(move || {
            for stream in listener.incoming() {
                let Ok(mut stream) = stream else { continue };
                let mut request = [0u8; 2048];
                let _ = stream.read(&mut request);
                let header = format!(
                    "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nContent-Type: application/octet-stream\r\nConnection: close\r\n\r\n",
                    body.len()
                );
                let _ = stream.write_all(header.as_bytes());
                let _ = stream.write_all(&body);
                let _ = stream.flush();
            }
        });

        Self {
            base_url: format!("http://127.0.0.1:{port}"),
        }
    }
}

struct LocalRelease {
    version: String,
    asset_name: String,
    asset_url: String,
}

impl ReleaseSource for LocalRelease {
    fn get_releases(&self) -> self_update::Result<Vec<Release>> {
        let release = Release::builder()
            .version(self.version.clone())
            .asset(ReleaseAsset::new(
                self.asset_name.clone(),
                self.asset_url.clone(),
            ))
            .build()?;
        Ok(vec![release])
    }
}

fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("bongocat-install-rehearsal-{name}"));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).expect("scratch directory");
    dir
}

fn write(path: &Path, contents: &str) {
    fs::create_dir_all(path.parent().expect("parent directory")).expect("parent directory");
    fs::write(path, contents).expect("write file");
}

fn read(path: &Path) -> String {
    fs::read_to_string(path).expect("read file")
}

/// Build the archive, serve it, and hand back the release source that points at it.
fn publish(root: &Path, entries: &[(&str, &str)]) -> LocalRelease {
    let asset_name = format!("BongoCat-{RELEASE_VERSION}-{TEST_TARGET}.zip");
    let archive = root.join(&asset_name);

    let file = fs::File::create(&archive).expect("create archive");
    let mut writer = ZipWriter::new(file);
    let options = SimpleFileOptions::default().compression_method(CompressionMethod::Stored);
    for (name, contents) in entries {
        writer.start_file(*name, options).expect("start entry");
        writer
            .write_all(contents.as_bytes())
            .expect("write entry contents");
    }
    writer.finish().expect("finish archive");

    let body = fs::read(&archive).expect("read archive bytes");
    let server = LocalServer::serve(body);

    LocalRelease {
        version: RELEASE_VERSION.to_owned(),
        asset_name: asset_name.clone(),
        asset_url: format!("{}/{asset_name}", server.base_url),
    }
}

#[test]
fn a_local_release_replaces_the_configured_executable() {
    let root = scratch("single");
    let executable = format!(
        "{}{}",
        bongocat_update::RELEASE_BINARY_NAME,
        std::env::consts::EXE_SUFFIX
    );
    let installed = root.join("installed").join(&executable);
    write(&installed, "old-binary");

    let release = publish(&root, &[(&executable, "new-binary")]);

    let status = custom::Update::configure()
        .source(release)
        .bin_name(bongocat_update::RELEASE_BINARY_NAME)
        .bin_install_path(&installed)
        .target(TEST_TARGET)
        .current_version(CURRENT_VERSION)
        .no_confirm(true)
        .show_output(false)
        .show_download_progress(false)
        .build()
        .expect("build the updater")
        .update()
        .expect("run the update");

    assert!(status.is_updated(), "the release must be installed");
    assert_eq!(status.version(), RELEASE_VERSION);
    assert_eq!(
        read(&installed),
        "new-binary",
        "the configured install path must hold the released executable"
    );
}

#[test]
fn a_local_bundle_release_swaps_the_whole_app_directory() {
    let root = scratch("bundle");
    let bundle = root
        .join("installed")
        .join(bongocat_update::RELEASE_BUNDLE_NAME);
    let bundled_executable = format!(
        "{}/Contents/MacOS/{}{}",
        bongocat_update::RELEASE_BUNDLE_NAME,
        bongocat_update::RELEASE_BINARY_NAME,
        std::env::consts::EXE_SUFFIX
    );
    write(
        &bundle.join("Contents/MacOS").join(format!(
            "{}{}",
            bongocat_update::RELEASE_BINARY_NAME,
            std::env::consts::EXE_SUFFIX
        )),
        "old-binary",
    );
    write(
        &bundle.join("Contents/Resources/stale.txt"),
        "stale-resource",
    );

    let release = publish(
        &root,
        &[
            (&bundled_executable, "new-binary"),
            (
                &format!(
                    "{}/Contents/Resources/models/a.moc3",
                    bongocat_update::RELEASE_BUNDLE_NAME
                ),
                "new-model",
            ),
        ],
    );

    custom::Update::configure()
        .source(release)
        .bin_name(bongocat_update::RELEASE_BINARY_NAME)
        .bundle_path_in_archive(bongocat_update::RELEASE_BUNDLE_NAME)
        .bundle_install_path(&bundle)
        .target(TEST_TARGET)
        .current_version(CURRENT_VERSION)
        .no_confirm(true)
        .show_output(false)
        .show_download_progress(false)
        .build()
        .expect("build the updater")
        .update()
        .expect("run the update");

    assert_eq!(
        read(&bundle.join("Contents/MacOS").join(format!(
            "{}{}",
            bongocat_update::RELEASE_BINARY_NAME,
            std::env::consts::EXE_SUFFIX
        ))),
        "new-binary",
        "bundle mode must replace the whole tree, not just the executable"
    );
    assert_eq!(
        read(&bundle.join("Contents/Resources/models/a.moc3")),
        "new-model",
        "resources must arrive with the bundle"
    );
    assert!(
        !bundle.join("Contents/Resources/stale.txt").exists(),
        "a whole-tree swap must not leave files from the previous bundle"
    );
}
