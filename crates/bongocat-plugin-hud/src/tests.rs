//! Cross-module tests for the panel vocabulary.

use crate::{HudFont, HudRect, PluginView};
use bongocat_plugin_sdk::{ActionIcon, Anchor, SelectOption, UiNode};

fn font() -> HudFont {
    crate::load_platform_font().expect("the platform ships a usable text face")
}

#[test]
fn icon_text_rasterizes_at_desktop_scales_without_an_interaction_target() {
    use bongocat_plugin_sdk::StatusIcon;
    let renderer = crate::HudRenderer::new(font());
    for icon in [
        StatusIcon::Badge("中".into()),
        StatusIcon::Rgba([255, 0, 0, 128].repeat(1024)),
        StatusIcon::Rgba([0, 0, 0, 128].repeat(1024)),
    ] {
        let view = PluginView {
            anchor: Anchor::BottomRight,
            nodes: vec![UiNode::IconText {
                icon: icon.clone(),
                text: "Pinyin / 拼音".into(),
            }],
        };
        assert!(view.validate());
        let layout = crate::layout(&view, HudRect::new(0.0, 0.0, 800.0, 600.0), renderer.font());
        assert!(layout.interactive.is_empty());
        for scale in [1.0, 1.25, 1.5, 2.0] {
            let bitmap = renderer.rasterize(&view, 800, 600, scale).unwrap();
            assert!(!bitmap.is_empty());
            assert_eq!(
                bitmap.pixels.len(),
                bitmap.width as usize * bitmap.height as usize * 4
            );
            assert!(bitmap.origin.right() <= 800.0 && bitmap.origin.bottom() <= 600.0);
            if matches!(icon, StatusIcon::Rgba(_)) {
                let x = (crate::PANEL_PADDING * scale + 2.0) as usize;
                let y = (crate::PANEL_PADDING * scale + 2.0) as usize;
                let pixel = &bitmap.pixels[(y * bitmap.width as usize + x) * 4..][..4];
                if icon.is_template() {
                    assert!(
                        pixel[..3].iter().all(|value| *value > 100),
                        "a black template is legible on the dark HUD"
                    );
                } else {
                    assert!(
                        pixel[0] > pixel[1] + 50,
                        "the red icon is actually composited"
                    );
                }
                assert!(pixel[..3].iter().all(|channel| channel <= &pixel[3]));
            }
        }
    }
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
            UiNode::button("toggle", "Start / 开始", ActionIcon::Play),
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
        nodes: vec![UiNode::button("restart", "Restart", ActionIcon::Restart)],
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
            group: None,
        },
        SelectOption {
            value: "box-navy".into(),
            label: "Box Navy".into(),
            group: None,
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
        preview: false,
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
    let mut loading = UiNode::button("busy", "Busy", ActionIcon::Pause);
    if let UiNode::Button { loading, .. } = &mut loading {
        *loading = true;
    }
    let view = PluginView {
        nodes: vec![
            loading,
            UiNode::button("ready", "Ready", ActionIcon::Play),
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
