#![forbid(unsafe_code)]

use std::{env, path::PathBuf, process::ExitCode};

use legacy_config_inspector::inspect_dir;

fn main() -> ExitCode {
    let mut args = env::args_os().skip(1);
    let Some(flag) = args.next() else {
        return usage();
    };
    if flag == "--help" || flag == "-h" {
        println!("Usage: legacy-config-inspector --input <DIRECTORY>");
        return ExitCode::SUCCESS;
    }
    if flag != "--input" {
        return usage();
    }
    let Some(input) = args.next() else {
        return usage();
    };
    if args.next().is_some() {
        return usage();
    }

    let report = inspect_dir(&PathBuf::from(input));
    match serde_json::to_string_pretty(&report) {
        Ok(json) => println!("{json}"),
        Err(_) => return ExitCode::FAILURE,
    }
    if report.is_blocked() {
        ExitCode::from(2)
    } else {
        ExitCode::SUCCESS
    }
}

fn usage() -> ExitCode {
    eprintln!("Usage: legacy-config-inspector --input <DIRECTORY>");
    ExitCode::from(64)
}
