#![cfg(target_os = "linux")]

use std::fs;
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

/// Exercise the real GPUI startup and independent Alpha worker together.
/// The caller provisions a disposable development loopback device first.
#[test]
#[ignore = "requires a disposable BongoCat Alpha (Development) camera and Wayland session"]
fn alpha_enabled_startup_opens_settings_and_produces_video() {
    struct ChildGuard(Child);
    impl Drop for ChildGuard {
        fn drop(&mut self) {
            if self.0.try_wait().ok().flatten().is_none() {
                let _ = self.0.kill();
                let _ = self.0.wait();
            }
        }
    }

    let node = std::env::var("BONGOCAT_ALPHA_TEST_DEVICE").expect("test camera path");
    let scratch = tempfile::tempdir().expect("isolated data directory");
    let root = scratch.path().join("com.ayangweb.bongo-cat/development");
    fs::create_dir_all(&root).expect("create development root");
    let fixture = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .expect("workspace root")
        .join("shared/config/fixtures/default.json");
    let mut config: serde_json::Value =
        serde_json::from_slice(&fs::read(fixture).expect("configuration fixture"))
            .expect("fixture JSON");
    config["alpha_output"]["enabled"] = true.into();
    config["system"]["show_status_icon"] = false.into();
    fs::write(
        root.join("config.json"),
        serde_json::to_vec(&config).unwrap(),
    )
    .expect("write isolated configuration");

    let mut app = ChildGuard(
        Command::new(env!("CARGO_BIN_EXE_bongocat-app"))
            .args(["--run-seconds", "18"])
            .env("XDG_DATA_HOME", scratch.path())
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .expect("start application"),
    );
    let capture = scratch.path().join("frame.bgra");
    let deadline = Instant::now() + Duration::from_secs(9);
    let pixels = loop {
        if let Some(status) = app.0.try_wait().expect("application status") {
            panic!("application exited before producing Alpha video: {status}");
        }
        let _ = Command::new("/usr/bin/timeout")
            .args([
                "2s",
                "/usr/bin/v4l2-ctl",
                "--device",
                &node,
                "--stream-mmap=2",
                "--stream-count=1",
            ])
            .arg(format!("--stream-to={}", capture.display()))
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status();
        if let Ok(bytes) = fs::read(&capture)
            && bytes.len() == 1280 * 720 * 4
        {
            break bytes;
        }
        assert!(Instant::now() < deadline, "no complete Alpha frame");
        std::thread::sleep(Duration::from_millis(100));
    };
    assert!(
        pixels.chunks_exact(4).any(|pixel| pixel[3] == 0),
        "missing transparent pixels"
    );
    assert!(
        pixels.chunks_exact(4).any(|pixel| pixel[3] == 255),
        "missing opaque pixels"
    );
    assert!(
        pixels
            .chunks_exact(4)
            .any(|pixel| (1..255).contains(&pixel[3])),
        "missing translucent pixels"
    );
    // A covered Wayland surface must not throttle the independent camera to
    // the compositor's one-frame-per-second background cadence.
    let stream = Command::new("/usr/bin/timeout")
        .args([
            "4s",
            "/usr/bin/v4l2-ctl",
            "--device",
            &node,
            "--stream-mmap=2",
            "--stream-count=10",
            "--stream-to=/dev/null",
        ])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .expect("capture camera cadence");
    assert!(
        stream.success(),
        "camera did not produce ten frames in four seconds"
    );
    assert!(app.0.wait().expect("application exit").success());
}
