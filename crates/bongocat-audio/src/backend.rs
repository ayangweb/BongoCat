//! The audio device, and the one shape every device has to be.
//!
//! The trait is one method wide on purpose: this crate has one backend, and a
//! second one would be a second implementation of the same contract rather than a
//! new capability. A missing file is classified before the device is opened, so a
//! model with no sound is reported as a missing file and not as an audio failure.

use super::*;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum BackendError {
    ResourceIo,
    DecodeFailed,
    OutputUnavailable,
}

pub(crate) trait AudioBackend: Send {
    fn prepare(&mut self, _paths: &[PathBuf]) -> Result<usize, BackendError> {
        Ok(0)
    }

    fn activate_prepared(&mut self, _paths: &[PathBuf]) -> Result<(), BackendError> {
        Ok(())
    }

    fn play(&mut self, path: &Path, volume: MotionAudioVolume) -> Result<(), BackendError>;
    fn stop(&mut self) -> bool;
    fn is_playing(&self) -> bool;
}

#[derive(Default)]
pub(crate) struct SystemAudioBackend {
    pub(crate) output: Option<rodio::MixerDeviceSink>,
    pub(crate) player: Option<rodio::Player>,
    pub(crate) prepared: HashMap<PathBuf, rodio::buffer::SamplesBuffer>,
}

impl AudioBackend for SystemAudioBackend {
    fn prepare(&mut self, paths: &[PathBuf]) -> Result<usize, BackendError> {
        use rodio::Source;

        let mut prepared = HashMap::with_capacity(paths.len());
        for path in paths {
            let file = std::fs::File::open(path).map_err(|_| BackendError::ResourceIo)?;
            let decoder = rodio::Decoder::try_from(file).map_err(|_| BackendError::DecodeFailed)?;
            let buffer = rodio::buffer::SamplesBuffer::new(
                decoder.channels(),
                decoder.sample_rate(),
                decoder.collect::<Vec<_>>(),
            );
            prepared.insert(path.clone(), buffer);
        }
        let prepared_resources = prepared.len();
        self.prepared.extend(prepared);
        Ok(prepared_resources)
    }

    fn activate_prepared(&mut self, paths: &[PathBuf]) -> Result<(), BackendError> {
        self.prepared.retain(|path, _| paths.contains(path));
        Ok(())
    }

    fn play(&mut self, path: &Path, volume: MotionAudioVolume) -> Result<(), BackendError> {
        let source = self
            .prepared
            .get(path)
            .cloned()
            .ok_or(BackendError::ResourceIo)?;
        if self.output.is_none() {
            let mut output = rodio::DeviceSinkBuilder::from_default_device()
                .and_then(|builder| {
                    builder
                        .with_buffer_size(rodio::cpal::BufferSize::Fixed(
                            PREFERRED_OUTPUT_BUFFER_FRAMES,
                        ))
                        .open_stream()
                })
                .or_else(|_| rodio::DeviceSinkBuilder::open_default_sink())
                .map_err(|_| BackendError::OutputUnavailable)?;
            output.log_on_drop(false);
            self.output = Some(output);
        }
        let output = self
            .output
            .as_ref()
            .ok_or(BackendError::OutputUnavailable)?;
        let player = rodio::Player::connect_new(output.mixer());
        player.set_volume(volume.get());
        player.append(source);
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
