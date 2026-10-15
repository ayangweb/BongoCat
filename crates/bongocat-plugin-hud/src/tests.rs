//! Cross-module tests for the panel vocabulary.

use crate::{HudFont, HudRect, PluginView};
use bongocat_plugin_sdk::{Anchor, UiNode};

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
