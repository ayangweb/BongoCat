use bongocat_config_store_spike::{BUNDLE_ID, BuildEnvironment, ConfigStore, StorageLayout};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let environment = if cfg!(debug_assertions) {
        BuildEnvironment::Development
    } else {
        BuildEnvironment::Production
    };
    let base = std::env::temp_dir().join("bongocat-config-store-spike");
    let store = ConfigStore::new(StorageLayout::under(base, environment))?;
    let config = store.load_or_default()?;
    println!(
        "config-store-spike: bundle_id={BUNDLE_ID} environment={} schema_version={} config={}",
        environment.directory_name(),
        config.schema_version,
        store.layout().config.display()
    );
    Ok(())
}
