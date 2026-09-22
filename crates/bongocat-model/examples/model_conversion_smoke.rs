//! Convert a BongoCatMver source with the real store and report what it wrote.
//!
//! This is the manual counterpart to the environment-driven test: it runs the
//! same detection and conversion the product runs, against a source the
//! maintainer names, and then prints the resulting package tree with the size of
//! every composed key image. It exists because the real sources are third-party
//! model folders that cannot be committed to the repository, and because the
//! only other way to see a conversion is to drive the settings window.
//!
//! ```text
//! cargo run -p bongocat-model --example model_conversion_smoke -- --source <folder>
//! ```
//!
//! Without `--store` the models are installed into a temporary store that is
//! removed when the run ends, so the tool never touches product data.

use bongocat_model::{ModelPackageLimits, ModelSourceContent, ModelStore};
use std::{
    io::{self, Write},
    path::{Path, PathBuf},
    process::ExitCode,
};

fn main() -> ExitCode {
    let mut source = None;
    let mut store_base = None;
    let mut arguments = std::env::args().skip(1);
    while let Some(argument) = arguments.next() {
        match argument.as_str() {
            "--source" => source = arguments.next().map(PathBuf::from),
            "--store" => store_base = arguments.next().map(PathBuf::from),
            "--help" => {
                let _ = writeln!(
                    io::stdout(),
                    "usage: model_conversion_smoke --source <folder> [--store <directory>]"
                );
                return ExitCode::SUCCESS;
            }
            other => {
                let _ = writeln!(io::stderr(), "unexpected argument: {other}");
                return ExitCode::from(2);
            }
        }
    }
    let Some(source) = source else {
        let _ = writeln!(io::stderr(), "--source is required");
        return ExitCode::from(2);
    };

    let temporary = match store_base {
        Some(_) => None,
        None => Some(tempfile::tempdir().expect("temporary store")),
    };
    let base = match (&store_base, &temporary) {
        (Some(base), _) => base.clone(),
        (None, Some(temporary)) => temporary.path().to_owned(),
        (None, None) => unreachable!("either a store base or a temporary one exists"),
    };
    let store = ModelStore::new(
        base.join("models"),
        base.join("locks/models.writer.lock"),
        ModelPackageLimits::default(),
    )
    .expect("model store");

    let content = match store.inspect_source(&source) {
        Ok(content) => content,
        Err(error) => {
            let _ = writeln!(io::stderr(), "source rejected: {error:?}");
            return ExitCode::FAILURE;
        }
    };
    let mut stdout = io::stdout();
    match content {
        ModelSourceContent::Package => {
            let _ = writeln!(stdout, "{} is a BongoCat package", source.display());
        }
        ModelSourceContent::Mver { modes } => {
            let _ = writeln!(
                stdout,
                "{} is a BongoCatMver source with {} model(s)",
                source.display(),
                modes.len()
            );
            for mode in modes {
                let id = match store.allocate_unique_id() {
                    Ok(id) => id,
                    Err(error) => {
                        let _ = writeln!(io::stderr(), "id allocation failed: {error:?}");
                        return ExitCode::FAILURE;
                    }
                };
                let started = std::time::Instant::now();
                let installed =
                    match store.import_mver_with_observer(id, mode, &source, |_| {}, || false) {
                        Ok(installed) => installed,
                        Err(error) => {
                            let _ = writeln!(io::stderr(), "{} failed: {error:?}", mode.as_str());
                            return ExitCode::FAILURE;
                        }
                    };
                let _ = writeln!(
                    stdout,
                    "\n== {} -> {} ({} files, {} bytes, {:.2}s)",
                    mode.as_str(),
                    installed.root().display(),
                    installed.index().package_file_count,
                    installed.index().package_total_bytes,
                    started.elapsed().as_secs_f64()
                );
                let _ = writeln!(
                    stdout,
                    "   entry {} | moc {} | textures {}",
                    installed.index().entry,
                    installed.index().moc,
                    installed.index().textures.len()
                );
                report_images(&mut stdout, installed.root());
            }
        }
    }
    ExitCode::SUCCESS
}

/// Print every installed overlay and its packaged size.
fn report_images(stdout: &mut impl Write, root: &Path) {
    for side in ["left-keys", "right-keys"] {
        let directory = root.join("resources").join(side);
        let Ok(entries) = std::fs::read_dir(&directory) else {
            continue;
        };
        let mut sizes = entries
            .filter_map(Result::ok)
            .filter_map(|entry| {
                let path = entry.path();
                let bytes = entry.metadata().ok()?.len();
                let name = path.file_stem()?.to_str()?.to_owned();
                Some((name, bytes))
            })
            .collect::<Vec<_>>();
        sizes.sort();
        let total = sizes.iter().map(|(_, bytes)| bytes).sum::<u64>();
        let _ = writeln!(
            stdout,
            "   resources/{side}: {} image(s), {total} bytes",
            sizes.len()
        );
        for (name, bytes) in &sizes {
            let _ = writeln!(stdout, "      {name}: {bytes} bytes");
        }
    }
    for asset in ["background.png", "cover.png"] {
        let path = root.join("resources").join(asset);
        if let Ok(metadata) = std::fs::metadata(&path) {
            let _ = writeln!(stdout, "   resources/{asset}: {} bytes", metadata.len());
        }
    }
}
