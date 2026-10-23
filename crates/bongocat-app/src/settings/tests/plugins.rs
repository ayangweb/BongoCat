//! Plugin language follows committed application settings without a UI window.

use super::*;
use bongocat_plugin_service::{PluginService, PluginServiceClient};
use std::io::Write as _;

const PLUGIN_ID: &str = "com.bongocat.pomodoro";

fn pomodoro_package() -> Option<Vec<u8>> {
    let workspace = std::path::PathBuf::from(std::env::var_os("BONGOCAT_PLUGIN_TEST_ROOT")?);
    let triple = host_target_triple();
    let file = format!("bongocat-pomodoro{}", std::env::consts::EXE_SUFFIX);
    let binary = [
        workspace
            .join("target")
            .join(triple)
            .join("release")
            .join(&file),
        workspace
            .join("target")
            .join(triple)
            .join("debug")
            .join(&file),
        workspace.join("target/release").join(&file),
        workspace.join("target/debug").join(&file),
    ]
    .into_iter()
    .find(|path| path.is_file())
    .or_else(|| panic!("BONGOCAT_PLUGIN_TEST_ROOT is set but {file} was not built"))?;
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

fn host_target_triple() -> &'static str {
    #[cfg(target_os = "macos")]
    {
        return if cfg!(target_arch = "aarch64") {
            "aarch64-apple-darwin"
        } else {
            "x86_64-apple-darwin"
        };
    }
    #[cfg(target_os = "windows")]
    {
        return "x86_64-pc-windows-msvc";
    }
    #[allow(unreachable_code)]
    ""
}

/// Install the package into a service store the way the product does: through
/// the manager, against the digest its catalog entry advertises.
fn install_into(store: &std::path::Path, bytes: &[u8]) {
    let mut manager = bongocat_plugin_manager::PluginManager::new(
        bongocat_plugin_service::plugin_directory(store),
    )
    .unwrap();
    manager
        .install(bytes, &bongocat_plugin_manager::sha256_of(bytes))
        .unwrap();
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
    install_into(plugin_store.path(), &package);
    let (plugins, _) = PluginService::start(plugin_store.path(), 4).unwrap();
    let plugin_client = plugins.client();
    application.attach_plugin_service(plugin_client.clone());
    plugin_client.set_enabled_blocking(PLUGIN_ID, true).unwrap();
    assert_start_label(&plugin_client, "开始");

    let settings = ApplicationSettingsService::start(application).unwrap();
    let client = settings.client();
    let initial = client.read_snapshot_blocking().unwrap();
    let mut current = initial.clone();
    let mut stale_revision = None;
    // Plugins serve Simplified Chinese and English only, so every other
    // application language reads English.
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
