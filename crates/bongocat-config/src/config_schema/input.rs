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
    pub gamepad: GamepadInputConfig,
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
