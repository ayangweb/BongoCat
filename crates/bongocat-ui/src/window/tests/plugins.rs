//! The plugin center's commands and its page's own contract.
//!
//! The page's rendering is GPUI's; what is worth a test here is the decision layer
//! above it — which control a row gets, what a press sends, and that a refused press
//! leaves the page showing what the host actually says.

use super::*;
use bongocat_ui_protocol::SettingsPluginIcon;

fn snapshot_with_plugins(plugins: SettingsPlugins) -> SettingsSnapshot {
    let mut snapshot = crate::tests::snapshot(7, true, true);
    snapshot.config_revision = Some(7);
    snapshot.plugins = plugins;
    snapshot
}

fn entry(id: &str, installed: bool, enabled: bool) -> SettingsPluginEntry {
    SettingsPluginEntry {
        id: id.to_string(),
        name: format!("{id} name"),
        description: String::new(),
        author: String::new(),
        icon: SettingsPluginIcon::default(),
        installed_version: installed.then(|| "1.0.0".to_string()),
        available_version: Some("1.0.0".to_string()),
        installed,
        enabled,
        // A test entry has no process, so it is never running — which is the state a
        // card shows before the worker has said otherwise, and the one a test of the
        // command layer should not depend on.
        running: false,
        update_available: false,
        fields: Vec::new(),
        values: Default::default(),
        log: Vec::new(),
        refusal: None,
        failure: None,
    }
}

#[gpui_kit::test]
fn installing_a_plugin_sends_the_typed_command_and_adopts_the_answer(cx: &mut TestAppContext) {
    let (view, visual, endpoint) = settings_view_with_endpoint(cx);
    view.update(visual, |view, _| {
        view.snapshot = Some(snapshot_with_plugins(SettingsPlugins {
            available: true,
            entries: vec![entry("pomodoro", false, false)],
            ..SettingsPlugins::default()
        }));
    });

    view.update(visual, |view, cx| {
        view.install_plugin("pomodoro".to_string(), cx);
    });
    visual.run_until_parked();

    // The command carries its own reply, so one receive is the whole exchange: what
    // the button sent, and the answer the window is waiting for.
    let crate::SettingsCommand::InstallPlugin { plugin, reply } =
        endpoint.try_recv().expect("the install command")
    else {
        panic!("the install button must use the typed install command");
    };
    assert_eq!(plugin, "pomodoro");

    let mut installed = snapshot_with_plugins(SettingsPlugins {
        available: true,
        entries: vec![entry("pomodoro", true, true)],
        ..SettingsPlugins::default()
    });
    installed.revision = 8;
    reply
        .respond(Ok(installed))
        .expect("the window is listening");
    visual.run_until_parked();

    assert!(view.read_with(visual, |view, _| view.plugin_is_enabled("pomodoro")));
}

#[gpui_kit::test]
fn toggling_a_switch_reads_the_current_state_rather_than_the_pressed_one(cx: &mut TestAppContext) {
    let (view, visual, endpoint) = settings_view_with_endpoint(cx);
    view.update(visual, |view, _| {
        view.snapshot = Some(snapshot_with_plugins(SettingsPlugins {
            available: true,
            entries: vec![entry("pomodoro", true, false)],
            ..SettingsPlugins::default()
        }));
    });

    view.update(visual, |view, cx| {
        view.toggle_plugin_enabled("pomodoro".to_string(), cx);
    });
    visual.run_until_parked();

    let crate::SettingsCommand::SetPluginEnabled {
        plugin, enabled, ..
    } = endpoint.try_recv().expect("the switch command")
    else {
        panic!("the switch must use the typed enabled command");
    };
    assert_eq!(plugin, "pomodoro");
    assert!(
        enabled,
        "a switch that was off asks for on, not for what it was"
    );
}

#[gpui_kit::test]
fn a_press_for_a_plugin_the_page_no_longer_lists_is_dropped(cx: &mut TestAppContext) {
    let (view, visual, endpoint) = settings_view_with_endpoint(cx);
    view.update(visual, |view, _| {
        view.snapshot = Some(snapshot_with_plugins(SettingsPlugins {
            available: true,
            entries: vec![entry("pomodoro", true, false)],
            ..SettingsPlugins::default()
        }));
    });

    view.update(visual, |view, cx| {
        view.toggle_plugin_enabled("removed-while-open".to_string(), cx);
    });
    visual.run_until_parked();

    assert!(
        endpoint.try_recv().is_err(),
        "a plugin that is not listed has no state to toggle"
    );
}

#[gpui_kit::test]
fn a_second_plugin_press_while_one_is_in_flight_is_refused(cx: &mut TestAppContext) {
    let (view, visual, endpoint) = settings_view_with_endpoint(cx);
    view.update(visual, |view, _| {
        view.snapshot = Some(snapshot_with_plugins(SettingsPlugins {
            available: true,
            entries: vec![entry("pomodoro", true, false)],
            ..SettingsPlugins::default()
        }));
    });

    view.update(visual, |view, cx| {
        view.install_plugin("pomodoro".to_string(), cx);
    });
    view.update(visual, |view, cx| {
        view.uninstall_plugin("pomodoro".to_string(), cx);
    });
    visual.run_until_parked();

    assert!(
        matches!(
            endpoint.try_recv().expect("only the first press is sent"),
            crate::SettingsCommand::InstallPlugin { .. }
        ),
        "the second press arrives while an install is in flight and is dropped"
    );
    assert!(endpoint.try_recv().is_err());
}

#[gpui_kit::test]
fn a_refused_command_leaves_the_page_on_the_hosts_answer(cx: &mut TestAppContext) {
    let (view, visual, endpoint) = settings_view_with_endpoint(cx);
    view.update(visual, |view, _| {
        view.snapshot = Some(snapshot_with_plugins(SettingsPlugins {
            available: true,
            entries: vec![entry("pomodoro", true, false)],
            ..SettingsPlugins::default()
        }));
    });

    view.update(visual, |view, cx| {
        view.toggle_plugin_enabled("pomodoro".to_string(), cx);
    });
    visual.run_until_parked();
    let crate::SettingsCommand::SetPluginEnabled { reply, .. } =
        endpoint.try_recv().expect("the switch command")
    else {
        panic!("the switch must use the typed enabled command");
    };
    reply
        .respond(Err(SettingsError::new(SettingsErrorCode::PluginHostBusy)))
        .expect("the window is listening");
    visual.run_until_parked();

    assert!(
        !view.read_with(visual, |view, _| view.plugin_is_enabled("pomodoro")),
        "a refused press must not leave the switch where the press left it"
    );
    // The reason is a notification, so it is asserted as a rendered notification:
    // the view field is consumed by the next render, and asserting on it would be
    // asserting on a frame nobody sees.
    visual.update(|window, cx| window.render_frame(cx));
    assert!(
        visual.update(|window, _| window.try_find("notification").is_some()),
        "a refused plugin command must say why"
    );
}
