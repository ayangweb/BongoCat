//! What a plugin puts on the model window.
//!
//! A scene is a tree of nodes the host lays out and rasterizes. It is not a
//! general UI toolkit and deliberately does not grow into one: a node draws,
//! binds a value, or asks for an action when it is pressed. Everything about it
//! is bounded by a constant in this file, so a scene that parses is a scene the
//! host can lay out without asking a question first.
//!
//! Layout is a stack, and only a stack. A plugin that wants two things side by
//! side nests two children; a plugin that wants a grid declares it as a nested
//! pair of stacks. That keeps the layout pass small enough to reason about and
//! means a scene's size is knowable without running it — which is what lets the
//! host decide whether a panel still fits the model window before drawing it.

pub mod inspect;
pub mod value;

pub use inspect::{Inspector, SceneAction};

use super::{BehaviorId, Color};
use serde::{Deserialize, Serialize};

/// The most nodes one scene may contain, at any depth.
///
/// A scene is rasterized on a worker thread and re-rasterized whenever a bound
/// value changes, so the bound is on work per redraw rather than on safety. It
/// is set high enough that no real panel reaches it and low enough that a scene
/// trying to reach it is refused before it is rasterized rather than after.
pub const MAXIMUM_SCENE_NODES: usize = 512;

/// The deepest a scene may nest.
///
/// Layout is recursive, so this is a stack-depth bound as much as a design one.
/// Sixty-four is far past anything a model window panel needs and is inside what
/// any thread can recurse.
pub const MAXIMUM_SCENE_DEPTH: usize = 64;

/// One node of a scene.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum SceneNode {
    /// Children laid out one after another.
    Stack(StackNode),
    /// A run of text on one line.
    Text(TextNode),
    /// Empty space that grows to fill what the stack has left.
    Spacer(SpacerNode),
    /// A horizontal rule.
    Divider(DividerNode),
    /// A filled bar, for a proportion.
    ProgressBar(ProgressBarNode),
    /// A ring, for a proportion where the number matters more than the length.
    ProgressRing(ProgressRingNode),
    /// A PNG the plugin ships.
    Image(ImageNode),
    /// A pressable region that runs one action.
    Button(ButtonNode),
}

impl SceneNode {
    /// The node kind as it appears in the scene file, for messages that name it.
    pub const fn kind(&self) -> &'static str {
        match self {
            Self::Stack(_) => "stack",
            Self::Text(_) => "text",
            Self::Spacer(_) => "spacer",
            Self::Divider(_) => "divider",
            Self::ProgressBar(_) => "progress_bar",
            Self::ProgressRing(_) => "progress_ring",
            Self::Image(_) => "image",
            Self::Button(_) => "button",
        }
    }
}

/// Which way a stack lays its children out.
#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum StackAxis {
    /// Top to bottom. The default, because a panel is a column far more often
    /// than it is a row.
    #[default]
    Vertical,
    /// Left to right.
    Horizontal,
}

/// How a child is placed across the axis a stack does not lay out on.
#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Align {
    #[default]
    Start,
    Center,
    End,
    /// Take the remaining space. Only meaningful for the cross axis, and only one
    /// child in a stack may ask for it — a second would leave the first with
    /// nothing, which layout resolves by giving it zero rather than by guessing.
    Stretch,
}

#[derive(Clone, Debug, Default, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct StackNode {
    #[serde(default)]
    pub axis: StackAxis,
    /// Logical pixels between consecutive children.
    #[serde(default)]
    pub spacing: f32,
    /// Logical pixels of empty space inside the stack, `[horizontal, vertical]`.
    #[serde(default)]
    pub padding: [f32; 2],
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub background: Option<Color>,
    /// Corner radius of the background, in logical pixels.
    #[serde(default)]
    pub radius: f32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub border: Option<Color>,
    /// Border width in logical pixels. Zero when `border` is absent.
    #[serde(default)]
    pub border_width: f32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cross_align: Option<Align>,
    #[serde(default)]
    pub children: Vec<SceneNode>,
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum TextWeight {
    #[default]
    Regular,
    Bold,
}

/// A single run of text.
///
/// `max_lines` is a truncation bound rather than a wrapping instruction: a panel
/// is a fixed size, and a value that grew without limit — an error message, a
/// long file name — must not be what makes a panel overflow the model window.
#[derive(Clone, Debug, Default, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct TextNode {
    /// Defaults to a literal empty string. A text node with no value is a label
    /// whose content is decided elsewhere, and refusing to parse it would only
    /// mean its author had to write `"value": ""` to say nothing.
    #[serde(default = "empty_value")]
    pub value: super::scene::value::SceneValue,
    /// Cap height in logical pixels.
    #[serde(default = "default_text_size")]
    pub size: f32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub color: Option<Color>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub align: Option<Align>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub weight: Option<TextWeight>,
    /// How the text is fitted horizontally inside the node's box.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub truncate: Option<bool>,
    /// Maximum lines to draw. Zero means one.
    #[serde(default, skip_serializing_if = "is_zero")]
    pub max_lines: u16,
}

const fn default_text_size() -> f32 {
    14.0
}

fn empty_value() -> super::scene::value::SceneValue {
    super::scene::value::SceneValue::Text(String::new())
}

const fn is_zero(value: &u16) -> bool {
    *value == 0
}

#[derive(Clone, Debug, Default, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct SpacerNode {
    /// Share of the stack's leftover space this spacer takes. Zero still takes
    /// its share of nothing, which is a spacer that contributes nothing.
    #[serde(default = "one")]
    pub grow: f32,
}

const fn one() -> f32 {
    1.0
}

#[derive(Clone, Debug, Default, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct DividerNode {
    #[serde(default = "one_pixel")]
    pub thickness: f32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub color: Option<Color>,
}

const fn one_pixel() -> f32 {
    1.0
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ProgressBarNode {
    /// The proportion to fill, in `[0, 1]`.
    pub value: super::scene::value::SceneValue,
    #[serde(default = "default_bar_height")]
    pub height: f32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fill: Option<Color>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub track: Option<Color>,
    #[serde(default)]
    pub radius: f32,
}

const fn default_bar_height() -> f32 {
    6.0
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ProgressRingNode {
    pub value: super::scene::value::SceneValue,
    /// Outer diameter in logical pixels.
    #[serde(default = "default_ring_size")]
    pub size: f32,
    #[serde(default = "default_ring_thickness")]
    pub thickness: f32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fill: Option<Color>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub track: Option<Color>,
}

const fn default_ring_size() -> f32 {
    48.0
}

const fn default_ring_thickness() -> f32 {
    4.0
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ImageNode {
    /// Path under the plugin's own directory. Never absolute, never `..`.
    pub asset: String,
    #[serde(default = "default_image_size")]
    pub size: f32,
    /// Keep the image's own aspect ratio inside the box. On by default because
    /// an image stretched to a square is almost never what a plugin means.
    #[serde(default = "yes")]
    pub preserve_aspect: bool,
}

const fn default_image_size() -> f32 {
    32.0
}

const fn yes() -> bool {
    true
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ButtonVariant {
    #[default]
    Primary,
    Secondary,
    /// No chrome of its own: an invisible pressable area. For a panel that wants
    /// its own look and only needs the press target and the action.
    Transparent,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ButtonNode {
    /// Identifies the button when the host reports a press. Unique within one
    /// scene; the loader refuses a scene that names the same button twice.
    pub id: String,
    pub label: super::scene::value::SceneValue,
    /// What a press runs. The host owns what that means, so a button cannot ask
    /// for anything the behaviors did not already declare.
    pub action: super::BehaviorAction,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target: Option<BehaviorId>,
    #[serde(default)]
    pub variant: ButtonVariant,
    /// Whether the press is currently ignored. Bound so a button can grey out
    /// while the behavior it drives cannot act.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub disabled: Option<super::scene::value::SceneValue>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub color: Option<Color>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub text_color: Option<Color>,
    #[serde(default)]
    pub radius: f32,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::SceneValue;

    #[test]
    fn a_node_names_its_own_kind_in_the_file() {
        let node: SceneNode = serde_json::from_str(r#"{"type":"spacer","grow":2}"#).unwrap();
        assert_eq!(node, SceneNode::Spacer(SpacerNode { grow: 2.0 }));
        assert_eq!(node.kind(), "spacer");
    }

    #[test]
    fn defaults_are_chosen_so_the_common_case_stays_short() {
        let node: SceneNode = serde_json::from_str(r#"{"type":"text","value":"hi"}"#).unwrap();
        let SceneNode::Text(text) = node else {
            panic!("expected a text node");
        };
        assert_eq!(text.size, 14.0);
        assert_eq!(text.max_lines, 0);
        assert_eq!(text.weight, None);
    }

    #[test]
    fn an_unknown_field_is_refused_rather_than_ignored() {
        // A typo in a plugin is a plugin that silently shows nothing, which is
        // much harder to report than a load failure.
        let error = serde_json::from_str::<SceneNode>(r#"{"type":"spacer","grows":2}"#);
        assert!(error.is_err());
    }

    #[test]
    fn an_unknown_node_kind_is_refused() {
        assert!(serde_json::from_str::<SceneNode>(r#"{"type":"webview"}"#).is_err());
    }

    #[test]
    fn a_bar_requires_a_value() {
        assert!(serde_json::from_str::<SceneNode>(r#"{"type":"progress_bar"}"#).is_err());
        assert!(
            serde_json::from_str::<SceneNode>(
                r#"{"type":"progress_bar","value":{"fraction":"t.p","fallback":0}}"#
            )
            .is_ok()
        );
    }

    #[test]
    fn a_button_requires_an_id_a_label_and_an_action() {
        assert!(serde_json::from_str::<SceneNode>(r#"{"type":"button","id":"a"}"#).is_err());
        assert!(
            serde_json::from_str::<SceneNode>(
                r#"{"type":"button","id":"a","label":"Go","action":"toggle"}"#
            )
            .is_ok()
        );
    }

    #[test]
    fn a_spacer_grows_by_default() {
        let node: SceneNode = serde_json::from_str(r#"{"type":"spacer"}"#).unwrap();
        assert_eq!(node, SceneNode::Spacer(SpacerNode { grow: 1.0 }));
    }

    #[test]
    fn a_literal_value_round_trips_as_a_string() {
        let node: SceneNode = serde_json::from_str(r#"{"type":"text","value":"Focus"}"#).unwrap();
        let SceneNode::Text(text) = node else {
            panic!("expected a text node");
        };
        assert_eq!(text.value, SceneValue::Text("Focus".to_string()));
    }
}
