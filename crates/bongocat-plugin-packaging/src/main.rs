//! Plugin-only release tooling.
//!
//! This binary deliberately has no application packaging, updater, or release
//! code. It builds the three plugin archives, signs the catalog with the
//! plugin-owned Minisign key, and checks the immutable release history.
#![forbid(unsafe_code)]

mod archive;
mod catalog;
mod history;
mod output;
mod signing;
mod workspace;

use std::io::Write as _;
use std::path::PathBuf;

use thiserror::Error;

pub const PLATFORMS: [Platform; 3] = [
    Platform::MacosAarch64,
    Platform::MacosX86_64,
    Platform::WindowsX86_64,
];

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum Platform {
    MacosAarch64,
    MacosX86_64,
    WindowsX86_64,
}

impl Platform {
    pub const fn triple(self) -> &'static str {
        match self {
            Self::MacosAarch64 => "aarch64-apple-darwin",
            Self::MacosX86_64 => "x86_64-apple-darwin",
            Self::WindowsX86_64 => "x86_64-pc-windows-msvc",
        }
    }

    pub const fn key(self) -> &'static str {
        match self {
            Self::MacosAarch64 => "macos-aarch64",
            Self::MacosX86_64 => "macos-x86_64",
            Self::WindowsX86_64 => "windows-x86_64",
        }
    }

    pub const fn is_apple(self) -> bool {
        matches!(self, Self::MacosAarch64 | Self::MacosX86_64)
    }

    pub fn parse(value: &str) -> Result<Self> {
        PLATFORMS
            .into_iter()
            .find(|platform| platform.triple() == value || platform.key() == value)
            .ok_or_else(|| Error::Usage(format!("unsupported target {value}")))
    }

    pub fn host() -> Result<Self> {
        let triple = match (std::env::consts::OS, std::env::consts::ARCH) {
            ("macos", "aarch64") => Self::MacosAarch64,
            ("macos", "x86_64") => Self::MacosX86_64,
            ("windows", "x86_64") => Self::WindowsX86_64,
            (os, arch) => {
                return Err(Error::Usage(format!(
                    "unsupported host {os}-{arch}; pass --target explicitly"
                )));
            }
        };
        Ok(triple)
    }
}

#[derive(Debug, Error)]
pub enum Error {
    #[error("{0}")]
    Usage(String),
    #[error("{0}")]
    Invalid(String),
    #[error(transparent)]
    Io(#[from] std::io::Error),
    #[error(transparent)]
    Json(#[from] serde_json::Error),
    #[error(transparent)]
    Zip(#[from] zip::result::ZipError),
    #[error(transparent)]
    Signing(#[from] minisign::PError),
    #[error(transparent)]
    Utf8(#[from] std::str::Utf8Error),
    #[error(transparent)]
    Persist(#[from] tempfile::PersistError),
}

pub type Result<T> = std::result::Result<T, Error>;

fn main() {
    if let Err(error) = run(std::env::args().skip(1).collect()) {
        let _ = writeln!(std::io::stderr(), "error: {error}");
        std::process::exit(1);
    }
}

fn run(args: Vec<String>) -> Result<()> {
    let Some(command) = args.first().map(String::as_str) else {
        return Err(Error::Usage(usage()));
    };
    match command {
        "build" => build(&args[1..]),
        "catalog" => catalog::command(&args[1..]),
        "verify" => history::verify_command(&args[1..]),
        "keygen" => signing::keygen(&args[1..]),
        "-h" | "--help" | "help" => {
            let _ = writeln!(std::io::stdout(), "{}", usage());
            Ok(())
        }
        _ => Err(Error::Usage(usage())),
    }
}

fn usage() -> String {
    "usage:
  bongocat-plugin-packaging build --root <workspace> --target <triple|platform> --output <dir>
  bongocat-plugin-packaging catalog <assemble|dev|sign|verify> ...
  bongocat-plugin-packaging catalog assemble ... [--release-url <base>]
  bongocat-plugin-packaging keygen --private <file> --public <file>
  bongocat-plugin-packaging verify --catalog <plugins.json> --state <state-dir> --public-key <file>"
        .into()
}

fn build(args: &[String]) -> Result<()> {
    let root = arg(args, "--root")?
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."));
    let target = Platform::parse(
        arg(args, "--target")?.ok_or_else(|| Error::Usage("build requires --target".into()))?,
    )?;
    let output = PathBuf::from(
        arg(args, "--output")?.ok_or_else(|| Error::Usage("build requires --output".into()))?,
    );
    let packages = workspace::build_target(&root, target, &output)?;
    for package in packages {
        let _ = writeln!(std::io::stdout(), "{}", package.display());
    }
    Ok(())
}

pub fn arg<'a>(args: &'a [String], name: &str) -> Result<Option<&'a str>> {
    let mut values = args.iter().enumerate().filter(|(_, value)| *value == name);
    let Some((index, _)) = values.next() else {
        return Ok(None);
    };
    if values.next().is_some() {
        return Err(Error::Usage(format!("{name} may appear only once")));
    }
    args.get(index + 1)
        .map(String::as_str)
        .filter(|value| !value.starts_with("--"))
        .map(Some)
        .ok_or_else(|| Error::Usage(format!("{name} requires a value")))
}

pub fn reject_unknown(args: &[String], valued: &[&str], flags: &[&str]) -> Result<()> {
    let mut index = 0;
    while index < args.len() {
        let value = args[index].as_str();
        if valued.contains(&value) {
            if args.get(index + 1).is_none() {
                return Err(Error::Usage(format!("{value} requires a value")));
            }
            index += 2;
        } else if flags.contains(&value) {
            index += 1;
        } else {
            return Err(Error::Usage(format!("unknown argument {value}")));
        }
    }
    Ok(())
}
