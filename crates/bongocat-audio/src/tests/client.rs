//! Two clones get two sequences, in the order they asked.

use super::*;

#[test]
fn allocated_audio_sequences_wrap_atomically_at_u64_max() {
    let service = MotionAudioService::start_with_backend(
        4,
        Box::new(RecordingBackend {
            events: Arc::new(Mutex::new(Vec::new())),
            failures: VecDeque::new(),
            playing: false,
        }),
    )
    .expect("audio service");
    let client = service.client();
    client
        .shared
        .next_sequence
        .store(u64::MAX, Ordering::Relaxed);
    let first = client
        .try_publish_with_sequence(|sequence| play(sequence, "first.flac"))
        .expect("first command accepted");
    let second = client
        .try_publish_with_sequence(|sequence| play(sequence, "second.flac"))
        .expect("second command accepted");
    assert_eq!((first, second), (u64::MAX, 0));
    let diagnostics = client
        .wait_for_sequence(second, TIMEOUT)
        .expect("wrapped commands processed");
    assert_eq!(diagnostics.last_processed_sequence, Some(second));
}

#[test]
fn cloned_clients_allocate_and_enqueue_in_one_order() {
    let service = MotionAudioService::start_with_backend(
        4,
        Box::new(RecordingBackend {
            events: Arc::new(Mutex::new(Vec::new())),
            failures: VecDeque::new(),
            playing: false,
        }),
    )
    .expect("audio service");
    let first_client = service.client();
    let second_client = first_client.clone();
    let first = thread::spawn(move || {
        first_client
            .try_publish_with_sequence(|sequence| play(sequence, "first.flac"))
            .expect("first command accepted")
    });
    let second = thread::spawn(move || {
        second_client
            .try_publish_with_sequence(|sequence| play(sequence, "second.flac"))
            .expect("second command accepted")
    });
    let sequences = [
        first.join().expect("first producer"),
        second.join().expect("second producer"),
    ];
    let mut ordered = sequences;
    ordered.sort_unstable();
    assert_eq!(ordered, [0, 1]);
    let client = service.client();
    client
        .wait_for_sequence(1, TIMEOUT)
        .expect("concurrently allocated commands processed");
}
