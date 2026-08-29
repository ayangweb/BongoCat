use accesskit::{
    Action, ActionData, ActionHandler, ActionRequest, ActivationHandler, Invalid, Live, Node,
    NodeId, Rect, Role, Toggled, TreeId, TreeInfo, TreeUpdate,
};
use async_channel::{Receiver, Sender, bounded};
use gpui::Window;
use raw_window_handle::{HasWindowHandle, RawWindowHandle};
use std::sync::{Arc, Mutex};

const ROOT_ID: NodeId = NodeId(1);
const APPEARANCE_ID: NodeId = NodeId(2);
const SYSTEM_THEME_ID: NodeId = NodeId(3);
const LIGHT_THEME_ID: NodeId = NodeId(4);
const DARK_THEME_ID: NodeId = NodeId(5);
const MODEL_NAME_ID: NodeId = NodeId(6);
const REFRESH_ID: NodeId = NodeId(7);
const STATUS_ID: NodeId = NodeId(8);
const ACTION_QUEUE_CAPACITY: usize = 32;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AccessibilityTheme {
    System,
    Light,
    Dark,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum AccessibilityAction {
    SelectTheme(AccessibilityTheme),
    FocusTheme(AccessibilityTheme),
    FocusModelName,
    SetModelName(String),
    FocusRefresh,
    RefreshRuntime,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AccessibilityFocus {
    Root,
    Theme(AccessibilityTheme),
    ModelName,
    Refresh,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AccessibilitySnapshot {
    pub selected_theme: &'static str,
    pub model_name: String,
    pub runtime_status: String,
    pub runtime_busy: bool,
    pub runtime_error: bool,
    pub focus: AccessibilityFocus,
}

impl Default for AccessibilitySnapshot {
    fn default() -> Self {
        Self {
            selected_theme: "System",
            model_name: String::new(),
            runtime_status: "Runtime unavailable".into(),
            runtime_busy: false,
            runtime_error: false,
            focus: AccessibilityFocus::Root,
        }
    }
}

impl AccessibilitySnapshot {
    fn tree_update(&self) -> TreeUpdate {
        let mut root = Node::new(Role::Window);
        root.set_label("BongoCat Settings");
        root.set_bounds(Rect::new(0.0, 0.0, 760.0, 520.0));
        root.set_children(vec![APPEARANCE_ID]);

        let mut appearance = Node::new(Role::Group);
        appearance.set_label("Appearance");
        appearance.set_bounds(Rect::new(180.0, 0.0, 760.0, 520.0));
        appearance.set_children(vec![
            SYSTEM_THEME_ID,
            LIGHT_THEME_ID,
            DARK_THEME_ID,
            MODEL_NAME_ID,
            STATUS_ID,
            REFRESH_ID,
        ]);

        let themes = [
            (SYSTEM_THEME_ID, "System"),
            (LIGHT_THEME_ID, "Light"),
            (DARK_THEME_ID, "Dark"),
        ]
        .into_iter()
        .enumerate()
        .map(|(index, (id, label))| {
            let mut node = Node::new(Role::RadioButton);
            node.set_label(label);
            node.set_toggled(if self.selected_theme == label {
                Toggled::True
            } else {
                Toggled::False
            });
            node.add_action(Action::Click);
            node.add_action(Action::Focus);
            node.set_bounds(Rect::new(
                204.0 + index as f64 * 170.0,
                110.0,
                364.0 + index as f64 * 170.0,
                146.0,
            ));
            (id, node)
        });

        let mut model_name = Node::new(Role::TextInput);
        model_name.set_label("Model display name");
        model_name.set_value(self.model_name.clone());
        model_name.add_action(Action::Focus);
        model_name.add_action(Action::SetValue);
        model_name.set_bounds(Rect::new(204.0, 245.0, 736.0, 281.0));

        let mut status = Node::new(Role::Status);
        status.set_label("Runtime status");
        status.set_value(self.runtime_status.clone());
        status.set_live(Live::Polite);
        if self.runtime_busy {
            status.set_busy();
        }
        if self.runtime_error {
            status.set_invalid(Invalid::True);
        }
        status.set_bounds(Rect::new(204.0, 376.0, 620.0, 408.0));

        let mut refresh = Node::new(Role::Button);
        refresh.set_label("Refresh");
        refresh.add_action(Action::Click);
        refresh.add_action(Action::Focus);
        refresh.set_bounds(Rect::new(632.0, 376.0, 720.0, 408.0));

        let mut nodes = vec![(ROOT_ID, root), (APPEARANCE_ID, appearance)];
        nodes.extend(themes);
        nodes.extend([
            (MODEL_NAME_ID, model_name),
            (STATUS_ID, status),
            (REFRESH_ID, refresh),
        ]);
        TreeUpdate {
            nodes,
            tree: Some(TreeInfo::new(ROOT_ID)),
            tree_id: TreeId::ROOT,
            focus: match self.focus {
                AccessibilityFocus::Root => ROOT_ID,
                AccessibilityFocus::Theme(AccessibilityTheme::System) => SYSTEM_THEME_ID,
                AccessibilityFocus::Theme(AccessibilityTheme::Light) => LIGHT_THEME_ID,
                AccessibilityFocus::Theme(AccessibilityTheme::Dark) => DARK_THEME_ID,
                AccessibilityFocus::ModelName => MODEL_NAME_ID,
                AccessibilityFocus::Refresh => REFRESH_ID,
            },
        }
    }
}

struct TreeProvider(Arc<Mutex<AccessibilitySnapshot>>);

impl ActivationHandler for TreeProvider {
    fn request_initial_tree(&mut self) -> Option<TreeUpdate> {
        Some(
            self.0
                .lock()
                .expect("accessibility snapshot lock poisoned")
                .tree_update(),
        )
    }
}

struct BridgeActionHandler {
    sender: Sender<AccessibilityAction>,
}

impl ActionHandler for BridgeActionHandler {
    fn do_action(&mut self, request: ActionRequest) {
        let Some(action) = action_from_request(&request) else {
            return;
        };
        if let Err(error) = self.sender.try_send(action) {
            eprintln!("gpui-settings-spike: accessibility action queue rejected request: {error}");
        }
    }
}

fn theme_for_node(node: NodeId) -> Option<AccessibilityTheme> {
    match node {
        SYSTEM_THEME_ID => Some(AccessibilityTheme::System),
        LIGHT_THEME_ID => Some(AccessibilityTheme::Light),
        DARK_THEME_ID => Some(AccessibilityTheme::Dark),
        _ => None,
    }
}

fn action_from_request(request: &ActionRequest) -> Option<AccessibilityAction> {
    match (request.target_node, request.action, request.data.as_ref()) {
        (MODEL_NAME_ID, Action::Focus, _) => Some(AccessibilityAction::FocusModelName),
        (MODEL_NAME_ID, Action::SetValue, Some(ActionData::Value(value))) => {
            Some(AccessibilityAction::SetModelName(value.to_string()))
        }
        (REFRESH_ID, Action::Focus, _) => Some(AccessibilityAction::FocusRefresh),
        (REFRESH_ID, Action::Click, _) => Some(AccessibilityAction::RefreshRuntime),
        (node, Action::Click, _) => theme_for_node(node).map(AccessibilityAction::SelectTheme),
        (node, Action::Focus, _) => theme_for_node(node).map(AccessibilityAction::FocusTheme),
        _ => None,
    }
}

pub struct AccessibilityBridge {
    snapshot: Arc<Mutex<AccessibilitySnapshot>>,
    #[cfg(target_os = "macos")]
    adapter: accesskit_macos::SubclassingAdapter,
    #[cfg(target_os = "macos")]
    view: *mut core::ffi::c_void,
    #[cfg(target_os = "windows")]
    adapter: accesskit_windows::SubclassingAdapter,
}

impl AccessibilityBridge {
    pub fn attach(window: &Window) -> Result<(Self, Receiver<AccessibilityAction>), String> {
        let snapshot = Arc::new(Mutex::new(AccessibilitySnapshot::default()));
        let (action_sender, action_receiver) = bounded(ACTION_QUEUE_CAPACITY);
        let raw = HasWindowHandle::window_handle(window)
            .map_err(|error| format!("read GPUI raw window handle: {error}"))?
            .as_raw();

        #[cfg(target_os = "macos")]
        let (adapter, view) = match raw {
            RawWindowHandle::AppKit(handle) => {
                let view = handle.ns_view.as_ptr();
                // SAFETY: GPUI owns this NSView for the complete Window lifetime.
                // The adapter is installed while WindowOptions::show is false,
                // retained by this bridge, and dropped before the GPUI window.
                let adapter = unsafe {
                    accesskit_macos::SubclassingAdapter::new(
                        view,
                        TreeProvider(Arc::clone(&snapshot)),
                        BridgeActionHandler {
                            sender: action_sender,
                        },
                    )
                };
                (adapter, view)
            }
            other => return Err(format!("expected AppKit window handle, found {other:?}")),
        };

        #[cfg(target_os = "windows")]
        let adapter = match raw {
            RawWindowHandle::Win32(handle) => {
                let hwnd = accesskit_windows::HWND(handle.hwnd.get() as *mut core::ffi::c_void);
                accesskit_windows::SubclassingAdapter::new(
                    hwnd,
                    TreeProvider(Arc::clone(&snapshot)),
                    BridgeActionHandler {
                        sender: action_sender,
                    },
                )
            }
            other => return Err(format!("expected Win32 window handle, found {other:?}")),
        };

        #[cfg(not(any(target_os = "macos", target_os = "windows")))]
        return Err(format!("unsupported GPUI accessibility handle: {raw:?}"));

        #[cfg(target_os = "macos")]
        return Ok((
            Self {
                snapshot,
                adapter,
                view,
            },
            action_receiver,
        ));

        #[cfg(target_os = "windows")]
        return Ok((Self { snapshot, adapter }, action_receiver));
    }

    pub fn update(&mut self, snapshot: AccessibilitySnapshot) {
        *self
            .snapshot
            .lock()
            .expect("accessibility snapshot lock poisoned") = snapshot.clone();
        if let Some(events) = self.adapter.update_if_active(|| snapshot.tree_update()) {
            events.raise();
        }
    }

    #[cfg(target_os = "macos")]
    pub fn verify_platform_tree(&self) -> Result<(), String> {
        use objc2::{msg_send, runtime::AnyObject};
        use std::{collections::BTreeMap, ffi::CStr, os::raw::c_char};

        unsafe fn string_from_object(value: *mut AnyObject) -> Option<String> {
            if value.is_null() {
                return None;
            }
            // SAFETY: value is an NSString returned by an NSAccessibility
            // string property and remains alive in the current autorelease pool.
            let utf8: *const c_char = unsafe { msg_send![value, UTF8String] };
            (!utf8.is_null()).then(|| {
                // SAFETY: NSString guarantees a NUL-terminated UTF-8 view for
                // the lifetime of the string object.
                unsafe { CStr::from_ptr(utf8) }
                    .to_string_lossy()
                    .into_owned()
            })
        }

        unsafe fn accessibility_title(object: *mut AnyObject) -> Option<String> {
            // SAFETY: object is an AccessKit NSAccessibility element.
            let value: *mut AnyObject = unsafe { msg_send![object, accessibilityTitle] };
            // SAFETY: accessibilityTitle returns NSString or nil.
            unsafe { string_from_object(value) }
        }

        unsafe fn accessibility_role(object: *mut AnyObject) -> Option<String> {
            // SAFETY: object is an AccessKit NSAccessibility element.
            let value: *mut AnyObject = unsafe { msg_send![object, accessibilityRole] };
            // SAFETY: accessibilityRole returns NSString or nil.
            unsafe { string_from_object(value) }
        }

        // SAFETY: the bridge retains the dynamically subclassed GPUI NSView.
        // All Objective-C messages run on the AppKit main thread and returned
        // objects remain owned by the accessibility adapter during inspection.
        unsafe {
            let children: *mut AnyObject =
                msg_send![self.view.cast::<AnyObject>(), accessibilityChildren];
            if children.is_null() {
                return Err("GPUI content view returned no accessibility children".into());
            }
            let count: usize = msg_send![children, count];
            if count == 0 {
                return Err("GPUI content view accessibility children were empty".into());
            }
            let root: *mut AnyObject = msg_send![children, objectAtIndex: 0_usize];
            if accessibility_role(root).as_deref() != Some("AXGroup") {
                return Err("AccessKit root was not exposed as an AppKit AXGroup".into());
            }
            let root_children: *mut AnyObject = msg_send![root, accessibilityChildren];
            if root_children.is_null() {
                return Err("AccessKit root returned no semantic descendants".into());
            }
            let root_child_count: usize = msg_send![root_children, count];
            if root_child_count != 1 {
                return Err(format!(
                    "AccessKit root exposed {root_child_count} children instead of one settings group"
                ));
            }
            let appearance: *mut AnyObject = msg_send![root_children, objectAtIndex: 0_usize];
            if accessibility_title(appearance).as_deref() != Some("Appearance")
                || accessibility_role(appearance).as_deref() != Some("AXGroup")
            {
                return Err("Appearance was not exposed as a titled AppKit AXGroup".into());
            }
            let controls: *mut AnyObject = msg_send![appearance, accessibilityChildren];
            if controls.is_null() {
                return Err("Appearance group returned no semantic controls".into());
            }
            let control_count: usize = msg_send![controls, count];
            let mut controls_by_title = BTreeMap::new();
            let mut dark_theme_control = None;
            for index in 0..control_count {
                let control: *mut AnyObject = msg_send![controls, objectAtIndex: index];
                if let (Some(title), Some(role)) =
                    (accessibility_title(control), accessibility_role(control))
                {
                    if title == "Dark" {
                        dark_theme_control = Some(control);
                    }
                    controls_by_title.insert(title, role);
                }
            }
            for (title, expected_role) in [
                ("System", "AXRadioButton"),
                ("Light", "AXRadioButton"),
                ("Dark", "AXRadioButton"),
                ("Model display name", "AXTextField"),
                ("Runtime status", "AXGroup"),
                ("Refresh", "AXButton"),
            ] {
                if controls_by_title.get(title).map(String::as_str) != Some(expected_role) {
                    return Err(format!(
                        "{title} was not exposed with AppKit role {expected_role}"
                    ));
                }
            }
            let dark_theme_control = dark_theme_control
                .ok_or_else(|| "Dark theme AXRadioButton was not discoverable".to_string())?;
            let performed: bool = msg_send![dark_theme_control, accessibilityPerformPress];
            if !performed {
                return Err("Dark theme AXRadioButton rejected accessibility press".into());
            }
            println!(
                "gpui-settings-spike: accessibility tree root_role=AXGroup nodes={} controls={control_count}",
                control_count + 2
            );
        }
        Ok(())
    }

    #[cfg(not(target_os = "macos"))]
    pub fn verify_platform_tree(&self) -> Result<(), String> {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn semantic_tree_contains_required_settings_nodes() {
        let update = AccessibilitySnapshot {
            selected_theme: "Dark",
            model_name: "Mochi".into(),
            runtime_status: "Runtime Ready".into(),
            runtime_busy: false,
            runtime_error: false,
            focus: AccessibilityFocus::ModelName,
        }
        .tree_update();
        assert_eq!(update.tree.unwrap().root, ROOT_ID);
        assert_eq!(update.focus, MODEL_NAME_ID);
        assert_eq!(update.nodes.len(), 8);
        assert!(
            update
                .nodes
                .iter()
                .any(|(id, node)| { *id == MODEL_NAME_ID && node.value() == Some("Mochi") })
        );
        assert!(
            update.nodes.iter().any(|(id, node)| {
                *id == DARK_THEME_ID && node.toggled() == Some(Toggled::True)
            })
        );
        let status = update
            .nodes
            .iter()
            .find(|(id, _)| *id == STATUS_ID)
            .map(|(_, node)| node)
            .unwrap();
        assert!(!status.is_busy());
        assert_eq!(status.invalid(), None);
    }

    #[test]
    fn translates_supported_platform_actions_to_typed_ui_actions() {
        let request = |target_node, action, data| ActionRequest {
            action,
            target_tree: TreeId::ROOT,
            target_node,
            data,
        };

        assert_eq!(
            action_from_request(&request(DARK_THEME_ID, Action::Click, None)),
            Some(AccessibilityAction::SelectTheme(AccessibilityTheme::Dark))
        );
        assert_eq!(
            action_from_request(&request(
                MODEL_NAME_ID,
                Action::SetValue,
                Some(ActionData::Value("Mochi".into()))
            )),
            Some(AccessibilityAction::SetModelName("Mochi".into()))
        );
        assert_eq!(
            action_from_request(&request(STATUS_ID, Action::Click, None)),
            None
        );
    }
}
