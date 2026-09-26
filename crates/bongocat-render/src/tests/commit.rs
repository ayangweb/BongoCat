//! A model switch cannot invalidate a frame, and a stale one is visible.

use super::*;

#[test]
fn model_commit_frame_survives_latest_frame_coalescing() {
    let (producer, consumer) = latest_render_channel();
    let token = ModelCommitToken {
        command_sequence: 7,
        model_generation: 3,
    };
    let mut commit = frame(1);
    commit.model_commit = Some(token);
    producer.publish(commit).expect("publish model commit");
    for number in 2..10_000 {
        producer.publish(frame(number)).expect("publish frame");
    }

    let commit = consumer.take_latest().expect("reliable commit frame");
    assert_eq!(commit.model_commit, Some(token));
    assert_eq!(commit.frame_number, 1);
    let latest = consumer.take_latest().expect("latest data frame");
    assert_eq!(latest.model_commit, None);
    assert_eq!(latest.frame_number, 9_999);
    assert_eq!(
        consumer.diagnostics(),
        RenderTransportDiagnostics {
            published: 9_999,
            coalesced: 9_997,
            consumed: 2,
            ..RenderTransportDiagnostics::default()
        }
    );
}

#[test]
fn model_commit_only_consumer_preserves_the_latest_data_frame() {
    let (producer, consumer) = latest_render_channel();
    producer.publish(frame(1)).expect("publish data frame");
    assert!(consumer.take_model_commit().is_none());

    let token = ModelCommitToken {
        command_sequence: 7,
        model_generation: 3,
    };
    let mut commit = frame(2);
    commit.model_commit = Some(token);
    producer.publish(commit).expect("publish model commit");

    let commit = consumer.take_model_commit().expect("reliable commit frame");
    assert_eq!(commit.model_commit, Some(token));
    assert_eq!(commit.frame_number, 2);
    assert_eq!(
        consumer.take_latest().map(|frame| frame.frame_number),
        Some(1)
    );
    assert_eq!(
        consumer.diagnostics(),
        RenderTransportDiagnostics {
            published: 2,
            consumed: 2,
            ..RenderTransportDiagnostics::default()
        }
    );
}

#[test]
fn model_commit_only_consumer_discards_superseded_model_data() {
    let (producer, consumer) = latest_render_channel();
    let mut stale = frame(1);
    stale.model_generation = 2;
    producer.publish(stale).expect("publish stale data frame");

    let token = ModelCommitToken {
        command_sequence: 7,
        model_generation: 3,
    };
    let mut commit = frame(2);
    commit.model_commit = Some(token);
    producer.publish(commit).expect("publish model commit");

    assert_eq!(
        consumer
            .take_model_commit()
            .and_then(|frame| frame.model_commit),
        Some(token)
    );
    assert!(consumer.take_latest().is_none());
    assert_eq!(
        consumer.diagnostics(),
        RenderTransportDiagnostics {
            published: 2,
            coalesced: 1,
            consumed: 1,
            ..RenderTransportDiagnostics::default()
        }
    );
}

#[test]
fn frame_sequence_must_increase_within_and_across_model_generations() {
    let (producer, consumer) = latest_render_channel();
    producer.publish(frame(2)).expect("first frame");
    let rejected = producer.publish(frame(1)).expect_err("older frame");
    assert!(matches!(rejected, RenderPublishError::NonMonotonic(_)));
    let mut replacement = frame(3);
    replacement.model_generation = 4;
    replacement.frame_number = 0;
    producer
        .publish(replacement)
        .expect("new model generation may reset frame number");
    assert_eq!(
        consumer.take_latest().map(|frame| frame.model_generation),
        Some(4)
    );
    assert_eq!(consumer.diagnostics().non_monotonic, 1);
}

#[test]
fn model_commit_feedback_is_reliable_and_never_overwrites() {
    let (producer, consumer) = latest_render_channel();
    let first = ModelCommitFeedback {
        token: ModelCommitToken {
            command_sequence: 7,
            model_generation: 3,
        },
        outcome: ModelCommitOutcome::Prepared,
    };
    let second = ModelCommitFeedback {
        token: ModelCommitToken {
            command_sequence: 8,
            model_generation: 4,
        },
        outcome: ModelCommitOutcome::Rejected(ModelCommitErrorCode::ResourcePreparationFailed),
    };
    consumer.report_model_commit(first).expect("first feedback");
    let occupied = consumer
        .report_model_commit(second)
        .expect_err("feedback cannot overwrite");
    assert_eq!(occupied.into_feedback(), second);
    assert_eq!(producer.take_model_commit_feedback(), Some(first));
    consumer
        .report_model_commit(second)
        .expect("second feedback after drain");
    assert_eq!(producer.take_model_commit_feedback(), Some(second));
    assert_eq!(
        producer.diagnostics(),
        RenderTransportDiagnostics {
            feedback_reported: 2,
            feedback_consumed: 2,
            feedback_occupied: 1,
            ..RenderTransportDiagnostics::default()
        }
    );
}

#[test]
fn model_commit_feedback_is_rejected_after_close() {
    let (producer, consumer) = latest_render_channel();
    producer.close();
    let feedback = ModelCommitFeedback {
        token: ModelCommitToken {
            command_sequence: 1,
            model_generation: 0,
        },
        outcome: ModelCommitOutcome::Prepared,
    };
    let rejected = consumer
        .report_model_commit(feedback)
        .expect_err("closed feedback");
    assert_eq!(rejected.into_feedback(), feedback);
    assert_eq!(consumer.diagnostics().feedback_rejected_after_close, 1);
}

#[test]
fn model_commit_error_codes_are_stable_and_unique() {
    let mut codes = ModelCommitErrorCode::ALL
        .iter()
        .map(|code| code.as_str())
        .collect::<Vec<_>>();
    assert!(codes.iter().all(|code| code.starts_with("model_commit_")));
    codes.sort_unstable();
    codes.dedup();
    assert_eq!(codes.len(), ModelCommitErrorCode::ALL.len());
    assert_eq!(
        ModelCommitErrorCode::ResourcePreparationFailed.to_string(),
        "model_commit_resource_preparation_failed"
    );
}
