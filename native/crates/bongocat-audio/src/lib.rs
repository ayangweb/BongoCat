#![forbid(unsafe_code)]

use std::{
    fmt, io,
    path::{Path, PathBuf},
    sync::{
        Arc, Condvar, Mutex,
        atomic::{AtomicBool, Ordering},
        mpsc::{self, Receiver, RecvTimeoutError, SyncSender, TrySendError},
    },
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};

const WORKER_POLL_INTERVAL: Duration = Duration::from_millis(10);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MotionAudioState {
    Starting,
    Ready,
    Degraded,
    Stopping,
    Stopped,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MotionAudioErrorCode {
    ResourceIo,
    DecodeFailed,
    OutputUnavailable,
    WorkerUnavailable,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MotionAudioStopReason {
    MotionStopped,
    MotionReplaced,
    ModelSwitched,
    Disabled,
    Shutdown,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MotionAudioVolume(f32);

impl MotionAudioVolume {
    pub const FULL: Self = Self(1.0);

    pub fn new(value: f32) -> Option<Self> {
        value
            .is_finite()
            .then_some(value)
            .filter(|value| (0.0..=1.0).contains(value))
            .map(Self)
    }

    pub const fn get(self) -> f32 {
        self.0
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum MotionAudioCommand {
    Play {
        sequence: u64,
        path: PathBuf,
        volume: MotionAudioVolume,
    },
    Stop {
        sequence: u64,
        reason: MotionAudioStopReason,
    },
}

impl MotionAudioCommand {
    pub const fn sequence(&self) -> u64 {
        match self {
            Self::Play { sequence, .. } | Self::Stop { sequence, .. } => *sequence,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MotionAudioDiagnostics {
    pub state: MotionAudioState,
    pub enqueued_commands: u64,
    pub processed_commands: u64,
    pub discarded_commands: u64,
    pub play_requests: u64,
    pub playback_starts: u64,
    pub stop_requests: u64,
    pub voices_stopped: u64,
    pub queue_overflows: u64,
    pub rejected_after_shutdown: u64,
    pub resource_failures: u64,
    pub decode_failures: u64,
    pub output_failures: u64,
    pub current_voice_sequence: Option<u64>,
    pub last_processed_sequence: Option<u64>,
    pub last_error: Option<MotionAudioErrorCode>,
}

impl MotionAudioDiagnostics {
    fn starting() -> Self {
        Self {
            state: MotionAudioState::Starting,
            enqueued_commands: 0,
            processed_commands: 0,
            discarded_commands: 0,
            play_requests: 0,
            playback_starts: 0,
            stop_requests: 0,
            voices_stopped: 0,
            queue_overflows: 0,
            rejected_after_shutdown: 0,
            resource_failures: 0,
            decode_failures: 0,
            output_failures: 0,
            current_voice_sequence: None,
            last_processed_sequence: None,
            last_error: None,
        }
    }

    fn unavailable() -> Self {
        let mut diagnostics = Self::starting();
        diagnostics.state = MotionAudioState::Stopped;
        diagnostics.last_error = Some(MotionAudioErrorCode::WorkerUnavailable);
        diagnostics
    }
}

struct SharedState {
    diagnostics: Mutex<MotionAudioDiagnostics>,
    changed: Condvar,
    shutdown_requested: AtomicBool,
    overflow_recovery_requested: AtomicBool,
}

impl SharedState {
    fn publish(&self, update: impl FnOnce(&mut MotionAudioDiagnostics)) {
        let mut diagnostics = self
            .diagnostics
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        update(&mut diagnostics);
        self.changed.notify_all();
    }

    fn snapshot(&self) -> MotionAudioDiagnostics {
        self.diagnostics
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone()
    }
}

#[derive(Clone)]
pub struct MotionAudioClient {
    sender: SyncSender<MotionAudioCommand>,
    shared: Arc<SharedState>,
}

#[derive(Debug, PartialEq)]
pub enum MotionAudioPublishError {
    QueueFull(MotionAudioCommand),
    ServiceStopped(MotionAudioCommand),
}

impl fmt::Display for MotionAudioPublishError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::QueueFull(_) => formatter.write_str("motion audio command queue is full"),
            Self::ServiceStopped(_) => formatter.write_str("motion audio service is stopped"),
        }
    }
}

impl std::error::Error for MotionAudioPublishError {}

impl MotionAudioClient {
    pub fn unavailable() -> Self {
        let (sender, receiver) = mpsc::sync_channel(1);
        drop(receiver);
        Self {
            sender,
            shared: Arc::new(SharedState {
                diagnostics: Mutex::new(MotionAudioDiagnostics::unavailable()),
                changed: Condvar::new(),
                shutdown_requested: AtomicBool::new(true),
                overflow_recovery_requested: AtomicBool::new(false),
            }),
        }
    }

    pub fn try_publish(&self, command: MotionAudioCommand) -> Result<(), MotionAudioPublishError> {
        if self.shared.shutdown_requested.load(Ordering::Acquire) {
            self.shared.publish(|diagnostics| {
                diagnostics.rejected_after_shutdown =
                    diagnostics.rejected_after_shutdown.saturating_add(1);
            });
            return Err(MotionAudioPublishError::ServiceStopped(command));
        }
        match self.sender.try_send(command) {
            Ok(()) => {
                self.shared.publish(|diagnostics| {
                    diagnostics.enqueued_commands = diagnostics.enqueued_commands.saturating_add(1);
                });
                Ok(())
            }
            Err(TrySendError::Full(command)) => {
                self.shared
                    .overflow_recovery_requested
                    .store(true, Ordering::Release);
                self.shared.publish(|diagnostics| {
                    diagnostics.queue_overflows = diagnostics.queue_overflows.saturating_add(1);
                });
                Err(MotionAudioPublishError::QueueFull(command))
            }
            Err(TrySendError::Disconnected(command)) => {
                self.shared.publish(|diagnostics| {
                    diagnostics.rejected_after_shutdown =
                        diagnostics.rejected_after_shutdown.saturating_add(1);
                });
                Err(MotionAudioPublishError::ServiceStopped(command))
            }
        }
    }

    pub fn diagnostics(&self) -> MotionAudioDiagnostics {
        self.shared.snapshot()
    }

    pub fn wait_for_sequence(
        &self,
        sequence: u64,
        timeout: Duration,
    ) -> Option<MotionAudioDiagnostics> {
        let deadline = Instant::now().checked_add(timeout)?;
        let mut diagnostics = self
            .shared
            .diagnostics
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        loop {
            if diagnostics
                .last_processed_sequence
                .is_some_and(|processed| processed >= sequence)
            {
                return Some(diagnostics.clone());
            }
            if diagnostics.state == MotionAudioState::Stopped {
                return None;
            }
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                return None;
            }
            let (next, result) = self
                .shared
                .changed
                .wait_timeout(diagnostics, remaining)
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            diagnostics = next;
            if result.timed_out()
                && !diagnostics
                    .last_processed_sequence
                    .is_some_and(|processed| processed >= sequence)
            {
                return None;
            }
        }
    }
}

#[derive(Debug)]
pub struct MotionAudioStartError(io::Error);

impl fmt::Display for MotionAudioStartError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "cannot start motion audio worker: {}", self.0)
    }
}

impl std::error::Error for MotionAudioStartError {}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MotionAudioShutdownError {
    TimedOut,
    WorkerPanicked,
}

impl fmt::Display for MotionAudioShutdownError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::TimedOut => formatter.write_str("motion audio shutdown timed out"),
            Self::WorkerPanicked => formatter.write_str("motion audio worker panicked"),
        }
    }
}

impl std::error::Error for MotionAudioShutdownError {}

pub struct MotionAudioService {
    client: MotionAudioClient,
    worker: Option<JoinHandle<()>>,
}

impl MotionAudioService {
    pub fn start(command_capacity: usize) -> Result<Self, MotionAudioStartError> {
        Self::start_with_backend(command_capacity, Box::<SystemAudioBackend>::default())
    }

    fn start_with_backend(
        command_capacity: usize,
        backend: Box<dyn AudioBackend>,
    ) -> Result<Self, MotionAudioStartError> {
        assert!(
            command_capacity > 0,
            "audio command capacity must be non-zero"
        );
        let (sender, receiver) = mpsc::sync_channel(command_capacity);
        let shared = Arc::new(SharedState {
            diagnostics: Mutex::new(MotionAudioDiagnostics::starting()),
            changed: Condvar::new(),
            shutdown_requested: AtomicBool::new(false),
            overflow_recovery_requested: AtomicBool::new(false),
        });
        let client = MotionAudioClient {
            sender,
            shared: Arc::clone(&shared),
        };
        let worker = thread::Builder::new()
            .name("bongocat-motion-audio".into())
            .spawn(move || run_worker(receiver, shared, backend))
            .map_err(MotionAudioStartError)?;
        Ok(Self {
            client,
            worker: Some(worker),
        })
    }

    pub fn client(&self) -> MotionAudioClient {
        self.client.clone()
    }

    pub fn shutdown(
        mut self,
        timeout: Duration,
    ) -> Result<MotionAudioDiagnostics, MotionAudioShutdownError> {
        self.request_shutdown();
        let stopped = self.wait_until_stopped(timeout)?;
        self.join_worker()?;
        Ok(stopped)
    }

    fn request_shutdown(&self) {
        self.client
            .shared
            .shutdown_requested
            .store(true, Ordering::Release);
        self.client.shared.changed.notify_all();
    }

    fn wait_until_stopped(
        &self,
        timeout: Duration,
    ) -> Result<MotionAudioDiagnostics, MotionAudioShutdownError> {
        let deadline = Instant::now()
            .checked_add(timeout)
            .ok_or(MotionAudioShutdownError::TimedOut)?;
        let mut diagnostics = self
            .client
            .shared
            .diagnostics
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        while diagnostics.state != MotionAudioState::Stopped {
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                return Err(MotionAudioShutdownError::TimedOut);
            }
            let (next, result) = self
                .client
                .shared
                .changed
                .wait_timeout(diagnostics, remaining)
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            diagnostics = next;
            if result.timed_out() && diagnostics.state != MotionAudioState::Stopped {
                return Err(MotionAudioShutdownError::TimedOut);
            }
        }
        Ok(diagnostics.clone())
    }

    fn join_worker(&mut self) -> Result<(), MotionAudioShutdownError> {
        if let Some(worker) = self.worker.take() {
            worker
                .join()
                .map_err(|_| MotionAudioShutdownError::WorkerPanicked)?;
        }
        Ok(())
    }
}

impl Drop for MotionAudioService {
    fn drop(&mut self) {
        if self.worker.is_none() {
            return;
        }
        self.request_shutdown();
        let _ = self.join_worker();
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum BackendError {
    #[cfg(any(target_os = "macos", target_os = "windows", test))]
    ResourceIo,
    #[cfg(any(target_os = "macos", target_os = "windows", test))]
    DecodeFailed,
    OutputUnavailable,
}

trait AudioBackend: Send {
    fn play(&mut self, path: &Path, volume: MotionAudioVolume) -> Result<(), BackendError>;
    fn stop(&mut self) -> bool;
    fn is_playing(&self) -> bool;
}

fn run_worker(
    receiver: Receiver<MotionAudioCommand>,
    shared: Arc<SharedState>,
    mut backend: Box<dyn AudioBackend>,
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
                if !backend.is_playing() {
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
}

fn recover_after_overflow(
    receiver: &Receiver<MotionAudioCommand>,
    shared: &SharedState,
    backend: &mut dyn AudioBackend,
) {
    if !shared
        .overflow_recovery_requested
        .swap(false, Ordering::AcqRel)
    {
        return;
    }
    let mut discarded = 0u64;
    while receiver.try_recv().is_ok() {
        discarded = discarded.saturating_add(1);
    }
    let stopped = backend.stop();
    shared.publish(|diagnostics| {
        diagnostics.discarded_commands = diagnostics.discarded_commands.saturating_add(discarded);
        if stopped {
            diagnostics.voices_stopped = diagnostics.voices_stopped.saturating_add(1);
        }
        diagnostics.current_voice_sequence = None;
    });
}

fn process_command(
    command: MotionAudioCommand,
    shared: &SharedState,
    backend: &mut dyn AudioBackend,
) {
    let sequence = command.sequence();
    match command {
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
                    Err(error) => {
                        let code = match error {
                            #[cfg(any(target_os = "macos", target_os = "windows", test))]
                            BackendError::ResourceIo => {
                                diagnostics.resource_failures =
                                    diagnostics.resource_failures.saturating_add(1);
                                MotionAudioErrorCode::ResourceIo
                            }
                            #[cfg(any(target_os = "macos", target_os = "windows", test))]
                            BackendError::DecodeFailed => {
                                diagnostics.decode_failures =
                                    diagnostics.decode_failures.saturating_add(1);
                                MotionAudioErrorCode::DecodeFailed
                            }
                            BackendError::OutputUnavailable => {
                                diagnostics.output_failures =
                                    diagnostics.output_failures.saturating_add(1);
                                MotionAudioErrorCode::OutputUnavailable
                            }
                        };
                        diagnostics.current_voice_sequence = None;
                        diagnostics.last_error = Some(code);
                        diagnostics.state = MotionAudioState::Degraded;
                    }
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

#[cfg(any(target_os = "macos", target_os = "windows"))]
#[derive(Default)]
struct SystemAudioBackend {
    output: Option<rodio::MixerDeviceSink>,
    player: Option<rodio::Player>,
}

#[cfg(any(target_os = "macos", target_os = "windows"))]
impl AudioBackend for SystemAudioBackend {
    fn play(&mut self, path: &Path, volume: MotionAudioVolume) -> Result<(), BackendError> {
        let file = std::fs::File::open(path).map_err(|_| BackendError::ResourceIo)?;
        let decoder = rodio::Decoder::try_from(file).map_err(|_| BackendError::DecodeFailed)?;
        if self.output.is_none() {
            let mut output = rodio::DeviceSinkBuilder::open_default_sink()
                .map_err(|_| BackendError::OutputUnavailable)?;
            output.log_on_drop(false);
            self.output = Some(output);
        }
        let output = self.output.as_ref().expect("audio output was initialized");
        let player = rodio::Player::connect_new(output.mixer());
        player.set_volume(volume.get());
        player.append(decoder);
        self.player = Some(player);
        Ok(())
    }

    fn stop(&mut self) -> bool {
        self.player.take().is_some()
    }

    fn is_playing(&self) -> bool {
        self.player.as_ref().is_some_and(|player| !player.empty())
    }
}

#[cfg(not(any(target_os = "macos", target_os = "windows")))]
#[derive(Default)]
struct SystemAudioBackend;

#[cfg(not(any(target_os = "macos", target_os = "windows")))]
impl AudioBackend for SystemAudioBackend {
    fn play(&mut self, _path: &Path, _volume: MotionAudioVolume) -> Result<(), BackendError> {
        Err(BackendError::OutputUnavailable)
    }

    fn stop(&mut self) -> bool {
        false
    }

    fn is_playing(&self) -> bool {
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::VecDeque;

    const TIMEOUT: Duration = Duration::from_secs(2);

    #[derive(Clone, Debug, Eq, PartialEq)]
    enum BackendEvent {
        Play(PathBuf),
        Stop,
    }

    struct RecordingBackend {
        events: Arc<Mutex<Vec<BackendEvent>>>,
        failures: VecDeque<BackendError>,
        playing: bool,
    }

    #[derive(Default)]
    struct BlockingState {
        entered: bool,
        released: bool,
        playing: bool,
    }

    struct BlockingBackend {
        state: Arc<(Mutex<BlockingState>, Condvar)>,
    }

    impl AudioBackend for BlockingBackend {
        fn play(&mut self, _path: &Path, _volume: MotionAudioVolume) -> Result<(), BackendError> {
            let (lock, changed) = &*self.state;
            let mut state = lock
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            state.entered = true;
            changed.notify_all();
            while !state.released {
                state = changed
                    .wait(state)
                    .unwrap_or_else(std::sync::PoisonError::into_inner);
            }
            state.playing = true;
            Ok(())
        }

        fn stop(&mut self) -> bool {
            let mut state = self
                .state
                .0
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            let was_playing = state.playing;
            state.playing = false;
            was_playing
        }

        fn is_playing(&self) -> bool {
            self.state
                .0
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .playing
        }
    }

    impl AudioBackend for RecordingBackend {
        fn play(&mut self, path: &Path, _volume: MotionAudioVolume) -> Result<(), BackendError> {
            if let Some(error) = self.failures.pop_front() {
                return Err(error);
            }
            self.events
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .push(BackendEvent::Play(path.to_owned()));
            self.playing = true;
            Ok(())
        }

        fn stop(&mut self) -> bool {
            if !self.playing {
                return false;
            }
            self.events
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .push(BackendEvent::Stop);
            self.playing = false;
            true
        }

        fn is_playing(&self) -> bool {
            self.playing
        }
    }

    fn play(sequence: u64, name: &str) -> MotionAudioCommand {
        MotionAudioCommand::Play {
            sequence,
            path: PathBuf::from(name),
            volume: MotionAudioVolume::FULL,
        }
    }

    #[test]
    fn volume_rejects_non_finite_and_out_of_range_values() {
        assert_eq!(MotionAudioVolume::new(0.0), Some(MotionAudioVolume(0.0)));
        assert_eq!(MotionAudioVolume::new(1.0), Some(MotionAudioVolume::FULL));
        assert_eq!(MotionAudioVolume::new(-0.1), None);
        assert_eq!(MotionAudioVolume::new(1.1), None);
        assert_eq!(MotionAudioVolume::new(f32::NAN), None);
    }

    #[test]
    fn accepted_commands_replace_one_voice_in_order() {
        let events = Arc::new(Mutex::new(Vec::new()));
        let service = MotionAudioService::start_with_backend(
            4,
            Box::new(RecordingBackend {
                events: Arc::clone(&events),
                failures: VecDeque::new(),
                playing: false,
            }),
        )
        .expect("audio service");
        let client = service.client();
        client
            .try_publish(play(1, "first.flac"))
            .expect("first play");
        client
            .try_publish(play(2, "second.flac"))
            .expect("replacement play");
        client
            .try_publish(MotionAudioCommand::Stop {
                sequence: 3,
                reason: MotionAudioStopReason::MotionStopped,
            })
            .expect("stop");
        let diagnostics = client
            .wait_for_sequence(3, TIMEOUT)
            .expect("commands processed");
        assert_eq!(diagnostics.playback_starts, 2);
        assert_eq!(diagnostics.voices_stopped, 2);
        assert_eq!(diagnostics.current_voice_sequence, None);
        assert_eq!(
            *events
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner),
            vec![
                BackendEvent::Play(PathBuf::from("first.flac")),
                BackendEvent::Stop,
                BackendEvent::Play(PathBuf::from("second.flac")),
                BackendEvent::Stop,
            ]
        );
        let stopped = service.shutdown(TIMEOUT).expect("clean shutdown");
        assert_eq!(stopped.state, MotionAudioState::Stopped);
    }

    #[test]
    fn backend_failure_is_observable_and_later_play_recovers() {
        let events = Arc::new(Mutex::new(Vec::new()));
        let service = MotionAudioService::start_with_backend(
            2,
            Box::new(RecordingBackend {
                events,
                failures: VecDeque::from([BackendError::ResourceIo, BackendError::DecodeFailed]),
                playing: false,
            }),
        )
        .expect("audio service");
        let client = service.client();
        client
            .try_publish(play(1, "broken.flac"))
            .expect("failed play queued");
        let failed = client
            .wait_for_sequence(1, TIMEOUT)
            .expect("failed play processed");
        assert_eq!(failed.state, MotionAudioState::Degraded);
        assert_eq!(failed.resource_failures, 1);
        assert_eq!(failed.last_error, Some(MotionAudioErrorCode::ResourceIo));

        client
            .try_publish(play(2, "broken.flac"))
            .expect("decode failure queued");
        let decode_failed = client
            .wait_for_sequence(2, TIMEOUT)
            .expect("decode failure processed");
        assert_eq!(decode_failed.decode_failures, 1);
        assert_eq!(
            decode_failed.last_error,
            Some(MotionAudioErrorCode::DecodeFailed)
        );

        client
            .try_publish(play(3, "valid.flac"))
            .expect("recovery play queued");
        let recovered = client
            .wait_for_sequence(3, TIMEOUT)
            .expect("recovery play processed");
        assert_eq!(recovered.state, MotionAudioState::Ready);
        assert_eq!(recovered.playback_starts, 1);
        assert_eq!(recovered.last_error, None);
        service.shutdown(TIMEOUT).expect("clean shutdown");
    }

    #[test]
    fn queue_overflow_discards_untrusted_backlog_and_stops_the_voice() {
        let state = Arc::new((Mutex::new(BlockingState::default()), Condvar::new()));
        let service = MotionAudioService::start_with_backend(
            1,
            Box::new(BlockingBackend {
                state: Arc::clone(&state),
            }),
        )
        .expect("audio service");
        let client = service.client();
        client.try_publish(play(1, "one.flac")).expect("first play");
        {
            let (lock, changed) = &*state;
            let entered = lock
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            let (mut entered, result) = changed
                .wait_timeout_while(entered, TIMEOUT, |state| !state.entered)
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            assert!(!result.timed_out(), "backend did not start processing");
            client
                .try_publish(play(2, "two.flac"))
                .expect("queued replacement");
            assert_eq!(
                client.try_publish(play(3, "overflow.flac")),
                Err(MotionAudioPublishError::QueueFull(play(3, "overflow.flac")))
            );
            entered.released = true;
            changed.notify_all();
        }

        let deadline = Instant::now() + TIMEOUT;
        let recovered = loop {
            let diagnostics = client.diagnostics();
            if diagnostics.discarded_commands == 1 && diagnostics.current_voice_sequence.is_none() {
                break diagnostics;
            }
            assert!(Instant::now() < deadline, "overflow recovery timed out");
            thread::yield_now();
        };
        assert_eq!(recovered.queue_overflows, 1);
        assert_eq!(recovered.enqueued_commands, 2);
        assert_eq!(recovered.processed_commands, 1);
        assert_eq!(recovered.voices_stopped, 1);
        service.shutdown(TIMEOUT).expect("clean shutdown");
    }

    #[test]
    fn shutdown_stops_voice_releases_worker_and_rejects_late_commands() {
        let events = Arc::new(Mutex::new(Vec::new()));
        let service = MotionAudioService::start_with_backend(
            1,
            Box::new(RecordingBackend {
                events: Arc::clone(&events),
                failures: VecDeque::new(),
                playing: false,
            }),
        )
        .expect("audio service");
        let client = service.client();
        client
            .try_publish(play(9, "voice.flac"))
            .expect("play queued");
        client
            .wait_for_sequence(9, TIMEOUT)
            .expect("play processed");
        let stopped = service.shutdown(TIMEOUT).expect("clean shutdown");
        assert_eq!(stopped.state, MotionAudioState::Stopped);
        assert_eq!(stopped.current_voice_sequence, None);
        assert_eq!(stopped.voices_stopped, 1);
        assert_eq!(
            client.try_publish(play(10, "late.flac")),
            Err(MotionAudioPublishError::ServiceStopped(play(
                10,
                "late.flac"
            )))
        );
        assert_eq!(
            events
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .last(),
            Some(&BackendEvent::Stop)
        );
    }

    #[cfg(any(target_os = "macos", target_os = "windows"))]
    #[test]
    fn product_backend_classifies_missing_and_invalid_audio_without_opening_a_device() {
        let service = MotionAudioService::start(2).expect("product audio service");
        let client = service.client();
        client
            .try_publish(play(1, "does-not-exist.flac"))
            .expect("missing resource request");
        let missing = client
            .wait_for_sequence(1, TIMEOUT)
            .expect("missing resource processed");
        assert_eq!(missing.resource_failures, 1);
        assert_eq!(missing.last_error, Some(MotionAudioErrorCode::ResourceIo));

        let invalid = Path::new(env!("CARGO_MANIFEST_DIR")).join("Cargo.toml");
        client
            .try_publish(MotionAudioCommand::Play {
                sequence: 2,
                path: invalid,
                volume: MotionAudioVolume::FULL,
            })
            .expect("invalid resource request");
        let invalid = client
            .wait_for_sequence(2, TIMEOUT)
            .expect("invalid resource processed");
        assert_eq!(invalid.decode_failures, 1);
        assert_eq!(invalid.last_error, Some(MotionAudioErrorCode::DecodeFailed));
        service.shutdown(TIMEOUT).expect("clean shutdown");
    }

    #[cfg(any(target_os = "macos", target_os = "windows"))]
    #[test]
    fn bundled_flac_is_accepted_by_the_product_decoder() {
        use rodio::Source;

        let path = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../resources/models/standard/live2d_motion1.flac");
        let file = std::fs::File::open(path).expect("bundled FLAC");
        let mut decoder = rodio::Decoder::try_from(file).expect("decode bundled FLAC");
        assert_eq!(decoder.sample_rate().get(), 48_000);
        assert_eq!(decoder.channels().get(), 2);
        assert!(decoder.next().is_some());
    }
}
