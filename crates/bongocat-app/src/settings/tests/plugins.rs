//! Plugin language follows committed application settings without a UI window.

use super::*;
use bongocat_plugin_service::{PluginService, PluginServiceClient};
use std::io::Write as _;

const PLUGIN_ID: &str = "com.bongocat.pomodoro";

fn pomodoro_package() -> Option<Vec<u8>> {
    let workspace = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let binary = ["debug", "release"]
        .into_iter()
        .map(|profile| {
            workspace
                .join("target")
                .join(profile)
                .join(format!("bongocat-pomodoro{}", std::env::consts::EXE_SUFFIX))
        })
        .find(|path| path.is_file())?;
    let mut zip = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
    let options = zip::write::SimpleFileOptions::default().unix_permissions(0o755);
    zip.start_file("plugin.json", options).unwrap();
    zip.write_all(&fs::read(workspace.join("plugins/pomodoro/plugin.json")).unwrap())
        .unwrap();
    zip.start_file("resources/icon.png", options).unwrap();
    zip.write_all(&fs::read(workspace.join("plugins/pomodoro/resources/icon.png")).unwrap())
        .unwrap();
    zip.start_file(binary.file_name().unwrap().to_str().unwrap(), options)
        .unwrap();
    zip.write_all(&fs::read(binary).unwrap()).unwrap();
    Some(zip.finish().unwrap().into_inner())
}

fn assert_start_label(client: &PluginServiceClient, expected: &str) {
    let center = client.read_center_blocking().unwrap();
    let actions = serde_json::to_value(&center.cards[0].actions).unwrap();
    let button = actions
        .as_array()
        .unwrap()
        .iter()
        .find(|node| node["id"] == "toggle")
        .expect("timer button");
    assert_eq!(button["label"], expected);
}

#[test]
fn plugins_follow_application_language_before_windows_open_and_after_restart() {
    let Some(package) = pomodoro_package() else {
        return;
    };
    let base = tempdir().unwrap();
    let layout = StorageLayout::under(base.path(), crate::BUILD_ENVIRONMENT);
    let mut application = Application::start_with_layout(layout.clone()).unwrap();
    application
        .set_language(bongocat_config::Language::ChineseSimplified)
        .unwrap();
    let plugin_store = tempdir().unwrap();
    let (plugins, _) = PluginService::start(plugin_store.path(), 4).unwrap();
    let plugin_client = plugins.client();
    application.attach_plugin_service(plugin_client.clone());
    plugin_client.install_blocking(package).unwrap();
    plugin_client.set_enabled_blocking(PLUGIN_ID, true).unwrap();
    assert_start_label(&plugin_client, "开始");

    let settings = ApplicationSettingsService::start(application).unwrap();
    let client = settings.client();
    let initial = client.read_snapshot_blocking().unwrap();
    let mut current = initial.clone();
    let mut stale_revision = None;
    for (language, label) in [
        (SettingsLanguage::English, "Start"),
        (SettingsLanguage::ChineseTraditional, "Start"),
        (SettingsLanguage::Arabic, "Start"),
        (SettingsLanguage::Vietnamese, "Start"),
        (SettingsLanguage::Portuguese, "Start"),
        (SettingsLanguage::Korean, "Start"),
        (SettingsLanguage::French, "Start"),
        (SettingsLanguage::System, "Start"),
        (SettingsLanguage::ChineseSimplified, "开始"),
    ] {
        current = client
            .set_language_blocking(current.config_revision.unwrap(), language)
            .unwrap();
        if language == SettingsLanguage::English {
            stale_revision = current.config_revision;
        }
        assert_start_label(&plugin_client, label);
    }
    assert_eq!(
        client
            .set_language_blocking(stale_revision.unwrap(), SettingsLanguage::English)
            .unwrap_err()
            .code(),
        SettingsErrorCode::SnapshotOutdated
    );
    assert_start_label(&plugin_client, "开始");
    let occupied = layout.config.with_extension("json.tmp");
    fs::create_dir(&occupied).unwrap();
    assert_eq!(
        client
            .set_language_blocking(current.config_revision.unwrap(), SettingsLanguage::English)
            .unwrap_err()
            .code(),
        SettingsErrorCode::ConfigTargetOccupied
    );
    fs::remove_dir(occupied).unwrap();
    assert_eq!(
        client.read_snapshot_blocking().unwrap().language,
        current.language
    );
    assert_start_label(&plugin_client, "开始");
    plugin_client
        .set_enabled_blocking(PLUGIN_ID, false)
        .unwrap();
    plugin_client.set_enabled_blocking(PLUGIN_ID, true).unwrap();
    assert_start_label(&plugin_client, "开始");
    client.shutdown_blocking().unwrap();
    settings.join().unwrap();
    plugins.shutdown().unwrap();

    let (plugins, _) = PluginService::start(plugin_store.path(), 4).unwrap();
    let mut application = Application::start_with_layout(layout).unwrap();
    application.attach_plugin_service(plugins.client());
    assert_start_label(&plugins.client(), "开始");
    application.shutdown().unwrap();
    plugins.shutdown().unwrap();
}
