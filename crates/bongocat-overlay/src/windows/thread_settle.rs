//! The handle and thread budget a model switch has to stay inside.
//!
//! A switch allocates and frees the model's textures, so a switch that leaked
//! one would grow the process without bound. Rather than trust a single sample,
//! the count is taken after a warmup and again after the switch, and a bounded
//! step-up is accepted because a driver pool may still be starting up: a
//! per-switch leak grows with the switch count and lands far beyond that limit,
//! while a stable count is the only evidence that actually means "no leak".

use super::*;

pub(crate) const HANDLE_GROWTH_LIMIT: u32 = 4;

// Process-global D3D11, DXGI, and system thread-pool workers can be created
// after the warmup settle window and then stay for the life of the process, so
// the warmup high-water mark is a snapshot rather than a hard ceiling. Allow a
// bounded step-up above it instead of failing on one late worker: a per-switch
// leak grows with the measured switch count and stays far beyond this limit.
// `settle_process_threads` still requires the accepted count to be stable.
pub(crate) const THREAD_GROWTH_LIMIT: u32 = 2;

// Match the proven overlay lifecycle probe so delayed driver pools are fully
// initialized before the model-switch resource interval begins.
pub(crate) const SWITCH_WARMUP_CYCLES: u64 = 100;

pub(crate) const THREAD_SETTLE_INTERVAL: Duration = Duration::from_millis(10);

pub(crate) const THREAD_SETTLE_SAMPLES: u32 = 25;

pub(crate) const THREAD_SETTLE_TIMEOUT: Duration = Duration::from_secs(2);

pub(crate) fn process_handle_count() -> WindowsResult<u32> {
    let mut count = 0;
    // SAFETY: GetCurrentProcess returns a process pseudo-handle and count is
    // writable for the complete synchronous query.
    unsafe { GetProcessHandleCount(GetCurrentProcess(), &mut count)? };
    Ok(count)
}

pub(crate) fn process_thread_count() -> WindowsResult<u32> {
    // SAFETY: the returned snapshot handle is immediately wrapped and closed
    // by Drop after enumeration; THREADENTRY32 carries the required size.
    let snapshot = ThreadSnapshot(unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPTHREAD, 0)? });
    let process_id = unsafe { GetCurrentProcessId() };
    let mut entry = THREADENTRY32 {
        dwSize: size_of::<THREADENTRY32>() as u32,
        ..Default::default()
    };
    unsafe { Thread32First(snapshot.0, &mut entry)? };
    let mut count = 0_u32;
    loop {
        if entry.th32OwnerProcessID == process_id {
            count = count.saturating_add(1);
        }
        match unsafe { Thread32Next(snapshot.0, &mut entry) } {
            Ok(()) => {}
            Err(error) if error.code() == ERROR_NO_MORE_FILES.to_hresult() => break,
            Err(error) => return Err(error),
        }
    }
    if count == 0 {
        return Err(invariant_error(
            "thread snapshot did not contain the current process",
        ));
    }
    Ok(count)
}

/// Reports whether the settled thread count outgrew the warmup high-water mark
/// beyond the bounded allowance for process-global driver and pool workers.
pub(crate) fn thread_growth_exceeded(warmup_thread_high_water: u32, threads_after: u32) -> bool {
    threads_after > warmup_thread_high_water.saturating_add(THREAD_GROWTH_LIMIT)
}

pub(crate) struct SettledProcessThreads {
    pub(crate) high_water: u32,
    pub(crate) settled_count: u32,
}

pub(crate) fn settle_process_threads(
    mut high_water: u32,
    timeout: Duration,
) -> Result<SettledProcessThreads, OverlayError> {
    let deadline = Instant::now() + timeout;
    let mut last_count = None;
    let mut stable_samples = 0_u32;
    while Instant::now() < deadline {
        pump_window_messages();
        thread::sleep(THREAD_SETTLE_INTERVAL);
        let current = process_thread_count().map_err(windows_error(
            "count process threads while settling resource probe",
        ))?;
        high_water = high_water.max(current);
        if last_count == Some(current) {
            stable_samples = stable_samples.saturating_add(1);
        } else {
            last_count = Some(current);
            stable_samples = 1;
        }
        if stable_samples >= THREAD_SETTLE_SAMPLES {
            return Ok(SettledProcessThreads {
                high_water,
                settled_count: current,
            });
        }
    }
    Err(OverlayError::new(format!(
        "process thread count did not stabilize below high-water mark {high_water}"
    )))
}

pub(crate) struct ThreadSnapshot(HANDLE);

impl Drop for ThreadSnapshot {
    fn drop(&mut self) {
        // SAFETY: this owner contains one successful ToolHelp snapshot handle
        // and Drop runs once after enumeration has stopped.
        let _ = unsafe { CloseHandle(self.0) };
    }
}
