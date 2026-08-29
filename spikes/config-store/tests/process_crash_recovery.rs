use bongocat_config_store_spike::{
    BuildEnvironment, ConfigError, ConfigStore, NativeConfig, RecoveryAction, StorageLayout,
};
use std::{
    fs,
    process::{Child, Command, Stdio},
    thread,
    time::{Duration, Instant},
};
use tempfile::tempdir;

struct ChildGuard(Child);

impl Drop for ChildGuard {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

#[test]
fn forced_process_exit_releases_writer_lock_and_preserves_current_config() {
    let base = tempdir().unwrap();
    let layout = StorageLayout::under(base.path(), BuildEnvironment::Development);
    let store = ConfigStore::new(layout.clone()).unwrap();
    let original = store.load_or_default().unwrap();

    let child = Command::new(env!("CARGO_BIN_EXE_bongocat-config-store-spike"))
        .args([
            "--hold-lock-after-temp-sync",
            base.path().to_str().unwrap(),
            "development",
        ])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let mut child = ChildGuard(child);
    let ready_path = layout.locks.join("crash-probe.ready");
    let deadline = Instant::now() + Duration::from_secs(10);
    while !ready_path.is_file() {
        assert!(
            Instant::now() < deadline,
            "crash probe did not become ready"
        );
        assert!(
            child.0.try_wait().unwrap().is_none(),
            "crash probe exited before acquiring its writer lock"
        );
        thread::sleep(Duration::from_millis(25));
    }

    assert!(matches!(
        store.commit(&NativeConfig::default()),
        Err(ConfigError::LockUnavailable)
    ));
    child.0.kill().unwrap();
    assert!(!child.0.wait().unwrap().success());
    fs::remove_file(ready_path).unwrap();

    assert_eq!(
        store.recover_interrupted_commit().unwrap(),
        RecoveryAction::ArchivedStaleTemp
    );
    assert_eq!(store.load_or_default().unwrap(), original);
    let interrupted: NativeConfig =
        serde_json::from_slice(&fs::read(layout.backups.join("config.interrupted.json")).unwrap())
            .unwrap();
    assert_eq!(interrupted.appearance.language, "zh-CN");
}
