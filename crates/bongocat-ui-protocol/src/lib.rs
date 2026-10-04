#![forbid(unsafe_code)]

//! Typed contracts shared by the settings/update services and their UI views.
//!
//! This crate deliberately has no GPUI, operating-system, config, platform,
//! localization, runtime, model, or update-library dependency. It owns the stable
//! command/snapshot/error shapes
//! and the bounded channel clients; concrete application services and GPUI
//! views adapt those contracts at their own boundaries.
//!
//! `bongocat-input` is the one workspace crate it does depend on, for
//! [`ModifierKey`]: the setting the overlay page records and renders is a
//! physical key, and re-spelling it here would let the value the window sends
//! drift from the usage the runtime matches it against.

mod settings;
mod update;

pub use settings::*;
pub use update::*;
