//! Laying a plugin's view tree out.
//!
//! The layout is a small, deterministic column model: a panel stacks its nodes
//! with a fixed gap and pads its content, text measures from the font, and a
//! button is sized from its label. It is deliberately not a flexbox — a plugin's
//! panel is a label, a count and a couple of buttons — and every rule here is
//! pure, so a layout can be asserted without a GPU or a window.

use crate::{
    BUTTON_HEIGHT, BUTTON_PADDING, HudFont, HudRect, InteractiveNode, LABEL_SIZE, PANEL_GAP,
    PANEL_PADDING, VALUE_SIZE,
};
use bongocat_plugin_sdk::{Anchor, PluginView, UiNode};

/// One node of a laid-out view: its rectangle and the node it came from.
pub struct PlacedNode<'a> {
    pub node: &'a UiNode,
    pub rect: HudRect,
}

/// The size a view's content occupies, in logical pixels.
pub fn measure_view(view: &PluginView, font: &HudFont) -> (f32, f32) {
    let mut width: f32 = 0.0;
    let mut height: f32 = 0.0;
    for (index, node) in view.nodes.iter().enumerate() {
        let (node_width, node_height) = measure_node(node, font);
        if index > 0 {
            height += PANEL_GAP;
        }
        width = width.max(node_width);
        height += node_height;
    }
    (width, height)
}

/// The size one node occupies, in logical pixels.
pub fn measure_node(node: &UiNode, font: &HudFont) -> (f32, f32) {
    match node {
        UiNode::Text { text } => {
            let size = text_size(text);
            (font.measure_text(text, size), font.line_height(size))
        }
        UiNode::Button { label, .. } => {
            let width = font.measure_text(label, LABEL_SIZE) + BUTTON_PADDING * 2.0;
            (width, BUTTON_HEIGHT)
        }
        UiNode::Toggle { label, .. } => {
            let width = font.measure_text(label, LABEL_SIZE) + BUTTON_PADDING * 2.0 + 34.0;
            (width, BUTTON_HEIGHT)
        }
        UiNode::Integer { label, value, .. } => {
            let text = format!("{label}: {value}");
            (
                font.measure_text(&text, LABEL_SIZE),
                font.line_height(LABEL_SIZE),
            )
        }
        // The model window is a display surface: a choice is drawn as the label
        // and the option it holds, exactly as a number is. Picking another option
        // is a settings-panel action, and the panel is where the host draws it.
        UiNode::Select {
            label,
            value,
            options,
            ..
        } => {
            let text = format!("{label}: {}", select_option_label(value, options));
            (
                font.measure_text(&text, LABEL_SIZE),
                font.line_height(LABEL_SIZE),
            )
        }
    }
}

/// The label of the option a select currently holds.
///
/// A select is validated against its own list before the host sees it, so a
/// missing option means a plugin bug rather than user data. The value is then
/// shown as it came, which is what a reader can act on, instead of an empty line.
pub(crate) fn select_option_label<'a>(
    value: &'a str,
    options: &'a [bongocat_plugin_sdk::SelectOption],
) -> &'a str {
    options
        .iter()
        .find(|option| option.value == value)
        .map_or(value, |option| option.label.as_str())
}

/// The text size a node's string is drawn at.
///
/// A countdown's remaining time is the one value a user reads at a glance, and it
/// is the only text a plugin does not label, so a bare time-shaped string is
/// drawn larger. Everything else is a label and uses the label size.
pub(crate) fn text_size(text: &str) -> f32 {
    if looks_like_time(text) {
        VALUE_SIZE
    } else {
        LABEL_SIZE
    }
}

/// Whether a string is a bare clock value such as `05:00` or `180:00`.
///
/// A countdown's remaining time is the one value a user reads at a glance, so it
/// is drawn larger than a label. The shape is `digits:digits` with at most three
/// digits a side, which is what `mm:ss` and its over-an-hour form both look like.
fn looks_like_time(text: &str) -> bool {
    let Some((minutes, seconds)) = text.split_once(':') else {
        return false;
    };
    !minutes.is_empty()
        && !seconds.is_empty()
        && minutes.len() <= 3
        && seconds.len() == 2
        && minutes.bytes().all(|byte| byte.is_ascii_digit())
        && seconds.bytes().all(|byte| byte.is_ascii_digit())
}

/// The panel rectangle for an anchor inside a viewport.
pub fn anchored_rect(anchor: Anchor, viewport: HudRect, content: (f32, f32)) -> HudRect {
    let width = content.0 + PANEL_PADDING * 2.0;
    let height = content.1 + PANEL_PADDING * 2.0;
    let margin = PANEL_GAP;
    let (x, y) = match anchor {
        Anchor::TopLeft => (viewport.x + margin, viewport.y + margin),
        Anchor::TopRight => (viewport.right() - width - margin, viewport.y + margin),
        Anchor::BottomLeft => (viewport.x + margin, viewport.bottom() - height - margin),
        Anchor::BottomRight => (
            viewport.right() - width - margin,
            viewport.bottom() - height - margin,
        ),
    };
    HudRect::new(x, y, width, height)
}

/// Lay a view out and return the panel rectangle and its interactive nodes.
pub fn layout_view(view: &PluginView, viewport: HudRect, font: &HudFont) -> crate::HudLayout {
    crate::layout(view, viewport, font)
}

/// Collect the interactive nodes of a view, in draw order.
///
/// Nodes are stacked from the panel's content origin with the shared gap, so the
/// rectangles line up with what the rasterizer draws.
pub fn collect_interactive(
    view: &PluginView,
    panel: HudRect,
    font: &HudFont,
    interactive: &mut Vec<InteractiveNode>,
) {
    let mut y = panel.y + PANEL_PADDING;
    let x = panel.x + PANEL_PADDING;
    let available = (panel.width - PANEL_PADDING * 2.0).max(0.0);
    for node in &view.nodes {
        let (node_width, node_height) = measure_node(node, font);
        let rect = HudRect::new(x, y, node_width.min(available).max(0.0), node_height);
        if let Some(id) = interactive_id(node) {
            interactive.push(InteractiveNode {
                id: id.to_owned(),
                rect,
            });
        }
        y += node_height + PANEL_GAP;
    }
}

/// The id a node can be activated by, if it is interactive.
pub fn interactive_id(node: &UiNode) -> Option<&str> {
    match node {
        UiNode::Button { id, .. } => Some(id),
        UiNode::Toggle { key, .. } => Some(key),
        UiNode::Integer { key, .. } => Some(key),
        UiNode::Select { key, .. } => Some(key),
        UiNode::Text { .. } => None,
    }
}

/// Lay every node of a view out, for the rasterizer to paint.
pub(crate) fn placed_nodes<'a>(
    view: &'a PluginView,
    panel: HudRect,
    font: &HudFont,
) -> Vec<PlacedNode<'a>> {
    let mut placed = Vec::with_capacity(view.nodes.len());
    let mut y = panel.y + PANEL_PADDING;
    let x = panel.x + PANEL_PADDING;
    let available = (panel.width - PANEL_PADDING * 2.0).max(0.0);
    for node in &view.nodes {
        let (node_width, node_height) = measure_node(node, font);
        placed.push(PlacedNode {
            node,
            rect: HudRect::new(x, y, node_width.min(available).max(0.0), node_height),
        });
        y += node_height + PANEL_GAP;
    }
    placed
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::HudFont;

    fn font() -> HudFont {
        crate::load_platform_font().expect("the platform ships a usable text face")
    }

    #[test]
    fn a_time_shaped_string_is_the_large_value_and_a_label_is_not() {
        assert!(looks_like_time("05:00"));
        assert!(looks_like_time("123:45"));
        assert!(!looks_like_time("Ready / 完成"));
        assert!(!looks_like_time("5:0"));
        assert!(!looks_like_time("aa:bb"));
    }

    #[test]
    fn each_anchor_places_the_panel_in_its_own_corner() {
        let viewport = HudRect::new(0.0, 0.0, 200.0, 100.0);
        let content = (40.0, 20.0);
        let size = (
            content.0 + PANEL_PADDING * 2.0,
            content.1 + PANEL_PADDING * 2.0,
        );
        let top_left = anchored_rect(Anchor::TopLeft, viewport, content);
        let top_right = anchored_rect(Anchor::TopRight, viewport, content);
        let bottom_left = anchored_rect(Anchor::BottomLeft, viewport, content);
        let bottom_right = anchored_rect(Anchor::BottomRight, viewport, content);
        assert_eq!(top_left.x, viewport.x + PANEL_GAP);
        assert_eq!(top_right.right(), viewport.right() - PANEL_GAP);
        assert_eq!(bottom_left.bottom(), viewport.bottom() - PANEL_GAP);
        assert_eq!(bottom_right.x, viewport.right() - size.0 - PANEL_GAP);
        assert_eq!(bottom_right.y, viewport.bottom() - size.1 - PANEL_GAP);
    }

    #[test]
    fn a_buttons_rectangle_is_where_the_layout_put_it() {
        let font = font();
        let view = PluginView {
            anchor: Anchor::TopLeft,
            nodes: vec![
                UiNode::Text {
                    text: "05:00".into(),
                },
                UiNode::button("toggle", "Start"),
            ],
        };
        let viewport = HudRect::new(0.0, 0.0, 300.0, 300.0);
        let layout = crate::layout(&view, viewport, &font);
        let button = crate::hit_test(
            &layout,
            layout.panel.x + PANEL_PADDING + 2.0,
            layout.panel.y + PANEL_PADDING + 40.0,
        );
        assert_eq!(button.map(|node| node.id.as_str()), Some("toggle"));
    }
}
