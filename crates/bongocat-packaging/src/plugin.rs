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
/// The name the repository's own `plugins.json` already points its downloads at, so a
/// development build finds a freshly packed archive with nothing rewritten and nothing
/// published.
const BUILD_DIRECTORY: &str = "build";

/// The manifest inside a plugin's own directory, and the one this step rewrites.
const MANIFEST: &str = "plugin.json";

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
pub fn archive_path(plugins_workspace: &Path, id: &str) -> PathBuf {
    plugins_workspace
        .join(BUILD_DIRECTORY)
        .join(format!("{id}.zip"))
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

    let archive = archive_path(plugins_workspace, id);
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
        assert_eq!(archive, archive_path(plugins.path(), "pomodoro"));

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
            !archive_path(plugins.path(), "pomodoro").exists(),
            "and no half-written archive is left where the next run would find it"
        );
    }

    #[test]
    fn the_archive_lands_where_the_repositorys_own_catalog_looks_for_it() {
        // The development loop has no publish step, so the name here and the `path` in
        // `plugins.json` are one fact written down twice. This is the check that says so.
        let root = Path::new(env!("CARGO_MANIFEST_DIR"))
            .ancestors()
            .nth(2)
            .expect("inside a workspace");
        let catalog: serde_json::Value = serde_json::from_slice(
            &fs::read(root.join(PLUGINS_DIRECTORY).join("plugins.json"))
                .expect("the repository ships a catalog"),
        )
        .expect("valid JSON");
        let entry = catalog["plugins"]
            .as_array()
            .expect("a list")
            .iter()
            .find(|entry| entry["id"] == "pomodoro")
            .expect("the packed plugin is in it");
        for (_target, download) in entry["downloads"].as_object().expect("a map") {
            let path = download["path"]
                .as_str()
                .expect("a development download is a path on this machine");
            assert_eq!(
                path, "build/pomodoro.zip",
                "so the catalog and the packer agree on one name"
            );
        }
    }
}
