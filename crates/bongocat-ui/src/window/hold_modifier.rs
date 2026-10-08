//! Recording the one modifier key the overlay watches for.
//!
//! The shortcut recorder above it cannot be reused as-is, and the reason is the
//! keyboard rather than the code. GPUI reports a modifier as a boolean flag
//! change carrying no key identity: on macOS a modifier press produces no key
//! event at all, and on Windows both shift virtual-key codes collapse into the
//! same modifier branch. There is therefore no key event to read, no left/right to
//! tell apart, and nothing that would distinguish a held modifier from the flag
//! left over after the release.
//!
//! So this control reads the runtime's pressed set instead, which is where the
//! physical key is actually known. That makes the gesture a *hold* rather than a
//! tap: the key has to still be down when the recorder next looks, which is
//! exactly the gesture the setting itself asks the user to make, and it is why a
//! short tap cannot slip past the recorder.

use super::*;
use gpui_kit::AnyElement;
use gpui_kit::base::TestSupportExt as _;
use gpui_kit::component::Sizable as _;

/// How often the recorder looks at the pressed set while it is armed.
///
/// Short enough that the row has already filled in by the time a user who is
/// holding a key looks at it, and long enough that the settings channel is not
/// carrying a steady stream of requests. Each one is an atomic read of state the
/// runtime already publishes on every input edge, so this is not a per-frame cost
/// on either side.
pub(crate) const RECORDER_POLL_INTERVAL: Duration = Duration::from_millis(50);

/// Keyboard tab positions for the recorder and the control beside it.
///
/// The recorder, the import card, the model rows and the shortcut rows each own a
/// band; this row is on the Overlay page and takes the one between the card and
/// the model list.
const RECORDER_TAB_INDEX: isize = 30;
const RECORDER_CLEAR_TAB_INDEX: isize = 31;

/// The row that records the modifier the overlay watches for.
///
/// It deliberately does not take a [`SettingGate`] from a switch. It is bound by
/// two switches — click-through and hover-hide — that sit on either side of it,
/// and the gate rule binds one switch to the rows below it. Which of the two owns
/// this row would be arbitrary, and a row greyed out until an unrelated switch is
/// on is worse than one that stores a value that takes effect the moment either
/// switch is turned on. Only structural editing blocking disables it.
///
/// The frame is the shortcut recorder's, deliberately: the two controls do the
/// same gesture and the page reads as one idea rather than two.
pub(super) fn hold_modifier_row(
    view: &Entity<SettingsView>,
    language: SettingsLanguage,
    editing_blocked: bool,
) -> SettingItem {
    let view = view.clone();
    SettingItem::new(
        bongocat_i18n::text(
            language.catalog_locale(),
            "settings.overlay.hold_modifier_to_interact.label",
        ),
        SettingField::element(
            move |options: &RenderOptions, window: &mut Window, app: &mut App| {
                hold_modifier_control(&view, language, editing_blocked, options, window, app)
            },
        ),
    )
    .description(hold_modifier_row_description(language))
    .disabled(editing_blocked)
}

/// The row's description, which is the one description the Model window page
/// carries.
///
/// ADR-0066 allows a row description exactly where the title and the control do
/// not make the behaviour clear, and this is that case. The title says what the
/// key is for but not which two settings it suspends; the control shows only the
/// recorded key; and nothing else on screen says that the gesture is a hold
/// rather than a toggle, or that both settings come back on release — which is
/// what a user would otherwise be unsure enough to leave the setting alone.
///
/// It also reaches the search index, so "穿透" or "click-through" now finds this
/// row from anywhere in the settings window.
pub(super) fn hold_modifier_row_description(language: SettingsLanguage) -> &'static str {
    bongocat_i18n::text(
        language.catalog_locale(),
        "settings.overlay.hold_modifier_to_interact.description",
    )
}

/// The recorder element itself, without the page row around it.
///
/// Split out from [`hold_modifier_row`] so the rendered tests can put the real
/// control inside the packaged `SettingItem::render` wrapper rather than a
/// hand-built approximation of it — the wrapper is what supplies the
/// `RenderOptions` the control reads its disabled state from.
pub(super) fn hold_modifier_control(
    view: &Entity<SettingsView>,
    language: SettingsLanguage,
    editing_blocked: bool,
    options: &RenderOptions,
    window: &mut Window,
    app: &mut App,
) -> AnyElement {
    let tokens = Tokens::from_theme(app);
    let disabled = editing_blocked || options.is_disabled();
    let view_state = view.read(app);
    let recording = view_state.hold_modifier_recording;
    let modifier = view_state
        .snapshot
        .as_ref()
        .and_then(|snapshot| snapshot.overlay.hold_modifier_to_interact);
    let recorder_focus = view_state.hold_modifier_recorder_focus.clone();
    let clear_focus = view_state.hold_modifier_clear_focus.clone();
    // Three states, the same three the shortcut recorder has: recording,
    // recorded, and empty. The recording copy says what to do rather than
    // naming a key, because nothing is decided until a key is held.
    let text = if recording {
        bongocat_i18n::text(
            language.catalog_locale(),
            "settings.overlay.hold_modifier_to_interact.recording",
        )
        .to_owned()
    } else if let Some(modifier) = modifier {
        modifier_key_display(modifier, language)
    } else {
        bongocat_i18n::text(
            language.catalog_locale(),
            "settings.overlay.hold_modifier_to_interact.record_placeholder",
        )
        .to_owned()
    };
    let recorder_focus_for_click = recorder_focus.clone();
    let recorder_focus_for_key = recorder_focus.clone();
    let mut frame = div()
        .id(ElementId::from("hold-modifier-recorder"))
        // Observed before the focus binding so the rendered test can click the
        // frame itself; the clear control sits inside it, and "a press that is not
        // on it starts a recording" is only measurable against the frame's bounds.
        .test_support()
        .key_context("SettingsControl")
        .track_focus(&recorder_focus)
        .tab_index(RECORDER_TAB_INDEX)
        .tab_stop(!disabled)
        .h(px(32.))
        .min_w(px(180.))
        .flex()
        .items_center()
        .gap_1()
        .px_1()
        .border_1()
        .border_color(if recording {
            tokens.accent
        } else {
            tokens.border
        })
        .rounded_md()
        .when(recording, |this| this.focus_ring_style(window, app))
        .cursor_pointer()
        .text_color(if recording || modifier.is_some() {
            tokens.text
        } else {
            tokens.muted
        })
        .when(disabled, |this| this.cursor_default())
        .child(
            div()
                .min_w_0()
                .flex_1()
                .flex()
                .items_center()
                .justify_center()
                .child(text),
        );
    if !disabled {
        frame = frame
            .on_click({
                let view = view.clone();
                move |_, window, cx| {
                    view.update(cx, |view, cx| {
                        view.begin_hold_modifier_recording(window, cx)
                    })
                }
            })
            .on_key_down({
                let view = view.clone();
                move |event, window, cx| {
                    if event.keystroke.key.eq_ignore_ascii_case("escape") {
                        cx.stop_propagation();
                        view.update(cx, |view, cx| view.cancel_hold_modifier_recording(cx));
                        window.blur(cx);
                        return;
                    }
                    if view.read(cx).hold_modifier_recording {
                        return;
                    }
                    if is_activation_key(event) {
                        cx.stop_propagation();
                        let focus = recorder_focus_for_key.clone();
                        view.update(cx, |view, cx| {
                            view.begin_hold_modifier_recording(window, cx)
                        });
                        window.focus(&focus, cx);
                    }
                }
            })
            // A press that starts outside the frame and lands on it must not read
            // as a click, the same way the shortcut recorder stops a mouse-down
            // that began outside it.
            .on_mouse_down_out({
                let view = view.clone();
                move |_, window, cx| {
                    view.update(cx, |view, cx| view.cancel_hold_modifier_recording(cx));
                    window.blur(cx);
                }
            });
    }
    let clear_disabled = disabled || modifier.is_none();
    let clear_view = view.clone();
    let clear_focus_for_key = clear_focus.clone();
    frame
        .child(
            div()
                .flex_none()
                .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                .child(
                    icon_command_control(
                        &clear_focus,
                        RECORDER_CLEAR_TAB_INDEX,
                        Button::new("clear-hold-modifier-control")
                            .ghost()
                            .xsmall()
                            .icon(gpui_kit::assets::IconName::Close)
                            .tooltip(bongocat_i18n::text(
                                language.catalog_locale(),
                                "shortcuts.actions.clear",
                            ))
                            .disabled(clear_disabled),
                    )
                    .id(ElementId::from("hold-modifier-clear"))
                    .test_support()
                    .when(!clear_disabled, |this| {
                        this.on_click({
                            let clear_view = clear_view.clone();
                            move |_, window, cx| {
                                clear_view.update(cx, |view, cx| view.clear_hold_modifier(cx));
                                window.focus(&recorder_focus_for_click, cx);
                            }
                        })
                        .on_key_down(move |event, window, cx| {
                            if is_activation_key(event) {
                                cx.stop_propagation();
                                clear_view.update(cx, |view, cx| view.clear_hold_modifier(cx));
                                window.focus(&clear_focus_for_key, cx);
                            }
                        })
                    }),
                ),
        )
        .into_any_element()
}

impl SettingsView {
    /// Arm the recorder and start watching for a held modifier.
    ///
    /// Nothing is sent to the service: the recorder has no persisted state of its
    /// own, so there is no ordering to wait for and no round trip to make the
    /// control feel slow. The first poll happens after one interval, by which
    /// time a key the user pressed in response to the click is already down.
    pub(super) fn begin_hold_modifier_recording(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.hold_modifier_recording || self.editing_blocked(self.snapshot.as_ref()) {
            return;
        }
        let focus = self.hold_modifier_recorder_focus.clone();
        self.hold_modifier_recording = true;
        self.hold_modifier_recording_generation =
            self.hold_modifier_recording_generation.wrapping_add(1);
        let generation = self.hold_modifier_recording_generation;
        // Losing focus ends the recording the same way it ends for the shortcut
        // rows: the user clicked away, and a recorder that keeps listening after
        // that would overwrite the value with whatever they type next.
        self.hold_modifier_blur_subscription = Some(cx.on_blur(&focus, window, |view, _, cx| {
            view.cancel_hold_modifier_recording(cx);
        }));
        window.focus(&focus, cx);
        cx.notify();

        let executor = cx.background_executor().clone();
        let client = self.client.clone();
        cx.spawn(async move |this, cx| {
            loop {
                executor.timer(RECORDER_POLL_INTERVAL).await;
                // The read comes first, and it is what suspends this loop: the
                // entity is not touched until the answer is in hand, which keeps
                // the poll out of the borrow the click handler is still holding.
                let Ok(pressed) = client.read_pressed_modifiers().await else {
                    // The service is gone, so the window is on its way out; there
                    // is nothing left to record into.
                    break;
                };
                // Ownership is settled before anything is written. A recording
                // that was cancelled, replaced by a later click, or closed with
                // the window still has a poll in flight, and its answer must not
                // reach the value — nor keep the loop asking.
                let owned = match this.update(cx, |view, _| {
                    view.hold_modifier_recording
                        && view.hold_modifier_recording_generation == generation
                        // Nothing is on screen to show a recording on, and the
                        // window is recreated for each open with a fresh view, so
                        // there is no value worth holding on to.
                        && !view.window_hidden()
                }) {
                    Ok(owned) => owned,
                    Err(_) => break,
                };
                if !owned {
                    break;
                }
                let Some(modifier) = pressed.first() else {
                    continue;
                };
                let recorded = this.update(cx, |view, cx| {
                    view.record_hold_modifier(modifier, cx);
                });
                if recorded.is_err() {
                    break;
                }
                break;
            }
        })
        .detach();
    }

    /// Store a modifier the recorder observed, and stop recording.
    ///
    /// Recording writes the whole overlay settings struct because that is the one
    /// typed command this page already sends for every other overlay control, and
    /// it carries the revision the change is expected to land on.
    fn record_hold_modifier(&mut self, modifier: ModifierKey, cx: &mut Context<Self>) {
        self.end_hold_modifier_recording(cx);
        self.write_hold_modifier(Some(modifier), cx);
    }

    /// Clear the recorded modifier, leaving the setting off.
    ///
    /// The setting is a plain value rather than a binding, so removing it is a
    /// write of `None` through the same command the recorder uses — there is no
    /// separate "unbind" path to keep consistent with it.
    pub(super) fn clear_hold_modifier(&mut self, cx: &mut Context<Self>) {
        self.end_hold_modifier_recording(cx);
        self.write_hold_modifier(None, cx);
    }

    /// Persist the modifier, or `None` to leave the setting off.
    ///
    /// Both directions go through this one write because there is a single
    /// persisted value and one typed command that carries it; two callers would
    /// only be an invitation to change one and forget the other.
    fn write_hold_modifier(&mut self, modifier: Option<ModifierKey>, cx: &mut Context<Self>) {
        if window_mode_active(self.snapshot.as_ref().map(|s| s.overlay)) {
            return;
        }
        if self.editing_blocked(self.snapshot.as_ref()) {
            return;
        }
        let Some(snapshot) = self.snapshot.as_ref() else {
            return;
        };
        if snapshot.overlay.hold_modifier_to_interact == modifier {
            return;
        }
        let Some(expected_config_revision) = snapshot.config_revision else {
            return;
        };
        let mut settings = snapshot.overlay;
        settings.hold_modifier_to_interact = modifier;
        self.start_request(
            PendingOperation::OverlaySettings,
            Some(SettingValue::OverlaySettings {
                expected_config_revision,
                settings,
            }),
            cx,
        );
    }

    pub(super) fn cancel_hold_modifier_recording(&mut self, cx: &mut Context<Self>) {
        if !self.hold_modifier_recording {
            return;
        }
        self.end_hold_modifier_recording(cx);
    }

    /// Leave the recording state without touching the stored value.
    ///
    /// Every exit goes through here so the poll generation always moves: a poll
    /// that is still in flight must not be able to write the value of a recording
    /// that has already ended or been replaced.
    fn end_hold_modifier_recording(&mut self, cx: &mut Context<Self>) {
        self.hold_modifier_recording = false;
        self.hold_modifier_recording_generation =
            self.hold_modifier_recording_generation.wrapping_add(1);
        self.hold_modifier_blur_subscription = None;
        cx.notify();
    }
}
