//! Laying a plugin's view tree out.
//!
//! The layout is a small, deterministic row/column model: a panel stacks its content and groups adjacent buttons horizontally
//! with a fixed gap and pads its content, text measures from the font, and a
//! button is sized from its label. It is deliberately not a flexbox — a plugin's
//! panel is a label, a count and a couple of buttons — and every rule here is
//! pure, so a layout can be asserted without a GPU or a window.

use crate::{
    BADGE_PADDING, BUTTON_HEIGHT, BUTTON_PADDING, HudFont, HudRect, InteractiveNode, LABEL_SIZE,
    PANEL_GAP, PANEL_PADDING, STATUS_PANEL_PADDING, VALUE_SIZE,
};
use bongocat_plugin_sdk::{Anchor, PluginView, StatusIcon, UiNode};

/// One node of a laid-out view: its rectangle and the node it came from.
pub struct PlacedNode<'a> {
    pub node: &'a UiNode,
    pub rect: HudRect,
}

/// The size a view's content occupies, in logical pixels.
pub fn measure_view(view: &PluginView, font: &HudFont) -> (f32, f32) {
    let panel = HudRect::new(0.0, 0.0, f32::MAX, f32::MAX);
    let nodes = placed_nodes(view, panel, font);
    let padding = panel_padding(view);
    (
        nodes
            .iter()
            .map(|n| n.rect.right() - padding)
            .fold(0.0, f32::max),
        nodes
            .iter()
            .map(|n| n.rect.bottom() - padding)
            .fold(0.0, f32::max),
    )
}

/// The size one node occupies, in logical pixels.
pub fn measure_node(node: &UiNode, font: &HudFont) -> (f32, f32) {
    match node {
        UiNode::IconText { icon, text } => (
            status_icon_width(icon, font) + PANEL_GAP + font.measure_text(text, LABEL_SIZE),
            LABEL_SIZE.max(font.line_height(LABEL_SIZE)),
        ),
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

/// The text size a status badge is drawn at.
///
/// One character is a glyph that fills the slot; a longer label is a code, so it
/// is set slightly smaller to keep the same optical weight beside the artwork.
pub(crate) fn badge_size(badge: &str) -> f32 {
    LABEL_SIZE * if badge.chars().count() > 1 { 0.75 } else { 1.0 }
}

/// The width of the slot a status icon occupies, in logical pixels.
///
/// Artwork is drawn at a fixed height and keeps its own aspect ratio, so its slot
/// is square. A badge is text: it keeps the square while its label fits inside it,
/// and takes the width the label needs — plus a little air — when it does not.
/// Squeezing a three-letter language code into the square is what drew it across
/// the label beside it.
pub(crate) fn status_icon_width(icon: &StatusIcon, font: &HudFont) -> f32 {
    match icon {
        StatusIcon::Badge(badge) => {
            let width = font.measure_text(badge, badge_size(badge));
            // A sub-pixel overshoot is rounding, not an overflow: the label still
            // sits inside the square and keeps it.
            if width <= LABEL_SIZE + 0.5 {
                LABEL_SIZE
            } else {
                width + BADGE_PADDING * 2.0
            }
        }
        StatusIcon::Rgba(_) => LABEL_SIZE,
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

pub(crate) fn anchored_rect_with_padding(
    anchor: Anchor,
    viewport: HudRect,
    content: (f32, f32),
    padding: f32,
) -> HudRect {
    let width = content.0 + padding * 2.0;
    let height = content.1 + padding * 2.0;
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

/// A single status stays compact; stacked plugin controls keep the panel padding.
pub(crate) fn panel_padding(view: &PluginView) -> f32 {
    if matches!(view.nodes.as_slice(), [UiNode::IconText { .. }]) {
        STATUS_PANEL_PADDING
    } else {
        PANEL_PADDING
    }
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
    for placed in placed_nodes(view, panel, font) {
        if let Some(id) = interactive_id(placed.node) {
            interactive.push(InteractiveNode {
                id: id.to_owned(),
                rect: placed.rect,
            });
        }
    }
}

/// The id a node can be activated by, if it is interactive.
///
/// Only buttons and toggles are: a text row, a number, a choice and a status are
/// what a panel displays, and the settings panel is where they are edited. A
/// button the plugin reports as carrying out its action is not interactive
/// either — the action it asked for is already in flight.
pub fn interactive_id(node: &UiNode) -> Option<&str> {
    match node {
        UiNode::Button {
            id, loading: false, ..
        } => Some(id),
        UiNode::Toggle { key, .. } => Some(key),
        UiNode::Button { loading: true, .. } => None,
        UiNode::Integer { .. }
        | UiNode::Select { .. }
        | UiNode::IconText { .. }
        | UiNode::Text { .. } => None,
    }
}

/// Lay every node of a view out, for the rasterizer to paint.
pub(crate) fn placed_nodes<'a>(
    view: &'a PluginView,
    panel: HudRect,
    font: &HudFont,
) -> Vec<PlacedNode<'a>> {
    let mut placed = Vec::with_capacity(view.nodes.len());
    let padding = panel_padding(view);
    let mut y = panel.y + padding;
    let x = panel.x + padding;
    let available = (panel.width - padding * 2.0).max(0.0);
    let mut index = 0;
    while index < view.nodes.len() {
        let start = index;
        let buttons = matches!(view.nodes[index], UiNode::Button { .. });
        index += 1;
        if buttons {
            while index < view.nodes.len() && matches!(view.nodes[index], UiNode::Button { .. }) {
                index += 1;
            }
        }
        let mut row_x = x;
        let mut row_height: f32 = 0.0;
        for node in &view.nodes[start..index] {
            let (width, height) = measure_node(node, font);
            placed.push(PlacedNode {
                node,
                rect: HudRect::new(row_x, y, width.min(available).max(0.0), height),
            });
            row_x += width + PANEL_GAP;
            row_height = row_height.max(height);
        }
        y += row_height + PANEL_GAP;
    }
    placed
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::HudFont;
    use bongocat_plugin_sdk::ActionIcon;

    fn font() -> HudFont {
        crate::load_platform_font().expect("the platform ships a usable text face")
    }

    #[test]
    fn status_icon_uses_the_label_size_without_an_oversized_row() {
        let font = font();
        let node = UiNode::IconText {
            icon: bongocat_plugin_sdk::StatusIcon::Badge("EN".into()),
            text: "Input Source".into(),
        };
        let (width, height) = measure_node(&node, &font);
        assert_eq!(
            width,
            LABEL_SIZE + PANEL_GAP + font.measure_text("Input Source", LABEL_SIZE)
        );
        assert_eq!(height, LABEL_SIZE.max(font.line_height(LABEL_SIZE)));
    }

    #[test]
    fn a_short_badge_keeps_the_square_slot_artwork_uses() {
        let font = font();

        for badge in ["中", "EN"] {
            assert_eq!(
                status_icon_width(&StatusIcon::Badge(badge.into()), &font),
                LABEL_SIZE,
                "{badge} fits the square"
            );
        }
    }

    #[test]
    fn a_wide_badge_takes_the_width_it_needs_without_growing_the_row() {
        let font = font();
        let icon = StatusIcon::Badge("ENG".into());
        let node = UiNode::IconText {
            icon: icon.clone(),
            text: "美式键盘".into(),
        };

        let (width, height) = measure_node(&node, &font);
        let slot = status_icon_width(&icon, &font);

        assert!(
            slot > LABEL_SIZE,
            "a three-letter code is wider than the square"
        );
        assert_eq!(
            width,
            slot + PANEL_GAP + font.measure_text("美式键盘", LABEL_SIZE),
            "the label starts after the slot the badge needs"
        );
        assert_eq!(
            height,
            LABEL_SIZE.max(font.line_height(LABEL_SIZE)),
            "the slot keeps the fixed icon height"
        );
    }

    #[test]
    fn a_single_status_uses_compact_panel_padding() {
        let font = font();
        let view = PluginView {
            anchor: Anchor::BottomRight,
            nodes: vec![UiNode::IconText {
                icon: bongocat_plugin_sdk::StatusIcon::Badge("EN".into()),
                text: "Input Source".into(),
            }],
        };
        let layout = crate::layout(&view, HudRect::new(0.0, 0.0, 800.0, 600.0), &font);
        let (content_width, content_height) = measure_view(&view, &font);
        assert_eq!(
            layout.panel.width,
            content_width + STATUS_PANEL_PADDING * 2.0
        );
        assert_eq!(
            layout.panel.height,
            content_height + STATUS_PANEL_PADDING * 2.0
        );
    }

    #[test]
    fn adjacent_buttons_share_a_row_and_hit_test_matches_paint() {
        let font = font();
        let view = PluginView {
            anchor: Anchor::TopRight,
            nodes: vec![
                UiNode::Text {
                    text: "25:00".into(),
                },
                UiNode::button("toggle", "Start", ActionIcon::Play),
                UiNode::button("restart", "Restart", ActionIcon::Restart),
            ],
        };
        let layout = crate::layout(&view, HudRect::new(0.0, 0.0, 800.0, 600.0), &font);
        let a = &layout.interactive[0];
        let b = &layout.interactive[1];
        assert_eq!(a.rect.y, b.rect.y);
        assert_eq!(b.rect.x - a.rect.right(), PANEL_GAP);
        assert!(b.rect.right() <= layout.panel.right() - PANEL_PADDING + 0.01);
        for target in &layout.interactive {
            assert_eq!(
                crate::hit_test(&layout, target.rect.x + 15.0, target.rect.y + 15.0)
                    .unwrap()
                    .id,
                target.id
            );
        }
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
        let top_left =
            anchored_rect_with_padding(Anchor::TopLeft, viewport, content, PANEL_PADDING);
        let top_right =
            anchored_rect_with_padding(Anchor::TopRight, viewport, content, PANEL_PADDING);
        let bottom_left =
            anchored_rect_with_padding(Anchor::BottomLeft, viewport, content, PANEL_PADDING);
        let bottom_right =
            anchored_rect_with_padding(Anchor::BottomRight, viewport, content, PANEL_PADDING);
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
                UiNode::button("toggle", "Start", ActionIcon::Play),
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
