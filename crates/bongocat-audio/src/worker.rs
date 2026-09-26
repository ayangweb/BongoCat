//! The worker loop, and what it does when the queue overflows.
//!
//! The queue is bounded, and overflow is handled by discarding the backlog rather
//! than by blocking the runtime: a sound that is late is worse than a sound that
//! is skipped. What survives an overflow is the model's prepared resources, so the
//! next motion still has something to play.

use super::*;

pub(crate) fn run_worker(
    receiver: Receiver<MotionAudioCommand>,
    shared: Arc<SharedState>,
    mut backend: Box<dyn AudioBackend>,
    panic_after_stopped: bool,
) {
    shared.publish(|diagnostics| diagnostics.state = MotionAudioState::Ready);
    loop {
        if shared.shutdown_requested.load(Ordering::Acquire) {
            break;
        }
        recover_after_overflow(&receiver, &shared, backend.as_mut());
        match receiver.recv_timeout(WORKER_POLL_INTERVAL) {
            Ok(command) => process_command(command, &shared, backend.as_mut()),
            Err(RecvTimeoutError::Timeout) => {
                if !backend.is_playing()
                    && shared
                        .diagnostics
                        .lock()
                        .unwrap_or_else(std::sync::PoisonError::into_inner)
                        .current_voice_sequence
                        .is_some()
                {
                    shared.publish(|diagnostics| diagnostics.current_voice_sequence = None);
                }
            }
            Err(RecvTimeoutError::Disconnected) => break,
        }
    }
    shared.publish(|diagnostics| diagnostics.state = MotionAudioState::Stopping);
    let mut discarded = 0u64;
    while receiver.try_recv().is_ok() {
        discarded = discarded.saturating_add(1);
    }
    let stopped = backend.stop();
    drop(backend);
    shared.publish(|diagnostics| {
        diagnostics.discarded_commands = diagnostics.discarded_commands.saturating_add(discarded);
        if stopped {
            diagnostics.voices_stopped = diagnostics.voices_stopped.saturating_add(1);
        }
        diagnostics.current_voice_sequence = None;
        diagnostics.state = MotionAudioState::Stopped;
    });
    if panic_after_stopped {
        panic!("motion audio worker panic injection");
    }
}

pub(crate) fn recover_after_overflow(
    receiver: &Receiver<MotionAudioCommand>,
    shared: &SharedState,
    backend: &mut dyn AudioBackend,
) {
    let mut retained = Vec::new();
    let mut discarded = 0u64;
    {
        let _publish_guard = shared
            .publish_lock
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if !shared
            .overflow_recovery_requested
            .swap(false, Ordering::AcqRel)
        {
            return;
        }
        while let Ok(command) = receiver.try_recv() {
            if matches!(
                &command,
                MotionAudioCommand::Prepare { .. } | MotionAudioCommand::ActivatePrepared { .. }
            ) {
                retained.push(command);
            } else {
                discarded = discarded.saturating_add(1);
            }
        }
    }
    let stopped = backend.stop();
    shared.publish(|diagnostics| {
        diagnostics.discarded_commands = diagnostics.discarded_commands.saturating_add(discarded);
        if stopped {
            diagnostics.voices_stopped = diagnostics.voices_stopped.saturating_add(1);
        }
        diagnostics.current_voice_sequence = None;
    });
    // Model preparation is a liveness boundary: a runtime activation waits for
    // its Prepare sequence before committing the model. Transient playback
    // commands may be discarded, but lifecycle commands are replayed after
    // the voice reset so that accepted model audio cannot leave activation
    // pending forever.
    for command in retained {
        process_command(command, shared, backend);
    }
}

pub(crate) fn process_command(
    command: MotionAudioCommand,
    shared: &SharedState,
    backend: &mut dyn AudioBackend,
) {
    let sequence = command.sequence();
    match command {
        MotionAudioCommand::Prepare { paths, .. } => {
            let result = backend.prepare(&paths);
            shared.publish(|diagnostics| {
                diagnostics.processed_commands = diagnostics.processed_commands.saturating_add(1);
                diagnostics.prepare_requests = diagnostics.prepare_requests.saturating_add(1);
                diagnostics.last_processed_sequence = Some(sequence);
                match result {
                    Ok(prepared) => {
                        diagnostics.prepared_resources = diagnostics
                            .prepared_resources
                            .saturating_add(prepared as u64);
                        diagnostics.last_error = None;
                        diagnostics.state = MotionAudioState::Ready;
                    }
                    Err(error) => record_backend_error(diagnostics, error),
                }
            });
        }
        MotionAudioCommand::ActivatePrepared { paths, .. } => {
            let result = backend.activate_prepared(&paths);
            shared.publish(|diagnostics| {
                diagnostics.processed_commands = diagnostics.processed_commands.saturating_add(1);
                diagnostics.last_processed_sequence = Some(sequence);
                match result {
                    Ok(()) => {
                        diagnostics.last_error = None;
                        diagnostics.state = MotionAudioState::Ready;
                    }
                    Err(error) => record_backend_error(diagnostics, error),
                }
            });
        }
        MotionAudioCommand::Play { path, volume, .. } => {
            let stopped = backend.stop();
            let result = backend.play(&path, volume);
            shared.publish(|diagnostics| {
                diagnostics.processed_commands = diagnostics.processed_commands.saturating_add(1);
                diagnostics.play_requests = diagnostics.play_requests.saturating_add(1);
                if stopped {
                    diagnostics.voices_stopped = diagnostics.voices_stopped.saturating_add(1);
                }
                diagnostics.last_processed_sequence = Some(sequence);
                match result {
                    Ok(()) => {
                        diagnostics.playback_starts = diagnostics.playback_starts.saturating_add(1);
                        diagnostics.current_voice_sequence = Some(sequence);
                        diagnostics.last_error = None;
                        diagnostics.state = MotionAudioState::Ready;
                    }
                    Err(error) => record_backend_error(diagnostics, error),
                }
            });
        }
        MotionAudioCommand::Stop { .. } => {
            let stopped = backend.stop();
            shared.publish(|diagnostics| {
                diagnostics.processed_commands = diagnostics.processed_commands.saturating_add(1);
                diagnostics.stop_requests = diagnostics.stop_requests.saturating_add(1);
                if stopped {
                    diagnostics.voices_stopped = diagnostics.voices_stopped.saturating_add(1);
                }
                diagnostics.current_voice_sequence = None;
                diagnostics.last_processed_sequence = Some(sequence);
            });
        }
    }
}

pub(crate) fn record_backend_error(diagnostics: &mut MotionAudioDiagnostics, error: BackendError) {
    let code = match error {
        BackendError::ResourceIo => {
            diagnostics.resource_failures = diagnostics.resource_failures.saturating_add(1);
            MotionAudioErrorCode::ResourceIo
        }
        BackendError::DecodeFailed => {
            diagnostics.decode_failures = diagnostics.decode_failures.saturating_add(1);
            MotionAudioErrorCode::DecodeFailed
        }
        BackendError::OutputUnavailable => {
            diagnostics.output_failures = diagnostics.output_failures.saturating_add(1);
            MotionAudioErrorCode::OutputUnavailable
        }
    };
    diagnostics.current_voice_sequence = None;
    diagnostics.last_error = Some(code);
    diagnostics.state = MotionAudioState::Degraded;
}
