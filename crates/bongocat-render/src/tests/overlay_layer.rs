//! The layer channel answers "nothing new" differently from "nothing to draw".
//!
//! This file exists because of a bug a user reported as a flickering panel, and the bug
//! was here rather than in any plugin. The producer publishes a layer set at its own
//! cadence — a plugin's worth of work, about ten times a second — and the overlay's frame
//! loop asks sixty times a second. The two answers it used to get back were the same
//! answer, so five frames out of six the overlay was handed an empty list and replaced its
//! layers with nothing. A panel was visible for one frame in six: a strobe, not a panel.
//!
//! The model's own frame channel has always answered `None` for "nothing new". These tests
//! hold the layer channel to the same contract, because a channel that conflates the two
//! cannot be used correctly by a caller and cannot be misused by accident either.

use super::*;

/// A layer the producer will accept, one pixel wide so the fixture stays trivial.
fn layer(id: u64) -> OverlayLayer {
    OverlayLayer {
        id,
        placement: OverlayLayerPlacement {
            anchor: OverlayAnchor::BottomLeft,
            margin: [0.02, 0.02],
            nudge: [0.0, 0.0],
            width_fraction: 0.5,
            opacity: 0.9,
        },
        raster: OverlayLayerRaster {
            width: 1,
            height: 1,
            pixels: Arc::from(vec![0u8, 0, 0, 255].into_boxed_slice()),
            content: id,
        },
    }
}

#[test]
fn silence_is_not_the_same_answer_as_an_empty_set() {
    // The whole of the contract, in one test, because the bug was the difference between
    // two answers that looked identical from the outside.
    let (producer, consumer) = overlay_layer_channel();

    assert_eq!(
        consumer.take_latest(),
        None,
        "a producer that has never spoken is not a producer that said there is nothing"
    );

    producer
        .publish_checked(vec![layer(1), layer(2)])
        .expect("publish");
    assert_eq!(
        consumer.take_latest().map(|layers| layers.len()),
        Some(2),
        "and a publish is an answer"
    );

    assert_eq!(
        consumer.take_latest(),
        None,
        "while asking again with nothing published since is silence, which a caller must \\
         be able to tell from a published empty set — treating silence as 'no layers' is \\
         what made a panel blink between the producer's cadence and the display's"
    );
    assert_eq!(
        consumer.take_latest(),
        None,
        "and it stays silence however often it is asked, since a frame loop asks far more \\
         often than a producer publishes"
    );

    producer.publish_checked(Vec::new()).expect("publish");
    assert_eq!(
        consumer.take_latest(),
        Some(Vec::new()),
        "whereas a published empty set is the producer saying every plugin is off, and it \\
         must be delivered or the window could never clear a panel the user switched off"
    );
}

#[test]
fn a_newer_publish_replaces_an_older_one_and_a_republish_counts_as_new() {
    let (producer, consumer) = overlay_layer_channel();

    producer.publish_checked(vec![layer(1)]).expect("publish");
    assert_eq!(
        consumer.take_latest().map(|layers| layers[0].id),
        Some(1),
        "the first answer"
    );

    // Republishing identical content is still a new answer. A panel that repaints to the
    // same pixels is not *visibly* new, but a producer saying "this is what is on screen"
    // again is not silence, and reporting it as silence would mean a consumer that skipped
    // work could never notice the producer is alive.
    producer.publish_checked(vec![layer(1)]).expect("publish");
    assert_eq!(
        consumer.take_latest().map(|layers| layers.len()),
        Some(1),
        "so it is delivered rather than swallowed"
    );

    producer.publish_checked(vec![layer(7)]).expect("publish");
    assert_eq!(
        consumer.take_latest().map(|layers| layers[0].id),
        Some(7),
        "and a genuinely newer publish replaces the old one"
    );
}

#[test]
fn a_layer_a_gpu_could_not_take_is_dropped_and_counted_not_refused() {
    // The filter is on the way in, so a malformed raster never reaches a backend — and the
    // *rest* of the set still arrives, because one bad panel is not a reason to blank the
    // window.
    let (producer, consumer) = overlay_layer_channel();
    let mut broken = layer(2);
    broken.raster = OverlayLayerRaster {
        width: 4,
        height: 4,
        pixels: Arc::from(vec![0u8; 3].into_boxed_slice()),
        content: 2,
    };

    producer
        .publish_checked(vec![layer(1), broken])
        .expect("publish");

    let layers = consumer.take_latest().expect("an answer");
    assert_eq!(
        layers.iter().map(|layer| layer.id).collect::<Vec<_>>(),
        vec![1],
        "the good layer survives a bad neighbour"
    );
    assert_eq!(
        producer.diagnostics().unuploadable,
        1,
        "and the dropped one is counted rather than silently lost: a plugin whose panel \\
         never reaches the screen is a plugin that looks like it is switched off"
    );
}

#[test]
fn asking_far_more_often_than_the_producer_publishes_is_what_a_frame_loop_does() {
    // The shape of the bug, run as a test: sixty asks against three publishes. Under the old
    // draining contract this returned empty fifty-seven times, and a caller that drew from
    // it showed a panel for one frame in six.
    let (producer, consumer) = overlay_layer_channel();
    let mut delivered = Vec::new();
    for index in 0..60 {
        if index % 20 == 0 {
            producer
                .publish_checked(vec![layer(1 + (index / 20) as u64)])
                .expect("publish");
        }
        if let Some(layers) = consumer.take_latest() {
            delivered.push(layers);
        }
    }

    assert_eq!(
        delivered.len(),
        3,
        "three publishes, three answers, however often the loop asks in between"
    );
    for layers in &delivered {
        assert_eq!(layers.len(), 1, "and each answer carried its panel");
    }
}
