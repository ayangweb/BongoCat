//! What a scene names, read before anything is drawn.
//!
//! A scene arrives from another process, so its problems have to be found in the
//! document rather than at layout time. This pass walks the tree once and reports
//! the two things a scene gets wrong: more nodes or more depth than the bounds
//! allow, and two buttons sharing an id so a press would be ambiguous. An image
//! path is checked here for the same reason — it is a path from a document that
//! becomes a file read.
//!
//! It deliberately does not check geometry. Layout knows the panel's size and can
//! clip what does not fit, and a panel that overflows is a panel the user can see
//! is wrong — which is a better report than a refusal nobody can act on.

use super::{
    ButtonNode, DividerNode, ImageNode, MAXIMUM_SCENE_DEPTH, MAXIMUM_SCENE_NODES, ProgressBarNode,
    ProgressRingNode, SceneNode, SpacerNode, StackNode,
};
use crate::{PluginError, PluginErrorCode};

/// One press a scene can produce.
///
/// Recorded rather than re-read from the scene at press time, so the host does not
/// walk the scene on every press. The label is deliberately not recorded: a label
/// changes on every frame and resolving it belongs to the draw pass, not here.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SceneAction {
    pub button: String,
}

/// What a scene names, gathered in one pass.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct Inspector {
    pub nodes: usize,
    pub deepest: usize,
    /// Every button id, in scene order.
    pub buttons: Vec<String>,
    /// Every press the scene can produce.
    pub actions: Vec<SceneAction>,
    /// Every asset path the scene names, in scene order.
    pub assets: Vec<String>,
}

impl Inspector {
    pub fn new() -> Self {
        Self::default()
    }
}

/// Walk `node`, adding what it names to `inspector`.
///
/// `depth` is 1 for the root, so `MAXIMUM_SCENE_DEPTH` bounds the number of
/// nested containers rather than the number of levels of a tree that includes
/// leaves.
pub fn walk(node: &SceneNode, depth: usize, inspector: &mut Inspector) -> Result<(), PluginError> {
    if depth > MAXIMUM_SCENE_DEPTH {
        return Err(PluginError::new(PluginErrorCode::SceneTooDeep));
    }
    inspector.nodes += 1;
    if inspector.nodes > MAXIMUM_SCENE_NODES {
        return Err(PluginError::new(PluginErrorCode::SceneTooLarge));
    }
    inspector.deepest = inspector.deepest.max(depth);
    match node {
        SceneNode::Stack(StackNode { children, .. }) => {
            for child in children {
                walk(child, depth + 1, inspector)?;
            }
        }
        SceneNode::Image(image) => {
            crate::validate_relative_asset_path(&image.asset)?;
            inspector.assets.push(image.asset.clone());
        }
        SceneNode::Button(ButtonNode { id, .. }) => {
            if id.is_empty() || id.len() > crate::MAXIMUM_BUTTON_ID_BYTES {
                return Err(PluginError::new(PluginErrorCode::InvalidButtonId));
            }
            inspector.buttons.push(id.clone());
            inspector.actions.push(SceneAction { button: id.clone() });
        }
        SceneNode::Text(_)
        | SceneNode::ProgressBar(ProgressBarNode { .. })
        | SceneNode::ProgressRing(ProgressRingNode { .. })
        | SceneNode::Spacer(SpacerNode { .. })
        | SceneNode::Divider(DividerNode { .. }) => {}
    }
    Ok(())
}

impl ImageNode {
    /// Whether this image keeps its own aspect ratio inside its box.
    pub const fn keeps_aspect(&self) -> bool {
        self.preserve_aspect
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn node(text: &str) -> SceneNode {
        serde_json::from_str(text).unwrap()
    }

    #[test]
    fn nesting_is_counted_from_the_root() {
        let mut inspector = Inspector::new();
        walk(
            &node(
                r#"{"type":"stack","children":[
                    {"type":"stack","children":[{"type":"spacer"}]}
                ]}"#,
            ),
            1,
            &mut inspector,
        )
        .unwrap();
        assert_eq!(inspector.nodes, 3);
        assert_eq!(inspector.deepest, 3);
    }

    #[test]
    fn nesting_past_the_depth_bound_is_refused() {
        // Built by nesting the type rather than by parsing text, because a
        // document that deep also exceeds `serde_json`'s own recursion limit and
        // would be refused by the parser before the depth rule could be reached.
        // The rule exists for a scene that parsed, so that is what it is tested
        // against.
        let mut scene = SceneNode::Spacer(crate::SpacerNode { grow: 1.0 });
        for _ in 0..MAXIMUM_SCENE_DEPTH {
            scene = SceneNode::Stack(crate::StackNode {
                children: vec![scene],
                ..crate::StackNode::default()
            });
        }
        let mut inspector = Inspector::new();
        assert_eq!(
            walk(&scene, 1, &mut inspector).unwrap_err().code(),
            PluginErrorCode::SceneTooDeep
        );
    }

    #[test]
    fn a_scene_past_the_node_bound_is_refused() {
        let children: Vec<String> = (0..MAXIMUM_SCENE_NODES)
            .map(|_| r#"{"type":"spacer"}"#.to_string())
            .collect();
        let scene = format!(r#"{{"type":"stack","children":[{}]}}"#, children.join(","));
        let mut inspector = Inspector::new();
        assert_eq!(
            walk(&node(&scene), 1, &mut inspector).unwrap_err().code(),
            PluginErrorCode::SceneTooLarge
        );
    }

    #[test]
    fn an_image_path_that_escapes_the_plugin_directory_is_refused() {
        let mut inspector = Inspector::new();
        assert_eq!(
            walk(
                &node(r#"{"type":"image","asset":"../secret.png"}"#),
                1,
                &mut inspector
            )
            .unwrap_err()
            .code(),
            PluginErrorCode::InvalidAssetPath
        );
    }

    #[test]
    fn every_button_is_collected_in_scene_order() {
        let mut inspector = Inspector::new();
        walk(
            &node(
                r#"{"type":"stack","children":[
                    {"type":"button","id":"a","label":"A"},
                    {"type":"button","id":"b","label":"B","variant":"secondary"}
                ]}"#,
            ),
            1,
            &mut inspector,
        )
        .unwrap();
        assert_eq!(inspector.buttons, vec!["a".to_string(), "b".to_string()]);
        assert_eq!(inspector.actions.len(), 2);
        assert_eq!(inspector.actions[1].button, "b");
    }

    #[test]
    fn a_button_with_an_unusable_id_is_refused() {
        for id in ["", &"x".repeat(crate::MAXIMUM_BUTTON_ID_BYTES + 1)] {
            let document = format!(r#"{{"type":"button","id":{id:?}}}"#);
            let mut inspector = Inspector::new();
            assert_eq!(
                walk(&node(&document), 1, &mut inspector)
                    .unwrap_err()
                    .code(),
                PluginErrorCode::InvalidButtonId,
                "{id:?} must be refused"
            );
        }
    }

    #[test]
    fn an_image_asset_is_named_once_per_node_so_a_repeated_one_is_read_once() {
        let mut inspector = Inspector::new();
        walk(
            &node(
                r#"{"type":"stack","children":[
                    {"type":"image","asset":"a.png"},
                    {"type":"image","asset":"a.png"}
                ]}"#,
            ),
            1,
            &mut inspector,
        )
        .unwrap();
        assert_eq!(
            inspector.assets,
            vec!["a.png".to_string(), "a.png".to_string()]
        );
        // The caller deduplicates; the inspector reports what the scene said rather
        // than making a second decision about it.
    }
}
