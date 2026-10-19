//! The manager drives a real plugin binary discovered from a directory.
use bongocat_plugin_manager::{PluginManager, PluginState};
use std::io::Write as _;

#[cfg(target_os = "windows")]
const BINARY_FILE: &str = "bongocat-pomodoro.exe";
#[cfg(not(target_os = "windows"))]
const BINARY_FILE: &str = "bongocat-pomodoro";

/// The compiled pomodoro package. The test skips rather than pretending when
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

/// Build the package the way the packaging tool does: one ZIP with the manifest,
/// executable and the package-local icon, installed through the manager.
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
        .start_file("resources/icon.png", options)
        .expect("start icon");
    archive
        .write_all(&std::fs::read(source.join("resources/icon.png")).unwrap())
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

    manager.enable_plugin("com.bongocat.pomodoro").unwrap();
    assert_eq!(manager.list_plugins()[0].1, PluginState::Enabled);
    let started = manager
        .plugin_output("com.bongocat.pomodoro")
        .unwrap()
        .clone();
    assert!(started.model.nodes.iter().any(|node| matches!(
        node,
        bongocat_plugin_sdk::UiNode::Button { id, .. } if id == "toggle"
    )));

    assert!(
        manager
            .configure_plugin(
                "com.bongocat.pomodoro",
                [(
                    "minutes".into(),
                    bongocat_plugin_sdk::ConfigValue::Integer(0)
                )]
                .into()
            )
            .is_err()
    );
    assert!(
        manager
            .action_plugin("com.bongocat.pomodoro", "unknown")
            .is_err()
    );
    assert_eq!(manager.list_plugins()[0].1, PluginState::Enabled);

    manager
        .configure_plugin(
            "com.bongocat.pomodoro",
            [(
                "minutes".into(),
                bongocat_plugin_sdk::ConfigValue::Integer(5),
            )]
            .into(),
        )
        .unwrap();
    manager.tick(250);
    manager
        .action_plugin("com.bongocat.pomodoro", "restart")
        .unwrap();
    assert!(
        manager
            .plugin_output("com.bongocat.pomodoro")
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
            .plugin_output("com.bongocat.pomodoro")
            .unwrap()
            .model
            .nodes
            .iter()
            .any(|node| matches!(node,
                bongocat_plugin_sdk::UiNode::Text { text } if text == "04:59"
            ))
    );
    manager.disable_plugin("com.bongocat.pomodoro").unwrap();
    assert_eq!(manager.list_plugins()[0].1, PluginState::Disabled);
    assert!(
        manager
            .configure_plugin("com.bongocat.pomodoro", Default::default())
            .is_err()
    );
    manager.shutdown().unwrap();
}

#[test]
fn persistence_failure_stops_uncommitted_configuration_and_can_be_retried() {
    let (Some(source), Some(binary)) = (source(), target_binary()) else {
        return;
    };
    let directory = tempfile::tempdir().unwrap();
    install(&source, &binary, directory.path());
    let id = "com.bongocat.pomodoro";
    let mut manager = PluginManager::new(directory.path().to_path_buf()).unwrap();
    manager.discover_plugins().unwrap();
    manager.enable_plugin(id).unwrap();
    let file = manager.data_root(id, false).unwrap().join("session.json");
    let saved = std::fs::read(&file).unwrap();
    std::fs::remove_file(&file).unwrap();
    std::fs::create_dir(&file).unwrap();
    assert!(
        manager
            .configure_plugin(
                id,
                [(
                    "minutes".into(),
                    bongocat_plugin_sdk::ConfigValue::Integer(5)
                )]
                .into()
            )
            .is_err()
    );
    assert_eq!(manager.list_plugins()[0].1, PluginState::Failed);
    assert_eq!(
        manager.plugin_output(id).unwrap().config["minutes"],
        bongocat_plugin_sdk::ConfigValue::Integer(25)
    );
    assert!(manager.action_plugin(id, "toggle").is_err());
    std::fs::remove_dir(&file).unwrap();
    std::fs::write(&file, saved).unwrap();
    manager.enable_plugin(id).unwrap();
    assert_eq!(
        manager.plugin_output(id).unwrap().config["minutes"],
        bongocat_plugin_sdk::ConfigValue::Integer(25)
    );
    let saved = std::fs::read(&file).unwrap();
    std::fs::remove_file(&file).unwrap();
    std::fs::create_dir(&file).unwrap();
    assert!(manager.configure_plugin(id, Default::default()).is_err());
    std::fs::remove_dir(&file).unwrap();
    std::fs::write(&file, saved).unwrap();
    // A failed entry still carries its saved enable request. Uninstall must
    // clear that request before retaining the session for a later reinstall.
    manager.uninstall(id).unwrap();
    install(&source, &binary, directory.path());
    let mut reopened = PluginManager::new(directory.path().to_path_buf()).unwrap();
    reopened.discover_plugins().unwrap();
    reopened.resume_enabled().unwrap();
    assert_eq!(reopened.list_plugins()[0].1, PluginState::Discovered);
    reopened.shutdown().unwrap();
    manager.shutdown().unwrap();
}

#[test]
fn failed_start_does_not_mark_the_plugin_enabled_or_commit_output() {
    let (Some(source), Some(binary)) = (source(), target_binary()) else {
        return;
    };
    let directory = tempfile::tempdir().unwrap();
    install(&source, &binary, directory.path());
    let id = "com.bongocat.pomodoro";
    let mut manager = PluginManager::new(directory.path().to_path_buf()).unwrap();
    manager.discover_plugins().unwrap();
    let file = manager.data_root(id, true).unwrap().join("session.json");
    std::fs::create_dir(&file).unwrap();
    assert!(manager.enable_plugin(id).is_err());
    assert_eq!(manager.list_plugins()[0].1, PluginState::Failed);
    assert!(manager.plugin_output(id).unwrap().config.is_empty());
    assert!(manager.action_plugin(id, "toggle").is_err());
    std::fs::remove_dir(&file).unwrap();
    manager.enable_plugin(id).unwrap();
    assert_eq!(manager.list_plugins()[0].1, PluginState::Enabled);
    manager.shutdown().unwrap();
}
