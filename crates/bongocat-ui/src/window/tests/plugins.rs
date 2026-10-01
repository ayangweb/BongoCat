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
    // A schema only reaches the window through a running process's handshake, so a
    // fixture with fields but no running plugin is a state the projection cannot
    // produce. Set both, or the card's button reads as the one it must not.
    entry.running = entry.enabled;
    entry.settings_available = entry.running;
    entry
}

/// A window showing the plugin page over this snapshot, and the view behind it.
///
/// Takes the context rather than making one, so a test can share a harness shape across
/// several cases without each repeating the window builder. The page is rebuilt from the
/// view on every press, because that is what the real window does: a card's control
/// calls into the view and the next frame draws from whatever the view then holds. A
/// harness holding one frozen snapshot would pass without ever proving the panel follows
/// the view, which is the half of the layout decision worth testing.
fn page_over(
    cx: &mut TestAppContext,
    client: crate::SettingsClient,
    seeded: SettingsSnapshot,
) -> (Entity<SettingsView>, &mut VisualTestContext) {
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
    view.update(visual, |view, _| view.snapshot = Some(seeded));
    (view, visual)
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
        // A test entry has no process, so it never has a schema either. Derived rather
        // than set, so a test cannot build a state the projection cannot produce.
        settings_available: false,
        fields: Vec::new(),
        actions: Vec::new(),
        values: Default::default(),
        position: None,
        positions: Vec::new(),
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

/// A card's settings control opens a form even when the plugin is switched off.
///
/// The panel is a *sibling* of the grid rather than a section of one card, because
/// `gpui-kit` renders a field row only inside a setting group. That is a layout
/// decision with a cost — the form is below the grid, not on the card that owns it —
/// so the two halves of it are pinned here: the card's control opens the panel, and
/// the panel's own close control puts it away again.
///
/// **A stopped plugin, deliberately.** This is the bug that made a plugin card useless
/// the moment it was installed: a plugin's schema arrives with its running process's
/// handshake, so a switched-off plugin had no fields, the configure button was drawn
/// only when there were fields, and the card was left with a delete button and no way
/// to configure anything. The button is unconditional now, and pressing it on a stopped
/// plugin turns it on instead.
#[gpui_kit::test]
fn a_stopped_plugins_settings_control_starts_it_rather_than_vanishing(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);
    let (client, endpoint) = crate::SettingsClient::bounded(4);
    let mut stopped = entry("pomodoro", true, false);
    stopped.running = false;
    stopped.fields.clear();
    stopped.values.clear();
    stopped.settings_available = false;
    let seeded = snapshot_with_plugins(SettingsPlugins {
        available: true,
        catalog_read: true,
        entries: vec![stopped],
        ..SettingsPlugins::default()
    });
    let (view, visual) = page_over(cx, client, seeded);
    visual.update(|window, cx| window.render_frame(cx));

    // Present at all. The old behaviour drew nothing here, which is the whole point.
    let configure = ElementId::from("plugin-configure-pomodoro");
    assert!(
        visual.update(|window, _| window.try_find(configure.clone()).is_some()),
        "a card that has just been installed must still offer a way to configure it — a \
         button that appears only once a plugin is running is missing at exactly the moment \
         a user looks for it"
    );

    visual.update(|window, cx| window.click(configure, cx));
    // Parked rather than read straight away: the command goes out through a spawned
    // task, and reading before the executor has run it would assert on an empty queue
    // and pass for the wrong reason.
    visual.run_until_parked();
    assert!(
        endpoint.try_recv().is_ok_and(|command| {
            matches!(
                command,
                crate::SettingsCommand::SetPluginEnabled { enabled: true, .. }
            )
        }),
        "so the only thing that can make the form exist is the thing that was asked for"
    );
    assert!(
        !view.read_with(visual, |view, _| view.plugin_settings_are_open("pomodoro")),
        "and no empty panel is drawn in the meantime"
    );
}

/// The form opens once the plugin's handshake arrives, not before.
///
/// The answer to "has it started yet" is not in the reply to the command that enabled
/// it — that reply is about the switch — so the page settles this off the snapshot poll.
#[gpui_kit::test]
fn a_form_waiting_on_a_plugin_opens_when_its_schema_arrives(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);
    let (client, _endpoint) = crate::SettingsClient::bounded(4);
    let mut stopped = entry("pomodoro", true, false);
    stopped.running = false;
    stopped.fields.clear();
    stopped.settings_available = false;
    let seeded = snapshot_with_plugins(SettingsPlugins {
        available: true,
        catalog_read: true,
        entries: vec![stopped.clone()],
        ..SettingsPlugins::default()
    });
    let (view, visual) = page_over(cx, client, seeded);

    visual.update(|window, cx| window.render_frame(cx));
    visual.update(|window, cx| window.click(ElementId::from("plugin-configure-pomodoro"), cx));
    assert!(
        view.read_with(visual, |view, _| view.plugin_awaiting_settings("pomodoro")),
        "so the page remembers it was asked, since no reply is going to say so"
    );

    // The plugin answers: running, with a schema.
    let mut answered = stopped.clone();
    answered.enabled = true;
    answered.running = true;
    answered.settings_available = true;
    answered.fields = vec![SettingsPluginField {
        key: "focus_minutes".to_string(),
        label: "Focus round".to_string(),
        description: None,
        kind: SettingsFieldKind::Integer,
        default: SettingsFieldValue::Integer(25),
        minimum: Some(1.0),
        maximum: Some(120.0),
        step: Some(1.0),
        unit: None,
        placeholder: None,
        multiline: false,
        options: Vec::new(),
    }];
    let answered_snapshot = snapshot_with_plugins(SettingsPlugins {
        available: true,
        catalog_read: true,
        entries: vec![answered],
        ..SettingsPlugins::default()
    });
    view.update(visual, |view, _| {
        assert!(
            view.adopt_snapshot(answered_snapshot),
            "adopting a snapshot that opens a panel has to report a change, or the caller \
             skips the redraw and the form never appears"
        );
    });
    visual.update(|window, cx| window.render_frame(cx));
    assert!(
        view.read_with(visual, |view, _| view.plugin_settings_are_open("pomodoro")),
        "and the form is there, drawn from the schema the running plugin declared"
    );
    assert!(
        !view.read_with(visual, |view, _| view.plugin_awaiting_settings("pomodoro")),
        "with the request settled, so a later press starts over rather than queueing"
    );
    assert!(
        visual.update(|window, _| window
            .try_find(ElementId::from("plugin-settings-header-pomodoro"))
            .is_some()),
        "and the header names the plugin the form belongs to"
    );
}

/// A control a plugin offered is pressable from the card.
///
/// The round trip the whole action mechanism exists for, asserted at the window: a
/// press sends the plugin's own id, and the page adopts the snapshot that comes back —
/// which is what carries the button's new label.
#[gpui_kit::test]
fn a_controls_press_sends_the_plugins_own_id(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);
    let (client, endpoint) = crate::SettingsClient::bounded(4);
    let mut running = entry("pomodoro", true, true);
    running.running = true;
    running.actions = vec![bongocat_ui_protocol::SettingsPluginAction {
        id: "toggle".to_string(),
        label: "Pause".to_string(),
        glyph: SettingsActionGlyph::Pause,
        disabled: false,
    }];
    let seeded = snapshot_with_plugins(SettingsPlugins {
        available: true,
        catalog_read: true,
        entries: vec![running],
        ..SettingsPlugins::default()
    });
    let (_view, visual) = page_over(cx, client, seeded);
    visual.update(|window, cx| window.render_frame(cx));

    let button = ElementId::from("plugin-action-pomodoro-toggle");
    assert!(
        visual.update(|window, _| window.try_find(button.clone()).is_some()),
        "a control the plugin offered is on its card, in the user's own words"
    );
    visual.update(|window, cx| window.click(button, cx));
    visual.run_until_parked();
    assert!(
        endpoint.try_recv().is_ok_and(|command| {
            matches!(
                command,
                crate::SettingsCommand::PressPluginAction { action, .. } if action == "toggle"
            )
        }),
        "a press sends the id the plugin declared, which is the same id a panel button uses — \
         so the plugin has one handler for both and the two cannot drift"
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
    let (view, visual) = page_over(cx, client, seeded);

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

/// The position row is above the plugin's own settings, and a plugin that draws no panel
/// has none.
///
/// Placement is the host's — one plugin per corner, chosen by the user — so the row is the
/// one thing on the form that is not the plugin's, and it belongs first. A plugin that says
/// it has no place in the model window is offered no row at all, which is how a sound stays
/// out of this: there is nothing on the window to move.
#[gpui_kit::test]
fn the_position_row_is_the_first_thing_a_drawing_plugins_form_offers(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);
    let (client, _endpoint) = crate::SettingsClient::bounded(4);
    let mut drawing = entry_with_fields("pomodoro", true, true);
    drawing.position = Some(bongocat_ui_protocol::SettingsPluginPosition {
        value: "bottom_left".to_string(),
        label: "Bottom left".to_string(),
    });
    drawing.positions = ["bottom_left", "center"]
        .into_iter()
        .map(|value| bongocat_ui_protocol::SettingsPluginPosition {
            value: value.to_string(),
            label: format!("{value} label"),
        })
        .collect();
    let seeded = snapshot_with_plugins(SettingsPlugins {
        available: true,
        catalog_read: true,
        entries: vec![drawing],
        ..SettingsPlugins::default()
    });
    let (view, visual) = page_over(cx, client, seeded);

    visual.update(|window, cx| window.click(ElementId::from("plugin-configure-pomodoro"), cx));
    let rows = view.read_with(visual, |view, _| {
        view.plugin_settings_row_labels("pomodoro")
    });
    assert_eq!(
        rows.first().map(String::as_str),
        Some("Position"),
        "because where a panel sits in the model window is not the plugin's own setting, \
         and a row about the window's arrangement reads better above the plugin's than \
         among them"
    );
    assert!(
        rows.iter().any(|label| label == "Minutes"),
        "and the plugin's own settings follow it, unchanged"
    );

    // A plugin that draws nothing is offered no position at all: a menu of nine for a
    // sound is a control that changes nothing.
    let (client, _endpoint) = crate::SettingsClient::bounded(4);
    let mut silent = entry_with_fields("typing-sound", true, true);
    silent.position = None;
    silent.positions = Vec::new();
    let seeded = snapshot_with_plugins(SettingsPlugins {
        available: true,
        catalog_read: true,
        entries: vec![silent],
        ..SettingsPlugins::default()
    });
    let (view, visual) = page_over(cx, client, seeded);
    visual.update(|window, cx| window.click(ElementId::from("plugin-configure-typing-sound"), cx));
    let rows = view.read_with(visual, |view, _| {
        view.plugin_settings_row_labels("typing-sound")
    });
    assert_eq!(
        rows.first().map(String::as_str),
        Some("Minutes"),
        "so its form starts with the plugin's own settings"
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
