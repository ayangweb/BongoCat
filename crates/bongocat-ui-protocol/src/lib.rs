#![forbid(unsafe_code)]

//! Typed contracts shared by the settings/update services and their UI views.
//!
//! This crate deliberately has no GPUI, operating-system, config, platform,
//! localization, runtime, model, or update-library dependency. It owns the stable
//! command/snapshot/error shapes
//! and the bounded channel clients; concrete application services and GPUI
//! views adapt those contracts at their own boundaries.

mod settings;
mod update;

pub use settings::*;
pub use update::*;
