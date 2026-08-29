use bongocat_model_package_spike::{
    ModelPackageLimits, inspect_model_package, inspect_physics_resource,
};
use serde::Serialize;
use std::env;
use std::ffi::OsStr;
use std::path::PathBuf;
use std::process::ExitCode;

fn main() -> ExitCode {
    let mut arguments = env::args_os();
    let executable = arguments
        .next()
        .and_then(|value| PathBuf::from(value).file_name().map(|name| name.to_owned()))
        .and_then(|name| name.to_str().map(str::to_owned))
        .unwrap_or_else(|| "bongocat-model-package-spike".to_owned());
    let arguments = arguments.collect::<Vec<_>>();
    match arguments.as_slice() {
        [package] => print_result(inspect_model_package(
            PathBuf::from(package),
            ModelPackageLimits::default(),
        )),
        [flag, physics] if flag == OsStr::new("--physics") => print_result(
            inspect_physics_resource(PathBuf::from(physics), 16 * 1024 * 1024),
        ),
        _ => {
            eprintln!(
                "usage: {executable} <model-package-directory>\n       {executable} --physics <physics3-json>"
            );
            ExitCode::from(2)
        }
    }
}

fn print_result<T: Serialize, E: std::fmt::Display>(result: Result<T, E>) -> ExitCode {
    match result {
        Ok(index) => match serde_json::to_string_pretty(&index) {
            Ok(json) => {
                println!("{json}");
                ExitCode::SUCCESS
            }
            Err(error) => {
                eprintln!("resource_summary_serialization_failed: {error}");
                ExitCode::FAILURE
            }
        },
        Err(error) => {
            eprintln!("{error}");
            ExitCode::FAILURE
        }
    }
}
