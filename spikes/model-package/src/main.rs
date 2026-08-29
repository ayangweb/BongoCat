use bongocat_model_package_spike::{ModelPackageLimits, inspect_model_package};
use std::env;
use std::path::PathBuf;
use std::process::ExitCode;

fn main() -> ExitCode {
    let mut arguments = env::args_os();
    let executable = arguments
        .next()
        .and_then(|value| PathBuf::from(value).file_name().map(|name| name.to_owned()))
        .and_then(|name| name.to_str().map(str::to_owned))
        .unwrap_or_else(|| "bongocat-model-package-spike".to_owned());
    let Some(package) = arguments.next() else {
        eprintln!("usage: {executable} <model-package-directory>");
        return ExitCode::from(2);
    };
    if arguments.next().is_some() {
        eprintln!("usage: {executable} <model-package-directory>");
        return ExitCode::from(2);
    }

    match inspect_model_package(PathBuf::from(package), ModelPackageLimits::default()) {
        Ok(index) => match serde_json::to_string_pretty(&index) {
            Ok(json) => {
                println!("{json}");
                ExitCode::SUCCESS
            }
            Err(error) => {
                eprintln!("model_index_serialization_failed: {error}");
                ExitCode::FAILURE
            }
        },
        Err(error) => {
            eprintln!("{error}");
            ExitCode::FAILURE
        }
    }
}
