//! The manager drives a real plugin binary discovered from a directory.
use bongocat_plugin_manager::{PluginManager, PluginState};
use std::io::Write as _;

#[cfg(target_os = "windows")]
const BINARY_FILE: &str = "bongocat-countdown.exe";
#[cfg(not(target_os = "windows"))]
const BINARY_FILE: &str = "bongocat-countdown";

/// The compiled countdown package. The test skips rather than pretending when
/// nothing has built the plugin binary yet.
fn source() -> Option<std::path::PathBuf> {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join("plugins/pomodoro");
    root.join("plugin.json").is_file().then_some(root)
}

fn target_binary() -> Option<std::path::PathBuf> {
    let target = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target");
    ["debug", "release"]
        .iter()
        .map(|profile| target.join(profile).join(BINARY_FILE))
        .find(|candidate| candidate.is_file())
}

/// Build the package the way the packaging tool does: one ZIP with the manifest
/// and the executable, installed through the manager's own lifecycle.
fn install(source: &std::path::Path, binary: &std::path::Path, directory: &std::path::Path) {
    let mut archive = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
    let options = zip::write::SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated);
    archive
        .start_file("plugin.json", options)
        .expect("start manifest");
    archive
        .write_all(&std::fs::read(source.join("plugin.json")).unwrap())
        .unwrap();
    archive
        .start_file(BINARY_FILE, options.unix_permissions(0o755))
        .expect("start binary");
    archive.write_all(&std::fs::read(binary).unwrap()).unwrap();
    let bytes = archive.finish().unwrap().into_inner();

    let mut manager = PluginManager::new(directory.to_path_buf()).unwrap();
    manager.install(&bytes).unwrap();
}

#[test]
fn discovery_lifecycle_config_and_ticks_run_a_real_process() {
    let (Some(source), Some(binary)) = (source(), target_binary()) else {
        return;
    };
    let directory = tempfile::tempdir().unwrap();
    install(&source, &binary, directory.path());

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
    manager
        .action_plugin("com.bongocat.countdown", "restart")
        .unwrap();
    assert!(
        manager
            .plugin_output("com.bongocat.countdown")
            .unwrap()
            .model
            .nodes
            .iter()
            .any(|node| matches!(node,
                bongocat_plugin_sdk::UiNode::Text { text } if text == "05:00"
            ))
    );
    manager.tick(1250);
    assert!(
        manager
            .plugin_output("com.bongocat.countdown")
            .unwrap()
            .model
            .nodes
            .iter()
            .any(|node| matches!(node,
                bongocat_plugin_sdk::UiNode::Text { text } if text == "04:59"
            ))
    );
    manager.disable_plugin("com.bongocat.countdown").unwrap();
    assert_eq!(manager.list_plugins()[0].1, PluginState::Disabled);
    manager.shutdown();
}
