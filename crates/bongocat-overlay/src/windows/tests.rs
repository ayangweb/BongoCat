//! The Windows adapter's tests, split by the module they cover.
//!
//! The prelude lives here rather than in each module: one test reaches the
//! adapter's root, the module it covers and a neighbour it needs, and
//! repeating that list eight times would say less than writing it once.

use super::*;

mod geometry;
mod pipelines;
mod renderer;
mod session;
mod textures;
mod thread_settle;
mod window;
mod window_proc;
