//! What a chord means, and the behaviour a model binds it to.
//!
//! Two vocabularies live here because they are the two ends of one binding: a
//! chord means an application command the runtime acts on, and a model behaviour
//! action is what the model does while that chord is held. Both parse from a
//! document string and both refuse what they do not know, so a hand-edited
//! configuration fails validation instead of binding to nothing.

use super::*;

/// Application-level commands that may be persisted as global shortcuts.
/// Keeping this list closed prevents an unvalidated string from becoming a
/// platform registration or runtime command.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ShortcutCommand {
    ToggleOverlay,
    OpenSettings,
    ToggleIgnoreMouseInput,
    ToggleIgnoreKeyboardInput,
    ToggleIgnoreGamepadInput,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, thiserror::Error)]
pub enum ShortcutCommandParseError {
    #[error("shortcut command must not be blank")]
    Empty,
    #[error("shortcut command is not supported")]
    Unknown,
}

impl ShortcutCommand {
    pub fn parse(value: &str) -> Result<Self, ShortcutCommandParseError> {
        match value.trim() {
            "toggle_overlay" => Ok(Self::ToggleOverlay),
            "open_settings" => Ok(Self::OpenSettings),
            "toggle_ignore_mouse_input" => Ok(Self::ToggleIgnoreMouseInput),
            "toggle_ignore_keyboard_input" => Ok(Self::ToggleIgnoreKeyboardInput),
            "toggle_ignore_gamepad_input" => Ok(Self::ToggleIgnoreGamepadInput),
            "" => Err(ShortcutCommandParseError::Empty),
            _ => Err(ShortcutCommandParseError::Unknown),
        }
    }

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::ToggleOverlay => "toggle_overlay",
            Self::OpenSettings => "open_settings",
            Self::ToggleIgnoreMouseInput => "toggle_ignore_mouse_input",
            Self::ToggleIgnoreKeyboardInput => "toggle_ignore_keyboard_input",
            Self::ToggleIgnoreGamepadInput => "toggle_ignore_gamepad_input",
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[cfg_attr(any(test, feature = "schema-generation"), derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct ShortcutBinding {
    #[cfg_attr(
        any(test, feature = "schema-generation"),
        schemars(length(min = 1), regex(pattern = ".*\\S.*"))
    )]
    pub command: String,
    #[cfg_attr(
        any(test, feature = "schema-generation"),
        schemars(length(min = 1), regex(pattern = ".*\\S.*"))
    )]
    pub shortcut: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[cfg_attr(any(test, feature = "schema-generation"), derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct ModelBehaviorBinding {
    /// The complete model identity, including source. An id alone is not
    /// unique because built-in and imported catalogs may contain the same id.
    pub model: ModelIdentity,
    #[cfg_attr(
        any(test, feature = "schema-generation"),
        schemars(length(min = 1), regex(pattern = ".*\\S.*"))
    )]
    pub behavior_id: String,
    #[cfg_attr(
        any(test, feature = "schema-generation"),
        schemars(length(min = 1), regex(pattern = ".*\\S.*"))
    )]
    pub shortcut: String,
}

/// A model action encoded by the Native shortcut contract. The model identity
/// is kept on the binding so the application can scope the action to one model.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ModelBehaviorAction {
    Motion { group: String, index: usize },
    Expression { name: String },
}

impl ModelBehaviorAction {
    /// The canonical `behavior_id` spelling this action persists as. Both the
    /// canonicalizing path and the default-assignment path go through here, so
    /// a generated binding and a user-recorded one can never disagree on how
    /// the same motion or expression is named.
    pub fn behavior_id(&self) -> String {
        match self {
            Self::Motion { group, index } => format!("motion:{group}:{index}"),
            Self::Expression { name } => format!("expression:{name}"),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, thiserror::Error)]
pub enum ModelBehaviorParseError {
    #[error("model behavior must not be blank")]
    Empty,
    #[error("model motion behavior must be motion:<group>:<index>")]
    InvalidMotion,
    #[error("model expression behavior must be expression:<name>")]
    InvalidExpression,
    #[error("model behavior kind is not supported")]
    UnknownKind,
}

impl ModelBehaviorBinding {
    pub fn parse_action(&self) -> Result<ModelBehaviorAction, ModelBehaviorParseError> {
        let value = self.behavior_id.trim();
        if value.is_empty() {
            return Err(ModelBehaviorParseError::Empty);
        }
        let mut parts = value.split(':');
        match parts.next() {
            Some("motion") => {
                let group = parts.next().unwrap_or_default().trim();
                let Some(index) = parts.next() else {
                    return Err(ModelBehaviorParseError::InvalidMotion);
                };
                if group.is_empty() || parts.next().is_some() {
                    return Err(ModelBehaviorParseError::InvalidMotion);
                }
                let index = index
                    .parse::<usize>()
                    .map_err(|_| ModelBehaviorParseError::InvalidMotion)?;
                Ok(ModelBehaviorAction::Motion {
                    group: group.to_owned(),
                    index,
                })
            }
            Some("expression") => {
                let name = parts.collect::<Vec<_>>().join(":");
                if name.trim().is_empty() {
                    return Err(ModelBehaviorParseError::InvalidExpression);
                }
                Ok(ModelBehaviorAction::Expression {
                    name: name.trim().to_owned(),
                })
            }
            Some(_) => Err(ModelBehaviorParseError::UnknownKind),
            None => Err(ModelBehaviorParseError::Empty),
        }
    }
}
