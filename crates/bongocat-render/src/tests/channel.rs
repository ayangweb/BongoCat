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
