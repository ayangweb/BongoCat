//! The service drives a real plugin process and publishes what it draws.
//!
//! The package is built the way the packaging tool builds one — the checked-in
//! manifest plus the compiled countdown binary, in one ZIP — and installed
//! through the manager, so this exercises the same lifecycle a download does. A
//! checkout with no built plugin binary skips rather than pretending.

use bongocat_plugin_hud::{HudInteraction, HudViewport};
use bongocat_plugin_service::{PluginService, plugin_directory};
use std::io::Write as _;

#[cfg(target_os = "windows")]
const BINARY_FILE: &str = "bongocat-countdown.exe";
#[cfg(not(target_os = "windows"))]
const BINARY_FILE: &str = "bongocat-countdown";

const PLUGIN_ID: &str = "com.bongocat.countdown";

fn plugin_package() -> Option<Vec<u8>> {
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
    archive
        .write_all(&std::fs::read(root.join("plugin.json")).ok()?)
        .ok()?;
    archive
        .start_file(BINARY_FILE, options.unix_permissions(0o755))
        .ok()?;
    archive.write_all(&std::fs::read(binary).ok()?).ok()?;
    archive.finish().ok().map(|cursor| cursor.into_inner())
}

/// Start a service on a private store and install the countdown package into it.
fn service_with_countdown() -> Option<(PluginService, tempfile::TempDir)> {
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
fn a_plugin_runs_and_keeps_running_without_a_window() {
    let Some((service, store)) = service_with_countdown() else {
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
    assert!(!plugins.join("data").join(PLUGIN_ID).is_dir());

    // The process is still there after a read that touched nothing.
    let again = client.read_center_blocking().unwrap();
    assert!(again.cards[0].enabled);
    service.shutdown().unwrap();
}

#[test]
fn a_click_on_a_control_reaches_the_plugin_that_drew_it() {
    let Some((service, _store)) = service_with_countdown() else {
        return;
    };
    let client = service.client();
    client.set_enabled_blocking(PLUGIN_ID, true).unwrap();
    // The countdown starts at zero, so the first `toggle` arms it at its full
    // length; the plugin's own view is what proves the click arrived.
    client.action_blocking(PLUGIN_ID, "toggle").unwrap();
    client.action_blocking(PLUGIN_ID, "restart").unwrap();
    let center = client.read_center_blocking().unwrap();
    assert!(
        center.cards[0].actions.iter().any(|node| matches!(
            node,
            bongocat_plugin_sdk::UiNode::Text { text } if text == "25:00"
        )),
        "restart must set the countdown to the configured length"
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
    assert!(
        layer
            .target_at(toggle.rect.x + 1.0, toggle.rect.y + 1.0)
            .is_some()
    );

    // A click the window resolved travels to the plugin and comes back as a new
    // frame: the countdown's text moves from its zero state to the full length.
    handles
        .interaction_sender
        .try_send(HudInteraction {
            plugin: PLUGIN_ID.to_owned(),
            id: "restart".to_owned(),
        })
        .unwrap();
    assert!(
        wait_for_text(&client),
        "the click must reach the plugin that drew the control"
    );
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

/// Whether the plugin's own model view comes back carrying the restarted length.
///
/// The view the plugin publishes is the same one the layer is drawn from, so
/// reading it is what proves the click reached the plugin rather than only the
/// queue.
fn wait_for_text(client: &bongocat_plugin_service::PluginServiceClient) -> bool {
    for _ in 0..200 {
        let Ok(center) = client.read_center_blocking() else {
            return false;
        };
        let restarted = center.cards.first().is_some_and(|card| {
            card.actions.iter().any(|node| {
                matches!(node, bongocat_plugin_sdk::UiNode::Text { text } if text == "25:00")
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
    let Some((service, store)) = service_with_countdown() else {
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
