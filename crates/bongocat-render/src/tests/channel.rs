//! The channel coalesces a frame and never drops a commit.

use super::*;

#[test]
fn latest_frame_transport_coalesces_without_blocking_the_producer() {
    let (producer, consumer) = latest_render_channel();
    for number in 0..10_000 {
        producer.publish(frame(number)).expect("publish frame");
    }
    assert_eq!(
        consumer.take_latest().map(|frame| frame.frame_number),
        Some(9_999)
    );
    assert_eq!(
        consumer.diagnostics(),
        RenderTransportDiagnostics {
            published: 10_000,
            coalesced: 9_999,
            consumed: 1,
            ..RenderTransportDiagnostics::default()
        }
    );
}

#[test]
fn close_rejects_new_frames_but_allows_pending_drain() {
    let (producer, consumer) = latest_render_channel();
    producer.publish(frame(1)).expect("publish frame");
    producer.close();
    let rejected = producer.publish(frame(2)).expect_err("closed channel");
    assert_eq!(rejected.into_frame().frame_number, 2);
    assert_eq!(
        consumer.take_latest().map(|frame| frame.frame_number),
        Some(1)
    );
    assert_eq!(
        producer.diagnostics(),
        RenderTransportDiagnostics {
            published: 1,
            consumed: 1,
            rejected_after_close: 1,
            ..RenderTransportDiagnostics::default()
        }
    );
}

#[test]
fn output_subscription_receives_latest_committed_frames_without_overlay_ticks() {
    let (producer, consumer) = latest_render_channel();
    let output = consumer.subscribe_latest();
    producer.publish(frame(1)).unwrap();
    let mut candidate = frame(2);
    candidate.model_generation = 4;
    candidate.model_commit = Some(ModelCommitToken {
        command_sequence: 7,
        model_generation: candidate.model_generation,
    });
    producer.publish(candidate).unwrap();
    assert_eq!(output.take_latest().unwrap().frame_number, 1);
    assert!(
        output.take_latest().is_none(),
        "uncommitted candidate must not reach output"
    );
    // Rejection keeps the old generation; a later committed frame may advance.
    producer.publish(frame(3)).unwrap();
    assert_eq!(output.take_latest().unwrap().model_generation, 3);
    let mut committed = frame(4);
    committed.model_generation = 4;
    producer.publish(committed).unwrap();
    let latest = output.take_latest().unwrap();
    assert_eq!((latest.frame_number, latest.model_generation), (4, 4));
    assert_eq!(
        consumer.take_latest().unwrap().frame_number,
        2,
        "overlay retains reliable commit"
    );
    assert_eq!(
        consumer.take_latest().unwrap().frame_number,
        4,
        "overlay has its own latest frame"
    );
}

#[test]
fn stalled_output_subscription_never_blocks_runtime_publication() {
    let (producer, consumer) = latest_render_channel();
    let output = consumer.subscribe_latest();
    let locked = output.slot.value.lock().unwrap();
    let (sent, received) = std::sync::mpsc::channel();
    let worker = std::thread::spawn(move || sent.send(producer.publish(frame(1))).unwrap());
    let published = received.recv_timeout(std::time::Duration::from_secs(1));
    drop(locked);
    worker.join().unwrap();
    assert!(
        published
            .expect("producer must not wait for output")
            .is_ok()
    );
    assert_eq!(consumer.take_latest().unwrap().frame_number, 1);
}
