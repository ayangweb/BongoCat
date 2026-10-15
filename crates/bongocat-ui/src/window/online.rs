use super::*;
use bongocat_ui_protocol::{OnlineError, OnlinePhase, RoomError};

pub(super) fn error_key(error: OnlineError) -> &'static str {
    match error {
        OnlineError::InvalidAddress => "online.error.invalid_address",
        OnlineError::ServiceUnavailable => "online.error.unavailable",
        OnlineError::ConnectionFailed => "online.error.connection",
        OnlineError::InvalidProtocol | OnlineError::Room(RoomError::ProtocolMismatch) => {
            "online.error.protocol"
        }
        OnlineError::Room(RoomError::RoomFull) => "online.error.full",
        OnlineError::Room(RoomError::RateLimited) => "online.error.rate",
        OnlineError::Room(RoomError::InvalidMessage) => "online.error.protocol",
        OnlineError::RenderFailed => "online.error.render",
    }
}

impl SettingsView {
    pub(super) fn start_online_polling(&self, cx: &mut Context<Self>) {
        let executor = cx.background_executor().clone();
        cx.spawn(async move |this, cx| {
            let mut revision = None;
            loop {
                executor.timer(Duration::from_millis(200)).await;
                if this
                    .update(cx, |view, cx| {
                        if view.window_hidden() {
                            return;
                        }
                        let current = view
                            .client
                            .online_client()
                            .map(|client| client.snapshot().revision);
                        if revision != current {
                            revision = current;
                            cx.notify();
                        }
                    })
                    .is_err()
                {
                    break;
                }
            }
        })
        .detach();
    }

    pub(super) fn join_online(&mut self, cx: &mut Context<Self>) {
        if self.online_join_pending {
            return;
        }
        let Some(online) = self.client.online_client() else {
            return;
        };
        let address = self
            .online_address
            .read(cx)
            .value()
            .to_string()
            .trim()
            .to_owned();
        if !bongocat_ui_protocol::valid_server_address(&address) {
            self.online_error = Some(OnlineError::InvalidAddress);
            cx.notify();
            return;
        }
        let client = self.client.clone();
        self.online_join_pending = true;
        let generation = self.online_request_generation;
        cx.notify();
        cx.spawn(async move |this, cx| {
            // Persistence and revision checking run on the settings worker, never
            // the GPUI executor. Joining starts only after the address was saved.
            let result = async {
                let current = client.read_snapshot().await?;
                let revision = current
                    .config_revision
                    .ok_or_else(|| SettingsError::new(SettingsErrorCode::ServiceUnavailable))?;
                client.set_online_server(revision, address.clone()).await
            }
            .await;
            let _ = this.update(cx, |view, cx| {
                view.online_join_pending = false;
                if view.online_request_generation != generation {
                    cx.notify();
                    return;
                }
                match result {
                    Ok(snapshot) => {
                        view.snapshot = Some(snapshot);
                        if let Err(error) = online.join(address) {
                            view.online_error = Some(error);
                        } else {
                            view.online_error = None;
                        }
                    }
                    Err(error) => view.report_service_error(error, cx),
                }
                cx.notify();
            });
        })
        .detach();
    }
    pub(super) fn leave_online(&mut self, cx: &mut Context<Self>) {
        self.online_request_generation += 1;
        self.online_error = None;
        if let Some(online) = self.client.online_client() {
            online.leave();
        }
        cx.notify();
    }
}

pub(super) fn page(view: Entity<SettingsView>, language: SettingsLanguage) -> SettingPage {
    let locale = language.catalog_locale();
    let input_view = view.clone();
    let address = SettingItem::new(
        bongocat_i18n::text(locale, "online.address"),
        SettingField::element(move |_: &RenderOptions, _: &mut Window, app: &mut App| {
            let view = input_view.read(app);
            let connected = view.client.online_client().is_some_and(|client| {
                client.snapshot().phase != OnlinePhase::Offline
                    && client.snapshot().phase != OnlinePhase::Failed
            });
            Input::new(&view.online_address)
                .disabled(connected || view.online_join_pending)
                .into_any_element()
        }),
    )
    .description(bongocat_i18n::text(locale, "online.privacy"));
    let action_view = view.clone();
    let actions = SettingItem::new(
        bongocat_i18n::text(locale, "online.room"),
        SettingField::element(move |_: &RenderOptions, _: &mut Window, app: &mut App| {
            let current = action_view.read(app);
            let phase = current
                .client
                .online_client()
                .map(|client| client.snapshot().phase)
                .unwrap_or_default();
            let pending = current.online_join_pending;
            let available = current.client.online_client().is_some() && current.snapshot.is_some();
            let is_idle = matches!(phase, OnlinePhase::Offline | OnlinePhase::Failed);
            let join_view = action_view.clone();
            let leave_view = action_view.clone();
            let retry_view = action_view.clone();
            div()
                .flex()
                .gap_2()
                .child(
                    Button::new("online-join")
                        .label(bongocat_i18n::text(
                            locale,
                            if phase == OnlinePhase::Failed {
                                "online.retry"
                            } else {
                                "online.join"
                            },
                        ))
                        .primary()
                        .loading(pending)
                        .disabled(!available || !is_idle || pending)
                        .on_click(move |_, _, app| {
                            join_view.update(app, |view, cx| view.join_online(cx));
                        }),
                )
                .child(
                    Button::new("online-leave")
                        .label(bongocat_i18n::text(
                            locale,
                            if phase == OnlinePhase::Connected {
                                "online.leave"
                            } else {
                                "online.cancel"
                            },
                        ))
                        .disabled(!available || (is_idle && !pending))
                        .on_click(move |_, _, app| {
                            leave_view.update(app, |view, cx| view.leave_online(cx));
                        }),
                )
                .when(
                    current
                        .client
                        .online_client()
                        .is_some_and(|client| !client.snapshot().render_failures.is_empty()),
                    |element| {
                        element.child(
                            Button::new("online-render-retry")
                                .label(bongocat_i18n::text(locale, "online.retry_render"))
                                .on_click(move |_, _, app| {
                                    retry_view.update(app, |view, cx| {
                                        if let Some(client) = view.client.online_client() {
                                            client.retry_rendering();
                                        }
                                        cx.notify();
                                    });
                                }),
                        )
                    },
                )
                .into_any_element()
        }),
    );
    let status_view = view.clone();
    let status = SettingItem::new(
        bongocat_i18n::text(locale, "online.status"),
        SettingField::element(move |_: &RenderOptions, _: &mut Window, app: &mut App| {
            let current = status_view.read(app);
            let snapshot = current
                .client
                .online_client()
                .map(|client| client.snapshot())
                .unwrap_or_default();
            let key = match snapshot.phase {
                OnlinePhase::Offline => "online.phase.offline",
                OnlinePhase::Connecting => "online.phase.connecting",
                OnlinePhase::Connected => "online.phase.connected",
                OnlinePhase::Reconnecting => "online.phase.reconnecting",
                OnlinePhase::Failed => "online.phase.failed",
            };
            div()
                .child(bongocat_i18n::text(locale, key))
                .into_any_element()
        }),
    );
    let members_view = view.clone();
    let members = SettingItem::new(
        bongocat_i18n::text(locale, "online.members"),
        SettingField::element(move |_: &RenderOptions, _: &mut Window, app: &mut App| {
            let current = members_view.read(app);
            let snapshot = current
                .client
                .online_client()
                .map(|client| client.snapshot())
                .unwrap_or_default();
            let own = snapshot.you;
            div()
                .flex()
                .flex_col()
                .gap_2()
                .when(snapshot.members.is_empty(), |element| {
                    element.child(bongocat_i18n::text(locale, "online.empty"))
                })
                .children(snapshot.members.iter().map(|id| {
                    let suffix = if Some(*id) == own {
                        bongocat_i18n::text(locale, "online.you")
                    } else if snapshot.render_failures.contains(id) {
                        bongocat_i18n::text(locale, "online.error.render")
                    } else {
                        String::new()
                    };
                    div().child(format!("ID {} {suffix}", id.0))
                }))
                .into_any_element()
        }),
    );
    SettingPage::new(bongocat_i18n::text(locale, "navigation.online.title"))
        .icon(gpui_kit::assets::IconName::Cat)
        .group(SettingGroup::new().items([address, actions, status, members]))
}
