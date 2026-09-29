//! What a scene names, read before anything is drawn.
//!
//! The overlay cannot run a plugin, so a scene's problems have to be found in
//! the file rather than at layout time. This pass walks the tree once at load and
//! reports the three things a scene gets wrong: more nodes or more depth than the
//! bounds allow, a binding to a path nothing produces, and two buttons sharing an
//! id so a press would be ambiguous.
//!
//! It deliberately does not check geometry. Layout knows the panel's size and can
//! clip what does not fit, and a panel that overflows is a panel the user can
//! see is wrong — which is a better report than a refusal nobody can act on.

use super::{
    ButtonNode, DividerNode, ImageNode, MAXIMUM_SCENE_DEPTH, MAXIMUM_SCENE_NODES, ProgressBarNode,
    ProgressRingNode, SceneNode, SpacerNode, StackNode,
};
use crate::{PluginError, PluginErrorCode};

/// One button a scene declared.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SceneAction {
    pub button: String,
    pub target: Option<crate::BehaviorId>,
    /// What a press on it runs.
    ///
    /// Recorded rather than re-read from the scene at press time, so the host does
    /// not walk the scene on every press. The button's label is deliberately not
    /// recorded: a label is a binding that changes, and resolving it belongs to the
    /// draw pass, not to the press.
    pub action: crate::BehaviorAction,
}

/// What a scene names, gathered in one pass.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct Inspector {
    pub nodes: usize,
    pub deepest: usize,
    /// Every binding path the scene reads, in the order it reads them.
    pub bindings: Vec<String>,
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

macro_rules! record_binding {
    ($value:expr, $inspector:expr) => {
        if let Some(binding) = binding_of($value) {
            $inspector.bindings.push(binding?.to_string());
        }
    };
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
        SceneNode::Text(text) => record_binding!(&text.value, inspector),
        SceneNode::ProgressBar(ProgressBarNode { value, .. })
        | SceneNode::ProgressRing(ProgressRingNode { value, .. }) => {
            record_binding!(value, inspector);
        }
        SceneNode::Image(image) => {
            crate::validate_relative_asset_path(&image.asset)?;
            inspector.assets.push(image.asset.clone());
        }
        SceneNode::Button(ButtonNode {
            id,
            label,
            action,
            target,
            disabled,
            ..
        }) => {
            if id.is_empty() || id.len() > crate::MAXIMUM_BUTTON_ID_BYTES {
                return Err(PluginError::new(PluginErrorCode::InvalidButtonId));
            }
            record_binding!(label, inspector);
            if let Some(disabled) = disabled {
                record_binding!(disabled, inspector);
            }
            inspector.buttons.push(id.clone());
            inspector.actions.push(SceneAction {
                button: id.clone(),
                target: target.clone(),
                action: *action,
            });
        }
        SceneNode::Spacer(SpacerNode { .. }) | SceneNode::Divider(DividerNode { .. }) => {}
    }
    Ok(())
}

/// The path a value binds to, or `None` for a literal.
///
/// The length is checked here rather than at resolve time so a scene with an
/// absurd path is refused at load, and so no consumer of a binding has to know
/// there is a bound as well as a shape.
fn binding_of(value: &crate::SceneValue) -> Option<Result<&str, PluginError>> {
    let path = match value {
        crate::SceneValue::Text(_) => return None,
        crate::SceneValue::Binding { binding, .. }
        | crate::SceneValue::Fraction { binding, .. }
        | crate::SceneValue::Flag { binding, .. } => binding,
    };
    if path.is_empty() || path.len() > crate::MAXIMUM_BINDING_PATH_BYTES {
        return Some(Err(PluginError::new(PluginErrorCode::InvalidBinding)));
    }
    Some(Ok(path))
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
    fn a_flat_text_scene_names_one_binding() {
        let mut inspector = Inspector::new();
        walk(
            &node(r#"{"type":"text","value":{"binding":"a.b","fallback":""}}"#),
            1,
            &mut inspector,
        )
        .unwrap();
        assert_eq!(inspector.bindings, vec!["a.b".to_string()]);
        assert_eq!(inspector.nodes, 1);
        assert_eq!(inspector.deepest, 1);
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
    fn every_button_and_target_is_collected_in_scene_order() {
        let mut inspector = Inspector::new();
        walk(
            &node(
                r#"{"type":"stack","children":[
                    {"type":"button","id":"a","label":"A","action":"toggle","target":"t"},
                    {"type":"button","id":"b","label":"B","action":"reset"}
                ]}"#,
            ),
            1,
            &mut inspector,
        )
        .unwrap();
        assert_eq!(inspector.buttons, vec!["a".to_string(), "b".to_string()]);
        assert_eq!(inspector.actions.len(), 2);
        assert_eq!(
            inspector.actions[0].target.as_ref().map(|id| id.as_str()),
            Some("t")
        );
        assert_eq!(inspector.actions[1].target, None);
    }

    #[test]
    fn a_disabled_binding_is_named_like_any_other() {
        let mut inspector = Inspector::new();
        walk(
            &node(
                r#"{"type":"button","id":"a","label":"A","action":"toggle",
                    "disabled":{"flag":"t.busy","fallback":false}}"#,
            ),
            1,
            &mut inspector,
        )
        .unwrap();
        assert!(inspector.bindings.contains(&"t.busy".to_string()));
    }
}
