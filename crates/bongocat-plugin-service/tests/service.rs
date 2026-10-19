//! The service drives a real plugin process and publishes what it draws.
//!
//! The package is built the way the packaging tool builds one — the checked-in
//! manifest, compiled pomodoro binary and package-local icon in one ZIP — and
//! installed through the manager, so this exercises the same lifecycle a
//! download does. A checkout with no built plugin binary skips rather than
//! pretending.

use bongocat_plugin_hud::{HudInteraction, HudViewport};
use bongocat_plugin_service::{PluginService, plugin_directory};
use std::io::Write as _;

#[cfg(target_os = "windows")]
const BINARY_FILE: &str = "bongocat-pomodoro.exe";
#[cfg(not(target_os = "windows"))]
const BINARY_FILE: &str = "bongocat-pomodoro";

const PLUGIN_ID: &str = "com.bongocat.pomodoro";

fn plugin_package() -> Option<Vec<u8>> {
    plugin_package_with_manifest(|_| {})
}

fn plugin_package_with_manifest(edit: impl FnOnce(&mut serde_json::Value)) -> Option<Vec<u8>> {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join("plugins/pomodoro");
    let target = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target");
    let binary = ["debug", "release"]
        .iter()
        .map(|profile| target.join(profile).join(BINARY_FILE))
        .find(|candidate| candidate.is_file())?;
    let mut archive = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
    let options = zip::write::SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated);
    archive.start_file("plugin.json", options).ok()?;
    let mut manifest: serde_json::Value =
        serde_json::from_slice(&std::fs::read(root.join("plugin.json")).ok()?).ok()?;
    edit(&mut manifest);
    archive
        .write_all(&serde_json::to_vec(&manifest).ok()?)
        .ok()?;
    archive.start_file("resources/icon.png", options).ok()?;
    archive
        .write_all(&std::fs::read(root.join("resources/icon.png")).ok()?)
        .ok()?;
    archive
        .start_file(BINARY_FILE, options.unix_permissions(0o755))
        .ok()?;
    archive.write_all(&std::fs::read(binary).ok()?).ok()?;
    archive.finish().ok().map(|cursor| cursor.into_inner())
}

#[test]
fn an_incomplete_translation_falls_back_to_english_for_both_card_and_controls() {
    for localizations in [
        serde_json::json!({}),
        serde_json::json!({"zh-CN": {"name": "番茄钟"}}),
        serde_json::json!({"zh-CN": {"name": " ", "description": "专注计时器"}}),
    ] {
        let Some(bytes) = plugin_package_with_manifest(|manifest| {
            manifest["metadata"]["localizations"] = localizations;
        }) else {
            return;
        };
        let store = tempfile::tempdir().unwrap();
        let (service, _) = PluginService::start(store.path(), 4).unwrap();
        let client = service.client();
        client.set_locale_blocking("zh-CN").unwrap();
        client.install_blocking(bytes).unwrap();
        let center = client.set_enabled_blocking(PLUGIN_ID, true).unwrap();
        let card = &center.cards[0];
        assert_eq!(
            bongocat_plugin_sdk::localized_copy(
                &card.localizations,
                "zh-CN",
                (&card.name, &card.description),
            ),
            ("Pomodoro", "A focus timer you can pause and restart")
        );
        assert!(card.actions.iter().any(|node| matches!(node,
            bongocat_plugin_sdk::UiNode::Button { id, label, .. }
                if id == "toggle" && label == "Start"
        )));
        service.shutdown().unwrap();
    }
}

/// Start a service on a private store and install the pomodoro package into it.
fn service_with_pomodoro() -> Option<(PluginService, tempfile::TempDir)> {
    let bytes = plugin_package()?;
    let store = tempfile::tempdir().ok()?;
    let (service, _handles) = PluginService::start(store.path(), 4).expect("plugin service starts");
    service
        .client()
        .install_blocking(bytes)
        .expect("the package installs");
    Some((service, store))
}

#[test]
fn the_store_is_private_to_the_storage_root_it_was_started_with() {
    assert_eq!(
        plugin_directory(std::path::Path::new("/tmp/example")),
        std::path::Path::new("/tmp/example/plugins")
    );
}

#[test]
fn shutdown_closes_admission_even_when_the_store_is_unavailable() {
    for unavailable in [false, true] {
        let store = tempfile::tempdir().unwrap();
        if unavailable {
            std::fs::write(plugin_directory(store.path()), b"not a directory").unwrap();
        }
        let (service, _) = PluginService::start(store.path(), 4).unwrap();
        let client = service.client();
        assert_eq!(
            client.read_center_blocking().unwrap().error.is_some(),
            unavailable
        );
        client.shutdown_blocking().unwrap();
        assert_eq!(
            client.read_center_blocking(),
            Err(bongocat_plugin_service::PluginCommandError::ServiceUnavailable)
        );
        service.shutdown().unwrap();
    }
}

#[test]
fn shutdown_does_not_report_success_when_session_persistence_fails() {
    let Some((service, store)) = service_with_pomodoro() else {
        return;
    };
    let client = service.client();
    client.set_enabled_blocking(PLUGIN_ID, true).unwrap();
    let session = plugin_directory(store.path())
        .join("data")
        .join(PLUGIN_ID)
        .join("session.json");
    std::fs::remove_file(&session).unwrap();
    std::fs::create_dir(&session).unwrap();
    assert!(client.shutdown_blocking().is_err());
    assert!(matches!(
        service.shutdown(),
        Err(bongocat_plugin_service::PluginServiceStartError::Shutdown(
            _
        ))
    ));
}

#[test]
fn a_plugin_runs_and_keeps_running_without_a_window() {
    let Some((service, store)) = service_with_pomodoro() else {
        return;
    };
    let client = service.client();
    let center = client.read_center_blocking().unwrap();
    assert_eq!(center.cards.len(), 1);
    assert_eq!(center.cards[0].id, PLUGIN_ID);
    assert_eq!(center.cards[0].version.as_deref(), Some("1.0.0"));
    assert!(!center.cards[0].enabled);

    let enabled = client.set_enabled_blocking(PLUGIN_ID, true).unwrap();
    assert!(enabled.cards[0].enabled);
    assert!(enabled.cards[0].actions.iter().any(
        |node| matches!(node, bongocat_plugin_sdk::UiNode::Button { id, .. } if id == "toggle")
    ));

    // The package tree is the store's, and the data root is separate from it.
    let plugins = plugin_directory(store.path());
    assert!(plugins.join("packages").join(PLUGIN_ID).is_dir());
    assert!(
        plugins
            .join("data")
            .join(PLUGIN_ID)
            .join("session.json")
            .is_file()
    );

    // The process is still there after a read that touched nothing.
    let again = client.read_center_blocking().unwrap();
    assert!(again.cards[0].enabled);
    service.shutdown().unwrap();
}

#[test]
fn a_click_on_a_control_reaches_the_plugin_that_drew_it() {
    let Some((service, _store)) = service_with_pomodoro() else {
        return;
    };
    let client = service.client();
    client.set_enabled_blocking(PLUGIN_ID, true).unwrap();
    // The pomodoro timer starts at zero, so the first `toggle` arms it at its full
    // length; the plugin's own view is what proves the click arrived.
    client.action_blocking(PLUGIN_ID, "toggle").unwrap();
    client.action_blocking(PLUGIN_ID, "restart").unwrap();
    let center = client.read_center_blocking().unwrap();
    assert!(
        center.cards[0].actions.iter().any(|node| matches!(
            node,
            bongocat_plugin_sdk::UiNode::Text { text } if text == "25:00"
        )),
        "restart must set the pomodoro timer to the configured length"
    );
    service.shutdown().unwrap();
}

#[test]
fn the_composed_layer_matches_the_window_that_reported_itself() {
    let Some(bytes) = plugin_package() else {
        return;
    };
    let store = tempfile::tempdir().unwrap();
    let (service, handles) = PluginService::start(store.path(), 4).unwrap();
    let client = service.client();
    client.install_blocking(bytes).unwrap();
    client.set_enabled_blocking(PLUGIN_ID, true).unwrap();

    // Before a window reports a size there is nothing to compose against.
    assert!(
        handles
            .hud_consumer
            .take_latest()
            .is_none_or(|layer| layer.is_empty())
    );
    handles
        .viewport_reporter
        .publish(HudViewport::new(360, 360, 2.0));

    let layer = wait_for_layer(&handles.hud_consumer);
    assert!(!layer.is_empty(), "a running plugin must draw a panel");
    assert_eq!(layer.window_width, 360);
    assert_eq!(layer.window_height, 360);
    let toggle = layer
        .targets
        .iter()
        .find(|target| target.id == "toggle")
        .expect("the toggle the plugin published is a target");
    assert_eq!(toggle.plugin, PLUGIN_ID);
    // The rectangle is in the window's device pixels, which is the space a
    // native window reports a pointer position in, so a click at the control's
    // own centre resolves to it.
    assert!(
        layer
            .target_at(
                toggle.rect.x + toggle.rect.width / 2.0,
                toggle.rect.y + toggle.rect.height / 2.0
            )
            .is_some_and(|target| target.id == "toggle")
    );
    assert!(
        toggle.rect.right() <= layer.window_width as f32
            && toggle.rect.bottom() <= layer.window_height as f32,
        "a control sits inside the window that draws it"
    );

    // A click the window resolved travels to the plugin and comes back as a new
    // frame. The panel already previews the full length, so what proves the
    // click landed is that the timer starts counting down from it.
    handles
        .interaction_sender
        .try_send(HudInteraction {
            plugin: PLUGIN_ID.to_owned(),
            id: "restart".to_owned(),
        })
        .unwrap();
    assert!(
        wait_for_text(
            &client,
            &[
                "24:59", "24:58", "24:57", "24:56", "24:55", "24:54", "24:53"
            ],
        ),
        "the click must reach the plugin that drew the control"
    );
    service.shutdown().unwrap();
}

#[test]
fn a_panel_that_is_not_changing_is_not_republished() {
    let Some(bytes) = plugin_package() else {
        return;
    };
    let store = tempfile::tempdir().unwrap();
    let (service, handles) = PluginService::start(store.path(), 4).unwrap();
    let client = service.client();
    client.install_blocking(bytes).unwrap();
    client.set_enabled_blocking(PLUGIN_ID, true).unwrap();
    handles
        .viewport_reporter
        .publish(HudViewport::new(360, 360, 2.0));

    // The panel is published once and then left alone: this is the contract the
    // model window's layer handling depends on, because a window that drew a
    // panel only when one arrived would show a panel for one frame and then a
    // hole — the flash a user sees as a panel that disappears.
    let first = wait_for_layer(&handles.hud_consumer);
    assert!(!first.is_empty());
    for _ in 0..100 {
        assert!(
            handles.hud_consumer.take_latest().is_none(),
            "an idle panel must not be republished"
        );
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    service.shutdown().unwrap();
}

/// The next non-empty layer, within a bounded wait.
fn wait_for_layer(consumer: &bongocat_plugin_hud::HudConsumer) -> bongocat_plugin_hud::HudLayer {
    for _ in 0..200 {
        if let Some(layer) = consumer.take_latest()
            && !layer.is_empty()
        {
            return layer;
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    panic!("no plugin layer was published");
}

/// Whether the plugin's own model view comes back carrying one of `expected`.
///
/// The view the plugin publishes is the same one the layer is drawn from, so
/// reading it is what proves the click reached the plugin rather than only the
/// queue.
fn wait_for_text(client: &bongocat_plugin_service::PluginServiceClient, expected: &[&str]) -> bool {
    for _ in 0..200 {
        let Ok(center) = client.read_center_blocking() else {
            return false;
        };
        let restarted = center.cards.first().is_some_and(|card| {
            card.actions.iter().any(|node| match node {
                bongocat_plugin_sdk::UiNode::Text { text } => expected.contains(&text.as_str()),
                _ => false,
            })
        });
        if restarted {
            return true;
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    false
}

#[test]
fn uninstalling_removes_the_package_and_keeps_the_data() {
    let Some((service, store)) = service_with_pomodoro() else {
        return;
    };
    let client = service.client();
    let data = plugin_directory(store.path()).join("data").join(PLUGIN_ID);
    std::fs::create_dir_all(&data).unwrap();
    std::fs::write(data.join("state.json"), b"{}").unwrap();

    let after = client.uninstall_blocking(PLUGIN_ID).unwrap();
    assert!(after.cards.is_empty());
    assert!(data.join("state.json").is_file());

    client.reset_data_blocking(PLUGIN_ID).unwrap();
    assert!(!data.exists());
    service.shutdown().unwrap();
}

#[test]
fn an_unknown_plugin_is_refused_by_its_own_code() {
    let store = tempfile::tempdir().unwrap();
    let (service, _handles) = PluginService::start(store.path(), 4).unwrap();
    let error = service
        .client()
        .set_enabled_blocking("dev.example.missing", true)
        .unwrap_err();
    assert_eq!(error, bongocat_plugin_service::PluginCommandError::NotFound);
    service.shutdown().unwrap();
}

fn settled(
    client: &bongocat_plugin_service::PluginServiceClient,
) -> bongocat_plugin_service::PluginCenterSnapshot {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    loop {
        let center = client.read_center_blocking().unwrap();
        if !center.loading {
            return center;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "catalog job must finish"
        );
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
}

fn versioned_package(bytes: &[u8], version: &str, broken: bool) -> Vec<u8> {
    use std::io::Read;
    let mut input = zip::ZipArchive::new(std::io::Cursor::new(bytes)).unwrap();
    let mut output = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
    for index in 0..input.len() {
        let mut entry = input.by_index(index).unwrap();
        let mut content = Vec::new();
        entry.read_to_end(&mut content).unwrap();
        if entry.name().unwrap() == "plugin.json" {
            let mut manifest: serde_json::Value = serde_json::from_slice(&content).unwrap();
            manifest["metadata"]["version"] = version.into();
            content = serde_json::to_vec(&manifest).unwrap();
        } else if broken {
            content = b"not an executable".to_vec();
        }
        output
            .start_file(
                entry.name().unwrap(),
                zip::write::SimpleFileOptions::default()
                    .unix_permissions(entry.unix_mode().unwrap_or(0o644)),
            )
            .unwrap();
        output.write_all(&content).unwrap();
    }
    output.finish().unwrap().into_inner()
}

#[test]
fn a_damaged_installed_session_keeps_catalog_backed_recovery_controls() {
    let Some(bytes) = plugin_package() else {
        return;
    };
    let catalog = tempfile::tempdir().unwrap();
    let store = tempfile::tempdir().unwrap();
    std::fs::write(catalog.path().join("pomodoro.zip"), &bytes).unwrap();
    let source = bongocat_plugin_service::PluginSource::Development(catalog.path().to_owned());
    let (service, _) =
        PluginService::start_with_source(store.path(), 4, source.clone(), None).unwrap();
    let client = service.client();
    settled(&client);
    client.install_blocking(bytes).unwrap();
    client.set_enabled_blocking(PLUGIN_ID, true).unwrap();
    service.shutdown().unwrap();
    std::fs::write(
        plugin_directory(store.path())
            .join("data")
            .join(PLUGIN_ID)
            .join("session.json"),
        b"not JSON",
    )
    .unwrap();
    let (service, _) = PluginService::start_with_source(store.path(), 4, source, None).unwrap();
    let client = service.client();
    let center = settled(&client);
    assert_eq!(center.cards.len(), 1);
    assert!(center.cards[0].failed);
    assert_eq!(center.cards[0].version.as_deref(), Some("1.0.0"));
    assert!(!center.cards[0].enabled);
    let reset = client.reset_data_blocking(PLUGIN_ID).unwrap();
    assert!(!reset.cards[0].failed);
    assert!(client.set_enabled_blocking(PLUGIN_ID, true).unwrap().cards[0].enabled);
    service.shutdown().unwrap();
}

#[test]
fn local_catalog_install_update_rollback_uninstall_and_restart_share_one_lifecycle() {
    let bytes =
        plugin_package().expect("build Pomodoro with just dev-plugins before lifecycle tests");
    let catalog = tempfile::tempdir().unwrap();
    let store = tempfile::tempdir().unwrap();
    std::fs::write(catalog.path().join("pomodoro.zip"), &bytes).unwrap();
    let source = bongocat_plugin_service::PluginSource::Development(catalog.path().to_owned());
    let (service, _) =
        PluginService::start_with_source(store.path(), 4, source.clone(), None).unwrap();
    let client = service.client();
    let available = settled(&client);
    assert!(available.error.is_none());
    assert_eq!(available.cards.len(), 1);
    assert!(
        available.cards[0].version.is_none(),
        "discovery must not install"
    );
    client.install_available_blocking(PLUGIN_ID).unwrap();
    assert_eq!(settled(&client).cards[0].version.as_deref(), Some("1.0.0"));
    client.set_enabled_blocking(PLUGIN_ID, true).unwrap();
    client
        .configure_blocking(
            PLUGIN_ID,
            "minutes",
            bongocat_plugin_sdk::ConfigValue::Integer(5),
        )
        .unwrap();

    std::fs::write(
        catalog.path().join("pomodoro.zip"),
        versioned_package(&bytes, "1.1.0", false),
    )
    .unwrap();
    client.refresh_blocking().unwrap();
    let update = settled(&client);
    assert!(update.cards[0].update_available);
    assert_eq!(update.cards[0].available_version.as_deref(), Some("1.1.0"));
    client.install_available_blocking(PLUGIN_ID).unwrap();
    let updated = settled(&client);
    assert!(updated.error.is_none(), "{:?}", updated.error);
    assert_eq!(updated.cards[0].version.as_deref(), Some("1.1.0"));
    assert!(updated.cards[0].enabled);
    assert!(
        updated.cards[0]
            .settings
            .iter()
            .any(|node| matches!(node, bongocat_plugin_sdk::UiNode::Integer { value: 5, .. }))
    );

    std::fs::write(
        catalog.path().join("pomodoro.zip"),
        versioned_package(&bytes, "1.2.0", true),
    )
    .unwrap();
    client.refresh_blocking().unwrap();
    settled(&client);
    client.install_available_blocking(PLUGIN_ID).unwrap();
    let refused = settled(&client);
    assert!(refused.error.is_some());
    assert_eq!(refused.cards[0].version.as_deref(), Some("1.1.0"));
    assert!(
        refused.cards[0].enabled,
        "failed startup must restore the working process"
    );

    service.shutdown().unwrap();
    let (service, _) =
        PluginService::start_with_source(store.path(), 4, source.clone(), None).unwrap();
    let client = service.client();
    let restarted = settled(&client);
    assert!(
        restarted.cards[0].enabled,
        "enabled state must survive application shutdown"
    );
    client.uninstall_blocking(PLUGIN_ID).unwrap();
    let removed = settled(&client);
    assert_eq!(
        removed.cards.len(),
        1,
        "uninstalled plugins stay available in the catalog"
    );
    assert!(removed.cards[0].version.is_none());
    assert!(
        plugin_directory(store.path())
            .join("data")
            .join(PLUGIN_ID)
            .join("session.json")
            .is_file()
    );
    service.shutdown().unwrap();

    let (service, _) = PluginService::start_with_source(store.path(), 4, source, None).unwrap();
    assert!(
        settled(&service.client()).cards[0].version.is_none(),
        "startup must not undo uninstall"
    );
    service.shutdown().unwrap();
}
