#![allow(unsafe_code)]

use crate::sys;
use bongocat_log::{LogLevel, LogRecord, LogSettingsController, LogStream, TextLogWriter};
use bongocat_storage::set_private_directory;
use std::{
    ffi::CStr,
    fs,
    os::raw::c_char,
    panic::{AssertUnwindSafe, catch_unwind},
    path::Path,
    sync::{
        Arc, Mutex, OnceLock,
        atomic::{AtomicBool, AtomicU64, Ordering},
        mpsc::{Receiver, SyncSender, TryRecvError, TrySendError, sync_channel},
    },
    thread::{self, JoinHandle},
    time::{Duration, SystemTime},
};

mod handle;
mod sanitize;
mod sink;

#[cfg(test)]
mod tests;

pub use handle::CoreLogError;
use handle::*;
pub use handle::{CoreLogHandle, CoreLogReporter, CoreLogStats};
use sanitize::*;
use sink::*;
