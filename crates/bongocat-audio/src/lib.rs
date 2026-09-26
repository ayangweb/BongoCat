//! Motion audio: the one model sound, played through the system's own device.
//!
//! There is exactly one voice, and it belongs to the model that is on screen. The
//! modules below are the contract the runtime speaks, the handle it holds, the
//! service that owns the worker thread, the device behind it, and what happens
//! when the queue overflows.

#![forbid(unsafe_code)]

use std::{
    io,
    path::{Path, PathBuf},
    sync::{
        Arc, Condvar, Mutex,
        atomic::{AtomicBool, AtomicU64, Ordering},
        mpsc::{self, Receiver, RecvTimeoutError, SyncSender, TrySendError},
    },
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};

use std::collections::HashMap;

// Playback completion is diagnostic state only. A short health check keeps
// that state reasonably fresh without waking an idle worker at 100 Hz.
const WORKER_POLL_INTERVAL: Duration = Duration::from_millis(100);
const PREFERRED_OUTPUT_BUFFER_FRAMES: u32 = 512;

/// Returns whether an observed audio sequence has reached a target in the
/// forward direction, including across the `u64::MAX -> 0` boundary.
///
/// Runtime command sequences and automatic playback use separate domains;
/// production audio commands allocate their own sequence from the audio
/// client. A raw `>=` comparison is still unsafe when that dedicated audio
/// sequence wraps, so all audio waiters use the same half-range ordering rule
/// as the runtime command tracker.
fn sequence_reached(observed: u64, target: u64) -> bool {
    observed.wrapping_sub(target) <= u64::MAX / 2
}

mod backend;
mod client;
mod command;
mod diagnostics;
mod error;
mod service;
mod state;
#[cfg(test)]
mod tests;
mod volume;
mod worker;

pub(crate) use backend::AudioBackend;
pub(crate) use backend::*;
pub(crate) use service::*;
pub(crate) use worker::*;

// The public surface. A `pub(crate)` glob narrows everything it carries, so
// the items the crate root re-exports are named here rather than left to it.
pub use client::MotionAudioClient;
pub use command::{MotionAudioCommand, MotionAudioPublishError};
pub use diagnostics::MotionAudioDiagnostics;
pub use error::{MotionAudioErrorCode, MotionAudioShutdownError, MotionAudioStartError};
pub use service::MotionAudioService;
pub use state::{MotionAudioState, MotionAudioStopReason};
pub use volume::MotionAudioVolume;
