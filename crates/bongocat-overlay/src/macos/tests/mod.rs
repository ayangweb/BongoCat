//! The macOS adapter's tests, split by the module they cover.
//!
//! The prelude lives here rather than in each module: one test reaches the
//! adapter's root, the module it covers and a neighbour it needs, and
//! repeating that list four times would say less than writing it once.

use super::*;

mod geometry;
mod renderer;
mod session;
mod textures;
