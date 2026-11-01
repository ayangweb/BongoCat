//! The multiplayer page: what this machine calls itself, what it draws, what it
//! joins and what it serves.
//!
//! The three configurable parts are kept apart because they answer three
//! questions a user asks one at a time, and the page never names a
//! `bongocat-net` type: the protocol's own vocabulary is the only one it speaks,
//! so a relay that changes shape does not reach into the window.
//!
//! The status group at the bottom is deliberately the only part that reads the
//! process rather than the configuration. The rows above it are what the user
//! chose; the rows in it are what this process is doing, and they are rendered
//! from `network_status` alone — a page that guessed at a connection from the
//! settings would say "connected" about a machine that never joined anything.
//!
//! Every control writes the whole `SettingsNetwork`, because the three parts are
//! one persisted object: a typed field edited during the same pause as a
//! dropdown in another group has to land in the same write, or one of the two
//! choices would be silently reverted.

use super::*;
use gpui_kit::component::Sizable as _;

/// Keyboard tab position of this page's controls.
///
/// The window's tab bands are the overlay's at 10, the import card's at 20, the
/// modifier recorder's at 30, the model rows' from 40 and the shortcut rows'
/// from 100, so this page takes the gap above the shortcut band.
const MULTIPLAYER_TAB_INDEX: isize = 90;

/// One row's text, read from the active catalog.
fn row_text(language: SettingsLanguage, key: &'static str) -> SharedString {
    bongocat_i18n::text(language.catalog_locale(), key).into()
}

/// A row that stores one typed value of the multiplayer settings.
///
/// The value is read through [`SettingsView::network_settings_value`] rather
/// than straight from the snapshot, which is what stops the packaged input from
/// pulling the caret back: the component re-reads its value on every frame, and
/// a snapshot that has not received the write yet would otherwise restore the
/// old text while the user is still typing.
///
/// Typing is debounced rather than written per keystroke: a field that wrote on
/// every character would send one command per letter.
fn network_text_row(
    view: &Entity<SettingsView>,
    language: SettingsLanguage,
    label_key: &'static str,
    read: impl Fn(&SettingsNetwork) -> String + 'static,
    write: impl Fn(&mut SettingsView, String, &mut Context<SettingsView>) + 'static,
) -> SettingItem {
    let reader = view.clone();
    SettingItem::new(
        row_text(language, label_key),
        // The packaged input is a `SharedString` field: the component owns the
        // text it shows, and the row's own value is the projection of one
        // setting out of the whole multiplayer patch.
        SettingField::input(
            move |app| SharedString::from(read(&reader.read(app).network_settings_value())),
            {
                let view = view.clone();
                move |value: SharedString, app: &mut App| {
                    view.update(app, |view, cx| write(view, value.to_string(), cx));
                }
            },
        ),
    )
}

/// A row that stores one switch of the multiplayer settings.
///
/// A switch is one gesture, so it is written straight through rather than held
/// for the debounce window.
fn network_switch_row(
    view: &Entity<SettingsView>,
    language: SettingsLanguage,
    label_key: &'static str,
    read: impl Fn(&SettingsNetwork) -> bool + 'static,
    write: impl Fn(&mut SettingsView, bool, &mut Context<SettingsView>) + 'static,
) -> SettingItem {
    let reader = view.clone();
    SettingItem::new(
        row_text(language, label_key),
        SettingField::switch(
            move |app| read(&reader.read(app).network_settings_value()),
            {
                let view = view.clone();
                move |value: bool, app: &mut App| {
                    view.update(app, |view, cx| write(view, value, cx));
                }
            },
        ),
    )
}

/// A row that offers one fixed list of translated options.
///
/// The options are handed to the row whole rather than as labels only, so the
/// menu is measured from the very list the dropdown shows. The state is reached
/// through a selector rather than passed in, because the entity the row reads is
/// the one the view already owns and subscribes to.
fn network_option_row(
    view: &Entity<SettingsView>,
    language: SettingsLanguage,
    label_key: &'static str,
    options: Vec<&'static str>,
    editing_blocked: bool,
    select: impl Fn(&SettingsView) -> &Entity<NetworkOptionSelectState> + 'static,
) -> SettingItem {
    let view = view.clone();
    SettingItem::new(
        row_text(language, label_key),
        SettingField::element(
            move |_: &RenderOptions, window: &mut Window, app: &mut App| {
                let state = select(view.read(app)).clone();
                Select::new(&state)
                    .menu_width(dropdown_menu_width(window, options.iter().copied()))
                    .disabled(editing_blocked)
                    .into_any_element()
            },
        ),
    )
    .disabled(editing_blocked)
}

/// The client's single address row: one dropdown whose entries are the
/// addresses themselves.
///
/// The entries are owned strings because the preset addresses carry the
/// configured port, so the row is its own function rather than the fixed-list
/// one above. The field it edits is the whole choice — source and url move
/// together — which is exactly what the two rows it replaces used to split.
fn network_address_row(
    view: &Entity<SettingsView>,
    language: SettingsLanguage,
    label_key: &'static str,
    editing_blocked: bool,
) -> SettingItem {
    let view = view.clone();
    SettingItem::new(
        row_text(language, label_key),
        SettingField::element(
            move |_: &RenderOptions, window: &mut Window, app: &mut App| {
                let state = view.read(app).network_client_address_select.clone();
                // The options are rebuilt here so the menu width and the list the
                // view state syncs stay the same shape: a port changed a moment
                // ago must not leave a preset naming the old one.
                let port = view.read(app).network_settings_value().server.port;
                let options = network_client_address_options(language, port);
                Select::new(&state)
                    .menu_width(dropdown_menu_width(window, options.iter().cloned()))
                    .disabled(editing_blocked)
                    .into_any_element()
            },
        ),
    )
    .disabled(editing_blocked)
}

/// A row that reports a fact about the running process.
///
/// The value is the whole row: a status is not a setting, so there is no field
/// to edit and no default to reset to.
/// How a status fact paints its dot.
#[derive(Clone, Copy)]
enum StatusTone {
    /// Alive and working.
    Live,
    /// In progress, or waiting for something to come back.
    Pending,
    /// A failure the user may need to act on.
    Fault,
    /// Not applicable right now.
    Idle,
}

/// A status row: a colored dot that answers "is this fine?" at a glance, then
/// the fact itself.
fn network_status_row_toned(
    language: SettingsLanguage,
    label_key: &'static str,
    value: String,
    tone: StatusTone,
) -> SettingItem {
    SettingItem::new(
        row_text(language, label_key),
        SettingField::element(move |_: &RenderOptions, _: &mut Window, app: &mut App| {
            let tokens = Tokens::from_theme(app);
            let dot = match tone {
                StatusTone::Live => tokens.success,
                StatusTone::Pending => tokens.warning,
                StatusTone::Fault => tokens.danger,
                StatusTone::Idle => tokens.muted,
            };
            div()
                .flex()
                .items_center()
                .gap_2()
                .text_sm()
                .child(div().size_2().rounded_full().bg(dot))
                .child(value.clone())
                .into_any_element()
        }),
    )
}

/// One device this machine draws, with the control that stops drawing it.
///
/// The id is the row's title because it is the name the peer chose for itself:
/// there is no friendlier label the page could put there, and inventing one
/// would be a second name for the same machine.
fn network_display_device_row(
    view: &Entity<SettingsView>,
    language: SettingsLanguage,
    device: String,
    editing_blocked: bool,
) -> SettingItem {
    let view = view.clone();
    SettingItem::new(
        device.clone(),
        SettingField::element(
            move |options: &RenderOptions, _: &mut Window, _: &mut App| {
                let disabled = editing_blocked || options.is_disabled();
                let removal = device.clone();
                let remove_view = view.clone();
                div()
                    .child(
                        Button::new(ElementId::Name(SharedString::from(format!(
                            "network-remove-display-device-{removal}"
                        ))))
                        .label(bongocat_i18n::text(
                            language.catalog_locale(),
                            "settings.multiplayer.display.remove",
                        ))
                        .with_size(options.size())
                        .ghost()
                        .xsmall()
                        .disabled(disabled)
                        .on_click(move |_, _, app| {
                            remove_view.update(app, |view, cx| {
                                view.remove_network_display_device(removal.clone(), cx)
                            });
                        }),
                    )
                    .into_any_element()
            },
        ),
    )
    .disabled(editing_blocked)
}

/// The row that adds one device id to the list this machine draws.
///
/// The field is view state rather than configuration: an id becomes part of the
/// settings only when the user commits it, and until then there is nothing to
/// send and nothing a refused write could lose.
fn network_display_add_row(
    view: &Entity<SettingsView>,
    language: SettingsLanguage,
    editing_blocked: bool,
) -> SettingItem {
    let view = view.clone();
    SettingItem::new(
        row_text(language, "settings.multiplayer.display.add.label"),
        SettingField::element(
            move |options: &RenderOptions, _: &mut Window, app: &mut App| {
                let disabled = editing_blocked || options.is_disabled();
                let state = view.read(app);
                let input = state.network_display_device_input.clone();
                let focus = state.network_display_device_add_focus.clone();
                let addable = !disabled && !input.read(app).value().trim().is_empty();
                let add_view = view.clone();
                div()
                    .flex()
                    .items_center()
                    .justify_end()
                    .gap_2()
                    .child(Input::new(&input).disabled(disabled).w_40())
                    .child(
                        div()
                            .key_context("SettingsControl")
                            .track_focus(&focus)
                            .tab_index(MULTIPLAYER_TAB_INDEX)
                            .tab_stop(addable)
                            .child(
                                Button::new("network-add-display-device")
                                    .label(bongocat_i18n::text(
                                        language.catalog_locale(),
                                        "settings.multiplayer.display.add.action",
                                    ))
                                    .with_size(options.size())
                                    .disabled(!addable)
                                    .on_click(move |_, window, app| {
                                        add_view.update(app, |view, cx| {
                                            view.add_network_display_device(window, cx)
                                        });
                                    }),
                            ),
                    )
                    .into_any_element()
            },
        ),
    )
    .disabled(editing_blocked)
}

/// Whether a peers-count string carries a number at its head.
///
/// The count row prints the room's number verbatim, and every other answer it
/// can give starts with a letter, so the first character is the whole test.
#[cfg_attr(not(test), allow(dead_code))]
fn peers_status_starts_with_digit(value: &str) -> bool {
    value.chars().next().is_some_and(|c| c.is_ascii_digit())
}

/// One device the room says is online but this machine is not drawing yet.
///
/// The room already announces every peer's id and name, so retyping the id
/// into the field below would only transcribe what this row can show: one
/// click adds it to the list this machine draws. Devices that are offline stay
/// hand-added — the typed field is how a machine is named before its first
/// join, and a device already in the list keeps its own row above, the one
/// with the removal control.
fn network_display_peer_row(
    view: &Entity<SettingsView>,
    language: SettingsLanguage,
    peer: SettingsNetworkPeer,
    editing_blocked: bool,
) -> SettingItem {
    let view = view.clone();
    // The label carries the sharing state so one board can tell at a glance
    // which devices are feeding the room and which are only watching.
    let label = if peer.name.is_empty() || peer.name == peer.id {
        peer.id.clone()
    } else {
        format!("{} ({})", peer.name, peer.id)
    };
    let sharing_key = if peer.publish {
        "settings.multiplayer.display.peer.sharing"
    } else {
        "settings.multiplayer.display.peer.not_sharing"
    };
    let title = format!(
        "{} — {}",
        label,
        bongocat_i18n::text(language.catalog_locale(), sharing_key)
    );
    SettingItem::new(
        SharedString::from(title),
        SettingField::element(
            move |options: &RenderOptions, _: &mut Window, _: &mut App| {
                let disabled = editing_blocked || options.is_disabled();
                let device = peer.id.clone();
                let peer_view = view.clone();
                div()
                    .child(
                        Button::new(ElementId::Name(SharedString::from(format!(
                            "network-show-peer-{device}"
                        ))))
                        .label(bongocat_i18n::text(
                            language.catalog_locale(),
                            "settings.multiplayer.display.peer.show",
                        ))
                        .with_size(options.size())
                        .ghost()
                        .xsmall()
                        .disabled(disabled)
                        .on_click(move |_, _, app| {
                            peer_view.update(app, |view, cx| {
                                view.show_network_display_device(device.clone(), cx)
                            });
                        }),
                    )
                    .into_any_element()
            },
        ),
    )
    .disabled(editing_blocked)
}

/// The row that replaces this process so the page's changes take effect.
///
/// The relay and the room are both built at startup, which is why no in-process
/// reload can apply them. Spawning the same executable and quitting is the whole
/// trick, and the child is started through a short `sleep` so the parent — and
/// with it the relay's port — is gone by the time the fresh process binds.
fn network_restart_row(
    view: &Entity<SettingsView>,
    language: SettingsLanguage,
    editing_blocked: bool,
) -> SettingItem {
    let view = view.clone();
    SettingItem::new(
        // The group title above the row is the restart notice itself; the
        // row needs no second heading between it and the button.
        SharedString::from(""),
        SettingField::element(
            move |options: &RenderOptions, _: &mut Window, _: &mut App| {
                let disabled = editing_blocked || options.is_disabled();
                let restart_view = view.clone();
                div()
                    .child(
                        Button::new("network-restart")
                            .label(bongocat_i18n::text(
                                language.catalog_locale(),
                                "settings.multiplayer.restart.action",
                            ))
                            .with_size(options.size())
                            .danger()
                            .xsmall()
                            .disabled(disabled)
                            .on_click(move |_, _, app| {
                                restart_view.update(app, |view, cx| {
                                    view.restart_to_apply_network_settings(cx)
                                });
                            }),
                    )
                    .into_any_element()
            },
        ),
    )
    .disabled(editing_blocked)
}

/// The multiplayer page's five groups.
///
/// The notice rides on the first group because it is a sentence about the page
/// as a whole rather than about one control: the relay and the room are both
/// opened while the application starts, so a switch turned on here takes effect
/// the next time BongoCat launches. Promising anything else would be a claim
/// the process does not keep.
pub(super) fn multiplayer_page(
    view: &Entity<SettingsView>,
    snapshot: Option<&SettingsSnapshot>,
    language: SettingsLanguage,
    navigation_memory: SettingsNavigationMemory,
    editing_blocked: bool,
) -> SettingPage {
    let page = SettingsNavigationPage::Multiplayer;
    let network = snapshot
        .map(|snapshot| snapshot.network.clone())
        .unwrap_or_default();
    let status = snapshot
        .map(|snapshot| snapshot.network_status.clone())
        .unwrap_or_default();

    let device_keywords = page.search_keywords(language, ["settings.multiplayer.device.title"]);
    let device_group = SettingGroup::new()
        .title(row_text(language, "settings.multiplayer.device.title"))
        .items([
            network_text_row(
                view,
                language,
                "settings.multiplayer.device.id.label",
                |settings| settings.device.id.clone(),
                |view, value, cx| view.set_network_device_id(value, cx),
            )
            .keywords(device_keywords.iter().cloned()),
            network_text_row(
                view,
                language,
                "settings.multiplayer.device.name.label",
                |settings| settings.device.name.clone(),
                |view, value, cx| view.set_network_device_name(value, cx),
            )
            .keywords(device_keywords.iter().cloned()),
        ]);

    // An empty list is a machine that only publishes, which is a state the page
    // has to be able to show rather than a fault to repair — so the group says
    // so under its title instead of leaving a blank space where the rows would
    // be.
    let display_keywords = page.search_keywords(language, ["settings.multiplayer.display.title"]);
    let mut display_items = vec![
        network_switch_row(
            view,
            language,
            "settings.multiplayer.display.self.label",
            |settings| {
                settings
                    .client
                    .display
                    .iter()
                    .any(|device| device == NETWORK_SELF_DEVICE)
            },
            |view, shown, cx| view.set_network_display_self(shown, cx),
        )
        .keywords(display_keywords.iter().cloned()),
    ];
    for device in network
        .client
        .display
        .iter()
        .filter(|device| device.as_str() != NETWORK_SELF_DEVICE)
        .cloned()
        .collect::<Vec<_>>()
    {
        display_items.push(
            network_display_device_row(view, language, device, editing_blocked)
                .keywords(display_keywords.iter().cloned()),
        );
    }
    // The room announces who is online; each peer this machine is not drawing
    // yet appears here with a one-click way to draw it. The rows refresh with
    // the status the page already polls, so a peer that joins shows up without
    // anything being retyped and a peer that leaves stops being offered.
    for peer in status
        .client
        .iter()
        .flat_map(|client| client.peers.iter())
        .filter(|peer| !network.client.display.contains(&peer.id))
        .cloned()
        .collect::<Vec<_>>()
    {
        display_items.push(
            network_display_peer_row(view, language, peer, editing_blocked)
                .keywords(display_keywords.iter().cloned()),
        );
    }
    display_items.push(
        network_display_add_row(view, language, editing_blocked)
            .keywords(display_keywords.iter().cloned()),
    );
    // The address row is the one place the source dropdown's answer changes a
    // row's shape: a client that follows the local relay has no address to type,
    // so the row shows the address it will use and refuses the caret.
    let client_keywords = page.search_keywords(language, ["settings.multiplayer.client.title"]);
    let url_row = if network.client.source.needs_url() {
        network_text_row(
            view,
            language,
            "settings.multiplayer.client.url.label",
            |settings| settings.client.url.clone().unwrap_or_default(),
            |view, value, cx| view.set_network_client_url(value, cx),
        )
    } else {
        network_text_row(
            view,
            language,
            "settings.multiplayer.client.url.label",
            |settings| network_local_relay_url(settings.server.port),
            |_, _, _| {},
        )
        .disabled(true)
    };
    let client_group = SettingGroup::new()
        .title(row_text(language, "settings.multiplayer.client.title"))
        .items([
            network_switch_row(
                view,
                language,
                "settings.multiplayer.client.enabled.label",
                |settings| settings.client.enabled,
                |view, enabled, cx| view.set_network_client_enabled(enabled, cx),
            )
            .keywords(client_keywords.iter().cloned()),
            network_address_row(
                view,
                language,
                "settings.multiplayer.client.url.label",
                editing_blocked,
            )
            .keywords(client_keywords.iter().cloned()),
            url_row.keywords(client_keywords.iter().cloned()),
            network_text_row(
                view,
                language,
                "settings.multiplayer.client.token.label",
                |settings| settings.client.token.clone().unwrap_or_default(),
                |view, value, cx| view.set_network_client_token(value, cx),
            )
            .keywords(client_keywords.iter().cloned()),
            network_switch_row(
                view,
                language,
                "settings.multiplayer.client.publish.label",
                |settings| settings.client.publish,
                |view, publish, cx| view.set_network_client_publish(publish, cx),
            )
            .keywords(client_keywords.iter().cloned()),
        ]);

    // Only `Custom` reads the address, so the row is disabled for every other
    // mode rather than hidden: the rule that decides is written once in the
    // protocol, and a row that vanished and reappeared would move the port
    // below it.
    let server_keywords = page.search_keywords(language, ["settings.multiplayer.server.title"]);
    let server_group = SettingGroup::new()
        .title(row_text(language, "settings.multiplayer.server.title"))
        .items([
            network_switch_row(
                view,
                language,
                "settings.multiplayer.server.enabled.label",
                |settings| settings.server.enabled,
                |view, enabled, cx| view.set_network_server_enabled(enabled, cx),
            )
            .keywords(server_keywords.iter().cloned()),
            network_option_row(
                view,
                language,
                "settings.multiplayer.server.listen.label",
                network_listen_mode_options(language).to_vec(),
                editing_blocked,
                |view| &view.network_listen_mode_select,
            )
            .keywords(server_keywords.iter().cloned()),
            network_text_row(
                view,
                language,
                "settings.multiplayer.server.address.label",
                |settings| settings.server.address.clone().unwrap_or_default(),
                |view, value, cx| view.set_network_server_address(value, cx),
            )
            .disabled(editing_blocked || !network.server.listen.needs_address())
            .keywords(server_keywords.iter().cloned()),
            SettingItem::new(
                row_text(language, "settings.multiplayer.server.port.label"),
                SettingField::number_input(
                    network_port_number_field_options(),
                    {
                        let view = view.clone();
                        move |app| f64::from(view.read(app).network_settings_value().server.port)
                    },
                    {
                        let view = view.clone();
                        move |value, app| {
                            view.update(app, |view, cx| {
                                view.set_network_server_port_value(value, cx)
                            });
                        }
                    },
                ),
            )
            .disabled(editing_blocked)
            .keywords(server_keywords.iter().cloned()),
            network_text_row(
                view,
                language,
                "settings.multiplayer.server.token.label",
                |settings| settings.server.token.clone().unwrap_or_default(),
                |view, value, cx| view.set_network_server_token(value, cx),
            )
            .keywords(server_keywords.iter().cloned()),
        ]);

    let status_keywords = page.search_keywords(language, ["settings.multiplayer.status.title"]);
    let server_tone = if status.server.is_some() {
        StatusTone::Live
    } else {
        StatusTone::Idle
    };
    let room_state = status.client.as_ref().map(|client| client.state);
    let latency_known = status
        .client
        .as_ref()
        .and_then(|client| client.latency_ms)
        .is_some();
    let server_status = status.server.map_or_else(
        || {
            bongocat_i18n::text(
                language.catalog_locale(),
                "settings.multiplayer.status.server.stopped",
            )
            .to_owned()
        },
        |server| server.url,
    );
    let client = status.client.clone();
    let client_status = client.as_ref().map_or_else(
        || {
            bongocat_i18n::text(
                language.catalog_locale(),
                "settings.multiplayer.status.client.state.off",
            )
            .to_owned()
        },
        |client| network_client_state_text(client.state, language).to_owned(),
    );
    // No room at all reads as the state a machine that never joined has, which
    // the function answers with the same "not in a room" text: the row cannot
    // count a room this process is not in, and it must not print a zero that
    // looks like one it is alone in.
    let peers_status = network_client_peers_text(
        status
            .client
            .as_ref()
            .map_or(SettingsNetworkClientState::Off, |client| client.state),
        language,
    );
    let bound_status = status
        .client
        .as_ref()
        .and_then(|client| client.bound.clone())
        .unwrap_or_else(|| {
            bongocat_i18n::text(
                language.catalog_locale(),
                "settings.multiplayer.status.bound.none",
            )
            .to_owned()
        });
    // The latency row reads a live sample that only exists once the room is up,
    // so a machine that never joined shows "not connected" rather than a zero it
    // never measured.
    let latency_status = status
        .client
        .as_ref()
        .and_then(|client| client.latency_ms)
        .map(|ms| format!("{ms} ms"))
        .unwrap_or_else(|| {
            bongocat_i18n::text(
                language.catalog_locale(),
                "settings.multiplayer.status.latency.none",
            )
            .to_owned()
        });
    // The tone questions are answered before the rows move their values in.
    let peers_live = peers_status_starts_with_digit(&peers_status);
    let bound_live = bound_status
        != bongocat_i18n::text(
            language.catalog_locale(),
            "settings.multiplayer.status.bound.none",
        )
        .to_owned();
    let status_group = SettingGroup::new()
        .title(row_text(
            language,
            "settings.multiplayer.status_and_devices.title",
        ))
        .description(row_text(language, "settings.multiplayer.display.empty"))
        .items([
            network_status_row_toned(
                language,
                "settings.multiplayer.status.server.label",
                server_status,
                server_tone,
            )
            .keywords(status_keywords.iter().cloned()),
            network_status_row_toned(
                language,
                "settings.multiplayer.status.client.label",
                client_status.clone(),
                match room_state {
                    Some(SettingsNetworkClientState::Connected { .. }) => StatusTone::Live,
                    Some(
                        SettingsNetworkClientState::Connecting
                        | SettingsNetworkClientState::Reconnecting,
                    ) => StatusTone::Pending,
                    Some(SettingsNetworkClientState::Failed) => StatusTone::Fault,
                    Some(SettingsNetworkClientState::Off) | None => StatusTone::Idle,
                },
            )
            .keywords(status_keywords.iter().cloned()),
            network_status_row_toned(
                language,
                "settings.multiplayer.status.latency.label",
                latency_status,
                if latency_known {
                    StatusTone::Live
                } else {
                    StatusTone::Idle
                },
            )
            .keywords(status_keywords.iter().cloned()),
            network_status_row_toned(
                language,
                "settings.multiplayer.status.peers.label",
                peers_status,
                if peers_live {
                    StatusTone::Live
                } else {
                    StatusTone::Idle
                },
            )
            .keywords(status_keywords.iter().cloned()),
            network_status_row_toned(
                language,
                "settings.multiplayer.status.bound.label",
                bound_status,
                if bound_live {
                    StatusTone::Live
                } else {
                    StatusTone::Idle
                },
            )
            .keywords(status_keywords.iter().cloned()),
        ])
        // The device rows this machine draws follow the facts about the room:
        // one board, because choosing what to draw is how a user acts on what
        // the status just told them.
        .items(display_items);

    // The restart row is its own board so it can be the first thing on the
    // page: every change here needs it, and a red button that must not be
    // missed belongs above the controls rather than buried in their board.
    let restart_group = SettingGroup::new()
        .title(row_text(language, "settings.multiplayer.restart_notice"))
        .items([network_restart_row(view, language, editing_blocked)]);

    SettingPage::new(page.title(language))
        .icon(page.icon())
        .title_suffix(render::page_reporter(page, navigation_memory))
        .groups(vec![
            restart_group,
            device_group,
            status_group,
            client_group,
            server_group,
        ])
}
