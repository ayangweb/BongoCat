//! The update window's arithmetic and its strings.

use crate::update_window::{
    ContentHeight, PendingCheck, WINDOW_MAX_HEIGHT, WINDOW_MIN_HEIGHT, asks_for_a_new_check,
    human_bytes, required_height, stage_message_key, update_error_message_key,
};
use crate::{
    UpdateErrorCode, UpdateFailureStage, UpdatePhase, UpdateSnapshot, UpdateUnavailableReason,
    UpdateWindowHandle,
};
use gpui_kit::{Bounds, point, px, size};

#[test]
fn every_stage_has_its_own_message() {
    let keys: Vec<&str> = UpdateFailureStage::ALL
        .into_iter()
        .map(stage_message_key)
        .collect();
    let mut unique = keys.clone();
    unique.sort_unstable();
    unique.dedup();
    assert_eq!(unique.len(), keys.len());
    for key in keys {
        assert!(key.starts_with("update.error.stage."));
    }
}

#[test]
fn every_error_code_resolves_to_text_and_the_handle_is_send() {
    for code in UpdateErrorCode::ALL {
        let key = update_error_message_key(code);
        for locale in ["en-US", "zh-CN"] {
            let message = bongocat_i18n::text(locale, key);
            assert_ne!(message, key, "{locale} is missing {key}");
            assert!(!message.trim().is_empty(), "{locale} has an empty {key}");
        }
    }

    fn assert_send<T: Send>() {}
    assert_send::<Option<UpdateWindowHandle>>();
}

#[test]
fn byte_counts_are_readable() {
    assert_eq!(human_bytes(0), "0 B");
    assert_eq!(human_bytes(512), "512 B");
    assert_eq!(human_bytes(1024), "1.0 KiB");
    assert_eq!(human_bytes(1024 * 1024 * 3 / 2), "1.5 MiB");
}

/// The window height is read off the frame's own numbers, so the padding is
/// whatever the frame says it is rather than a constant this test has to keep in
/// step with the style.
#[test]
fn a_frame_reports_the_height_its_content_actually_needs() {
    // One laid-out root: the padding above the content, the content's own height,
    // the gap, the spacer, the gap, the actions, and the padding below them.
    let laid_out = |content: f32, spacer: f32, actions: f32| {
        let padding = px(16.);
        let gap = px(12.);
        let content_top = padding;
        let content_bottom = content_top + px(content);
        let spacer_top = content_bottom + gap;
        vec![
            Bounds::new(point(px(16.), content_top), size(px(528.), px(content))),
            Bounds::new(point(px(16.), spacer_top), size(px(528.), px(spacer))),
            Bounds::new(
                point(px(16.), spacer_top + px(spacer) + gap),
                size(px(528.), px(actions)),
            ),
        ]
    };
    let exact = 16.0 + 100.0 + 12.0 + 0.0 + 12.0 + 32.0 + 16.0;
    assert_eq!(required_height(&laid_out(100., 0., 32.)), Some(px(exact)));
    // The spacer took the slack, and it is not part of what the content needs.
    // Counting it would make an oversized window a fixed point and it would never
    // come back down to its content.
    assert_eq!(
        required_height(&laid_out(100., 200., 32.)),
        Some(px(exact)),
        "the leftover space must not be counted as content"
    );
    assert_eq!(required_height(&[]), None);
    assert_eq!(
        required_height(&laid_out(100., 0., 32.)[..2]),
        None,
        "a root that is not the content, the spacer and the actions is not measurable"
    );
}

/// The height is the content's, clamped between a floor and a ceiling.
#[test]
fn the_target_height_is_the_content_clamped_between_the_floor_and_the_ceiling() {
    let target_for = |required: f32, viewport: f32| {
        let height = ContentHeight::default();
        height.required.set(Some(px(required)));
        height.target(px(viewport))
    };

    assert_eq!(target_for(212., 460.), Some(px(212.)));
    assert_eq!(
        target_for(12., 460.),
        Some(px(WINDOW_MIN_HEIGHT)),
        "content shorter than the floor still grows the window to the floor"
    );
    assert_eq!(
        target_for(900., 180.),
        Some(px(WINDOW_MAX_HEIGHT)),
        "content taller than the ceiling is capped, not honoured"
    );
    assert_eq!(
        target_for(900., WINDOW_MAX_HEIGHT),
        None,
        "a window already at the ceiling does not resize again"
    );
    assert_eq!(
        target_for(212., 212.),
        None,
        "a window already at its target is not resized again"
    );
    // A platform rounds a content size to whole pixels, so the height that was asked
    // for is not always the height that arrives. Chasing the difference would have
    // the window ask once per frame for a height it cannot hold.
    assert_eq!(
        target_for(410.5, 411.),
        None,
        "a window a half pixel from its target is not asked to move to it"
    );
    assert_eq!(
        target_for(410.5, 412.),
        Some(px(410.5)),
        "a window more than a half pixel from its target is asked to move to it"
    );
    assert_eq!(
        ContentHeight::default().target(px(460.)),
        None,
        "a window that has not been laid out yet is left alone"
    );
}

/// A check the view asked for is rendered instead of the result of the last one.
///
/// This is the whole point of [`PendingCheck`]: the window shows the check it is
/// waiting on rather than the answer it is not.
#[test]
fn a_requested_check_renders_instead_of_the_previous_result() {
    let mut snapshot = UpdateSnapshot::new("1.0.0", UpdatePhase::UpToDate);
    let mut pending = PendingCheck::default();
    assert!(!pending.is_pending());
    assert!(pending.answers(snapshot.revision));

    pending.begin(&mut snapshot);
    assert!(pending.is_pending());
    assert_eq!(
        snapshot.phase,
        UpdatePhase::Checking,
        "a view that asked for a check must not still be rendering the last result"
    );
    // The published state has not moved: the worker is another thread and has not
    // taken the command yet. Adopting this revision is what put the previous
    // result back on screen.
    assert!(!pending.answers(snapshot.revision));
    // Any other revision is the worker answering, and the answer wins.
    assert!(pending.answers(snapshot.revision + 1));

    pending.settle();
    assert!(!pending.is_pending());
    assert!(pending.answers(snapshot.revision));
}

/// A second check is not asked for while one is outstanding, and a build that
/// cannot update never asks at all.
///
/// The unavailable case is the one that would hang: its worker republishes the
/// phase it already has, which advances no revision, so a rendered progress bar
/// would have nothing to end it.
#[test]
fn a_check_is_only_asked_for_when_one_would_start() {
    assert!(asks_for_a_new_check(&UpdatePhase::Idle, false));
    assert!(asks_for_a_new_check(&UpdatePhase::UpToDate, false));
    assert!(
        !asks_for_a_new_check(&UpdatePhase::Idle, true),
        "a check this view already asked for must not be asked for twice"
    );
    for busy in [
        UpdatePhase::Checking,
        UpdatePhase::Downloading {
            release: crate::UpdateReleaseInfo {
                version: "9.9.9".to_owned(),
                notes: None,
                release_page_url: None,
            },
            progress: crate::UpdateProgressInfo::default(),
        },
    ] {
        assert!(
            !asks_for_a_new_check(&busy, false),
            "{busy:?} is the worker describing something it is already doing"
        );
    }
    assert!(
        !asks_for_a_new_check(
            &UpdatePhase::Unavailable {
                reason: UpdateUnavailableReason::DevelopmentBuild,
            },
            false
        ),
        "a build that cannot update has no check to start, so it must not be shown one"
    );
}
