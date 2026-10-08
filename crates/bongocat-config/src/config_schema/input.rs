//! What a mouse and a gamepad each mean.
//!
//! A captured key needs no configuration of its own: its pressed state is
//! cleared by the release, the reconciliation and the reset paths, and by
//! nothing else. There is no keyboard namespace to write to.

use super::*;

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[cfg_attr(any(test, feature = "schema-generation"), derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct InputConfig {
    #[serde(default = "default_pointer_sensitivity_percent")]
    #[cfg_attr(
        any(test, feature = "schema-generation"),
        schemars(range(min = 1, max = 400))
    )]
    pub pointer_sensitivity_percent: u16,
    pub gamepad: GamepadInputConfig,
    /// How the pointer is read before any model sees it.
    ///
    /// `#[serde(default)]` keeps a configuration written before the field
    /// existed on the shipped behaviour rather than failing the strict v1 parse,
    /// the same reason `GamepadInputConfig`'s own fields predate it.
    #[serde(default)]
    pub mouse: MouseInputConfig,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
#[cfg_attr(any(test, feature = "schema-generation"), derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct MouseInputConfig {
    /// Follow relative device motion instead of the absolute cursor position.
    ///
    /// Some applications, most full-screen games, capture the pointer and keep
    /// the operating-system cursor parked, so the absolute position stops
    /// moving while the device still reports every movement. With this on the
    /// pointer position is accumulated from that relative motion instead, so the
    /// model keeps following the pointer there. It is off by default because a
    /// pointer moved by something other than the device (a synthetic warp, an
    /// absolute pointing device) is not followed while it is on.
    ///
    /// `#[serde(default)]` keeps a document that omits the field on the off side
    /// rather than failing the strict v1 parse.
    #[serde(default)]
    pub force_move: bool,
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

const fn default_pointer_sensitivity_percent() -> u16 {
    100
}
