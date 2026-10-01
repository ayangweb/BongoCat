//! Packing a plugin into the archive the store installs.
//!
//! Separate from [`super`]'s product packaging on purpose, and the separation is the
//! point rather than tidiness. A plugin is built by its own workspace with its own
//! lockfile, so "adding a plugin must not grow the app" is a fact about the dependency
//! graph rather than a promise. What lives here is the other half: turning a binary
//! somebody built into a zip the store can verify and unpack, with a manifest that
//! names it correctly on the platform it was built for.
//!
//! # What this does and does not know
//!
//! It knows two things the plugin itself cannot:
//!
//! * **The executable's file name on this platform.** A plugin's own `plugin.json` names
//!   the binary without a suffix, because that is the name in its `Cargo.toml` on every
//!   platform; the `.exe` is this step's business, and it is the one place in the
//!   repository that appends one. The protocol deliberately does not: a manifest is a
//!   document, and a document that grew a platform conditional would need a platform
//!   conditional to read.
//! * **Where the built binary is.** Cargo's own layout, from the plugins workspace, which
//!   is separate from the product's so the two cannot share a target directory.
//!
//! What it does not know is anything about any plugin: no ids are listed here, no
//! settings, no panels. A plugin this file had never seen packs exactly like one it was
//! written for, and the only way it can be wrong is if the archive is not what the store
//! expects — which the store itself checks on install.

use std::collections::BTreeMap;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

/// The plugins workspace, beside the product's.
pub const PLUGINS_DIRECTORY: &str = "plugins";

/// Where a packed archive is written, relative to the plugins workspace.
///
/// The name the host's derived development catalog looks in, so a freshly packed archive is
/// found with nothing rewritten and nothing published. It is a second copy of a fact the two
/// sides cannot share — this tool and the host are different crates with different reasons to
/// exist — and `bongocat_plugin::catalog`'s test for the same name is where the two are
/// checked against each other.
pub const BUILD_DIRECTORY: &str = "build";

/// The manifest inside a plugin's own directory, and the one this step rewrites.
pub const MANIFEST: &str = "plugin.json";

/// What one packed plugin is called, as the archive's own manifest spells it.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Manifest {
    pub id: String,
    pub version: String,
    /// The executable as the plugin's own manifest names it, without a platform suffix.
    pub executable: String,
    /// The manifest as the plugin wrote it, with everything else left exactly alone.
    fields: BTreeMap<String, serde_json::Value>,
}

impl Manifest {
    /// Read and check a plugin's own manifest.
    ///
    /// The checks are the store's, and they are the store's *rules* rather than a second
    /// copy of them: the same id the directory is named for, and a manifest the protocol
    /// would accept. A plugin that fails either is a plugin the store would refuse after
    /// a download, and finding it out here names the file rather than the archive.
    pub fn read(directory: &Path) -> Result<Self, String> {
        let path = directory.join(MANIFEST);
        let bytes = fs::read(&path).map_err(|error| format!("{}: {error}", path.display()))?;
        let document: BTreeMap<String, serde_json::Value> = serde_json::from_slice(&bytes)
            .map_err(|error| format!("{}: {error}", path.display()))?;
        let text = |key: &str| {
            document
                .get(key)
                .and_then(serde_json::Value::as_str)
                .map(str::to_owned)
        };
        let id = text("id").ok_or_else(|| format!("{} names no id", path.display()))?;
        let version =
            text("version").ok_or_else(|| format!("{} names no version", path.display()))?;
        let executable =
            text("executable").ok_or_else(|| format!("{} names no executable", path.display()))?;
        if id
            != directory
                .file_name()
                .and_then(|name| name.to_str())
                .unwrap_or_default()
        {
            return Err(format!(
                "{} declares the id {id}, which is not the directory it is in",
                path.display()
            ));
        }
        if executable.contains('/') || executable.contains('\\') {
            return Err(format!(
                "{} names the executable {executable}, which is not a plain file name",
                path.display()
            ));
        }
        Ok(Self {
            id,
            version,
            executable,
            fields: document,
        })
    }

    /// This manifest, naming the executable as this platform spells it.
    ///
    /// One field rewritten and everything else passed through untouched, so a manifest
    /// field this step has never heard of still reaches the store rather than being
    /// dropped by a tool that predates it.
    pub fn for_platform(&self, suffix: &str) -> Vec<u8> {
        let mut document = self.fields.clone();
        document.insert(
            "executable".to_owned(),
            serde_json::Value::String(format!("{}{suffix}", self.executable)),
        );
        serde_json::to_vec_pretty(&document).expect("a manifest of strings and objects serializes")
    }
}

/// The file name a built executable has on `platform`.
///
/// The suffix rather than the triple, because that is the only difference between the
/// platforms a plugin is built for, and a function that returned a whole path would be
/// one more place where a Windows name could be assembled by accident.
pub fn executable_suffix(platform: &str) -> &'static str {
    if platform.contains("windows") {
        ".exe"
    } else {
        ""
    }
}

/// Where Cargo put a plugin's built binary.
///
/// The target directory is Cargo's, not this step's, and it is the *plugins* workspace's
/// — which is the whole of what keeps a plugin's build products out of the product's
/// target directory. The binary is the name the manifest declares, which is also the
/// `[[bin]]` name in the plugin's own manifest and need not be its crate name.
pub fn built_executable(plugins_workspace: &Path, binary: &str, triple: Option<&str>) -> PathBuf {
    let mut path = plugins_workspace.join("target");
    if let Some(triple) = triple {
        path = path.join(triple);
    }
    path = path.join("release");
    path.join(format!(
        "{binary}{}",
        executable_suffix(triple.unwrap_or_default())
    ))
}

/// The archive a packed plugin is written to.
///
/// One name for the host build, and a name carrying the triple for a cross build, so
/// packing for Windows cannot overwrite the archive a macOS run installs. The host's
/// own name stays bare because that is the one the development loop and the plugin
/// center look for, and adding a suffix to it would mean a second fact to keep in step.
pub fn archive_path(plugins_workspace: &Path, id: &str, triple: Option<&str>) -> PathBuf {
    let name = match triple {
        Some(triple) => format!("{id}-{triple}.zip"),
        None => format!("{id}.zip"),
    };
    plugins_workspace.join(BUILD_DIRECTORY).join(name)
}

/// Every plugin in the workspace, in id order.
///
/// A plugin is a directory that holds a `plugin.json`, and nothing else in this
/// workspace is one — so the manifest is the whole of the discovery rule and adding
/// a plugin is adding a directory, never editing a list somewhere else. The
/// directory name is the id, which is also the rule [`Manifest::read`] enforces, so
/// a directory whose manifest disagrees is found here and refused there.
pub fn plugin_ids(plugins_workspace: &Path) -> Vec<String> {
    let Ok(entries) = fs::read_dir(plugins_workspace) else {
        return Vec::new();
    };
    let mut ids: Vec<String> = entries
        .flatten()
        .filter(|entry| entry.path().is_dir())
        .filter_map(|entry| {
            let id = entry.file_name().into_string().ok()?;
            if id.starts_with('.') || id == BUILD_DIRECTORY || id == "target" {
                return None;
            }
            entry.path().join(MANIFEST).is_file().then_some(id)
        })
        .collect();
    ids.sort();
    ids
}

/// Whether the packed archive is already what this build would produce.
///
/// The archive is a function of exactly three things: the binary Cargo built, the
/// manifest the plugin wrote, and the packer itself. So an archive that is newer
/// than the other two *is* the current one, and rewriting it would produce a file
/// with the same contents at a later date.
///
/// Cargo is the answer to "did anything the plugin depends on change" — including
/// the SDK, which lives outside this workspace and which no comparison of file times
/// inside the plugins tree would ever notice. This function does not try to know
/// what went into the binary; it asks the thing that knows, and reads the answer off
/// the binary's own modification time.
pub fn is_packed_up_to_date(plugins_workspace: &Path, id: &str, triple: Option<&str>) -> bool {
    let Ok(manifest) = Manifest::read(&plugins_workspace.join(id)) else {
        return false;
    };
    let archive = archive_path(plugins_workspace, id, triple);
    let Some(packed_at) = modified_at(&archive) else {
        return false;
    };
    let sources = [
        built_executable(plugins_workspace, &manifest.executable, triple),
        plugins_workspace.join(id).join(MANIFEST),
    ];
    sources
        .iter()
        .all(|path| modified_at(path).is_some_and(|changed| changed <= packed_at))
}

/// When a file was last written, as a comparable instant.
///
/// A file this cannot read is reported as absent rather than as very old, because
/// the caller's question is "is the archive newer than its inputs" and a missing
/// input is not an input that can be older.
fn modified_at(path: &Path) -> Option<std::time::SystemTime> {
    fs::metadata(path).ok()?.modified().ok()
}

/// Pack one plugin into the archive the store installs.
///
/// Writes the manifest beside the binary in a staging directory, then zips the two. The
/// staging directory is a temporary one that is removed afterwards rather than a
/// directory under version control, so what an author edits in the tree is always the
/// binary Cargo wrote and the manifest they wrote — never a generated copy of either.
pub fn pack(plugins_workspace: &Path, id: &str, triple: Option<&str>) -> Result<PathBuf, String> {
    let directory = plugins_workspace.join(id);
    let manifest = Manifest::read(&directory)?;
    let executable = built_executable(plugins_workspace, &manifest.executable, triple);
    if !executable.is_file() {
        return Err(format!(
            "{} is not there, so there is nothing to pack; build the plugin first",
            executable.display()
        ));
    }

    let staging = tempfile::tempdir().map_err(|error| error.to_string())?;
    let binary_name = format!(
        "{}{}",
        manifest.executable,
        executable_suffix(triple.unwrap_or_default())
    );
    fs::write(
        staging.path().join(MANIFEST),
        manifest.for_platform(executable_suffix(triple.unwrap_or_default())),
    )
    .map_err(|error| error.to_string())?;
    fs::copy(&executable, staging.path().join(&binary_name))
        .map_err(|error| format!("{}: {error}", executable.display()))?;

    let archive = archive_path(plugins_workspace, id, triple);
    if let Some(parent) = archive.parent() {
        fs::create_dir_all(parent).map_err(|error| error.to_string())?;
    }
    let file = fs::File::create(&archive).map_err(|error| error.to_string())?;
    let mut writer = zip::ZipWriter::new(std::io::BufWriter::new(file));
    let options: zip::write::FileOptions<()> =
        zip::write::FileOptions::default().compression_method(zip::CompressionMethod::Deflated);
    writer
        .start_file(MANIFEST, options)
        .map_err(|error| error.to_string())?;
    writer
        .write_all(&manifest.for_platform(executable_suffix(triple.unwrap_or_default())))
        .map_err(|error| error.to_string())?;
    // The mode is set here as well as in the store, and both are worth having: this one
    // makes the archive correct for anything that unpacks it, and the store's makes an
    // archive that was not built by this tool runnable anyway.
    #[cfg(unix)]
    let options = options.unix_permissions(0o755);
    writer
        .start_file(&binary_name, options)
        .map_err(|error| error.to_string())?;
    let bytes = fs::read(&executable).map_err(|error| error.to_string())?;
    writer
        .write_all(&bytes)
        .map_err(|error| error.to_string())?;
    writer.finish().map_err(|error| error.to_string())?;
    Ok(archive)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A plugin directory named for its own id, which is the rule `Manifest::read`
    /// checks and the shape every plugin in the workspace has.
    fn plugin_directory(parent: &Path, id: &str) -> PathBuf {
        let directory = parent.join(id);
        fs::create_dir_all(&directory).expect("a plugin directory");
        directory
    }

    fn written(directory: &Path, manifest: &str) {
        fs::write(directory.join(MANIFEST), manifest).expect("writes a manifest");
    }

    /// The manifest document inside a packed archive, read the way the store reads it.
    fn packed_manifest(archive: &Path) -> serde_json::Value {
        let bytes = fs::read(archive).expect("reads the archive");
        let mut zip = zip::ZipArchive::new(std::io::Cursor::new(bytes)).expect("a zip");
        let mut manifest = String::new();
        {
            use std::io::Read;
            zip.by_name(MANIFEST)
                .expect("the manifest is a member")
                .read_to_string(&mut manifest)
                .expect("the manifest is text");
        }
        serde_json::from_str(&manifest).expect("valid JSON")
    }

    /// Give a file a modification time strictly after everything already written.
    ///
    /// A comparison of file times cannot tell "edited" from "not edited" without a clock
    /// of its own, and the alternative — sleeping — makes a test suite that takes
    /// seconds to say something a timestamp can say in microseconds. Two seconds is more
    /// than any filesystem's timestamp resolution needs and less than any timer a test
    /// would wait out.
    fn later_than(path: &Path) {
        let when = std::time::SystemTime::now() + std::time::Duration::from_secs(2);
        let file = fs::File::options()
            .write(true)
            .open(path)
            .expect("a file to restamp");
        file.set_modified(when).expect("restamps a file");
    }

    #[test]
    fn a_manifest_is_read_as_the_fields_the_store_needs() {
        let parent = tempfile::tempdir().expect("a temporary directory");
        let directory = plugin_directory(parent.path(), "pomodoro");
        written(
            &directory,
            r#"{"schema_version":1,"api_version":1,"id":"pomodoro","name":"Pomodoro",
                "version":"1.0.0","executable":"pomodoro","icon":{"emoji":"🍅"}}"#,
        );
        let manifest = Manifest::read(&directory).expect("a manifest the store accepts");
        assert_eq!(manifest.id, "pomodoro");
        assert_eq!(manifest.version, "1.0.0");
        assert_eq!(manifest.executable, "pomodoro");
    }

    #[test]
    fn a_manifest_whose_id_is_not_its_directory_is_refused_before_anything_is_packed() {
        // The store compares these two after a download, and refuses. Finding it out here
        // names the file rather than the archive.
        let parent = tempfile::tempdir().expect("a temporary directory");
        let root = plugin_directory(parent.path(), "pomodoro");
        written(
            &root,
            r#"{"schema_version":1,"api_version":1,"id":"key-stats","name":"Key stats",
                "version":"1.0.0","executable":"key-stats"}"#,
        );
        let error = Manifest::read(&root).expect_err("a directory and an id must agree");
        assert!(error.contains("key-stats"), "{error}");
    }

    #[test]
    fn a_manifest_naming_a_path_rather_than_a_file_is_refused() {
        let parent = tempfile::tempdir().expect("a temporary directory");
        let root = plugin_directory(parent.path(), "pomodoro");
        written(
            &root,
            r#"{"schema_version":1,"api_version":1,"id":"pomodoro","name":"Pomodoro",
                "version":"1.0.0","executable":"../../bin/sh"}"#,
        );
        let error = Manifest::read(&root).expect_err("an executable is a file name, not a path");
        assert!(error.contains("plain file name"), "{error}");
    }

    #[test]
    fn the_only_field_this_step_rewrites_is_the_executable() {
        // Everything else is passed through, so a manifest field this tool predates still
        // reaches the store rather than being dropped on the way.
        let parent = tempfile::tempdir().expect("a temporary directory");
        let directory = plugin_directory(parent.path(), "pomodoro");
        written(
            &directory,
            r#"{"schema_version":1,"api_version":1,"id":"pomodoro","name":"Pomodoro",
                "version":"1.0.0","executable":"pomodoro","min_app_version":"2.0.0",
                "icon":{"emoji":"🍅"}}"#,
        );
        let manifest = Manifest::read(&directory).expect("reads");
        let packed: serde_json::Value =
            serde_json::from_slice(&manifest.for_platform(".exe")).expect("valid JSON");
        assert_eq!(packed["executable"], "pomodoro.exe");
        assert_eq!(packed["min_app_version"], "2.0.0", "passed through");
        assert_eq!(packed["icon"]["emoji"], "🍅", "passed through");
        assert_eq!(packed["id"], "pomodoro");
    }

    #[test]
    fn the_executable_suffix_is_the_only_difference_between_the_platforms() {
        assert_eq!(executable_suffix("x86_64-pc-windows-msvc"), ".exe");
        assert_eq!(executable_suffix("aarch64-apple-darwin"), "");
        assert_eq!(
            executable_suffix(""),
            "",
            "a host build has no triple in its path"
        );
    }

    /// A workspace holding one plugin, its manifest and a fake Cargo output.
    ///
    /// The same shape `pack` reads, so a test that builds one of these is testing the
    /// real layout rather than a fixture shaped to pass.
    fn built_workspace(id: &str, manifest: &str) -> (tempfile::TempDir, PathBuf) {
        let plugins = tempfile::tempdir().expect("a temporary directory");
        let source = plugin_directory(plugins.path(), id);
        written(&source, manifest);
        let target = plugins.path().join("target/release");
        fs::create_dir_all(&target).expect("a cargo target directory");
        let binary = target.join(id);
        fs::write(&binary, b"not really an executable").expect("a built binary");
        (plugins, binary)
    }

    const ONE_PLUGIN: &str = r#"{"schema_version":1,"api_version":1,"id":"pomodoro","name":"Pomodoro",
        "version":"1.0.0","executable":"pomodoro"}"#;

    #[test]
    fn a_plugin_is_a_directory_with_a_manifest_and_nothing_else_is_one() {
        // The whole of "adding a plugin is adding a directory". A list of ids somewhere
        // else is a second place to forget one, and the failure it produces is a plugin
        // that exists and cannot be installed.
        let plugins = tempfile::tempdir().expect("a temporary directory");
        assert!(plugin_ids(plugins.path()).is_empty());

        plugin_directory(plugins.path(), "pomodoro");
        plugin_directory(plugins.path(), "typing-sound");
        // No manifest, so not a plugin: a scratch directory beside them is not a
        // candidate the tool would try to build.
        fs::create_dir_all(plugins.path().join("scratch")).expect("a directory");
        // Cargo's own output and this tool's archives, which are directories too and are
        // emphatically not plugins.
        fs::create_dir_all(plugins.path().join("target/release")).expect("a target directory");
        fs::create_dir_all(plugins.path().join(BUILD_DIRECTORY)).expect("a build directory");
        assert!(plugin_ids(plugins.path()).is_empty());

        written(&plugins.path().join("pomodoro"), ONE_PLUGIN);
        written(
            &plugins.path().join("typing-sound"),
            &ONE_PLUGIN.replace("pomodoro", "typing-sound"),
        );
        assert_eq!(
            plugin_ids(plugins.path()),
            ["pomodoro", "typing-sound"],
            "in id order, so a pack run does not depend on directory listing order"
        );
    }

    #[test]
    fn a_workspace_that_is_not_there_yields_no_plugins_rather_than_a_failure() {
        let missing = Path::new(env!("CARGO_MANIFEST_DIR")).join("no-such-plugins-workspace");
        assert!(plugin_ids(&missing).is_empty());
    }

    #[test]
    fn a_freshly_packed_plugin_is_already_current() {
        let (plugins, binary) = built_workspace("pomodoro", ONE_PLUGIN);
        assert!(
            !is_packed_up_to_date(plugins.path(), "pomodoro", None),
            "there is no archive yet, so there is nothing to keep"
        );
        pack(plugins.path(), "pomodoro", None).expect("packs");
        assert!(
            is_packed_up_to_date(plugins.path(), "pomodoro", None),
            "the archive is newer than the binary and the manifest it was made from"
        );
        // Touching the binary is a rebuild Cargo has already done, and it is what makes
        // the archive stale. Comparing only the manifest would have missed it, and the
        // symptom would be an editor that saves, launches, and shows the old binary.
        later_than(&binary);
        assert!(
            !is_packed_up_to_date(plugins.path(), "pomodoro", None),
            "a rebuilt binary is a stale archive, even though the manifest never moved"
        );
    }

    #[test]
    fn an_edited_manifest_is_a_stale_archive_even_with_an_untouched_binary() {
        // The manifest is packed verbatim, so an edit to it has to repack: an archive
        // holding yesterday's description is a card that changes text behind the user's
        // back.
        let (plugins, _) = built_workspace("pomodoro", ONE_PLUGIN);
        pack(plugins.path(), "pomodoro", None).expect("packs");
        assert!(is_packed_up_to_date(plugins.path(), "pomodoro", None));

        let source = plugins.path().join("pomodoro");
        written(&source, &ONE_PLUGIN.replace("1.0.0", "1.1.0"));
        later_than(&source.join(MANIFEST));
        assert!(!is_packed_up_to_date(plugins.path(), "pomodoro", None));
    }

    #[test]
    fn a_manifest_that_will_not_read_is_stale_rather_than_up_to_date() {
        // An unparseable manifest is a plugin the packer cannot write, so the answer to
        // "can this be skipped" has to be no. Answering yes would leave a broken
        // manifest looking like a finished build.
        let (plugins, _) = built_workspace("pomodoro", ONE_PLUGIN);
        pack(plugins.path(), "pomodoro", None).expect("packs");
        written(&plugins.path().join("pomodoro"), "{ not json");
        assert!(!is_packed_up_to_date(plugins.path(), "pomodoro", None));
    }

    #[test]
    fn a_cross_build_gets_its_own_archive_rather_than_overwriting_the_hosts() {
        // A developer who packs for Windows and then runs the product on macOS must not
        // find a Windows binary in the archive the store installs. One archive per
        // platform is what makes the up-to-date comparison mean anything, because the
        // host's own archive is then evidence about the host and only the host.
        let (plugins, _) = built_workspace("pomodoro", ONE_PLUGIN);
        let windows = plugins.path().join("target/x86_64-pc-windows-msvc/release");
        fs::create_dir_all(&windows).expect("a cross target directory");
        fs::write(windows.join("pomodoro.exe"), b"a Windows binary").expect("a binary");

        let host = pack(plugins.path(), "pomodoro", None).expect("packs for the host");
        let cross = pack(plugins.path(), "pomodoro", Some("x86_64-pc-windows-msvc"))
            .expect("packs for Windows");
        assert_ne!(host, cross, "two platforms, two archives");

        assert!(is_packed_up_to_date(plugins.path(), "pomodoro", None));
        assert!(is_packed_up_to_date(
            plugins.path(),
            "pomodoro",
            Some("x86_64-pc-windows-msvc")
        ));
        assert_eq!(
            packed_manifest(&cross)["executable"],
            "pomodoro.exe",
            "and the Windows archive names the executable the way Windows does"
        );
        assert_eq!(
            packed_manifest(&host)["executable"],
            "pomodoro",
            "while the host's names it the host's way"
        );
    }

    #[test]
    fn a_packed_archive_holds_exactly_the_manifest_and_the_binary() {
        // The bound the store enforces on unpack — a member name is a plain relative path
        // and the manifest's id is the directory's — checked here against the same zip
        // reader, so a packing mistake is a packing test and not an install failure.
        let plugins = tempfile::tempdir().expect("a temporary directory");
        let source = plugin_directory(plugins.path(), "pomodoro");
        written(
            &source,
            r#"{"schema_version":1,"api_version":1,"id":"pomodoro","name":"Pomodoro",
                "version":"1.0.0","executable":"pomodoro","icon":{"emoji":"🍅"}}"#,
        );
        let target = plugins.path().join("target/release");
        fs::create_dir_all(&target).expect("a cargo target directory");
        let binary = target.join("pomodoro");
        fs::write(&binary, b"not really an executable").expect("a built binary");

        let archive = pack(plugins.path(), "pomodoro", None).expect("packs");
        assert_eq!(archive, archive_path(plugins.path(), "pomodoro", None));

        let bytes = fs::read(&archive).expect("reads the archive");
        let mut zip = zip::ZipArchive::new(std::io::Cursor::new(bytes)).expect("a zip");
        let mut names: Vec<String> = (0..zip.len())
            .map(|index| zip.by_index(index).expect("a member").name().to_owned())
            .collect();
        names.sort();
        assert_eq!(names, ["plugin.json", "pomodoro"]);

        let mut manifest = String::new();
        {
            use std::io::Read;
            zip.by_name("plugin.json")
                .expect("the manifest is a member")
                .read_to_string(&mut manifest)
                .expect("the manifest is text");
        }
        let parsed: serde_json::Value = serde_json::from_str(&manifest).expect("valid JSON");
        assert_eq!(parsed["id"], "pomodoro");
        assert_eq!(
            parsed["executable"], "pomodoro",
            "no suffix on this platform"
        );
    }

    #[test]
    fn a_plugin_that_was_never_built_is_a_named_failure_rather_than_an_empty_archive() {
        let plugins = tempfile::tempdir().expect("a temporary directory");
        let source = plugin_directory(plugins.path(), "pomodoro");
        written(
            &source,
            r#"{"schema_version":1,"api_version":1,"id":"pomodoro","name":"Pomodoro",
                "version":"1.0.0","executable":"pomodoro"}"#,
        );
        let error = pack(plugins.path(), "pomodoro", None).expect_err("there is nothing to pack");
        assert!(error.contains("build the plugin first"), "{error}");
        assert!(
            !archive_path(plugins.path(), "pomodoro", None).exists(),
            "and no half-written archive is left where the next run would find it"
        );
    }
    /// The repository's own `plugins/` directory.
    fn repository_plugins() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .ancestors()
            .nth(2)
            .expect("inside a workspace")
            .join(PLUGINS_DIRECTORY)
    }

    /// Every plugin the repository ships, as `(id, manifest)` pairs.
    fn repository_plugins_with_manifests() -> Vec<(String, serde_json::Value)> {
        let plugins = repository_plugins();
        let ids = plugin_ids(&plugins);
        assert!(!ids.is_empty(), "the repository ships plugins");
        ids.into_iter()
            .map(|id| {
                let manifest: serde_json::Value = serde_json::from_slice(
                    &fs::read(plugins.join(&id).join(MANIFEST))
                        .expect("the plugin ships its own manifest"),
                )
                .expect("valid JSON");
                (id, manifest)
            })
            .collect()
    }

    /// The language a plugin's `default` is the copy for.
    ///
    /// Written here rather than read out of `bongocat-i18n` because that crate is the
    /// application's and this one is a build tool: a plugin's own table has a default
    /// rather than an entry for every language, and the language that default is *for*
    /// is the one the host resolves first. It is also the check that would notice the
    /// product changing its default language, because a plugin whose default were still
    /// English would then be a plugin with no copy in the reader's language at all.
    const DEFAULT_LOCALE: &str = "en-US";

    /// Every language the application's own catalogs ship, except the default.
    ///
    /// Read from `crates/bongocat-i18n/locales/` rather than written out here, because a
    /// list in a test is a list that goes stale the day a language is added: the test
    /// would keep passing on the languages it already knew about while the product grew
    /// a seventh and every plugin silently stopped covering it.
    fn translated_locales() -> Vec<String> {
        let root = repository_plugins()
            .parent()
            .expect("the repository root is above plugins/")
            .join("crates/bongocat-i18n/locales");
        let mut locales: Vec<String> = fs::read_dir(&root)
            .expect("the application ships locale catalogs")
            .flatten()
            .map(|entry| entry.path())
            .filter(|path| {
                path.extension()
                    .is_some_and(|extension| extension == "json")
            })
            .filter_map(|path| {
                path.file_stem()
                    .and_then(|stem| stem.to_str())
                    .map(str::to_owned)
            })
            .filter(|locale| locale != DEFAULT_LOCALE)
            .collect();
        locales.sort();
        assert!(
            locales.len() >= 2,
            "the product ships more than one language beyond the default"
        );
        locales
    }

    #[test]
    fn every_plugin_ships_a_manifest_that_names_its_own_directory() {
        // The one rule a plugin directory has to follow, checked over the repository's
        // own rather than over a fixture: it is what lets `just plugins` find a plugin by
        // looking at the tree, and what stops two plugins from disagreeing about an id.
        for (id, manifest) in repository_plugins_with_manifests() {
            assert_eq!(
                manifest["id"].as_str(),
                Some(id.as_str()),
                "{id}: the manifest's id and its directory name have to agree"
            );
            assert!(
                manifest["executable"].is_string(),
                "{id}: a manifest names the file to execute"
            );
        }
    }

    #[test]
    fn every_plugin_speaks_every_language_the_application_ships() {
        // The bug this check exists for: a plugin's own copy carried a default and two
        // Chinese entries, so a user on the other five languages read a plugin center
        // that was partly translated — or English, with a table around it. Nothing else
        // in the repository would notice, because the application's own catalogs are
        // complete and the plugin's are a different mechanism entirely.
        let locales = translated_locales();
        for (id, manifest) in repository_plugins_with_manifests() {
            for field in ["name", "description"] {
                assert_localized(&id, &manifest[field], field, &locales);
            }
        }
    }

    /// One localized field carries a default and an entry for every other language.
    ///
    /// The two halves check different things, and neither substitutes for the other. An
    /// entry per language is what stops a field from being silently skipped when a new
    /// one ships; and a table that is not a verbatim copy of the default is what stops
    /// "translated" from meaning "the same English string written seven times" — which is
    /// what a mechanical pass over the keys produces, and which no reader can tell from
    /// a real translation by counting entries.
    ///
    /// Individual languages are allowed to equal the default, because plenty of words are
    /// the same in two languages: a product's own name, and most of Simplified and
    /// Traditional Chinese. Refusing that would make the check wrong often enough that
    /// it would be turned off.
    fn assert_localized(id: &str, field: &serde_json::Value, name: &str, locales: &[String]) {
        let default = field["default"]
            .as_str()
            .unwrap_or_else(|| panic!("{id}: {name} has no default to fall back to"));
        assert!(
            !default.trim().is_empty(),
            "{id}: {name}'s default is empty"
        );
        let by_locale = field["by_locale"]
            .as_object()
            .unwrap_or_else(|| panic!("{id}: {name} carries no translations"));
        for locale in locales {
            let translation = by_locale
                .get(locale)
                .and_then(serde_json::Value::as_str)
                .unwrap_or_else(|| panic!("{id}: {name} has no {locale} copy"));
            assert!(
                !translation.trim().is_empty(),
                "{id}: {name} in {locale} is empty"
            );
        }
        assert!(
            locales.iter().any(
                |locale| by_locale.get(locale).and_then(serde_json::Value::as_str) != Some(default)
            ),
            "{id}: {name} is the {DEFAULT_LOCALE} copy repeated under every language, which is \
             a string that happens to be in a table rather than a translation"
        );
    }

    #[test]
    fn every_plugin_ships_an_icon_because_a_card_with_none_shows_a_letter() {
        for (id, manifest) in repository_plugins_with_manifests() {
            assert!(
                manifest["icon"]["emoji"].is_string() || manifest["icon"]["image"].is_string(),
                "{id}: a card with no icon is a letter, and every plugin the repository ships has \
                 one"
            );
        }
    }

    #[test]
    fn every_plugin_ships_its_own_copy_in_every_language_the_application_ships() {
        // A plugin's own words — the labels on its settings, the names of its choices,
        // the text on its panel — are the plugin's, and they live in the plugin's own
        // manifest rather than in a table the application maintains on its behalf. A
        // plugin that shipped only some of the application's languages reads as a plugin
        // with a translation rather than as a plugin with all of its strings, and
        // nothing else in the repository would notice.
        let locales = translated_locales();
        for (id, manifest) in repository_plugins_with_manifests() {
            let copy = manifest["copy"]
                .as_object()
                .unwrap_or_else(|| panic!("{id}: the manifest carries no copy"));
            assert!(!copy.is_empty(), "{id}: the copy is empty");
            for (key, text) in copy {
                assert_localized(&id, text, key, &locales);
            }
        }
    }
}
