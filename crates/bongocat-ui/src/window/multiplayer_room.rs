//! The multiplayer room page: the service connection, the room and its
//! members, and the chat whose lines bubble above the cat.
//!
//! Everything the page shows is a projection from the snapshot; the page never
//! talks to the room service itself. Its requests go through the settings
//! client, and the outcome arrives through later snapshot revisions — the same
//! contract the remote model library page follows.

use super::*;
use gpui_kit::AnyElement;
use gpui_kit::component::Sizable as _;
use gpui_kit::component::label::Label;

impl SettingsView {
    /// Connect to the address currently in the field. If the field was edited
    /// without pressing Enter, persist it first and then connect using that
    /// same value.
    pub(super) fn connect_multiplayer_service(&mut self, cx: &mut Context<Self>) {
        let Some(snapshot) = self.snapshot.as_ref() else {
            return;
        };
        let Some(expected_config_revision) = snapshot.config_revision else {
            return;
        };
        let server_url = self
            .multiplayer_server_input
            .read(cx)
            .value()
            .trim()
            .to_owned();
        if server_url.is_empty() {
            return;
        }
        let current_url = snapshot.multiplayer_server_url.clone();
        let client = self.client.clone();
        cx.spawn(async move |this, cx| {
            let saved = if current_url != server_url {
                client
                    .set_multiplayer_server_url(expected_config_revision, server_url)
                    .await
                    .map(Some)
            } else {
                Ok(None)
            };
            let result = match saved {
                Ok(saved_snapshot) => client
                    .connect_multiplayer_service()
                    .await
                    .map(|connected_snapshot| (saved_snapshot, connected_snapshot)),
                Err(error) => Err(error),
            };
            let _ = this.update(cx, |view, cx| {
                match result {
                    Ok((saved_snapshot, connected_snapshot)) => {
                        if let Some(snapshot) = saved_snapshot {
                            view.apply_snapshot_if_newer(snapshot);
                        }
                        view.apply_snapshot_if_newer(connected_snapshot);
                    }
                    Err(error) => view.pending_notification = Some(error),
                }
                cx.notify();
            });
        })
        .detach();
    }

    pub(super) fn disconnect_multiplayer_service(&mut self, cx: &mut Context<Self>) {
        let client = self.client.clone();
        cx.spawn(async move |this, cx| {
            let result = client.disconnect_multiplayer_service().await;
            let _ = this.update(cx, |view, cx| {
                match result {
                    Ok(snapshot) => view.apply_snapshot_if_newer(snapshot),
                    Err(error) => view.pending_notification = Some(error),
                }
                cx.notify();
            });
        })
        .detach();
    }

    /// Persist the service address the input currently holds. A no-op while
    /// the value matches the configuration, so pressing Enter twice does not
    /// write twice.
    pub(super) fn commit_multiplayer_server_url(&mut self, cx: &mut Context<Self>) {
        let Some(expected_config_revision) = self
            .snapshot
            .as_ref()
            .and_then(|snapshot| snapshot.config_revision)
        else {
            return;
        };
        let value = self
            .multiplayer_server_input
            .read(cx)
            .value()
            .trim()
            .to_owned();
        if self
            .snapshot
            .as_ref()
            .is_some_and(|snapshot| snapshot.multiplayer_server_url == value)
        {
            return;
        }
        self.start_request(
            PendingOperation::MultiplayerServerUrl,
            Some(SettingValue::MultiplayerServerUrl {
                expected_config_revision,
                server_url: value,
            }),
            cx,
        );
    }

    /// Persist the nickname the input currently holds.
    pub(super) fn commit_multiplayer_nickname(&mut self, cx: &mut Context<Self>) {
        let Some(expected_config_revision) = self
            .snapshot
            .as_ref()
            .and_then(|snapshot| snapshot.config_revision)
        else {
            return;
        };
        let value = self
            .multiplayer_nickname_input
            .read(cx)
            .value()
            .trim()
            .to_owned();
        if self
            .snapshot
            .as_ref()
            .is_some_and(|snapshot| snapshot.multiplayer_nickname == value)
        {
            return;
        }
        self.start_request(
            PendingOperation::MultiplayerNickname,
            Some(SettingValue::MultiplayerNickname {
                expected_config_revision,
                nickname: value,
            }),
            cx,
        );
    }

    /// Create a room with the name (and password, when one is typed) the page
    /// holds. The room itself arrives through the projection.
    pub(super) fn create_multiplayer_room(&mut self, cx: &mut Context<Self>) {
        let room_name = self
            .multiplayer_room_name_input
            .read(cx)
            .value()
            .trim()
            .to_owned();
        let password = self
            .multiplayer_password_input
            .read(cx)
            .value()
            .trim()
            .to_owned();
        let client = self.client.clone();
        cx.spawn(async move |this, cx| {
            let result = client.create_multiplayer_room(room_name, password).await;
            let _ = this.update(cx, |view, cx| {
                match result {
                    Ok(snapshot) => view.apply_snapshot_if_newer(snapshot),
                    Err(error) => view.pending_notification = Some(error),
                }
                cx.notify();
            });
        })
        .detach();
    }

    /// Join the room with the given id, using the password input's value when
    /// one is needed.
    pub(super) fn join_multiplayer_room(&mut self, room_id: String, cx: &mut Context<Self>) {
        let password = self
            .multiplayer_password_input
            .read(cx)
            .value()
            .trim()
            .to_owned();
        let client = self.client.clone();
        cx.spawn(async move |this, cx| {
            let result = client.join_multiplayer_room(room_id, password).await;
            let _ = this.update(cx, |view, cx| {
                match result {
                    Ok(snapshot) => view.apply_snapshot_if_newer(snapshot),
                    Err(error) => view.pending_notification = Some(error),
                }
                cx.notify();
            });
        })
        .detach();
    }

    pub(super) fn leave_multiplayer_room(&mut self, cx: &mut Context<Self>) {
        let client = self.client.clone();
        cx.spawn(async move |this, cx| {
            let result = client.leave_multiplayer_room().await;
            let _ = this.update(cx, |view, cx| {
                match result {
                    Ok(snapshot) => view.apply_snapshot_if_newer(snapshot),
                    Err(error) => view.pending_notification = Some(error),
                }
                cx.notify();
            });
        })
        .detach();
    }

    /// Send the chat input's content and clear the input. The bubble and the
    /// history line both arrive through the server's own broadcast, so the
    /// page does not optimistically append anything.
    pub(super) fn send_multiplayer_chat(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let content = self
            .multiplayer_chat_input
            .read(cx)
            .value()
            .trim()
            .to_owned();
        if content.is_empty() {
            return;
        }
        self.multiplayer_chat_input.update(cx, |input, cx| {
            input.set_value("", window, cx);
        });
        let client = self.client.clone();
        cx.spawn(async move |this, cx| {
            let result = client.send_multiplayer_chat(content).await;
            let _ = this.update(cx, |view, cx| {
                match result {
                    Ok(snapshot) => view.apply_snapshot_if_newer(snapshot),
                    Err(error) => view.pending_notification = Some(error),
                }
                cx.notify();
            });
        })
        .detach();
    }

    pub(super) fn kick_multiplayer_member(&mut self, member_id: String, cx: &mut Context<Self>) {
        let client = self.client.clone();
        cx.spawn(async move |this, cx| {
            let result = client.kick_multiplayer_member(member_id).await;
            let _ = this.update(cx, |view, cx| {
                match result {
                    Ok(snapshot) => view.apply_snapshot_if_newer(snapshot),
                    Err(error) => view.pending_notification = Some(error),
                }
                cx.notify();
            });
        })
        .detach();
    }

    pub(super) fn refresh_multiplayer_lobby(&mut self, cx: &mut Context<Self>) {
        let client = self.client.clone();
        cx.spawn(async move |this, cx| {
            let result = client.refresh_multiplayer_lobby().await;
            let _ = this.update(cx, |view, cx| {
                match result {
                    Ok(snapshot) => view.apply_snapshot_if_newer(snapshot),
                    Err(error) => view.pending_notification = Some(error),
                }
                cx.notify();
            });
        })
        .detach();
    }

    /// Surface one multiplayer failure exactly once: the projection carries a
    /// sequence with its last error, and the window shows the new one as a
    /// notification the next frame.
    pub(super) fn observe_multiplayer_error(&mut self) {
        let Some(snapshot) = self.snapshot.as_ref() else {
            return;
        };
        let Some(error) = snapshot.multiplayer.last_error.as_ref() else {
            return;
        };
        if error.seq > self.multiplayer_error_seq_seen {
            self.multiplayer_error_seq_seen = error.seq;
            self.multiplayer_error_pending = Some(error.code);
        }
    }
}

/// One input bound to a view-held `InputState`, sized like the row's controls.
fn input_element(
    view: &Entity<SettingsView>,
    read: impl Fn(&SettingsView) -> &Entity<InputState> + Copy + 'static,
) -> impl Fn(&RenderOptions, &mut Window, &mut App) -> AnyElement + 'static {
    let view = view.clone();
    move |options: &RenderOptions, _: &mut Window, app: &mut App| {
        let state = read(view.read(app)).clone();
        div()
            .w_full()
            .child(Input::new(&state).with_size(options.size()).w_full())
            .into_any_element()
    }
}

/// The page is split into connection, room, and chat groups so the controls do
/// not compete for one horizontal row at the compact settings width.
pub(super) fn groups(
    view: Entity<SettingsView>,
    snapshot: Option<&SettingsSnapshot>,
    language: SettingsLanguage,
    keywords: Vec<SharedString>,
) -> Vec<SettingGroup> {
    let locale = language.catalog_locale();
    let multiplayer = snapshot.map(|snapshot| &snapshot.multiplayer);
    let in_room = multiplayer.is_some_and(|multiplayer| multiplayer.room.is_some());
    let configured = snapshot.is_some_and(|snapshot| {
        !snapshot.multiplayer_server_url.trim().is_empty()
            && !snapshot.multiplayer_nickname.trim().is_empty()
    });
    let connecting = multiplayer.is_some_and(|multiplayer| multiplayer.is_connecting());
    let status_key = match multiplayer.map(|multiplayer| multiplayer.status) {
        Some(SettingsMultiplayerStatus::Connecting) => {
            "settings.multiplayer_room.status.connecting"
        }
        Some(SettingsMultiplayerStatus::Connected) => "settings.multiplayer_room.status.connected",
        Some(SettingsMultiplayerStatus::Failed(_)) => "settings.multiplayer_room.status.failed",
        _ => "settings.multiplayer_room.status.disconnected",
    };

    let mut connection_items: Vec<SettingItem> = Vec::new();

    // The connection's own row: what the page believes, one word, always.
    connection_items.push(
        SettingItem::new(
            bongocat_i18n::text(locale, "settings.multiplayer_room.status.title"),
            SettingField::element(move |_: &RenderOptions, _: &mut Window, _: &mut App| {
                Label::new(String::new()).into_any_element()
            }),
        )
        .description(bongocat_i18n::text(locale, status_key))
        .keywords(keywords.clone()),
    );

    // The two persisted settings. Both commit on Enter, which the first row's
    // description explains once for both.
    let server_view = view.clone();
    connection_items.push(
        SettingItem::new(
            bongocat_i18n::text(locale, "settings.multiplayer_room.server_url.title"),
            SettingField::element(
                move |options: &RenderOptions, _: &mut Window, app: &mut App| {
                    let state = server_view.read(app).multiplayer_server_input.clone();
                    let action_view = server_view.clone();
                    let server_url = state.read(app).value().trim().to_owned();
                    let status = action_view
                        .read(app)
                        .snapshot
                        .as_ref()
                        .map(|snapshot| snapshot.multiplayer.status);
                    let connecting = matches!(status, Some(SettingsMultiplayerStatus::Connecting));
                    let connected = matches!(status, Some(SettingsMultiplayerStatus::Connected));
                    div()
                        .flex()
                        .items_center()
                        .gap_2()
                        .w_full()
                        .child(Input::new(&state).with_size(options.size()).flex_1())
                        .child(
                            Button::new("multiplayer-connect-button")
                                .label(bongocat_i18n::text(
                                    locale,
                                    if connected {
                                        "settings.multiplayer_room.connection.disconnect"
                                    } else {
                                        "settings.multiplayer_room.connection.action"
                                    },
                                ))
                                .with_size(options.size())
                                .primary()
                                .disabled(connecting || (!connected && server_url.is_empty()))
                                .on_click(move |_, _, app| {
                                    action_view.update(app, |view, cx| {
                                        if connected {
                                            view.disconnect_multiplayer_service(cx);
                                        } else {
                                            view.connect_multiplayer_service(cx);
                                        }
                                    });
                                }),
                        )
                        .into_any_element()
                },
            ),
        )
        .description(bongocat_i18n::format_text(
            locale,
            "settings.multiplayer_room.connection.description",
            &[(
                "action",
                bongocat_i18n::text(locale, "settings.multiplayer_room.connection.enter")
                    .to_owned(),
            )],
        ))
        .layout(Axis::Vertical)
        .keywords(keywords.clone()),
    );
    let nickname_view = view.clone();
    connection_items.push(
        SettingItem::new(
            bongocat_i18n::text(locale, "settings.multiplayer_room.nickname.title"),
            SettingField::element(input_element(&nickname_view, |view| {
                &view.multiplayer_nickname_input
            })),
        )
        .layout(Axis::Vertical),
    );

    let room_items = match multiplayer.and_then(|multiplayer| multiplayer.room.as_ref()) {
        Some(room) => room_items(&view, room, configured, connecting, language, &keywords),
        None => lobby_items(
            &view,
            multiplayer,
            configured,
            connecting,
            language,
            &keywords,
        ),
    };
    let chat_items = chat_items(&view, multiplayer, in_room, language, &keywords);

    vec![
        SettingGroup::new()
            .title(bongocat_i18n::text(
                locale,
                "settings.multiplayer_room.sections.connection.title",
            ))
            .description(bongocat_i18n::text(
                locale,
                "settings.multiplayer_room.connection.description",
            ))
            .items(connection_items),
        SettingGroup::new()
            .title(bongocat_i18n::text(
                locale,
                "settings.multiplayer_room.sections.room.title",
            ))
            .description(bongocat_i18n::text(
                locale,
                if in_room {
                    "settings.multiplayer_room.sections.room.in_room"
                } else {
                    "settings.multiplayer_room.sections.room.outside_room"
                },
            ))
            .items(room_items),
        SettingGroup::new()
            .title(bongocat_i18n::text(
                locale,
                "settings.multiplayer_room.sections.chat.title",
            ))
            .items(chat_items),
    ]
}

/// The rows shown while the connection sits in a room: the room itself, one
/// row per member, and the leave action.
fn room_items(
    view: &Entity<SettingsView>,
    room: &SettingsRoomView,
    configured: bool,
    connecting: bool,
    language: SettingsLanguage,
    keywords: &[SharedString],
) -> Vec<SettingItem> {
    let locale = language.catalog_locale();
    let mut items = Vec::new();

    let room_view = view.clone();
    let room_id = room.room_id.clone();
    let member_line = bongocat_i18n::format_text(
        locale,
        "settings.multiplayer_room.members.count",
        &[("count", room.members.len().to_string())],
    );
    items.push(
        SettingItem::new(
            room.name.clone(),
            SettingField::element(
                move |options: &RenderOptions, _: &mut Window, app: &mut App| {
                    let busy = room_view.read(app).pending.is_some();
                    let action_view = room_view.clone();
                    Button::new("multiplayer-leave-button")
                        .label(bongocat_i18n::text(
                            language.catalog_locale(),
                            "settings.multiplayer_room.room.leave.action",
                        ))
                        .with_size(options.size())
                        .danger()
                        .outline()
                        .disabled(busy || connecting)
                        .on_click(move |_, _, app| {
                            action_view.update(app, |view, cx| view.leave_multiplayer_room(cx));
                        })
                        .into_any_element()
                },
            ),
        )
        .description(bongocat_i18n::format_text(
            locale,
            "settings.multiplayer_room.room.info",
            &[("id", room_id.clone()), ("count", member_line.clone())],
        ))
        .keywords(keywords.to_vec()),
    );

    for member in &room.members {
        let member_view = view.clone();
        let member_id = member.id.clone();
        let marker = if member.is_self {
            Some(bongocat_i18n::text(
                locale,
                "settings.multiplayer_room.members.you",
            ))
        } else if member.is_host {
            Some(bongocat_i18n::text(
                locale,
                "settings.multiplayer_room.members.host",
            ))
        } else {
            None
        };
        let can_kick = member.is_host && !member.is_self;
        let model_line = member
            .model_name
            .as_deref()
            .map(|model| {
                bongocat_i18n::format_text(
                    locale,
                    "settings.multiplayer_room.members.model",
                    &[("model", model.to_owned())],
                )
            })
            .unwrap_or_default();
        items.push(
            SettingItem::new(
                member.name.clone(),
                SettingField::element(
                    move |options: &RenderOptions, _: &mut Window, _: &mut App| {
                        if can_kick && configured {
                            let kick_view = member_view.clone();
                            let kick_id = member_id.clone();
                            Button::new(SharedString::from(format!("multiplayer-kick-{kick_id}")))
                                .label(bongocat_i18n::text(
                                    language.catalog_locale(),
                                    "settings.multiplayer_room.members.kick",
                                ))
                                .with_size(options.size())
                                .with_variant(ButtonVariant::Default)
                                .on_click(move |_, _, app| {
                                    let member_id = kick_id.clone();
                                    kick_view.update(app, |view, cx| {
                                        view.kick_multiplayer_member(member_id, cx)
                                    });
                                })
                                .into_any_element()
                        } else {
                            Label::new(String::new()).into_any_element()
                        }
                    },
                ),
            )
            .description({
                let marker_text = marker.unwrap_or("");
                [marker_text, model_line.as_str()]
                    .into_iter()
                    .filter(|part| !part.is_empty())
                    .collect::<Vec<_>>()
                    .join(" · ")
            })
            .keywords(keywords.to_vec()),
        );
    }

    items
}

/// The rows shown while outside a room: create, join, and the lobby list.
fn lobby_items(
    view: &Entity<SettingsView>,
    multiplayer: Option<&SettingsMultiplayer>,
    configured: bool,
    connecting: bool,
    language: SettingsLanguage,
    keywords: &[SharedString],
) -> Vec<SettingItem> {
    let locale = language.catalog_locale();
    let mut items = Vec::new();

    let create_view = view.clone();
    items.push(
        SettingItem::render(
            move |options: &RenderOptions, _: &mut Window, app: &mut App| {
                let state = create_view.read(app).multiplayer_room_name_input.clone();
                let action_view = create_view.clone();
                let busy = action_view.read(app).pending.is_some() || connecting;
                div()
                    .flex_col()
                    .gap_2()
                    .w_full()
                    .child(Label::new(bongocat_i18n::text(
                        language.catalog_locale(),
                        "settings.multiplayer_room.room.create.title",
                    )))
                    .child(
                        Label::new(bongocat_i18n::text(
                            language.catalog_locale(),
                            "settings.multiplayer_room.room.create.name_placeholder",
                        ))
                        .text_sm()
                        .text_color(app.theme().muted_foreground),
                    )
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap_2()
                            .w_full()
                            .child(Input::new(&state).with_size(options.size()).flex_1())
                            .child(
                                Button::new("multiplayer-create-button")
                                    .label(bongocat_i18n::text(
                                        language.catalog_locale(),
                                        "settings.multiplayer_room.room.create.action",
                                    ))
                                    .with_size(options.size())
                                    .primary()
                                    .disabled(busy || !configured)
                                    .on_click(move |_, _, app| {
                                        action_view.update(app, |view, cx| {
                                            view.create_multiplayer_room(cx)
                                        });
                                    }),
                            ),
                    )
                    .into_any_element()
            },
        )
        .keywords(keywords.to_vec()),
    );

    let join_view = view.clone();
    items.push(
        SettingItem::render(
            move |options: &RenderOptions, _: &mut Window, app: &mut App| {
                let id_state = join_view.read(app).multiplayer_room_id_input.clone();
                let password_state = join_view.read(app).multiplayer_password_input.clone();
                let action_view = join_view.clone();
                let room_id = action_view
                    .read(app)
                    .multiplayer_room_id_input
                    .read(app)
                    .value()
                    .trim()
                    .to_owned();
                let busy = action_view.read(app).pending.is_some() || connecting;
                div()
                    .flex_col()
                    .gap_2()
                    .w_full()
                    .child(Label::new(bongocat_i18n::text(
                        language.catalog_locale(),
                        "settings.multiplayer_room.room.join.title",
                    )))
                    .child(
                        Label::new(bongocat_i18n::text(
                            language.catalog_locale(),
                            "settings.multiplayer_room.room.password.placeholder",
                        ))
                        .text_sm()
                        .text_color(app.theme().muted_foreground),
                    )
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap_2()
                            .w_full()
                            .child(Input::new(&id_state).with_size(options.size()).flex_1())
                            .child(
                                Input::new(&password_state)
                                    .with_size(options.size())
                                    .flex_1(),
                            )
                            .child(
                                Button::new("multiplayer-join-button")
                                    .label(bongocat_i18n::text(
                                        language.catalog_locale(),
                                        "settings.multiplayer_room.room.join.action",
                                    ))
                                    .with_size(options.size())
                                    .primary()
                                    .disabled(busy || !configured || room_id.is_empty())
                                    .on_click(move |_, _, app| {
                                        let room_id = room_id.clone();
                                        action_view.update(app, |view, cx| {
                                            view.join_multiplayer_room(room_id, cx)
                                        });
                                    }),
                            ),
                    )
                    .into_any_element()
            },
        )
        .keywords(keywords.to_vec()),
    );

    let lobby_status = multiplayer.map_or(SettingsLobbyStatus::Unloaded, |multiplayer| {
        multiplayer.lobby_status
    });
    let lobby_view = view.clone();
    items.push(
        SettingItem::new(
            bongocat_i18n::text(locale, "settings.multiplayer_room.lobby.title"),
            SettingField::element(
                move |options: &RenderOptions, _: &mut Window, app: &mut App| {
                    let busy = lobby_view.read(app).pending.is_some();
                    let action_view = lobby_view.clone();
                    Button::new("multiplayer-lobby-refresh-button")
                        .label(bongocat_i18n::text(
                            language.catalog_locale(),
                            "settings.multiplayer_room.lobby.action",
                        ))
                        .with_size(options.size())
                        .with_variant(ButtonVariant::Default)
                        .disabled(busy)
                        .on_click(move |_, _, app| {
                            action_view.update(app, |view, cx| view.refresh_multiplayer_lobby(cx));
                        })
                        .into_any_element()
                },
            ),
        )
        .description(
            match lobby_status {
                SettingsLobbyStatus::Unloaded => {
                    bongocat_i18n::text(locale, "settings.multiplayer_room.lobby.empty_hint")
                }
                SettingsLobbyStatus::Loading => {
                    bongocat_i18n::text(locale, "settings.multiplayer_room.lobby.loading")
                }
                SettingsLobbyStatus::Failed => {
                    bongocat_i18n::text(locale, "settings.multiplayer_room.lobby.failed")
                }
                SettingsLobbyStatus::Ready => "",
            }
            .to_owned(),
        )
        .keywords(keywords.to_vec()),
    );

    if let Some(multiplayer) = multiplayer {
        if matches!(lobby_status, SettingsLobbyStatus::Ready) && multiplayer.lobby.is_empty() {
            items.push(
                SettingItem::new(
                    bongocat_i18n::text(locale, "settings.multiplayer_room.lobby.empty_title"),
                    SettingField::element(|_: &RenderOptions, _: &mut Window, _: &mut App| {
                        Label::new(String::new()).into_any_element()
                    }),
                )
                .description(bongocat_i18n::text(
                    locale,
                    "settings.multiplayer_room.lobby.empty_description",
                ))
                .keywords(keywords.to_vec()),
            );
        }
        for room in &multiplayer.lobby {
            let row_view = view.clone();
            let row_id = room.room_id.clone();
            let detail = bongocat_i18n::format_text(
                locale,
                "settings.multiplayer_room.lobby.row",
                &[
                    ("count", room.member_count.to_string()),
                    ("max", room.max_members.to_string()),
                    ("host", room.host_name.clone()),
                ],
            );
            items.push(
                SettingItem::new(
                    room.name.clone(),
                    SettingField::element(
                        move |options: &RenderOptions, _: &mut Window, _: &mut App| {
                            let row_id = row_id.clone();
                            let action_view = row_view.clone();
                            Button::new(SharedString::from(format!(
                                "multiplayer-lobby-join-{row_id}"
                            )))
                            .label(bongocat_i18n::text(
                                language.catalog_locale(),
                                "settings.multiplayer_room.lobby.join.action",
                            ))
                            .with_size(options.size())
                            .with_variant(ButtonVariant::Default)
                            .disabled(connecting || !configured)
                            .on_click(move |_, _, app| {
                                let room_id = row_id.clone();
                                action_view.update(app, |view, cx| {
                                    view.join_multiplayer_room(room_id, cx)
                                });
                            })
                            .into_any_element()
                        },
                    ),
                )
                .description(detail)
                .keywords(keywords.to_vec()),
            );
        }
    }

    items
}

/// The chat rows: the history when in a room, the input to send with, and the
/// one-line explanation of where the messages appear.
fn chat_items(
    view: &Entity<SettingsView>,
    multiplayer: Option<&SettingsMultiplayer>,
    in_room: bool,
    language: SettingsLanguage,
    keywords: &[SharedString],
) -> Vec<SettingItem> {
    let locale = language.catalog_locale();
    let mut items = Vec::new();

    let lines: Vec<String> = multiplayer
        .map(|multiplayer| {
            multiplayer
                .chat
                .iter()
                .rev()
                .take(8)
                .rev()
                .map(|message| format!("{}: {}", message.sender, message.content))
                .collect()
        })
        .unwrap_or_default();

    if in_room && !lines.is_empty() {
        let chat_lines = lines;
        items.push(
            SettingItem::new(
                bongocat_i18n::text(locale, "settings.multiplayer_room.chat.title"),
                SettingField::element(move |_: &RenderOptions, _: &mut Window, _: &mut App| {
                    div()
                        .flex()
                        .flex_col()
                        .gap_1()
                        .children(
                            chat_lines
                                .iter()
                                .cloned()
                                .map(|line| Label::new(line).into_any_element()),
                        )
                        .into_any_element()
                }),
            )
            .keywords(keywords.to_vec()),
        );
    }

    let chat_view = view.clone();
    items.push(
        SettingItem::render(
            move |options: &RenderOptions, _: &mut Window, app: &mut App| {
                let state = chat_view.read(app).multiplayer_chat_input.clone();
                let action_view = chat_view.clone();
                let description_key = if in_room {
                    "settings.multiplayer_room.chat.placeholder"
                } else {
                    "settings.multiplayer_room.chat.hint"
                };
                div()
                    .flex_col()
                    .gap_2()
                    .w_full()
                    .child(Label::new(bongocat_i18n::text(
                        language.catalog_locale(),
                        if in_room {
                            "settings.multiplayer_room.chat.input.title"
                        } else {
                            "settings.multiplayer_room.chat.title"
                        },
                    )))
                    .child(
                        Label::new(bongocat_i18n::text(
                            language.catalog_locale(),
                            description_key,
                        ))
                        .text_sm()
                        .text_color(app.theme().muted_foreground),
                    )
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap_2()
                            .w_full()
                            .child(Input::new(&state).with_size(options.size()).flex_1())
                            .child(
                                Button::new("multiplayer-chat-send-button")
                                    .label(bongocat_i18n::text(
                                        language.catalog_locale(),
                                        "settings.multiplayer_room.chat.action",
                                    ))
                                    .with_size(options.size())
                                    .primary()
                                    .disabled(!in_room)
                                    .on_click(move |event, window, app| {
                                        action_view.update(app, |view, cx| {
                                            view.send_multiplayer_chat(window, cx)
                                        });
                                        let _ = event;
                                    }),
                            ),
                    )
                    .into_any_element()
            },
        )
        .keywords(keywords.to_vec()),
    );

    items
}
