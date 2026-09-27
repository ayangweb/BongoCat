//! The overlay's tests, split by the module they cover.

use super::*;

use std::sync::atomic::{AtomicUsize, Ordering};

#[cfg(any(target_os = "macos", test))]
use super::timing::MAX_FRAME_TIMING_SAMPLES;

mod blend;
mod bounds;
mod dimensions;
mod frame;
mod preview;
mod product_session;
mod report;
mod timing;
