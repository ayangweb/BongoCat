#![forbid(unsafe_code)]

use sha2::{Digest, Sha256};
use std::env;
use std::error::Error;
use std::fmt::{self, Write as _};
use std::fs;
use std::io::Write as _;
use std::path::{Path, PathBuf};

const SDK_RELEASE: &str = "5-r.5";
const CORE_VERSION: &str = "06.00.0001";
const BINDGEN_VERSION: &str = "0.72.1";
const RUST_TARGET: &str = "1.85";
const CONFIG_REVISION: &str = "cubism-core-r5-v1";
const HEADER_NAME: &str = "Live2DCubismCore.h";
const SYNTHETIC_HEADER: &str = "fixtures/Live2DCubismCore.synthetic.h";
const EXPECTED_DIRECTORY: &str = "fixtures/expected";

const REQUIRED_SYMBOLS: &[&str] = &[
    "csmGetVersion",
    "csmGetLatestMocVersion",
    "csmGetMocVersion",
    "csmHasMocConsistency",
    "csmReviveMocInPlace",
    "csmGetSizeofModel",
    "csmInitializeModelInPlace",
    "csmUpdateModel",
    "csmGetRenderOrders",
    "csmReadCanvasInfo",
    "csmGetParameterCount",
    "csmGetPartCount",
    "csmGetPartOffscreenIndices",
    "csmGetDrawableCount",
    "csmGetDrawableBlendModes",
    "csmGetDrawableVertexPositions",
    "csmGetDrawableIndices",
    "csmResetDrawableDynamicFlags",
    "csmGetOffscreenCount",
    "csmGetOffscreenBlendModes",
    "csmGetOffscreenOpacities",
    "csmGetOffscreenOwnerIndices",
    "csmGetOffscreenMultiplyColors",
    "csmGetOffscreenScreenColors",
    "csmGetOffscreenMaskCounts",
    "csmGetOffscreenMasks",
    "csmGetOffscreenConstantFlags",
];

const CONFIG_DESCRIPTION: &str = concat!(
    "revision=cubism-core-r5-v1\n",
    "bindgen=0.72.1\n",
    "rust_target=1.85\n",
    "rust_edition=2024\n",
    "allowlist_function=^csm[A-Za-z0-9_]*$\n",
    "allowlist_type=^csm[A-Za-z0-9_]*$\n",
    "allowlist_var=^csm[A-Za-z0-9_]*$\n",
    "allowlist_recursively=false\n",
    "derive_debug=false\n",
    "generate_comments=false\n",
    "layout_tests=false\n",
    "formatter=prettyplease\n",
    "merge_extern_blocks=true\n",
    "sort_semantically=true\n",
    "use_core=true\n",
);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct Target {
    rust: &'static str,
    clang: &'static str,
}

const TARGETS: &[Target] = &[
    Target {
        rust: "x86_64-pc-windows-msvc",
        clang: "x86_64-pc-windows-msvc",
    },
    Target {
        rust: "aarch64-apple-darwin",
        clang: "arm64-apple-darwin",
    },
    Target {
        rust: "x86_64-apple-darwin",
        clang: "x86_64-apple-darwin",
    },
];

#[derive(Debug)]
struct ToolError(String);

impl fmt::Display for ToolError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl Error for ToolError {}

type Result<T> = std::result::Result<T, Box<dyn Error>>;

enum Command {
    CheckFixtures,
    RefreshFixtures,
    Generate {
        header: PathBuf,
        expected_header_sha256: String,
        target: Target,
        output_directory: PathBuf,
    },
}

fn main() {
    if let Err(error) = run(env::args_os().skip(1).collect()) {
        eprintln!("Cubism binding generation failed: {error}");
        std::process::exit(1);
    }
}

fn run(arguments: Vec<std::ffi::OsString>) -> Result<()> {
    match parse_command(arguments)? {
        Command::CheckFixtures => check_fixtures(),
        Command::RefreshFixtures => refresh_fixtures(),
        Command::Generate {
            header,
            expected_header_sha256,
            target,
            output_directory,
        } => generate_real_bindings(&header, &expected_header_sha256, target, &output_directory),
    }
}

fn parse_command(arguments: Vec<std::ffi::OsString>) -> Result<Command> {
    let mut arguments = arguments.into_iter();
    let Some(command) = arguments.next() else {
        return Err(tool_error(usage()));
    };
    let command = command
        .into_string()
        .map_err(|_| tool_error("command must be valid UTF-8"))?;

    match command.as_str() {
        "check-fixtures" => {
            reject_trailing(arguments)?;
            Ok(Command::CheckFixtures)
        }
        "refresh-fixtures" => {
            reject_trailing(arguments)?;
            Ok(Command::RefreshFixtures)
        }
        "generate" => parse_generate(arguments.collect()),
        "--help" | "-h" | "help" => Err(tool_error(usage())),
        other => Err(tool_error(format!(
            "unknown command {other:?}\n\n{}",
            usage()
        ))),
    }
}

fn parse_generate(arguments: Vec<std::ffi::OsString>) -> Result<Command> {
    let mut header = None;
    let mut expected_header_sha256 = None;
    let mut target = None;
    let mut output_directory = None;
    let mut arguments = arguments.into_iter();

    while let Some(flag) = arguments.next() {
        let flag = flag
            .into_string()
            .map_err(|_| tool_error("argument name must be valid UTF-8"))?;
        let value = arguments
            .next()
            .ok_or_else(|| tool_error(format!("missing value for {flag}")))?;
        match flag.as_str() {
            "--header" => set_once(&mut header, PathBuf::from(value), "--header")?,
            "--expected-header-sha256" => {
                let value = value
                    .into_string()
                    .map_err(|_| tool_error("--expected-header-sha256 must be valid UTF-8"))?;
                set_once(
                    &mut expected_header_sha256,
                    value.to_ascii_lowercase(),
                    "--expected-header-sha256",
                )?;
            }
            "--target" => {
                let value = value
                    .into_string()
                    .map_err(|_| tool_error("--target must be valid UTF-8"))?;
                set_once(&mut target, find_target(&value)?, "--target")?;
            }
            "--output-directory" => set_once(
                &mut output_directory,
                PathBuf::from(value),
                "--output-directory",
            )?,
            other => return Err(tool_error(format!("unknown generate option {other:?}"))),
        }
    }

    let expected_header_sha256 =
        expected_header_sha256.ok_or_else(|| tool_error("missing --expected-header-sha256"))?;
    validate_sha256(&expected_header_sha256)?;

    Ok(Command::Generate {
        header: header.ok_or_else(|| tool_error("missing --header"))?,
        expected_header_sha256,
        target: target.ok_or_else(|| tool_error("missing --target"))?,
        output_directory: output_directory
            .ok_or_else(|| tool_error("missing --output-directory"))?,
    })
}

fn set_once<T>(slot: &mut Option<T>, value: T, name: &str) -> Result<()> {
    if slot.replace(value).is_some() {
        return Err(tool_error(format!("{name} may only be specified once")));
    }
    Ok(())
}

fn reject_trailing(mut arguments: impl Iterator<Item = std::ffi::OsString>) -> Result<()> {
    if arguments.next().is_some() {
        return Err(tool_error("this command does not accept arguments"));
    }
    Ok(())
}

fn usage() -> &'static str {
    "usage:\n  bongocat-cubism-bindgen check-fixtures\n  bongocat-cubism-bindgen refresh-fixtures\n  bongocat-cubism-bindgen generate --header <absolute-path> --expected-header-sha256 <sha256> --target <rust-target> --output-directory <new-absolute-directory>"
}

fn find_target(name: &str) -> Result<Target> {
    TARGETS
        .iter()
        .copied()
        .find(|target| target.rust == name)
        .ok_or_else(|| {
            let supported = TARGETS
                .iter()
                .map(|target| target.rust)
                .collect::<Vec<_>>()
                .join(", ");
            tool_error(format!(
                "unsupported target {name:?}; supported targets: {supported}"
            ))
        })
}

fn check_fixtures() -> Result<()> {
    let header = manifest_directory().join(SYNTHETIC_HEADER);
    let first_header = fs::read(&header)?;
    let expected_header_sha256 = sha256(&first_header);

    for target in TARGETS {
        let first = generate_bindings(&header, *target)?;
        let second = generate_bindings(&header, *target)?;
        if first != second {
            return Err(tool_error(format!(
                "bindgen output is nondeterministic for {}",
                target.rust
            )));
        }
        validate_generated_bindings(&first, *target)?;

        let expected_path = expected_path(*target);
        let expected = fs::read_to_string(&expected_path).map_err(|error| {
            tool_error(format!(
                "cannot read expected binding {}: {error}",
                expected_path.display()
            ))
        })?;
        if first != expected {
            return Err(tool_error(format!(
                "generated binding drift for {}; review the generator/header change, then run refresh-fixtures",
                target.rust
            )));
        }

        println!(
            "verified target={} header_sha256={} bindings_sha256={}",
            target.rust,
            expected_header_sha256,
            sha256(first.as_bytes())
        );
    }
    Ok(())
}

fn refresh_fixtures() -> Result<()> {
    let header = manifest_directory().join(SYNTHETIC_HEADER);
    for target in TARGETS {
        let generated = generate_bindings(&header, *target)?;
        validate_generated_bindings(&generated, *target)?;
        write_atomic(&expected_path(*target), generated.as_bytes())?;
        println!(
            "refreshed target={} bindings_sha256={}",
            target.rust,
            sha256(generated.as_bytes())
        );
    }
    Ok(())
}

fn generate_real_bindings(
    header: &Path,
    expected_header_sha256: &str,
    target: Target,
    output_directory: &Path,
) -> Result<()> {
    let repository = repository_root()?;
    if !header.is_absolute() {
        return Err(tool_error("--header must be an absolute path"));
    }
    let header = header.canonicalize()?;
    if header.starts_with(&repository) {
        return Err(tool_error(
            "the licensed Cubism header must remain outside the repository",
        ));
    }
    if header.file_name().and_then(|name| name.to_str()) != Some(HEADER_NAME) {
        return Err(tool_error(format!("header filename must be {HEADER_NAME}")));
    }

    if !output_directory.is_absolute() {
        return Err(tool_error("--output-directory must be an absolute path"));
    }
    if output_directory.exists() {
        return Err(tool_error(
            "--output-directory must not already exist; existing output is never overwritten",
        ));
    }
    let output_parent = output_directory
        .parent()
        .ok_or_else(|| tool_error("output directory must have a parent"))?
        .canonicalize()?;
    if output_parent.starts_with(&repository) {
        return Err(tool_error(
            "generated bindings and provenance must remain outside the repository",
        ));
    }

    let header_bytes = fs::read(&header)?;
    let actual_header_sha256 = sha256(&header_bytes);
    if actual_header_sha256 != expected_header_sha256 {
        return Err(tool_error(format!(
            "header SHA-256 mismatch: expected {expected_header_sha256}, got {actual_header_sha256}"
        )));
    }

    let generated = generate_bindings(&header, target)?;
    validate_generated_bindings(&generated, target)?;
    let provenance = provenance(
        expected_header_sha256,
        target,
        &generated,
        &bindgen::clang_version().full,
    );

    fs::create_dir(output_directory)?;
    let bindings_path = output_directory.join("bindings.rs");
    let provenance_path = output_directory.join("provenance.json");
    if let Err(error) = write_new(&bindings_path, generated.as_bytes())
        .and_then(|()| write_new(&provenance_path, provenance.as_bytes()))
    {
        let _ = fs::remove_file(&bindings_path);
        let _ = fs::remove_file(&provenance_path);
        let _ = fs::remove_dir(output_directory);
        return Err(error);
    }

    println!(
        "generated target={} header_sha256={} bindings_sha256={} output_created=true",
        target.rust,
        expected_header_sha256,
        sha256(generated.as_bytes())
    );
    Ok(())
}

fn generate_bindings(header: &Path, target: Target) -> Result<String> {
    let rust_target = bindgen::RustTarget::stable(85, 0)
        .map_err(|error| tool_error(format!("invalid Rust target {RUST_TARGET}: {error}")))?;

    let bindings = bindgen::Builder::default()
        .header(header.to_string_lossy())
        .clang_arg(format!("--target={}", target.clang))
        .allowlist_function("^csm[A-Za-z0-9_]*$")
        .allowlist_type("^csm[A-Za-z0-9_]*$")
        .allowlist_var("^csm[A-Za-z0-9_]*$")
        .allowlist_recursively(false)
        .derive_debug(false)
        .generate_comments(false)
        .layout_tests(false)
        .formatter(bindgen::Formatter::Prettyplease)
        .merge_extern_blocks(true)
        .sort_semantically(true)
        .use_core()
        .rust_target(rust_target)
        .rust_edition(bindgen::RustEdition::Edition2024)
        .generate()
        .map_err(|error| tool_error(format!("bindgen failed for {}: {error}", target.rust)))?;

    Ok(bindings.to_string())
}

fn validate_generated_bindings(bindings: &str, target: Target) -> Result<()> {
    let generator_marker = format!("rust-bindgen {BINDGEN_VERSION}");
    if !bindings.starts_with("/* automatically generated by rust-bindgen ")
        || !bindings.contains(&generator_marker)
    {
        return Err(tool_error(format!(
            "generated bindings for {} do not report pinned bindgen {BINDGEN_VERSION}",
            target.rust
        )));
    }
    for symbol in REQUIRED_SYMBOLS {
        if !bindings.contains(symbol) {
            return Err(tool_error(format!(
                "generated bindings for {} are missing required symbol {symbol}",
                target.rust
            )));
        }
    }
    for forbidden in ["vendorInternalFunction", "VendorInternalType"] {
        if bindings.contains(forbidden) {
            return Err(tool_error(format!(
                "generated bindings for {} escaped the csm allowlist: {forbidden}",
                target.rust
            )));
        }
    }

    if bindings.contains("extern \"stdcall\"") {
        return Err(tool_error(format!(
            "generated bindings for {} unexpectedly use stdcall",
            target.rust
        )));
    }
    Ok(())
}

fn provenance(header_sha256: &str, target: Target, bindings: &str, clang_version: &str) -> String {
    format!(
        concat!(
            "{{\n",
            "  \"schema_version\": 1,\n",
            "  \"sdk_release\": \"{}\",\n",
            "  \"core_version\": \"{}\",\n",
            "  \"header_sha256\": \"{}\",\n",
            "  \"target\": \"{}\",\n",
            "  \"clang_target\": \"{}\",\n",
            "  \"bindgen_version\": \"{}\",\n",
            "  \"clang_version\": \"{}\",\n",
            "  \"rust_target\": \"{}\",\n",
            "  \"rust_edition\": \"2024\",\n",
            "  \"config_revision\": \"{}\",\n",
            "  \"config_sha256\": \"{}\",\n",
            "  \"bindings_sha256\": \"{}\"\n",
            "}}\n"
        ),
        SDK_RELEASE,
        CORE_VERSION,
        header_sha256,
        target.rust,
        target.clang,
        BINDGEN_VERSION,
        json_escape(clang_version),
        RUST_TARGET,
        CONFIG_REVISION,
        sha256(CONFIG_DESCRIPTION.as_bytes()),
        sha256(bindings.as_bytes()),
    )
}

fn json_escape(value: &str) -> String {
    let mut escaped = String::with_capacity(value.len());
    for character in value.chars() {
        match character {
            '"' => escaped.push_str("\\\""),
            '\\' => escaped.push_str("\\\\"),
            '\n' => escaped.push_str("\\n"),
            '\r' => escaped.push_str("\\r"),
            '\t' => escaped.push_str("\\t"),
            character if character.is_control() => {
                write!(&mut escaped, "\\u{:04x}", character as u32)
                    .expect("writing to a String cannot fail");
            }
            character => escaped.push(character),
        }
    }
    escaped
}

fn write_new(path: &Path, contents: &[u8]) -> Result<()> {
    let mut file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)?;
    file.write_all(contents)?;
    file.sync_all()?;
    Ok(())
}

fn write_atomic(path: &Path, contents: &[u8]) -> Result<()> {
    let parent = path
        .parent()
        .ok_or_else(|| tool_error("expected output path must have a parent"))?;
    fs::create_dir_all(parent)?;
    let temporary = parent.join(format!(
        ".{}.{}.tmp",
        path.file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("binding"),
        std::process::id()
    ));
    if temporary.exists() {
        return Err(tool_error(format!(
            "temporary output already exists: {}",
            temporary.display()
        )));
    }
    write_new(&temporary, contents)?;
    fs::rename(&temporary, path)?;
    Ok(())
}

fn sha256(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    let mut hexadecimal = String::with_capacity(digest.len() * 2);
    for byte in digest {
        write!(&mut hexadecimal, "{byte:02x}").expect("writing to a String cannot fail");
    }
    hexadecimal
}

fn validate_sha256(value: &str) -> Result<()> {
    if value.len() != 64 || !value.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(tool_error(
            "--expected-header-sha256 must be exactly 64 hexadecimal characters",
        ));
    }
    Ok(())
}

fn manifest_directory() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn repository_root() -> Result<PathBuf> {
    manifest_directory()
        .parent()
        .and_then(Path::parent)
        .ok_or_else(|| tool_error("cannot locate repository root"))?
        .canonicalize()
        .map_err(Into::into)
}

fn expected_path(target: Target) -> PathBuf {
    manifest_directory()
        .join(EXPECTED_DIRECTORY)
        .join(format!("{}.rs", target.rust))
}

fn tool_error(message: impl Into<String>) -> Box<dyn Error> {
    Box::new(ToolError(message.into()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn generation_targets_match_available_r5_desktop_artifacts() {
        let mut names = TARGETS.iter().map(|target| target.rust).collect::<Vec<_>>();
        names.sort_unstable();
        names.dedup();
        assert_eq!(names.len(), TARGETS.len());
        assert!(!names.contains(&"i686-pc-windows-msvc"));
        assert!(!names.contains(&"aarch64-pc-windows-msvc"));
    }

    #[test]
    fn sha256_validation_requires_a_complete_digest() {
        assert!(validate_sha256(&"a".repeat(64)).is_ok());
        assert!(validate_sha256(&"A".repeat(64)).is_ok());
        assert!(validate_sha256(&"a".repeat(63)).is_err());
        assert!(validate_sha256(&"z".repeat(64)).is_err());
    }

    #[test]
    fn provenance_does_not_include_source_paths() {
        let provenance = provenance(
            &"a".repeat(64),
            TARGETS[0],
            "bindings",
            "clang 21.0.0\nlocal",
        );
        assert!(provenance.contains("\\nlocal"));
        assert!(provenance.contains("\"sdk_release\": \"5-r.5\""));
        assert!(provenance.contains("\"core_version\": \"06.00.0001\""));
        assert!(!provenance.contains(env!("CARGO_MANIFEST_DIR")));
    }

    #[test]
    fn real_generation_rejects_repository_header() {
        let header = manifest_directory().join(SYNTHETIC_HEADER);
        let digest = sha256(&fs::read(&header).unwrap());
        let external = TempDir::new().unwrap();
        let output = external.path().join("output");

        let error = generate_real_bindings(&header, &digest, TARGETS[0], &output)
            .unwrap_err()
            .to_string();

        assert!(error.contains("header must remain outside the repository"));
        assert!(!output.exists());
    }

    #[test]
    fn real_generation_rejects_hash_mismatch_without_creating_output() {
        let external = TempDir::new().unwrap();
        let header = copy_external_synthetic_header(&external);
        let output = external.path().join("output");

        let error = generate_real_bindings(&header, &"0".repeat(64), TARGETS[0], &output)
            .unwrap_err()
            .to_string();

        assert!(error.contains("header SHA-256 mismatch"));
        assert!(!output.exists());
    }

    #[test]
    fn real_generation_never_overwrites_an_existing_directory() {
        let external = TempDir::new().unwrap();
        let header = copy_external_synthetic_header(&external);
        let digest = sha256(&fs::read(&header).unwrap());
        let output = external.path().join("output");
        fs::create_dir(&output).unwrap();
        fs::write(output.join("sentinel"), "keep").unwrap();

        let error = generate_real_bindings(&header, &digest, TARGETS[0], &output)
            .unwrap_err()
            .to_string();

        assert!(error.contains("must not already exist"));
        assert_eq!(fs::read_to_string(output.join("sentinel")).unwrap(), "keep");
    }

    #[test]
    fn real_generation_writes_only_bindings_and_path_free_provenance() {
        let external = TempDir::new().unwrap();
        let header = copy_external_synthetic_header(&external);
        let digest = sha256(&fs::read(&header).unwrap());
        let output = external.path().join("output");

        generate_real_bindings(&header, &digest, TARGETS[0], &output).unwrap();

        let mut entries = fs::read_dir(&output)
            .unwrap()
            .map(|entry| entry.unwrap().file_name())
            .collect::<Vec<_>>();
        entries.sort();
        assert_eq!(
            entries,
            ["bindings.rs", "provenance.json"].map(std::ffi::OsString::from)
        );
        let bindings = fs::read_to_string(output.join("bindings.rs")).unwrap();
        validate_generated_bindings(&bindings, TARGETS[0]).unwrap();
        let provenance = fs::read_to_string(output.join("provenance.json")).unwrap();
        assert!(provenance.contains(&digest));
        assert!(provenance.contains(&sha256(bindings.as_bytes())));
        assert!(!provenance.contains(external.path().to_str().unwrap()));
    }

    fn copy_external_synthetic_header(external: &TempDir) -> PathBuf {
        let header = external.path().join(HEADER_NAME);
        fs::copy(manifest_directory().join(SYNTHETIC_HEADER), &header).unwrap();
        header
    }
}
