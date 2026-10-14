//! The manager drives a real plugin binary discovered from a directory.
use bongocat_plugin_manager::{PluginManager, PluginState};

#[cfg(target_os = "windows")]
const BINARY_FILE: &str = "bongocat-countdown.exe";
#[cfg(not(target_os = "windows"))]
const BINARY_FILE: &str = "bongocat-countdown";

/// The compiled countdown package. The test skips rather than pretending when
/// nothing has built the plugin binary yet.
fn source() -> Option<std::path::PathBuf> {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join("plugins/countdown");
    root.join("plugin.json").is_file().then_some(root)
}

fn target_binary() -> Option<std::path::PathBuf> {
    let target = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target");
    ["debug", "release"]
        .iter()
        .map(|profile| target.join(profile).join(BINARY_FILE))
        .find(|candidate| candidate.is_file())
}

#[test]
fn discovery_lifecycle_config_and_ticks_run_a_real_process() {
    let (Some(source), Some(binary)) = (source(), target_binary()) else {
        return;
    };
    let directory = tempfile::tempdir().unwrap();
    let package = directory.path().join("countdown");
    std::fs::create_dir_all(&package).unwrap();
    std::fs::copy(source.join("plugin.json"), package.join("plugin.json")).unwrap();
    std::fs::copy(&binary, package.join(BINARY_FILE)).unwrap();

    let mut manager = PluginManager::new(directory.path().to_path_buf()).unwrap();
    assert_eq!(manager.discover_plugins().unwrap(), 1);
    assert_eq!(manager.list_plugins()[0].1, PluginState::Discovered);

    manager.enable_plugin("com.bongocat.countdown").unwrap();
    assert_eq!(manager.list_plugins()[0].1, PluginState::Enabled);
    let started = manager
        .plugin_output("com.bongocat.countdown")
        .unwrap()
        .clone();
    assert!(started.model.nodes.iter().any(|node| matches!(
        node,
        bongocat_plugin_sdk::UiNode::Button { id, .. } if id == "toggle"
    )));

    manager
        .configure_plugin(
            "com.bongocat.countdown",
            [(
                "minutes".into(),
                bongocat_plugin_sdk::ConfigValue::Integer(5),
            )]
            .into(),
        )
        .unwrap();
    manager.tick(250);
    manager.disable_plugin("com.bongocat.countdown").unwrap();
    assert_eq!(manager.list_plugins()[0].1, PluginState::Disabled);
    manager.shutdown();
}
