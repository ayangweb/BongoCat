//! A failure is observable, and the next motion still plays.

use super::*;

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
fn product_backend_classifies_missing_and_invalid_audio_without_opening_a_device() {
    let service = MotionAudioService::start(2).expect("product audio service");
    let client = service.client();
    client
        .try_publish(prepare(1, &["does-not-exist.flac"]))
        .expect("missing resource request");
    let missing = client
        .wait_for_sequence(1, TIMEOUT)
        .expect("missing resource processed");
    assert_eq!(missing.resource_failures, 1);
    assert_eq!(missing.last_error, Some(MotionAudioErrorCode::ResourceIo));

    let invalid = Path::new(env!("CARGO_MANIFEST_DIR")).join("Cargo.toml");
    client
        .try_publish(MotionAudioCommand::Prepare {
            sequence: 2,
            paths: vec![invalid],
        })
        .expect("invalid resource request");
    let invalid = client
        .wait_for_sequence(2, TIMEOUT)
        .expect("invalid resource processed");
    assert_eq!(invalid.decode_failures, 1);
    assert_eq!(invalid.last_error, Some(MotionAudioErrorCode::DecodeFailed));
    service.shutdown(TIMEOUT).expect("clean shutdown");
}

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
