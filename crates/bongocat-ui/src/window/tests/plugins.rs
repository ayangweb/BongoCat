//! The plugin center's commands, its grid and its page's own contract.
//!
//! The page's painting is GPUI's; what is worth a test here is the decision layer
//! above it — which control a card gets, what a press sends, that a refused press
//! leaves the page showing what the host actually says, and that the cards are laid
//! out beside each other rather than as sections of the page.

use super::*;
use bongocat_ui_protocol::SettingsPluginIcon;

fn snapshot_with_plugins(plugins: SettingsPlugins) -> SettingsSnapshot {
    let mut snapshot = crate::tests::snapshot(7, true, true);
    snapshot.config_revision = Some(7);
    snapshot.plugins = plugins;
    snapshot
}

/// An installed plugin that declares one setting, as the running host reports it.
///
/// A plugin with no fields has no panel to open, so a test of the panel needs a
/// plugin that declared one.
fn entry_with_fields(id: &str, installed: bool, enabled: bool) -> SettingsPluginEntry {
    let mut entry = entry(id, installed, enabled);
    let field = SettingsPluginField {
        key: "minutes".to_string(),
        label: "Minutes".to_string(),
        description: None,
        kind: SettingsFieldKind::Integer,
        default: SettingsFieldValue::Integer(25),
        minimum: Some(1.0),
        maximum: Some(120.0),
        step: Some(5.0),
        unit: None,
        placeholder: None,
        multiline: false,
        options: Vec::new(),
    };
    entry.fields = vec![field.clone()];
    entry.values = BTreeMap::from([(field.key, field.default)]);
    entry
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

/// A plugin with settings opens them from its own card.
///
/// The panel is a *sibling* of the grid rather than a section of one card, because
/// `gpui-kit` renders a field row only inside a setting group. That is a layout
/// decision with a cost — the form is below the grid, not on the card that owns it —
/// so the two halves of it are pinned here: the card's control opens the panel, and
/// the panel's own close control puts it away again.
#[gpui_kit::test]
fn a_cards_settings_control_opens_a_panel_that_closes_again(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);
    let (client, _endpoint) = crate::SettingsClient::bounded(4);
    let seeded = snapshot_with_plugins(SettingsPlugins {
        available: true,
        catalog_read: true,
        entries: vec![entry_with_fields("pomodoro", true, true)],
        ..SettingsPlugins::default()
    });

    // The page is rebuilt from the view on each press, because that is what the
    // window does: the card's control calls into the view and the next frame draws
    // from whatever the view then holds. A harness holding one frozen snapshot would
    // pass without ever proving the panel follows the view.
    let built: Rc<RefCell<Option<Entity<SettingsView>>>> = Rc::new(RefCell::new(None));
    let capture = Rc::clone(&built);
    let (_, visual) = cx.add_window_view(move |window, cx| {
        let view = cx.new(|cx| {
            SettingsView::new(
                client,
                SettingsWindowSeed {
                    language: SettingsLanguage::English,
                    appearance_theme: SettingsTheme::System,
                },
                Rc::new(|_| {}),
                Rc::new(|_| {}),
                window,
                cx,
            )
        });
        capture.borrow_mut().replace(view.clone());
        let page = cx.new(|_| PluginsPageHarness { view });
        Root::new(page, window, cx)
    });
    let view = built
        .borrow_mut()
        .take()
        .expect("the window builder must hand the page out");
    view.update(visual, |view, _| view.snapshot = Some(seeded.clone()));
    visual.update(|window, cx| window.render_frame(cx));

    let configure = ElementId::from("plugin-configure-pomodoro");
    let header = ElementId::from("plugin-settings-header-pomodoro");
    let close = ElementId::from("plugin-settings-close-pomodoro");

    visual.update(|window, cx| window.click(configure.clone(), cx));
    assert!(
        view.read_with(visual, |view, _| view.plugin_settings_are_open("pomodoro")),
        "the card's settings control opens the plugin's own form"
    );
    visual.update(|window, cx| window.render_frame(cx));
    assert!(
        visual.update(|window, _| window.try_find(header.clone()).is_some()),
        "and the form names the plugin it belongs to, in the panel's own header"
    );
    assert!(
        visual.update(|window, _| window.try_find(close.clone()).is_some()),
        "with its own control to put it away again"
    );

    visual.update(|window, cx| window.click(close, cx));
    assert!(
        !view.read_with(visual, |view, _| view.plugin_settings_are_open("pomodoro")),
        "and the panel's own control puts it away"
    );
    visual.update(|window, cx| window.render_frame(cx));
    assert!(
        visual.update(|window, _| window.try_find(header).is_none()),
        "so the page is the grid again, with no empty section left behind"
    );
}

/// The cards are a grid, not a column of sections.
///
/// This is the page's shape, and it is what `gpui-kit` cannot express for us: the
/// settings component renders every titled group of a multi-group page as a
/// second-level sidebar entry, so one group per plugin made the Plugins destination
/// a menu of plugin names with the cards hidden behind it. The page therefore owns
/// one untitled group and lays the cards out itself, and this asserts the observable
/// half of that — several cards share a row, and a card that has no room for a
/// sentence still holds its controls at the same height as its neighbours'.
#[gpui_kit::test]
fn the_plugin_cards_are_laid_out_beside_each_other(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);
    let (client, _endpoint) = crate::SettingsClient::bounded(4);
    let seeded = snapshot_with_plugins(SettingsPlugins {
        available: true,
        catalog_read: true,
        // The first row mixes an installed plugin with one that is not, so the row's
        // two cells have genuinely different controls to lay out.
        entries: vec![
            entry("pomodoro", true, true),
            entry("typing-sound", false, false),
            entry("key-display", true, false),
            entry("key-stats", false, false),
            entry("input-method", false, false),
            entry("ai-watch", false, false),
        ],
        ..SettingsPlugins::default()
    });

    let built: Rc<RefCell<Option<Entity<SettingsView>>>> = Rc::new(RefCell::new(None));
    let capture = Rc::clone(&built);
    let (_, visual) = cx.add_window_view(move |window, cx| {
        let view = cx.new(|cx| {
            SettingsView::new(
                client,
                SettingsWindowSeed {
                    language: SettingsLanguage::English,
                    appearance_theme: SettingsTheme::System,
                },
                Rc::new(|_| {}),
                Rc::new(|_| {}),
                window,
                cx,
            )
        });
        capture.borrow_mut().replace(view.clone());
        view.update(cx, |view, _| view.snapshot = Some(seeded.clone()));
        let page = cx.new(|_| PluginsPageHarness { view });
        Root::new(page, window, cx)
    });
    assert!(
        built.borrow_mut().take().is_some(),
        "the window builder must hand the page out"
    );
    visual.update(|window, cx| window.render_frame(cx));

    // The cards are measured through the same group box and page padding the product
    // draws them inside, so a grid that only works on its own would fail here.
    let width = visual.update(|window, _| {
        window
            .try_find("plugin-grid")
            .expect("the grid is drawn")
            .bounds()
            .size
            .width
    });
    assert!(
        width > px(0.0),
        "the grid must be measured at a real width, or the rows below prove nothing"
    );

    // How many cards share a row is the window's width decision, so it is read back
    // from the same function the page used rather than assumed.
    let columns = super::super::plugins::plugin_grid_columns_for_window(width);
    let first = rendered_bounds(visual, ElementId::from(("plugin-card", 0usize)));
    let second = rendered_bounds(visual, ElementId::from(("plugin-card", 1usize)));
    assert_eq!(
        first.origin.y, second.origin.y,
        "two cards must share the first row"
    );
    assert!(
        second.origin.x > first.origin.x,
        "and the second must be to the right of the first, not below it — a column of cards is \
         the section-per-plugin page this stopped being"
    );

    if columns > 2 {
        let third = rendered_bounds(visual, ElementId::from(("plugin-card", 2usize)));
        assert_eq!(
            third.origin.y, first.origin.y,
            "the third card belongs to the same row when the window is wide enough for one"
        );
    }
    let next = rendered_bounds(visual, ElementId::from(("plugin-card", columns)));
    assert!(
        next.origin.y > first.origin.y,
        "and the cards continue below the first row rather than running off to one side"
    );

    // A card whose plugin is not installed has one full-width button where an
    // installed card has a switch and a row of icons. A row is as tall as its
    // tallest cell, so the two must come out the same height — otherwise the card
    // with less to say ends up with its single button floating in the middle of a
    // cell sized for someone else's controls.
    let installed = rendered_bounds(visual, ElementId::from(("plugin-card", 0usize)));
    let uninstalled = rendered_bounds(visual, ElementId::from(("plugin-card", 1usize)));
    assert!(
        (installed.size.height - uninstalled.size.height).abs() < px(1.0),
        "two cards in one row must be as tall as each other: {} against {}",
        installed.size.height,
        uninstalled.size.height
    );
    assert!(
        uninstalled.size.height > px(0.0),
        "and a card with one button is still a card, not an empty box"
    );
}
