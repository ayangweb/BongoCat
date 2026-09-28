//! Keeping the motion audio service in step with the model the runtime shows.
//!
//! Audio is a degraded service, never a blocking one: a command that cannot be
//! queued is recorded and dropped, because a missing footstep must not stall a
//! frame.

use crate::*;
use bongocat_audio::{MotionAudioClient, MotionAudioCommand, MotionAudioStopReason};

pub(crate) fn prepare_model_audio(
    client: &MotionAudioClient,
    model: &CommittedModel,
) -> Option<u64> {
    let paths = model_audio_paths(model);
    if paths.is_empty() {
        return None;
    }
    client
        .try_publish_with_sequence(|sequence| MotionAudioCommand::Prepare { sequence, paths })
        .ok()
}

pub(crate) fn activate_model_audio(client: &MotionAudioClient, paths: Vec<std::path::PathBuf>) {
    let _ = client.try_publish_with_sequence(|sequence| MotionAudioCommand::ActivatePrepared {
        sequence,
        paths,
    });
}

pub(crate) fn model_audio_paths(model: &CommittedModel) -> Vec<std::path::PathBuf> {
    model
        .index()
        .motion_groups
        .iter()
        .flat_map(|group| group.motions.iter())
        .filter_map(|motion| motion.sound.as_deref())
        .map(|sound| model.root().join(sound))
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect()
}

pub(crate) fn motion_audio_path(
    model: Option<&CommittedModel>,
    motion: &MotionId,
) -> Option<std::path::PathBuf> {
    let model = model?;
    let sound = model
        .index()
        .motion_groups
        .iter()
        .find(|group| group.name == motion.group())?
        .motions
        .get(motion.index())?
        .sound
        .as_deref()?;
    Some(model.root().join(sound))
}

pub(crate) fn stop_motion_audio(client: &MotionAudioClient, reason: MotionAudioStopReason) {
    let _ =
        client.try_publish_with_sequence(|sequence| MotionAudioCommand::Stop { sequence, reason });
}
