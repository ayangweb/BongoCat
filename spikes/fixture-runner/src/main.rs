use std::path::PathBuf;

use bongocat_fixture_runner_spike::run_fixture_directory;

fn main() {
    let mut args = std::env::args_os().skip(1);
    let root = args
        .next()
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../shared/fixtures"));
    if args.next().is_some() {
        eprintln!("usage: bongocat-fixture-runner-spike [fixtures-root]");
        std::process::exit(2);
    }

    match run_fixture_directory(&root) {
        Ok(report) => println!(
            "fixture-runner: sequences={} events={} checkpoints={} audio_triggers={}",
            report.sequences, report.events, report.checkpoints, report.audio_triggers
        ),
        Err(error) => {
            eprintln!("fixture-runner: error: {error}");
            std::process::exit(1);
        }
    }
}
