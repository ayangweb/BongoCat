use bongocat_config_store_spike::{
    BUNDLE_ID, BuildEnvironment, ConfigStore, StorageLayout, platform_layout,
};
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

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut arguments = std::env::args().skip(1);
    match arguments.next().as_deref() {
        Some("--hold-lock-after-temp-sync") => {
            let base = arguments.next().ok_or("missing crash probe base path")?;
            let environment = arguments
                .next()
                .as_deref()
                .and_then(parse_environment)
                .ok_or("missing or invalid crash probe environment")?;
            if arguments.next().is_some() {
                return Err("unexpected crash probe argument".into());
            }
            return hold_lock_after_temp_sync(Path::new(&base), environment);
        }
        Some("--commit-language") => {
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
            return commit_language(Path::new(&base), environment, language);
        }
        Some(_) => return Err("unexpected config-store spike argument".into()),
        None => {}
    }

    let environment = if cfg!(debug_assertions) {
        BuildEnvironment::Development
    } else {
        BuildEnvironment::Production
    };
    let store = ConfigStore::new(platform_layout(environment)?)?;
    let config = store.load_or_default()?;
    println!(
        "config-store-spike: bundle_id={BUNDLE_ID} environment={} schema_version={} config={}",
        environment.directory_name(),
        config.schema_version,
        store.layout().config.display()
    );
    Ok(())
}
