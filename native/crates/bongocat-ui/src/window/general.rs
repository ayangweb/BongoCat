use super::*;

#[allow(clippy::too_many_arguments)]
pub(super) fn content(
    view: &mut SettingsView,
    _window: &mut Window,
    cx: &mut Context<SettingsView>,
    snapshot: Option<&SettingsSnapshot>,
    disabled: bool,
    overlay_visible: bool,
    overlay_settings: SettingsOverlay,
    motion_audio_enabled: bool,
    model_settings: SettingsModelSettings,
    active_model: SharedString,
    status: SharedString,
    tokens: Tokens,
)->gpui::Stateful<gpui::Div> {
let overlay_row = setting_row(
    "Show desktop cat",
    "Keep the Live2D overlay visible".into(),
    SettingRowState {
        enabled: overlay_visible,
        disabled,
    },
    cx.listener(move |view, _, _, cx| {
        if !disabled {
            view.set_overlay_visible(!overlay_visible, cx);
        }
    }),
    tokens,
);

let audio_row = setting_row(
    "Motion audio",
    "Play audio attached to model motions".into(),
    SettingRowState {
        enabled: motion_audio_enabled,
        disabled,
    },
    cx.listener(move |view, _, _, cx| {
        if !disabled {
            view.set_motion_audio_enabled(!motion_audio_enabled, cx);
        }
    }),
    tokens,
);

let always_on_top = overlay_settings.always_on_top;
let topmost_row = setting_row(
    "Always on top",
    "Keep the Live2D overlay above other windows".into(),
    SettingRowState {
        enabled: always_on_top,
        disabled,
    },
    cx.listener(move |view, _, _, cx| {
        if !disabled {
            let mut settings = overlay_settings;
            settings.always_on_top = !always_on_top;
            view.set_overlay_settings(settings, cx);
        }
    }),
    tokens,
);

let click_through = overlay_settings.click_through;
let click_through_row = setting_row(
    "Click-through overlay",
    "Let pointer input pass through the Live2D overlay".into(),
    SettingRowState {
        enabled: click_through,
        disabled,
    },
    cx.listener(move |view, _, _, cx| {
        if !disabled {
            let mut settings = overlay_settings;
            settings.click_through = !click_through;
            view.set_overlay_settings(settings, cx);
        }
    }),
    tokens,
);

let scale_row = div()
    .text_color(if disabled { tokens.muted } else { tokens.text })
    .child(
        GroupBox::new().outline().child(
            div()
                .flex()
                .items_center()
                .justify_between()
                .gap_3()
                .child(
                    div()
                        .min_w_0()
                        .flex_1()
                        .flex()
                        .flex_col()
                        .gap_1()
                        .child(div().child("Overlay scale"))
                        .child(
                            div()
                                .text_sm()
                                .text_color(tokens.muted)
                                .child("Resize the Live2D overlay"),
                        ),
                )
                .child(NumberInput::new(&view.overlay_scale_input)),
        ),
    );

let opacity_row = div()
    .text_color(if disabled { tokens.muted } else { tokens.text })
    .child(
        GroupBox::new().outline().child(
            div()
                .flex()
                .items_center()
                .justify_between()
                .gap_3()
                .child(
                    div()
                        .min_w_0()
                        .flex_1()
                        .flex()
                        .flex_col()
                        .gap_1()
                        .child(div().child("Overlay opacity"))
                        .child(
                            div()
                                .text_sm()
                                .text_color(tokens.muted)
                                .child("Adjust the overlay transparency"),
                        ),
                )
                .child(NumberInput::new(&view.overlay_opacity_input)),
        ),
    );

let startup_item = startup_item_presentation(
    snapshot.as_ref().map(|snapshot| snapshot.startup_item),
    disabled,
);

let mirror = model_settings.mirror;
let mirror_row = setting_row(
    "Mirror model",
    "Render the model mirrored horizontally".into(),
    SettingRowState {
        enabled: mirror,
        disabled,
    },
    cx.listener(move |view, _, _, cx| {
        if !disabled {
            let mut settings = model_settings;
            settings.mirror = !mirror;
            view.set_model_settings(settings, cx);
        }
    }),
    tokens,
);
let mirror_pointer = model_settings.mirror_pointer_tracking;
let mirror_pointer_row = setting_row(
    "Mirror pointer tracking",
    "Mirror horizontal pointer movement with the model".into(),
    SettingRowState {
        enabled: mirror_pointer,
        disabled,
    },
    cx.listener(move |view, _, _, cx| {
        if !disabled {
            let mut settings = model_settings;
            settings.mirror_pointer_tracking = !mirror_pointer;
            view.set_model_settings(settings, cx);
        }
    }),
    tokens,
);
let ignore_pointer = model_settings.ignore_pointer;
let ignore_pointer_row = setting_row(
    "Ignore pointer input",
    "Do not apply pointer movement to the model".into(),
    SettingRowState {
        enabled: ignore_pointer,
        disabled,
    },
    cx.listener(move |view, _, _, cx| {
        if !disabled {
            let mut settings = model_settings;
            settings.ignore_pointer = !ignore_pointer;
            view.set_model_settings(settings, cx);
        }
    }),
    tokens,
);
let stick_dead_zone_row = div()
    .text_color(if disabled { tokens.muted } else { tokens.text })
    .child(
        GroupBox::new().outline().child(
            div()
                .flex()
                .items_center()
                .justify_between()
                .gap_3()
                .child(div().flex_1().child("Gamepad stick dead zone"))
                .child(NumberInput::new(&view.stick_dead_zone_input)),
        ),
    );
let trigger_dead_zone_row = div()
    .text_color(if disabled { tokens.muted } else { tokens.text })
    .child(
        GroupBox::new().outline().child(
            div()
                .flex()
                .items_center()
                .justify_between()
                .gap_3()
                .child(div().flex_1().child("Gamepad trigger dead zone"))
                .child(NumberInput::new(&view.trigger_dead_zone_input)),
        ),
    );
let startup_item_action = startup_item.action;
let startup_item_row = setting_row(
    "Open at login",
    startup_item.description.into(),
    SettingRowState {
        enabled: startup_item.enabled,
        disabled: startup_item.action == StartupItemAction::None,
    },
    cx.listener(move |view, _, _, cx| match startup_item_action {
        StartupItemAction::None => {}
        StartupItemAction::Retry => view.refresh(cx),
        StartupItemAction::SetEnabled(enabled) => {
            view.set_startup_item_enabled(enabled, cx)
        }
    }),
    tokens,
);

div()
    .min_w_0()
    .min_h_0()
    .flex_1()
    .h_full()
    .flex()
    .flex_col()
    .gap_3()
    .p_5()
    .bg(tokens.canvas)
    .text_color(tokens.text)
    .id("general-content")
    .overflow_y_scroll()
    .child(div().text_2xl().child("General"))
    .child(
        GroupBox::new().outline().child(
            div()
                .flex()
                .items_center()
                .justify_between()
                .gap_3()
                .child(
                    div()
                        .min_w_0()
                        .flex_1()
                        .flex()
                        .flex_col()
                        .gap_1()
                        .child(div().child("Active model"))
                        .child(
                            div().text_sm().text_color(tokens.muted).child(active_model),
                        ),
                )
                .child(if view.error.is_some() {
                    Tag::danger().child(status).into_any_element()
                } else {
                    Tag::secondary().child(status).into_any_element()
                }),
        ),
    )
    .child(overlay_row)
    .child(topmost_row)
    .child(click_through_row)
    .child(scale_row)
    .child(opacity_row)
    .child(audio_row)
    .child(mirror_row)
    .child(mirror_pointer_row)
    .child(ignore_pointer_row)
    .child(stick_dead_zone_row)
    .child(trigger_dead_zone_row)
    .child(startup_item_row)
    .child(div().flex_1())
}
