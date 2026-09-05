#![forbid(unsafe_code)]

use std::{path::Path, process::Command};

fn fixture(name: &str) -> std::path::PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../shared/config/legacy-pinia")
        .join(name)
}

#[test]
fn ready_report_exits_zero() {
    let output = Command::new(env!("CARGO_BIN_EXE_legacy-config-inspector"))
        .args(["--input"])
        .arg(fixture("default"))
        .output()
        .expect("run inspector");

    assert!(output.status.success());
    let report: serde_json::Value = serde_json::from_slice(&output.stdout).expect("parse report");
    assert_eq!(report["status"], "ready");
}

#[test]
fn missing_input_exits_two_without_echoing_path() {
    let missing = fixture("directory-that-does-not-exist");
    let output = Command::new(env!("CARGO_BIN_EXE_legacy-config-inspector"))
        .args(["--input"])
        .arg(&missing)
        .output()
        .expect("run inspector");

    assert_eq!(output.status.code(), Some(2));
    let stdout = String::from_utf8(output.stdout).expect("utf-8 output");
    assert!(!stdout.contains(missing.to_string_lossy().as_ref()));
    let report: serde_json::Value = serde_json::from_str(&stdout).expect("parse report");
    assert_eq!(report["status"], "blocked");
}
