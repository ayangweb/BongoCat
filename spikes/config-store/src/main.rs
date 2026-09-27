use bongocat_config_store_spike::{BuildEnvironment, ConfigStore, StorageLayout};
use std::{
    fs::{self, OpenOptions},
    io::Write,
    path::Path,
    thread,
    time::Duration,
};

fn parse_environment(value: &str) -> Option<BuildEnvironment> {
    match value {
        "development" => Some(BuildEnvironment::Development),
        "production" => Some(BuildEnvironment::Production),
        _ => None,
    }
}

fn hold_lock_after_temp_sync(
    base: &Path,
    environment: BuildEnvironment,
) -> Result<(), Box<dyn std::error::Error>> {
    let store = ConfigStore::new(StorageLayout::under(base, environment))?;
    let mut candidate = store.load_or_default()?;
    candidate.appearance.language = "zh-CN".into();

    let _lock = store.acquire_writer_lock()?;
    let temp_path = store.layout().config.with_extension("json.tmp");
    let mut temp = OpenOptions::new()
        .create(true)
        .truncate(true)
        .write(true)
        .open(temp_path)?;
    temp.write_all(&serde_json::to_vec_pretty(&candidate)?)?;
    temp.sync_all()?;

    let ready_path = store.layout().locks.join("crash-probe.ready");
    fs::write(&ready_path, b"ready")?;
    OpenOptions::new()
        .write(true)
        .open(ready_path)?
        .sync_all()?;

    loop {
        thread::sleep(Duration::from_secs(1));
    }
}

fn commit_language(
    base: &Path,
    environment: BuildEnvironment,
    language: String,
) -> Result<(), Box<dyn std::error::Error>> {
    let store = ConfigStore::new(StorageLayout::under(base, environment))?;
    let mut config = store.load_or_default()?;
    config.appearance.language = language;
    store.commit(&config)?;
    Ok(())
}

/// The binary exists to be driven by `tests/process_crash_recovery.rs`, so every
/// mode takes its own storage root.
///
/// There is deliberately no default that resolves the real platform data
/// directory. That directory is defined for the two shipped platforms only, and a
/// default reaching for it would make this binary unbuildable for `--all-targets`
/// anywhere else — which is exactly the kind of compatibility shim the platform
/// contract test exists to reject. `platform_layout` stays part of the library and
/// is covered there; a developer who wants it can call it from a test.
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut arguments = std::env::args().skip(1);
    let mode = arguments.next().ok_or(
        "usage: config-store-spike --hold-lock-after-temp-sync <base> <environment> \
         | --commit-language <base> <environment> <language>",
    )?;
    match mode.as_str() {
        "--hold-lock-after-temp-sync" => {
            let base = arguments.next().ok_or("missing crash probe base path")?;
            let environment = arguments
                .next()
                .as_deref()
                .and_then(parse_environment)
                .ok_or("missing or invalid crash probe environment")?;
            if arguments.next().is_some() {
                return Err("unexpected crash probe argument".into());
            }
            hold_lock_after_temp_sync(Path::new(&base), environment)
        }
        "--commit-language" => {
            let base = arguments.next().ok_or("missing commit probe base path")?;
            let environment = arguments
                .next()
                .as_deref()
                .and_then(parse_environment)
                .ok_or("missing or invalid commit probe environment")?;
            let language = arguments.next().ok_or("missing commit probe language")?;
            if arguments.next().is_some() {
                return Err("unexpected commit probe argument".into());
            }
            commit_language(Path::new(&base), environment, language)
        }
        other => Err(format!("unexpected config-store spike argument: {other}").into()),
    }
}
