//! The product binary's tests, split by the module they cover.
//!
//! The prelude lives here rather than in each module: one test reaches the root,
//! the module it covers and a neighbour it needs, and repeating that list seven
//! times would say less than writing it once.

use super::*;
use crate::gamepad_observer::*;
use crate::overlay_placement::*;
use crate::preset_root::*;
use crate::product_options::*;
use crate::product_plugins::*;
use crate::product_shutdown::*;
// The post-install restart test is the only thing here that reaches
// `product_windows`, and both the delay it asserts on and that test are macOS
// only. Globbing the module everywhere else leaves an unused import behind on
// Windows, which clippy refuses.
#[cfg(target_os = "macos")]
use crate::product_windows::*;
use crate::taskbar_settle::*;
use crate::update_schedule::*;
use bongocat_ui_protocol::SettingsCommand;

mod gamepad_observer;
mod overlay_placement;
mod preset_root;
mod product_options;
mod product_plugins;
mod product_shutdown;
mod smoke_status;
mod taskbar_settle;
mod update_schedule;
