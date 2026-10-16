//! Cross-module tests for the panel vocabulary.

use crate::{HudFont, HudRect, PluginView};
use bongocat_plugin_sdk::{Anchor, SelectOption, UiNode};

fn font() -> HudFont {
    crate::load_platform_font().expect("the platform ships a usable text face")
}

#[test]
fn a_view_measures_to_its_widest_node_and_stacked_height() {
    let font = font();
    let view = PluginView {
        anchor: Anchor::TopRight,
        nodes: vec![
            UiNode::Text {
                text: "05:00".into(),
            },
            UiNode::button("toggle", "Start / 开始"),
        ],
    };
    let (width, height) = crate::measure_view(&view, &font);
    let text = crate::measure_view(
        &PluginView {
            anchor: Anchor::TopRight,
            nodes: vec![UiNode::Text {
                text: "05:00".into(),
            }],
        },
        &font,
    );
    assert!(width >= text.0);
    assert!(height > text.1);
}

#[test]
fn a_point_inside_a_control_is_reported_and_a_point_outside_is_not() {
    let font = font();
    let view = PluginView {
        anchor: Anchor::TopLeft,
        nodes: vec![UiNode::button("restart", "Restart")],
    };
    let viewport = HudRect::new(0.0, 0.0, 200.0, 200.0);
    let layout = crate::layout(&view, viewport, &font);
    let button = &layout.interactive[0];
    assert_eq!(
        crate::hit_test(&layout, button.rect.x + 1.0, button.rect.y + 1.0).map(|n| n.id.as_str()),
        Some("restart")
    );
    assert!(crate::hit_test(&layout, 199.0, 199.0).is_none());
}

#[test]
fn a_view_with_no_nodes_lays_out_nothing() {
    let font = font();
    let layout = crate::layout(
        &PluginView::default(),
        HudRect::new(0.0, 0.0, 100.0, 100.0),
        &font,
    );
    assert!(layout.panel.is_empty());
    assert!(layout.interactive.is_empty());
}

/// Selects are display-only on the model window, not action buttons.
#[test]
fn a_select_is_measured_by_its_chosen_option_without_an_action_target() {
    let font = font();
    let options = vec![
        SelectOption {
            value: "skcm-blue".into(),
            label: "SKCM Blue".into(),
        },
        SelectOption {
            value: "box-navy".into(),
            label: "Box Navy".into(),
        },
    ];
    // The panel shows the option's label, not the value the plugin was handed
    // back, and an unknown value falls back to showing itself rather than
    // drawing nothing.
    assert_eq!(
        crate::layout::select_option_label("box-navy", &options),
        "Box Navy"
    );
    assert_eq!(crate::layout::select_option_label("gone", &options), "gone");

    let node = UiNode::Select {
        key: "keyboard_pack".into(),
        label: "Keyboard sound".into(),
        value: "skcm-blue".into(),
        options,
    };
    let view = PluginView {
        anchor: Anchor::TopLeft,
        nodes: vec![node],
    };
    let viewport = HudRect::new(0.0, 0.0, 300.0, 300.0);
    let layout = crate::layout(&view, viewport, &font);
    assert!(layout.interactive.is_empty());
    assert!(layout.panel.width > 0.0);
}

#[test]
fn only_ready_buttons_and_toggles_are_interactive() {
    let mut loading = UiNode::button("busy", "Busy");
    if let UiNode::Button { loading, .. } = &mut loading {
        *loading = true;
    }
    let view = PluginView {
        nodes: vec![
            loading,
            UiNode::button("ready", "Ready"),
            UiNode::Toggle {
                key: "enabled".into(),
                label: "Enabled".into(),
                value: false,
            },
            UiNode::Integer {
                key: "minutes".into(),
                label: "Minutes".into(),
                value: 25,
                min: 1,
                max: 180,
            },
        ],
        ..Default::default()
    };
    let layout = crate::layout(&view, HudRect::new(0.0, 0.0, 300.0, 300.0), &font());
    assert_eq!(
        layout
            .interactive
            .iter()
            .map(|node| node.id.as_str())
            .collect::<Vec<_>>(),
        ["ready", "enabled"]
    );
}
