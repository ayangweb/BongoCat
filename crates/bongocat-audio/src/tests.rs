//! The audio crate's tests, split by the module they cover.

use super::*;

use std::collections::VecDeque;

const TIMEOUT: Duration = Duration::from_secs(2);

#[derive(Clone, Debug, Eq, PartialEq)]
enum BackendEvent {
    Prepare(Vec<PathBuf>),
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
    fn prepare(&mut self, paths: &[PathBuf]) -> Result<usize, BackendError> {
        self.events
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .push(BackendEvent::Prepare(paths.to_vec()));
        Ok(paths.len())
    }

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

fn prepare(sequence: u64, names: &[&str]) -> MotionAudioCommand {
    MotionAudioCommand::Prepare {
        sequence,
        paths: names.iter().map(PathBuf::from).collect(),
    }
}

mod backend;
mod client;
mod command;
mod service;
mod volume;
mod worker;
