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
/// Typing only updates the editor; the Apply action sends the complete draft.
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

fn network_token_row(
    view: &Entity<SettingsView>,
    language: SettingsLanguage,
    label_key: &'static str,
    editor: NetworkEditor,
    disabled: bool,
) -> SettingItem {
    let view = view.clone();
    SettingItem::new(
        row_text(language, label_key),
        SettingField::element(
            move |options: &RenderOptions, _: &mut Window, app: &mut App| {
                let view = view.read(app);
                let input = match editor {
                    NetworkEditor::Connection => &view.network_client_token_input,
                    NetworkEditor::Relay => &view.network_relay_token_input,
                };
                Input::new(input)
                    .mask_toggle()
                    .with_size(options.size())
                    .w_64()
                    .disabled(disabled || options.is_disabled())
                    .into_any_element()
            },
        ),
    )
    .disabled(disabled)
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
    network_named_switch_row(view, row_text(language, label_key), read, write)
}

fn network_named_switch_row(
    view: &Entity<SettingsView>,
    label: SharedString,
    read: impl Fn(&SettingsNetwork) -> bool + 'static,
    write: impl Fn(&mut SettingsView, bool, &mut Context<SettingsView>) + 'static,
) -> SettingItem {
    let reader = view.clone();
    SettingItem::new(
        label,
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

fn network_device_id_row(language: SettingsLanguage, id: String) -> SettingItem {
    let label = row_text(language, "settings.multiplayer.status.device_id.label");
    SettingItem::new(
        label,
        SettingField::element(
            move |options: &RenderOptions, _: &mut Window, _: &mut App| {
                let id = id.clone();
                let value = if id.is_empty() {
                    "—".to_owned()
                } else {
                    id.clone()
                };
                div()
                    .flex()
                    .items_center()
                    .gap_2()
                    .child(div().text_sm().max_w_64().truncate().child(value))
                    .child(
                        Button::new("network-copy-local-device-id")
                            .label(row_text(
                                language,
                                "settings.multiplayer.status.device_id.copy",
                            ))
                            .with_size(options.size())
                            .disabled(id.is_empty() || options.is_disabled())
                            .on_click(move |_, window, cx| {
                                if !platform::copy_text(id.clone(), cx) {
                                    window.push_notification(
                                        Notification::new()
                                            .message(row_text(
                                                language,
                                                "settings.multiplayer.status.device_id.copy_failed",
                                            ))
                                            .with_type(NotificationType::Error),
                                        cx,
                                    );
                                }
                            }),
                    )
                    .into_any_element()
            },
        ),
    )
}

/// One device this machine draws, with the control that stops drawing it.
///
/// The id is the row's title because it is the name the peer chose for itself:
/// there is no friendlier label the page could put there, and inventing one
/// would be a second name for the same machine.
fn network_peer_label(language: SettingsLanguage, peer: &SettingsNetworkPeer) -> SharedString {
    if peer.name.trim().is_empty() {
        row_text(language, "settings.multiplayer.display.peer.unnamed")
    } else {
        peer.name.clone().into()
    }
}

fn network_display_device_row(
    view: &Entity<SettingsView>,
    language: SettingsLanguage,
    device: String,
    peer: Option<&SettingsNetworkPeer>,
    remembered_name: Option<&str>,
    editing_blocked: bool,
) -> SettingItem {
    let detail = device.clone();
    let state = match peer {
        Some(peer) if peer.publish => "settings.multiplayer.display.peer.sharing",
        Some(_) => "settings.multiplayer.display.peer.not_sharing",
        None => "settings.multiplayer.display.peer.offline",
    };
    let label = peer.map_or_else(
        || {
            remembered_name.map_or_else(
                || row_text(language, "settings.multiplayer.display.peer.saved"),
                |name| name.to_owned().into(),
            )
        },
        |peer| network_peer_label(language, peer),
    );
    let show_id = label.as_ref() != detail;
    let read_id = device.clone();
    let item = network_named_switch_row(
        view,
        format!(
            "{label} — {}",
            bongocat_i18n::text(language.catalog_locale(), state)
        )
        .into(),
        move |settings| settings.client.display.contains(&read_id),
        move |view, shown, cx| view.set_network_display_device(device.clone(), shown, cx),
    )
    .disabled(editing_blocked);
    if show_id {
        item.description(detail)
    } else {
        item
    }
}

/// Whether a peers-count string carries a number at its head.
///
/// The count row prints the room's number verbatim, and every other answer it
/// can give starts with a letter, so the first character is the whole test.
#[cfg_attr(not(test), allow(dead_code))]
fn peers_status_starts_with_digit(value: &str) -> bool {
    value.chars().next().is_some_and(|c| c.is_ascii_digit())
}

fn network_editor_row(
    view: &Entity<SettingsView>,
    language: SettingsLanguage,
    editor: NetworkEditor,
    open: bool,
    connected: bool,
    editing_blocked: bool,
) -> SettingItem {
    let view = view.clone();
    let (id, label) = match (editor, open, connected) {
        (NetworkEditor::Connection, true, _) => {
            ("network-connect", "settings.multiplayer.editor.connect")
        }
        (NetworkEditor::Connection, false, true) => (
            "network-edit-connection",
            "settings.multiplayer.editor.change_connection",
        ),
        (NetworkEditor::Connection, false, false) => (
            "network-edit-connection",
            "settings.multiplayer.editor.edit_connection",
        ),
        (NetworkEditor::Relay, true, _) => (
            "network-apply-relay",
            "settings.multiplayer.editor.apply_relay",
        ),
        (NetworkEditor::Relay, false, _) => (
            "network-edit-relay",
            "settings.multiplayer.editor.edit_relay",
        ),
    };
    SettingItem::new(
        SharedString::from(""),
        SettingField::element(
            move |options: &RenderOptions, _: &mut Window, app: &mut App| {
                let disabled = editing_blocked || options.is_disabled();
                let action = view.clone();
                let secondary = view.clone();
                let applying = open && view.read(app).network_apply_pending;
                div()
                    .flex()
                    .items_center()
                    .gap_2()
                    .when(
                        open || editor == NetworkEditor::Connection && connected,
                        |row| {
                            row.child(
                                Button::new(format!("{id}-secondary"))
                                    .label(row_text(
                                        language,
                                        if open {
                                            "settings.multiplayer.editor.cancel"
                                        } else {
                                            "settings.multiplayer.editor.disconnect"
                                        },
                                    ))
                                    .with_size(options.size())
                                    .ghost()
                                    .disabled(disabled)
                                    .on_click(move |_, _, app| {
                                        secondary.update(app, |view, cx| {
                                            if open {
                                                view.cancel_network_edit(cx);
                                            } else {
                                                view.set_network_client_enabled(false, cx);
                                            }
                                        })
                                    }),
                            )
                        },
                    )
                    .child(
                        Button::new(id)
                            .label(row_text(language, label))
                            .with_size(options.size())
                            .loading(applying)
                            .disabled(disabled)
                            .on_click(move |_, _, app| {
                                action.update(app, |view, cx| {
                                    if open {
                                        view.apply_network_edit(cx);
                                    } else {
                                        view.edit_network(editor, cx);
                                    }
                                })
                            }),
                    )
                    .into_any_element()
            },
        ),
    )
    .disabled(editing_blocked)
}

/// Room status and display controls remain visible while the connection and
/// relay editors hold their changes until the user applies them.
pub(super) fn multiplayer_page(
    view: &Entity<SettingsView>,
    snapshot: Option<&SettingsSnapshot>,
    draft: Option<&(NetworkEditor, SettingsNetwork)>,
    remembered_names: &std::collections::BTreeMap<String, String>,
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

    let editing_connection = draft.is_some_and(|(editor, _)| *editor == NetworkEditor::Connection);
    let editing_relay = draft.is_some_and(|(editor, _)| *editor == NetworkEditor::Relay);
    let manual = draft
        .filter(|_| editing_connection)
        .map_or(network.client.source, |(_, draft)| draft.client.source)
        .needs_url();
    let needs_address = draft
        .filter(|_| editing_relay)
        .map_or(network.server.listen, |(_, draft)| draft.server.listen)
        .needs_address();
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
    let display = &network.client.display;
    let peers = status
        .client
        .as_ref()
        .map_or(&[][..], |room| room.peers.as_slice());
    let at_capacity = display
        .iter()
        .filter(|id| id.as_str() != NETWORK_SELF_DEVICE)
        .count()
        >= 2;
    for peer in peers {
        display_items.push(
            network_display_device_row(
                view,
                language,
                peer.id.clone(),
                Some(peer),
                None,
                editing_blocked || (at_capacity && !display.contains(&peer.id)),
            )
            .keywords(display_keywords.iter().cloned()),
        );
    }
    for device in display.iter().filter(|id| {
        id.as_str() != NETWORK_SELF_DEVICE && !peers.iter().any(|peer| &peer.id == *id)
    }) {
        display_items.push(
            network_display_device_row(
                view,
                language,
                device.clone(),
                None,
                remembered_names.get(device).map(String::as_str),
                editing_blocked,
            )
            .keywords(display_keywords.iter().cloned()),
        );
    }

    // The address row is the one place the source dropdown's answer changes a
    // row's shape: a client that follows the local relay has no address to type,
    // so the row shows the address it will use and refuses the caret.
    let client_keywords = page.search_keywords(language, ["settings.multiplayer.client.title"]);
    let url_row = if manual {
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
    let mut client_items = vec![network_editor_row(
        view,
        language,
        NetworkEditor::Connection,
        editing_connection,
        network.client.enabled,
        editing_blocked,
    )];
    if !editing_connection && network.client.enabled {
        let address = if network.client.source.needs_url() {
            network.client.url.clone().unwrap_or_default()
        } else {
            network_local_relay_url(network.server.port)
        };
        client_items.push(network_status_row_toned(
            language,
            "settings.multiplayer.client.url.label",
            address,
            StatusTone::Idle,
        ));
    }
    if editing_connection {
        client_items.extend([
            network_address_row(
                view,
                language,
                "settings.multiplayer.client.source.label",
                editing_blocked,
            )
            .keywords(client_keywords.iter().cloned()),
            url_row.keywords(client_keywords.iter().cloned()),
            network_token_row(
                view,
                language,
                "settings.multiplayer.client.token.label",
                NetworkEditor::Connection,
                !manual || editing_blocked,
            )
            .keywords(client_keywords.iter().cloned()),
        ]);
    }
    client_items.push(
        network_switch_row(
            view,
            language,
            "settings.multiplayer.client.publish.label",
            |settings| settings.client.publish,
            |view, value, cx| view.set_network_client_publish(value, cx),
        )
        .keywords(client_keywords.iter().cloned()),
    );
    let client_group = SettingGroup::new()
        .title(row_text(language, "settings.multiplayer.client.title"))
        .items(client_items);

    // Only `Custom` reads the address, so the row is disabled for every other
    // mode rather than hidden: the rule that decides is written once in the
    // protocol, and a row that vanished and reappeared would move the port
    // below it.
    let server_keywords = page.search_keywords(language, ["settings.multiplayer.server.title"]);
    let mut server_items = vec![network_editor_row(
        view,
        language,
        NetworkEditor::Relay,
        editing_relay,
        false,
        editing_blocked,
    )];
    if editing_relay {
        server_items.extend([
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
            .disabled(editing_blocked || !needs_address)
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
            network_token_row(
                view,
                language,
                "settings.multiplayer.server.token.label",
                NetworkEditor::Relay,
                editing_blocked,
            )
            .keywords(server_keywords.iter().cloned()),
        ]);
    }
    let server_group = SettingGroup::new()
        .title(row_text(language, "settings.multiplayer.server.title"))
        .items(server_items);

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
    let status_group = SettingGroup::new()
        .title(row_text(
            language,
            "settings.multiplayer.status_and_devices.title",
        ))
        .items([
            network_device_id_row(
                language,
                snapshot
                    .map(|snapshot| snapshot.network.device.id.clone())
                    .unwrap_or_default(),
            )
            .keywords(status_keywords.iter().cloned()),
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
        ]);
    let display_group = SettingGroup::new()
        .title(row_text(language, "settings.multiplayer.display.title"))
        .description(row_text(language, "settings.multiplayer.display.empty"))
        .items(display_items);
    let mut groups = vec![status_group, client_group, display_group, server_group];
    if editing_connection {
        groups.insert(2, device_group);
    }
    SettingPage::new(page.title(language))
        .icon(page.icon())
        .title_suffix(render::page_reporter(page, navigation_memory))
        .groups(groups)
}

#[cfg(test)]
#[test]
fn device_name_is_visible_when_it_matches_the_id() {
    let peer = SettingsNetworkPeer {
        id: "laptop".into(),
        name: "laptop".into(),
        publish: true,
    };
    assert_eq!(
        network_peer_label(SettingsLanguage::English, &peer).as_ref(),
        "laptop"
    );
}
