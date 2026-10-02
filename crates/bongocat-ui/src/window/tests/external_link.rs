//! Opening a link the user asked for, from a view callback.
//!
//! The regression here is issue #1081: on Windows, asking the operating system
//! to open a URL from inside a button callback killed the process. Nothing about
//! the URL was wrong, so what has to be asserted is *where* the launch runs and
//! *when* the outcome is reported — not that a browser opens.
//!
//! The launch is injected throughout, so a test run never spawns one.

use super::super::about::{FEEDBACK_URL, PROJECT_SOURCE_URL};
use super::*;

use bongocat_platform::ExternalUrlOpenError;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

/// The launch must not have happened by the time the callback returns, and must
/// happen afterwards.
///
/// This is the whole regression. The callback is running inside GPUI's mutable
/// borrow of its `App`; the launch used to be made inline, and making it pumps
/// the message queue, which re-enters GPUI's foreground tasks inside that same
/// borrow. Deferring it is what removes the re-entrancy.
#[gpui_kit::test]
fn a_link_request_is_handed_off_after_the_callback_returns(cx: &mut TestAppContext) {
    let (view, visual) = settings_view(cx);
    view.update(visual, |view, _| {
        view.snapshot = Some(crate::tests::snapshot(1, true, true));
    });

    let launched = Arc::new(AtomicBool::new(false));
    let marks_launch = Arc::clone(&launched);
    let read_inside_callback = Arc::clone(&launched);

    view.update(visual, move |view, cx| {
        view.open_external_link_with(
            FEEDBACK_URL,
            move |_url| {
                marks_launch.store(true, Ordering::SeqCst);
                Ok(())
            },
            cx,
        );
        assert!(
            !read_inside_callback.load(Ordering::SeqCst),
            "the launch ran inside the callback, which is the re-entrant borrow that \
             aborted the process in issue #1081"
        );
    });

    assert!(
        !launched.load(Ordering::SeqCst),
        "the request must still be scheduled when the callback returns"
    );

    visual.run_until_parked();
    assert!(
        launched.load(Ordering::SeqCst),
        "the scheduled launch must still run afterwards"
    );
}

/// A launch the platform refused has to reach the user, or the row is a dead
/// control — and it can only be reported from the completion, because raising a
/// notification is itself a view update and cannot happen inside the callback
/// that requested the link. A launch that succeeded reports nothing: the browser
/// is already the answer.
#[gpui_kit::test]
fn only_a_failed_launch_reports_and_only_after_the_callback(cx: &mut TestAppContext) {
    let (view, visual) = settings_view(cx);
    view.update(visual, |view, _| {
        view.snapshot = Some(crate::tests::snapshot(1, true, true));
    });

    view.update(visual, |view, cx| {
        view.open_external_link_with(PROJECT_SOURCE_URL, |_url| Ok(()), cx);
    });
    visual.run_until_parked();
    assert!(
        !visual.update(|window, _| window.try_find("notification").is_some()),
        "a link that opened must not raise a failure notification"
    );

    view.update(visual, |view, cx| {
        view.open_external_link_with(
            PROJECT_SOURCE_URL,
            |_url| Err(ExternalUrlOpenError::LaunchFailed),
            cx,
        );
    });
    assert!(
        !visual.update(|window, _| window.try_find("notification").is_some()),
        "the outcome cannot be known before the hand-off has reported back"
    );

    visual.run_until_parked();
    assert!(
        visual.update(|window, _| window.try_find("notification").is_some()),
        "a link the platform refused must tell the user"
    );
}
