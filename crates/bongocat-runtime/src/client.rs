//! The handle the rest of the product uses to talk to the runtime.
//!
//! A client is a cloneable set of producers plus the wait helpers. It never holds
//! runtime state, so handing one to the platform thread, the settings worker or
//! the renderer costs nothing and cannot observe a torn snapshot.

use crate::transport::{Producer, ShutdownDiagnosticsCounters, sequence_reached};
use crate::*;

#[derive(Clone)]
pub struct RuntimeClient {
    pub(crate) producer: Arc<Producer>,
    pub(crate) snapshot: Arc<SnapshotCell>,
    pub(crate) input_producer: InputProducer,
    pub(crate) cursor_producer: CursorProducer,
    pub(crate) gamepad_axis_producer: GamepadAxisProducer,
    pub(crate) platform_input_diagnostics: PlatformInputDiagnosticsProducer,
    pub(crate) motion_audio: MotionAudioClient,
    pub(crate) shutdown_diagnostics: Arc<ShutdownDiagnosticsCounters>,
}

impl RuntimeClient {
    pub fn send(&self, command: RuntimeCommand) -> Result<u64, SendError> {
        self.producer.send(command)
    }

    pub fn trigger_shortcut(&self, action: ShortcutAction) -> Result<u64, SendError> {
        self.send(action.into_runtime_command())
    }

    pub fn snapshot(&self) -> RuntimeSnapshot {
        let snapshot = self
            .snapshot
            .value
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone();
        self.with_transport_diagnostics(snapshot)
    }

    pub fn input_producer(&self) -> InputProducer {
        self.input_producer.clone()
    }

    pub fn cursor_producer(&self) -> CursorProducer {
        self.cursor_producer.clone()
    }

    pub fn gamepad_axis_producer(&self) -> GamepadAxisProducer {
        self.gamepad_axis_producer.clone()
    }

    pub fn platform_input_diagnostics_producer(&self) -> PlatformInputDiagnosticsProducer {
        self.platform_input_diagnostics.clone()
    }

    /// The two fields a frame source needs to pace itself.
    ///
    /// A frame source asks for these once per frame, and the overlay session asks
    /// for the whole [`RuntimeSnapshot`] right after. Cloning the full snapshot to
    /// read two scalars copied a 1.6 KB struct and reallocated the active model's
    /// name and behavior list on every frame, for a value the overlay was about to
    /// read anyway. These two are the only fields a revision-checked, transport-
    /// free read has to copy, so they are read in place under the same lock.
    pub fn frame_scheduling(&self) -> FrameScheduling {
        let snapshot = self
            .snapshot
            .value
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        FrameScheduling {
            maximum_fps: snapshot.maximum_fps,
            overlay_visible: snapshot.overlay_visible,
        }
    }

    /// How many gamepads the runtime currently considers connected.
    ///
    /// The frame source polls this once per frame to notice a plug or an unplug,
    /// and the whole [`RuntimeSnapshot`] is copied for the overlay session moments
    /// later. One counter is read in place under the same lock for the same
    /// reason [`Self::frame_scheduling`] exists, so a per-frame poll cannot bring
    /// back the per-frame copy the frame source stopped making.
    ///
    /// The runtime owns the connected set, so this is the only place a product
    /// behaviour learns whether a gamepad is attached (ADR-0071).
    pub fn connected_gamepad_count(&self) -> usize {
        self.snapshot
            .value
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .input
            .connected_gamepad_count
    }

    pub fn wait_for_revision(
        &self,
        minimum_revision: u64,
        timeout: Duration,
    ) -> Option<RuntimeSnapshot> {
        let deadline = Instant::now().checked_add(timeout)?;
        let mut snapshot = self
            .snapshot
            .value
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        loop {
            if snapshot.revision >= minimum_revision {
                return Some(self.with_transport_diagnostics(snapshot.clone()));
            }
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                return None;
            }
            let (next, result) = self
                .snapshot
                .changed
                .wait_timeout(snapshot, remaining)
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            snapshot = next;
            if result.timed_out() && snapshot.revision < minimum_revision {
                return None;
            }
        }
    }

    pub fn wait_for_state(
        &self,
        expected: RuntimeState,
        timeout: Duration,
    ) -> Option<RuntimeSnapshot> {
        let deadline = Instant::now().checked_add(timeout)?;
        let mut snapshot = self
            .snapshot
            .value
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        loop {
            if snapshot.state == expected {
                return Some(self.with_transport_diagnostics(snapshot.clone()));
            }
            if snapshot.state == RuntimeState::Stopped {
                return None;
            }
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                return None;
            }
            let (next, result) = self
                .snapshot
                .changed
                .wait_timeout(snapshot, remaining)
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            snapshot = next;
            if result.timed_out() && snapshot.state != expected {
                return None;
            }
        }
    }

    pub fn wait_for_command(
        &self,
        command_sequence: u64,
        timeout: Duration,
    ) -> Option<RuntimeSnapshot> {
        let deadline = Instant::now().checked_add(timeout)?;
        let mut snapshot = self
            .snapshot
            .value
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        loop {
            if snapshot
                .last_command_sequence
                .is_some_and(|sequence| sequence_reached(sequence, command_sequence))
            {
                return Some(self.with_transport_diagnostics(snapshot.clone()));
            }
            if snapshot.state == RuntimeState::Stopped {
                return None;
            }
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                return None;
            }
            let (next, result) = self
                .snapshot
                .changed
                .wait_timeout(snapshot, remaining)
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            snapshot = next;
            if result.timed_out()
                && !snapshot
                    .last_command_sequence
                    .is_some_and(|sequence| sequence_reached(sequence, command_sequence))
            {
                return None;
            }
        }
    }

    pub fn wait_for_model_preparation(
        &self,
        command_sequence: u64,
        timeout: Duration,
    ) -> Option<RuntimeSnapshot> {
        let deadline = Instant::now().checked_add(timeout)?;
        let mut snapshot = self
            .snapshot
            .value
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        loop {
            let prepared = snapshot
                .pending_model
                .as_ref()
                .is_some_and(|pending| pending.token.command_sequence == command_sequence);
            let completed = snapshot
                .last_command_sequence
                .is_some_and(|sequence| sequence_reached(sequence, command_sequence));
            if prepared || completed {
                return Some(self.with_transport_diagnostics(snapshot.clone()));
            }
            if snapshot.state == RuntimeState::Stopped {
                return None;
            }
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                return None;
            }
            let (next, result) = self
                .snapshot
                .changed
                .wait_timeout(snapshot, remaining)
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            snapshot = next;
            if result.timed_out() {
                let prepared = snapshot
                    .pending_model
                    .as_ref()
                    .is_some_and(|pending| pending.token.command_sequence == command_sequence);
                let completed = snapshot
                    .last_command_sequence
                    .is_some_and(|sequence| sequence_reached(sequence, command_sequence));
                if !prepared && !completed {
                    return None;
                }
            }
        }
    }

    pub fn wait_for_input_sequence(
        &self,
        input_sequence: u64,
        timeout: Duration,
    ) -> Option<RuntimeSnapshot> {
        let deadline = Instant::now().checked_add(timeout)?;
        let mut snapshot = self
            .snapshot
            .value
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        loop {
            if snapshot
                .input
                .last_input_sequence
                .is_some_and(|sequence| sequence_reached(sequence, input_sequence))
            {
                return Some(self.with_transport_diagnostics(snapshot.clone()));
            }
            if snapshot.state == RuntimeState::Stopped {
                return None;
            }
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                return None;
            }
            let (next, result) = self
                .snapshot
                .changed
                .wait_timeout(snapshot, remaining)
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            snapshot = next;
            if result.timed_out()
                && !snapshot
                    .input
                    .last_input_sequence
                    .is_some_and(|sequence| sequence_reached(sequence, input_sequence))
            {
                return None;
            }
        }
    }

    pub fn wait_for_cursor_samples(
        &self,
        minimum_consumed: u64,
        timeout: Duration,
    ) -> Option<RuntimeSnapshot> {
        let deadline = Instant::now().checked_add(timeout)?;
        let mut snapshot = self
            .snapshot
            .value
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        loop {
            if self.cursor_producer.diagnostics().consumed >= minimum_consumed {
                return Some(self.with_transport_diagnostics(snapshot.clone()));
            }
            if snapshot.state == RuntimeState::Stopped {
                return None;
            }
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                return None;
            }
            let (next, result) = self
                .snapshot
                .changed
                .wait_timeout(snapshot, remaining)
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            snapshot = next;
            if result.timed_out() && self.cursor_producer.diagnostics().consumed < minimum_consumed
            {
                return None;
            }
        }
    }

    pub(crate) fn with_transport_diagnostics(
        &self,
        mut snapshot: RuntimeSnapshot,
    ) -> RuntimeSnapshot {
        snapshot.input.transport = self.input_producer.diagnostics();
        snapshot.command_transport = self.producer.command_transport.snapshot();
        snapshot.cursor.transport = self.cursor_producer.diagnostics();
        snapshot.gamepad_axis_transport = self.gamepad_axis_producer.diagnostics();
        snapshot.platform_input = self.platform_input_diagnostics.diagnostics();
        snapshot.motion_audio = self.motion_audio.diagnostics();
        snapshot.shutdown = self.shutdown_diagnostics.snapshot();
        snapshot
    }
}
