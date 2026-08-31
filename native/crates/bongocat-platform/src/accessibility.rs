use std::{collections::BTreeSet, fmt};

#[cfg(any(target_os = "macos", target_os = "windows"))]
use accesskit::{
    Action, ActionHandler, ActionRequest, ActivationHandler, Node, NodeId, Rect, Role, Toggled,
    TreeId, TreeInfo, TreeUpdate,
};
#[cfg(any(target_os = "macos", target_os = "windows"))]
use async_channel::{Receiver, Sender, bounded};
#[cfg(any(target_os = "macos", target_os = "windows"))]
use raw_window_handle::RawWindowHandle;
#[cfg(any(target_os = "macos", target_os = "windows"))]
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicU64, Ordering},
};

#[cfg(any(target_os = "macos", target_os = "windows"))]
const ACTION_QUEUE_CAPACITY: usize = 32;

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct AccessibilityNodeId(u64);

impl AccessibilityNodeId {
    pub const fn new(value: u64) -> Self {
        Self(value)
    }

    pub const fn get(self) -> u64 {
        self.0
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AccessibilityRole {
    Window,
    Group,
    Button,
    Switch,
    Label,
    Status,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AccessibilityToggle {
    Off,
    On,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct AccessibilityBounds {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

impl AccessibilityBounds {
    pub const fn new(x: f64, y: f64, width: f64, height: f64) -> Self {
        Self {
            x,
            y,
            width,
            height,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct AccessibilityNode {
    pub id: AccessibilityNodeId,
    pub role: AccessibilityRole,
    pub label: String,
    pub description: Option<String>,
    pub value: Option<String>,
    pub toggled: Option<AccessibilityToggle>,
    pub disabled: bool,
    pub supports_click: bool,
    pub supports_focus: bool,
    pub bounds: Option<AccessibilityBounds>,
    pub children: Vec<AccessibilityNodeId>,
}

impl AccessibilityNode {
    pub fn new(id: AccessibilityNodeId, role: AccessibilityRole, label: impl Into<String>) -> Self {
        Self {
            id,
            role,
            label: label.into(),
            description: None,
            value: None,
            toggled: None,
            disabled: false,
            supports_click: false,
            supports_focus: false,
            bounds: None,
            children: Vec::new(),
        }
    }

    pub fn with_description(mut self, description: impl Into<String>) -> Self {
        self.description = Some(description.into());
        self
    }

    pub fn with_value(mut self, value: impl Into<String>) -> Self {
        self.value = Some(value.into());
        self
    }

    pub const fn with_toggle(mut self, toggled: AccessibilityToggle) -> Self {
        self.toggled = Some(toggled);
        self
    }

    pub const fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }

    pub const fn clickable(mut self) -> Self {
        self.supports_click = true;
        self
    }

    pub const fn focusable(mut self) -> Self {
        self.supports_focus = true;
        self
    }

    pub const fn with_bounds(mut self, bounds: AccessibilityBounds) -> Self {
        self.bounds = Some(bounds);
        self
    }

    pub fn with_children(mut self, children: Vec<AccessibilityNodeId>) -> Self {
        self.children = children;
        self
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct AccessibilityTree {
    pub root: AccessibilityNodeId,
    pub focus: AccessibilityNodeId,
    pub nodes: Vec<AccessibilityNode>,
}

impl AccessibilityTree {
    pub fn validate(&self) -> Result<(), AccessibilityError> {
        let mut ids = BTreeSet::new();
        for node in &self.nodes {
            if !ids.insert(node.id) {
                return Err(AccessibilityError::DuplicateNode);
            }
        }
        if !ids.contains(&self.root) {
            return Err(AccessibilityError::MissingRoot);
        }
        if !ids.contains(&self.focus) {
            return Err(AccessibilityError::MissingFocus);
        }
        if self
            .nodes
            .iter()
            .flat_map(|node| &node.children)
            .any(|child| !ids.contains(child))
        {
            return Err(AccessibilityError::MissingChild);
        }
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AccessibilityAction {
    Click,
    Focus,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct AccessibilityActionRequest {
    pub target: AccessibilityNodeId,
    pub action: AccessibilityAction,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct AccessibilityDiagnostics {
    pub actions_forwarded: u64,
    pub actions_rejected: u64,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AccessibilityError {
    UnsupportedPlatform,
    UnsupportedWindowHandle,
    DuplicateNode,
    MissingRoot,
    MissingFocus,
    MissingChild,
}

impl fmt::Display for AccessibilityError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::UnsupportedPlatform => "accessibility is unsupported on this platform",
            Self::UnsupportedWindowHandle => "accessibility received an unsupported window handle",
            Self::DuplicateNode => "accessibility tree contains a duplicate node",
            Self::MissingRoot => "accessibility tree root is missing",
            Self::MissingFocus => "accessibility tree focus is missing",
            Self::MissingChild => "accessibility tree contains a missing child",
        })
    }
}

impl std::error::Error for AccessibilityError {}

#[cfg(any(target_os = "macos", target_os = "windows"))]
#[derive(Default)]
struct AccessibilityCounters {
    actions_forwarded: AtomicU64,
    actions_rejected: AtomicU64,
}

#[cfg(any(target_os = "macos", target_os = "windows"))]
impl AccessibilityCounters {
    fn snapshot(&self) -> AccessibilityDiagnostics {
        AccessibilityDiagnostics {
            actions_forwarded: self.actions_forwarded.load(Ordering::Relaxed),
            actions_rejected: self.actions_rejected.load(Ordering::Relaxed),
        }
    }
}

#[cfg(any(target_os = "macos", target_os = "windows"))]
struct TreeProvider(Arc<Mutex<AccessibilityTree>>);

#[cfg(any(target_os = "macos", target_os = "windows"))]
impl ActivationHandler for TreeProvider {
    fn request_initial_tree(&mut self) -> Option<TreeUpdate> {
        let tree = self
            .0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        Some(tree_update(&tree))
    }
}

#[cfg(any(target_os = "macos", target_os = "windows"))]
struct BridgeActionHandler {
    sender: Sender<AccessibilityActionRequest>,
    counters: Arc<AccessibilityCounters>,
}

#[cfg(any(target_os = "macos", target_os = "windows"))]
impl ActionHandler for BridgeActionHandler {
    fn do_action(&mut self, request: ActionRequest) {
        let action = match request.action {
            Action::Click => AccessibilityAction::Click,
            Action::Focus => AccessibilityAction::Focus,
            _ => return,
        };
        let request = AccessibilityActionRequest {
            target: AccessibilityNodeId::new(request.target_node.0),
            action,
        };
        if self.sender.try_send(request).is_ok() {
            self.counters
                .actions_forwarded
                .fetch_add(1, Ordering::Relaxed);
        } else {
            self.counters
                .actions_rejected
                .fetch_add(1, Ordering::Relaxed);
        }
    }
}

#[cfg(any(target_os = "macos", target_os = "windows"))]
pub struct SettingsAccessibilityBridge {
    tree: Arc<Mutex<AccessibilityTree>>,
    counters: Arc<AccessibilityCounters>,
    #[cfg(target_os = "macos")]
    adapter: accesskit_macos::SubclassingAdapter,
    #[cfg(target_os = "windows")]
    adapter: accesskit_windows::SubclassingAdapter,
}

#[cfg(any(target_os = "macos", target_os = "windows"))]
impl SettingsAccessibilityBridge {
    pub fn attach(
        raw: RawWindowHandle,
        initial_tree: AccessibilityTree,
    ) -> Result<(Self, Receiver<AccessibilityActionRequest>), AccessibilityError> {
        initial_tree.validate()?;
        let tree = Arc::new(Mutex::new(initial_tree));
        let counters = Arc::new(AccessibilityCounters::default());
        let (sender, receiver) = bounded(ACTION_QUEUE_CAPACITY);
        let provider = TreeProvider(Arc::clone(&tree));
        let handler = BridgeActionHandler {
            sender,
            counters: Arc::clone(&counters),
        };

        #[cfg(target_os = "macos")]
        let adapter = match raw {
            RawWindowHandle::AppKit(handle) => {
                // SAFETY: GPUI owns the NSView for the complete Window lifetime. The bridge is
                // installed before the window is shown, retained by SettingsView, and dropped
                // before GPUI destroys the corresponding Window.
                unsafe {
                    accesskit_macos::SubclassingAdapter::new(
                        handle.ns_view.as_ptr(),
                        provider,
                        handler,
                    )
                }
            }
            _ => return Err(AccessibilityError::UnsupportedWindowHandle),
        };

        #[cfg(target_os = "windows")]
        let adapter = match raw {
            RawWindowHandle::Win32(handle) => {
                let hwnd = accesskit_windows::HWND(handle.hwnd.get() as *mut core::ffi::c_void);
                accesskit_windows::SubclassingAdapter::new(hwnd, provider, handler)
            }
            _ => return Err(AccessibilityError::UnsupportedWindowHandle),
        };

        Ok((
            Self {
                tree,
                counters,
                adapter,
            },
            receiver,
        ))
    }

    pub fn update(&mut self, tree: AccessibilityTree) -> Result<(), AccessibilityError> {
        tree.validate()?;
        *self
            .tree
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = tree.clone();
        if let Some(events) = self.adapter.update_if_active(|| tree_update(&tree)) {
            events.raise();
        }
        Ok(())
    }

    pub fn diagnostics(&self) -> AccessibilityDiagnostics {
        self.counters.snapshot()
    }
}

#[cfg(any(target_os = "macos", target_os = "windows"))]
fn tree_update(tree: &AccessibilityTree) -> TreeUpdate {
    let nodes = tree
        .nodes
        .iter()
        .map(|source| {
            let mut node = Node::new(match source.role {
                AccessibilityRole::Window => Role::Window,
                AccessibilityRole::Group => Role::Group,
                AccessibilityRole::Button => Role::Button,
                AccessibilityRole::Switch => Role::Switch,
                AccessibilityRole::Label => Role::Label,
                AccessibilityRole::Status => Role::Status,
            });
            node.set_label(source.label.clone());
            if let Some(description) = &source.description {
                node.set_description(description.clone());
            }
            if let Some(value) = &source.value {
                node.set_value(value.clone());
            }
            if let Some(toggled) = source.toggled {
                node.set_toggled(match toggled {
                    AccessibilityToggle::Off => Toggled::False,
                    AccessibilityToggle::On => Toggled::True,
                });
            }
            if source.disabled {
                node.set_disabled();
            }
            if source.supports_click {
                node.add_action(Action::Click);
            }
            if source.supports_focus {
                node.add_action(Action::Focus);
            }
            if let Some(bounds) = source.bounds {
                node.set_bounds(Rect::new(
                    bounds.x,
                    bounds.y,
                    bounds.x + bounds.width,
                    bounds.y + bounds.height,
                ));
            }
            node.set_children(
                source
                    .children
                    .iter()
                    .map(|id| NodeId(id.get()))
                    .collect::<Vec<_>>(),
            );
            (NodeId(source.id.get()), node)
        })
        .collect();
    TreeUpdate {
        nodes,
        tree: Some(TreeInfo::new(NodeId(tree.root.get()))),
        tree_id: TreeId::ROOT,
        focus: NodeId(tree.focus.get()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn valid_tree() -> AccessibilityTree {
        let root = AccessibilityNodeId::new(1);
        let control = AccessibilityNodeId::new(2);
        AccessibilityTree {
            root,
            focus: control,
            nodes: vec![
                AccessibilityNode::new(root, AccessibilityRole::Window, "Settings")
                    .with_children(vec![control]),
                AccessibilityNode::new(control, AccessibilityRole::Switch, "Open at login")
                    .with_toggle(AccessibilityToggle::On)
                    .clickable()
                    .focusable(),
            ],
        }
    }

    #[test]
    fn project_tree_validation_rejects_missing_and_duplicate_nodes() {
        assert_eq!(valid_tree().validate(), Ok(()));

        let mut duplicate = valid_tree();
        duplicate.nodes.push(duplicate.nodes[1].clone());
        assert_eq!(duplicate.validate(), Err(AccessibilityError::DuplicateNode));

        let mut missing_child = valid_tree();
        missing_child.nodes[0]
            .children
            .push(AccessibilityNodeId::new(99));
        assert_eq!(
            missing_child.validate(),
            Err(AccessibilityError::MissingChild)
        );

        let mut missing_focus = valid_tree();
        missing_focus.focus = AccessibilityNodeId::new(99);
        assert_eq!(
            missing_focus.validate(),
            Err(AccessibilityError::MissingFocus)
        );
    }

    #[cfg(any(target_os = "macos", target_os = "windows"))]
    #[test]
    fn accesskit_tree_retains_toggle_value_focus_and_actions() {
        let update = tree_update(&valid_tree());
        assert_eq!(update.tree.expect("tree info").root, NodeId(1));
        assert_eq!(update.focus, NodeId(2));
        let (_, control) = update
            .nodes
            .iter()
            .find(|(id, _)| *id == NodeId(2))
            .expect("startup control");
        assert_eq!(control.label(), Some("Open at login"));
        assert_eq!(control.toggled(), Some(Toggled::True));
        assert!(control.supports_action(Action::Click));
        assert!(control.supports_action(Action::Focus));
    }
}
