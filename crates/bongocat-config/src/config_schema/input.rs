//! What a key, a mouse and a gamepad each mean.
//!
//! Keyboard and gamepad input are separate structures rather than one list with a
//! source field, because they are replaced independently: changing a gamepad
//! binding must not rewrite the keyboard bindings in the same document, or the
//! revision the caller checked would no longer be the one it holds.

use super::*;

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[cfg_attr(any(test, feature = "schema-generation"), derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct InputConfig {
    pub keyboard: KeyboardInputConfig,
    pub gamepad: GamepadInputConfig,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[cfg_attr(any(test, feature = "schema-generation"), derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct KeyboardInputConfig {
    /// Final fallback for a captured keyboard key whose normal release,
    /// reconciliation and reset paths all failed to clear it.
    #[cfg_attr(
        any(test, feature = "schema-generation"),
        schemars(range(min = 0, max = 60_000))
    )]
    pub release_fallback_timeout_ms: u32,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[cfg_attr(any(test, feature = "schema-generation"), derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct GamepadInputConfig {
    #[cfg_attr(
        any(test, feature = "schema-generation"),
        schemars(range(min = 0.0, max = 1.0))
    )]
    pub stick_dead_zone: f64,
    #[cfg_attr(
        any(test, feature = "schema-generation"),
        schemars(range(min = 0.0, max = 1.0))
    )]
    pub trigger_dead_zone: f64,
}
