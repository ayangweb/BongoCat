//! The update window's frames, built for real.
//!
//! A phase that renders is not the same as a phase that offers the right action,
//! so these build an actual window and read back what it painted.

use crate::update_window::*;

use crate::tests::every_renderable_phase;
use crate::{
    SettingsClient, SettingsLanguage, SettingsServiceEndpoint, UpdateClient, UpdateErrorCode,
    UpdateFailureStage, UpdatePhase, UpdateReleaseInfo, UpdateServiceEndpoint, UpdateSnapshot,
    UpdateStateHandle,
};
use gpui_kit::test::{TestAppContextExt, TestWindowExt};
use gpui_kit::{AppContext, Entity, Pixels, Size, TestAppContext, px, size};
use std::{cell::RefCell, rc::Rc, time::Duration};

fn test_window_size() -> Size<Pixels> {
    size(px(560.0), px(460.0))
}

fn release(notes: Option<&str>) -> UpdateReleaseInfo {
    UpdateReleaseInfo {
        version: "9.9.9".to_owned(),
        notes: notes.map(str::to_owned),
        release_page_url: Some("https://example.invalid/v9.9.9".to_owned()),
    }
}

/// The window root the harness mounts.
///
/// The application mounts `Root` here, which adds the notification layer these tests
/// do not exercise. Mounting a plain wrapper instead is what lets the harness keep
/// the `Entity<UpdateView>` and drive the view directly — `update_window` hands the
/// root over as a type-erased `AnyView`, so it cannot be used for that.
struct Mount(Entity<UpdateView>);

impl Render for Mount {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        self.0.clone()
    }
}

struct Harness {
    handle: gpui_kit::WindowHandle<Mount>,
    view: Entity<UpdateView>,
    state: UpdateStateHandle,
    /// Kept alive so the window's language poll sees a live channel.
    _settings: SettingsServiceEndpoint,
    /// Kept alive so asking for a check finds a worker that can take it. A window
    /// that cannot ask is not the window these tests are about.
    _update: UpdateServiceEndpoint,
}

impl Harness {
    fn set_phase(&mut self, cx: &mut TestAppContext, phase: UpdatePhase) {
        cx.update_entity(&self.view, |view, _| {
            view.snapshot = UpdateSnapshot::new("1.0.0", phase.clone());
        });
    }

    /// Paint a frame and hand the window to `assertions`.
    fn paint<R>(
        &self,
        cx: &mut TestAppContext,
        assertions: impl FnOnce(&mut Window, &mut App) -> R,
    ) -> R {
        cx.update_window(self.handle.into(), |_, window, cx| {
            window.render_frame(cx);
            assertions(window, cx)
        })
        .expect("the update window stays open")
    }

    /// The height the window ends up at, having gone through the
    /// measure-then-resize cycle until it stops moving.
    ///
    /// The window does not resize itself while laying out: a frame measures its
    /// content and the frame after applies the result. Tests have no platform
    /// frame loop, so `simulate_next_frame` is what delivers the resize, and
    /// `bounds_changed` is what the platform's own resize callback does —
    /// without it `viewport_size` never moves and the next frame measures
    /// against a height the window no longer has.
    fn settled_height(&self, cx: &mut TestAppContext) -> f32 {
        let mut height = self.viewport_height(cx);
        for _ in 0..6 {
            self.paint(cx, |window, cx| {
                window.simulate_next_frame(cx);
                window.bounds_changed(cx);
            });
            let next = self.viewport_height(cx);
            if next == height {
                return height;
            }
            height = next;
        }
        height
    }

    fn viewport_height(&self, cx: &mut TestAppContext) -> f32 {
        self.paint(cx, |window, _| window.viewport_size().height.into())
    }
}

fn harness(cx: &mut TestAppContext, phase: UpdatePhase) -> Harness {
    harness_sized(cx, phase, test_window_size())
}

/// The same window, opened at a height other than the default.
fn harness_sized(cx: &mut TestAppContext, phase: UpdatePhase, size: Size<Pixels>) -> Harness {
    harness_started(cx, phase, UpdateWindowStart::Current, size)
}

/// The same window, opened onto a check it asks for rather than onto what the
/// worker has published.
fn harness_opened_for_a_check(cx: &mut TestAppContext, phase: UpdatePhase) -> Harness {
    harness_started(cx, phase, UpdateWindowStart::Check, test_window_size())
}

/// One builder for both openings, so the window under test is the same window.
fn harness_started(
    cx: &mut TestAppContext,
    phase: UpdatePhase,
    start: UpdateWindowStart,
    size: Size<Pixels>,
) -> Harness {
    cx.update(gpui_kit::init);
    let (raw_client, update_endpoint) = UpdateClient::bounded(8);
    let state = UpdateStateHandle::new(UpdateSnapshot::new("1.0.0", phase));
    let client = raw_client.track_state(state.clone());
    let (settings_client, settings_endpoint) = SettingsClient::bounded(4);
    let created: Rc<RefCell<Option<Entity<UpdateView>>>> = Rc::new(RefCell::new(None));
    let captured = Rc::clone(&created);
    let handle = cx.open_window(size, move |_window, cx| {
        let view = cx.new(|cx| {
            UpdateView::new(
                client,
                settings_client,
                SettingsLanguage::EnglishUnitedStates,
                SettingsTheme::System,
                start,
                cx,
            )
        });
        captured.borrow_mut().replace(view.clone());
        Mount(view)
    });
    let view = created
        .borrow_mut()
        .take()
        .expect("the window built its update view");
    Harness {
        handle,
        view,
        state,
        _settings: settings_endpoint,
        _update: update_endpoint,
    }
}

/// A changelog long enough to outgrow its budget, for the height tests.
fn long_notes() -> String {
    let mut notes = "# BongoCat 9.9.9\n\n## Added\n\n".to_owned();
    for index in 0..40 {
        notes.push_str(&format!(
            "- an entry number {index} that is long enough to wrap in the changelog\n"
        ));
    }
    notes
}

/// Every phase renders, and the footer offers exactly the actions that phase allows.
///
/// `offers_*` is pure logic with its own tests; whether `render` honours it — and
/// whether every variant survives being painted at all — is only observable here.
#[gpui_kit::test]
fn every_phase_renders_and_offers_only_its_own_actions(cx: &mut TestAppContext) {
    let mut harness = harness(cx, UpdatePhase::Idle);
    for phase in every_renderable_phase() {
        let expected = (
            phase.offers_check(),
            phase.offers_install(),
            phase.offers_restart(),
        );
        harness.set_phase(cx, phase.clone());
        let rendered = harness.paint(cx, |window, _| {
            (
                window.try_find("update-check").is_some(),
                window.try_find("update-install").is_some(),
                window.try_find("update-restart").is_some(),
                window.try_find("update-close").is_some(),
            )
        });
        assert!(
            rendered.3,
            "{phase:?} must always offer a way to close the window"
        );
        assert_eq!(
            (rendered.0, rendered.1, rendered.2),
            expected,
            "{phase:?} rendered the wrong footer actions"
        );
    }
}

/// The check action's label follows how many checks have already happened.
///
/// `Idle` has never checked, so "Check for Updates" is exact. `UpToDate` and every
/// `Failed` stage mean at least one check ran, so the same control says "Check
/// Again" — and deliberately not "Retry", because the click re-runs the whole
/// check → available → download pipeline rather than resuming anything.
#[gpui_kit::test]
fn the_check_action_label_follows_the_phase(cx: &mut TestAppContext) {
    let cases = [
        (UpdatePhase::Idle, "Check for Updates"),
        (UpdatePhase::UpToDate, "Check Again"),
        (
            UpdatePhase::Failed {
                stage: UpdateFailureStage::Download,
                code: UpdateErrorCode::DownloadTransportFailed,
                release: Some(release(Some("## What's new\n\n- a change\n"))),
            },
            "Check Again",
        ),
    ];
    let mut harness = harness(cx, UpdatePhase::Idle);
    for (phase, expected_label) in cases {
        harness.set_phase(cx, phase.clone());
        harness.paint(cx, |window, _| {
            let control = window
                .try_find("update-check-control")
                .unwrap_or_else(|| panic!("{phase:?} must render its check control"));
            assert_eq!(
                control.label(),
                Some(expected_label),
                "{phase:?} rendered the wrong check-action label"
            );
        });
    }
}

/// The changelog area is conditional, so both branches have to be painted.
#[gpui_kit::test]
fn the_notes_section_follows_the_announced_changelog(cx: &mut TestAppContext) {
    let mut harness = harness(cx, UpdatePhase::Idle);
    for (notes, expected) in [(Some("## What's new\n\n- a change\n"), true), (None, false)] {
        harness.set_phase(
            cx,
            UpdatePhase::Available {
                release: release(notes),
            },
        );
        let rendered = harness.paint(cx, |window, _| {
            window.try_find("update-release-notes-body").is_some()
        });
        assert_eq!(
            rendered, expected,
            "a release with notes={notes:?} rendered the wrong changelog area"
        );
    }
}

/// A phase the worker publishes has to reach the painted window.
///
/// This is the whole chain: shared state -> the window's poll timer -> the view's
/// snapshot -> `render`. Everything before the render is covered elsewhere; here it
/// is driven end to end, on the test clock, with no manual state assignment.
#[gpui_kit::test]
async fn a_published_phase_reaches_the_rendered_window(cx: &mut TestAppContext) {
    let harness = harness(cx, UpdatePhase::Idle);
    cx.update_entity(&harness.view, |view, cx| view.start_polling(cx));
    harness.paint(cx, |window, _| {
        assert!(
            window.try_find("update-install").is_none(),
            "an idle window must not offer to install anything"
        );
    });

    harness.state.publish(UpdatePhase::Available {
        release: release(None),
    });

    cx.wait_for(
        harness.handle.into(),
        Duration::from_secs(5),
        |window, _| window.try_find("update-install").is_some(),
    )
    .await;
}

/// A window opened for a check opens onto that check, not onto the last result.
///
/// A check is asked for from outside the window, by the system menu and the About
/// page, and the answer to the *previous* one is what the worker has published at
/// that moment. Painting that first is the flash this test exists for: the person
/// who just asked for an answer is shown the last one, and only a poll later sees
/// the progress bar.
///
/// `update-check` is the signal, and it is the honest one: a window showing
/// `UpToDate` offers to check again, and a window showing `Checking` does not. The
/// first frame is painted before anything has polled, so nothing but the request
/// this view made could be on screen.
#[gpui_kit::test]
async fn a_window_opened_for_a_check_paints_that_check_from_its_first_frame(
    cx: &mut TestAppContext,
) {
    let harness = harness_opened_for_a_check(cx, UpdatePhase::UpToDate);
    cx.update_entity(&harness.view, |view, cx| view.start_polling(cx));

    harness.paint(cx, |window, _| {
        assert!(
            window.try_find("update-check").is_none(),
            "a window opened for a check opened onto the result of the last one"
        );
    });
    // A poll that adopted the unchanged revision would put the last result back for
    // as long as the check takes.
    cx.update_entity(&harness.view, |view, cx| view.poll_state(cx));
    harness.paint(cx, |window, _| {
        assert!(
            window.try_find("update-check").is_none(),
            "the result of the last check came back over the check being asked for"
        );
    });

    // The worker's answer is what replaces it.
    harness.state.publish(UpdatePhase::Available {
        release: release(None),
    });
    cx.wait_for(
        harness.handle.into(),
        Duration::from_secs(5),
        |window, _| window.try_find("update-install").is_some(),
    )
    .await;
}

/// A window opened onto what the worker has published is not a check request.
///
/// The automatic check opens the window to surface its result. Asking again there
/// would repeat a request the user did not make.
#[gpui_kit::test]
fn a_window_opened_onto_the_published_state_offers_to_check(cx: &mut TestAppContext) {
    let harness = harness(cx, UpdatePhase::UpToDate);
    harness.paint(cx, |window, _| {
        assert!(
            window.try_find("update-check").is_some(),
            "a window opened onto a published result must be able to check again"
        );
    });
}

/// A check asked for over a result this window has not polled yet still wins.
///
/// The worker publishes on its own schedule, so a result can land between one poll
/// and the next. The request is therefore keyed on the revision read at the moment
/// it is made: keyed on the revision the window last polled, the very next poll
/// would find a revision that had already moved — that unpolled result — and adopt
/// it, which is the same flash, arriving one poll later instead of on the first
/// frame.
#[gpui_kit::test]
fn a_check_asked_over_an_unpolled_result_is_not_overwritten_by_it(cx: &mut TestAppContext) {
    let harness = harness(cx, UpdatePhase::UpToDate);
    // The worker publishes while the window is between polls.
    harness.state.publish(UpdatePhase::Available {
        release: release(None),
    });

    cx.update_entity(&harness.view, |view, cx| view.check(cx));
    harness.paint(cx, |window, _| {
        assert!(
            window.try_find("update-check").is_none(),
            "a window that just asked for a check cannot still be offering one"
        );
    });

    // The next poll finds that result, which is not the answer to the request just
    // made, and must not become what the window shows.
    cx.update_entity(&harness.view, |view, cx| view.poll_state(cx));
    harness.paint(cx, |window, _| {
        assert!(
            window.try_find("update-check").is_none(),
            "a result this window had not polled replaced the check it just asked for"
        );
        assert!(
            window.try_find("update-install").is_none(),
            "a result this window had not polled replaced the check it just asked for"
        );
    });
}

/// A Markdown changelog reaches the painted window, links and all.
///
/// Which targets become controls, and what a refused one shows instead, is
/// `update_markdown`'s business and is tested there against the parsed node and the
/// text the window ends up showing. This is the half a document-level test cannot
/// see: that a release's Markdown reaches this window at all.
///
/// The changelog is deliberately short: the notes area scrolls, and an element
/// scrolled out of view is not registered, so a long document would make this test
/// depend on how much of it happens to fit.
#[gpui_kit::test]
fn a_markdown_changelog_renders(cx: &mut TestAppContext) {
    let markdown = "## Fixes\n\n- fixed [the issue](https://example.com/issues/47)\n";
    let mut harness = harness(cx, UpdatePhase::Idle);
    harness.set_phase(
        cx,
        UpdatePhase::Available {
            release: release(Some(markdown)),
        },
    );
    harness.paint(cx, |window, _| {
        let notes = window
            .try_find("update-release-notes-body")
            .expect("the changelog area must render");
        assert!(notes.visible(), "the changelog must be on screen");
    });
}

/// Every syntax the changelog supports has to survive being painted.
///
/// A long document does not fit the notes area, so this asserts what it can: the
/// renderer walks headings, lists, code blocks, quotes, rules and mixed inline
/// styles without panicking and without dropping the changelog area.
#[gpui_kit::test]
fn a_rich_markdown_changelog_renders(cx: &mut TestAppContext) {
    let markdown = "\
# BongoCat 9.9.9

## Fixes

- fixed **shortcut** releases
- `preset` switching no longer needs a restart

```sh
bongocat --version
```

> Thanks to everyone who reported.

---

[Full changelog](https://example.com/compare/v1.0.0...v9.9.9)
";
    let mut harness = harness(cx, UpdatePhase::Idle);
    harness.set_phase(
        cx,
        UpdatePhase::Available {
            release: release(Some(markdown)),
        },
    );
    harness.paint(cx, |window, _| {
        assert!(
            window.try_find("update-release-notes-body").is_some(),
            "a changelog with every supported syntax must still render"
        );
    });
}

/// A changelog built out of every shape a manifest can use still reaches the window.
///
/// What each shape *becomes* is `update_markdown`'s business, and it is tested there
/// against the parsed node and against the text the window ends up showing. What only
/// this window can show is that such a document survives the trip: it lays out, it
/// stays inside the height budget rather than pushing the actions off the bottom, and
/// the phase's own controls are still reachable afterwards.
#[gpui_kit::test]
fn a_changelog_of_every_refused_shape_still_reaches_the_window(cx: &mut TestAppContext) {
    let markdown = "\
![a diagram](https://example.com/tracker.gif)

![a referenced diagram][shared]

<div><img src=\"https://example.com/pixel.png\"></div>

[click me](javascript:alert(1)) and [this](https://example.com/ok)

## Fixes

- fixed **shortcut** releases

[shared]: https://example.com/shared.png
";
    let mut harness = harness(cx, UpdatePhase::Idle);
    harness.set_phase(
        cx,
        UpdatePhase::Available {
            release: release(Some(markdown)),
        },
    );
    harness.paint(cx, |window, _| {
        let notes = window
            .try_find("update-release-notes-body")
            .expect("the changelog area must render");
        assert!(notes.visible(), "the changelog must be on screen");
        assert!(
            notes.bounds().size.height <= px(NOTES_MAX_HEIGHT),
            "a changelog this dense must hold its budget and scroll"
        );
    });
    assert!(
        harness.settled_height(cx) <= WINDOW_MAX_HEIGHT,
        "a changelog of every refused shape pushed the window past its ceiling"
    );
    harness.paint(cx, |window, _| {
        assert!(
            window.try_find("update-install").is_some(),
            "the phase's own actions must survive the changelog"
        );
    });
}

/// The window is as tall as the phase needs, and no taller.
///
/// This is the whole point of the change, and it is only observable through a
/// real layout. The window used to be one fixed height for all ten phases while
/// four of them render a single line of text, which left most of it empty.
#[gpui_kit::test]
fn the_window_is_only_as_tall_as_the_phase_needs(cx: &mut TestAppContext) {
    let mut harness = harness(cx, UpdatePhase::Idle);
    for phase in every_renderable_phase() {
        harness.set_phase(cx, phase.clone());
        let height = harness.settled_height(cx);
        assert!(
            (WINDOW_MIN_HEIGHT..=WINDOW_MAX_HEIGHT).contains(&height),
            "{phase:?} settled at {height}px, outside the window's own range"
        );
    }
}

/// The floor is what keeps a one-line phase compact, and a changelog long
/// enough to matter takes the window up to its content rather than past it.
#[gpui_kit::test]
fn a_one_line_phase_collapses_and_a_long_changelog_grows(cx: &mut TestAppContext) {
    let mut harness = harness(cx, UpdatePhase::Idle);
    let one_line = harness.settled_height(cx);
    // 460 is the height this window used for every phase. A phase that renders
    // a single status line should be nowhere near it.
    assert!(
        one_line < 260.,
        "a one-line phase settled at {one_line}px, barely shorter than the \
         fixed height it replaced"
    );

    harness.set_phase(
        cx,
        UpdatePhase::Available {
            release: release(Some(&long_notes())),
        },
    );
    let with_changelog = harness.settled_height(cx);
    assert!(
        with_changelog > one_line + 150.,
        "a long changelog settled at {with_changelog}px against {one_line}px \
         without one: it is not getting the room it needs"
    );
    assert!(
        with_changelog <= WINDOW_MAX_HEIGHT,
        "a changelog must scroll rather than grow the window past what the \
         product already used"
    );
    harness.paint(cx, |window, _| {
        let notes = window
            .try_find("update-release-notes-body")
            .expect("the changelog renders");
        assert_eq!(
            notes.bounds().size.height,
            px(NOTES_MAX_HEIGHT),
            "a changelog longer than its budget holds the budget and scrolls"
        );
    });
}

/// The height a phase produces must not depend on the height the window
/// happened to be at.
///
/// The measurement reads the changelog's own height, which is capped at a
/// constant precisely so it does not feed back into itself. Without that the
/// window would chase its own resize: growing hands the changelog more room,
/// which reports a taller content, which grows the window again.
#[gpui_kit::test]
fn the_height_does_not_depend_on_which_height_the_window_opened_at(cx: &mut TestAppContext) {
    let phase = UpdatePhase::Available {
        release: release(Some(&long_notes())),
    };
    let from_floor = harness_sized(cx, phase.clone(), size(px(560.), px(WINDOW_MIN_HEIGHT)));
    let floor_height = from_floor.settled_height(cx);
    let from_ceiling = harness_sized(cx, phase.clone(), size(px(560.), px(WINDOW_MAX_HEIGHT)));
    let ceiling_height = from_ceiling.settled_height(cx);

    assert_eq!(
        floor_height, ceiling_height,
        "the same content must settle at the same height from either direction"
    );
}

/// Every phase has to fit inside the ceiling with its actions reachable.
///
/// The window sizes itself to its content, so a phase whose content plus the
/// changelog's budget outgrew the ceiling would push its own actions off the
/// bottom. This is the check that keeps [`NOTES_MAX_HEIGHT`] honest: it is a
/// budget, and this is what it is spent against.
#[gpui_kit::test]
fn every_phase_fits_inside_the_ceiling(cx: &mut TestAppContext) {
    for phase in every_renderable_phase() {
        let mut harness = harness(cx, phase.clone());
        harness.set_phase(
            cx,
            UpdatePhase::Available {
                release: release(Some(&long_notes())),
            },
        );
        let height = harness.settled_height(cx);
        assert!(
            height <= WINDOW_MAX_HEIGHT,
            "{phase:?} with a long changelog settled at {height}px, past the \
             {WINDOW_MAX_HEIGHT}px ceiling"
        );
        harness.paint(cx, |window, _| {
            let close = window
                .try_find("update-close")
                .expect("the footer always offers a way to close");
            assert!(
                close.visible(),
                "{phase:?} pushed its actions out of a {height}px window"
            );
        });
    }
}

/// A changelog taller than its budget scrolls rather than being cut off.
///
/// The actions staying reachable is the half a screenshot would catch; the other
/// half is that the changelog box stops at the budget, which is what keeps the
/// measured height independent of the window's.
#[gpui_kit::test]
fn a_changelog_past_its_budget_scrolls(cx: &mut TestAppContext) {
    let mut harness = harness(cx, UpdatePhase::Idle);
    harness.set_phase(
        cx,
        UpdatePhase::Available {
            release: release(Some(&long_notes())),
        },
    );
    harness.paint(cx, |window, _| {
        let notes = window
            .try_find("update-release-notes-body")
            .expect("the changelog renders");
        assert_eq!(
            notes.bounds().size.height,
            px(NOTES_MAX_HEIGHT),
            "a changelog longer than the budget must hold the budget and scroll"
        );
    });
}

/// A window too small for its content still reports the height it needs.
///
/// This is what lets the window go straight to the right size instead of growing
/// to the ceiling and settling back down: the content column does not shrink, so
/// it still reports its own height while it overflows, and the leftover space
/// collapses to nothing, so both gaps around it are still the gaps. If a layout
/// change broke either, the window would resize to a height a padding's or a gap's
/// worth off — which no other assertion here would notice.
#[gpui_kit::test]
fn a_window_too_small_for_its_content_still_reports_what_it_needs(cx: &mut TestAppContext) {
    let phase = UpdatePhase::Available {
        release: release(Some(&long_notes())),
    };
    let roomy = harness_sized(cx, phase.clone(), size(px(560.), px(WINDOW_MAX_HEIGHT)));
    let roomy_height = roomy.settled_height(cx);

    // The same content in a window with nowhere near enough room for it. Checked
    // on its first frame, because the window corrects itself from there on.
    let cramped = harness_sized(cx, phase.clone(), size(px(560.), px(WINDOW_MIN_HEIGHT)));
    cramped.paint(cx, |window, _| {
        let content = window
            .try_find("update-content")
            .expect("the content column renders");
        let viewport = window.viewport_size().height;
        assert!(
            content.bounds().size.height > viewport,
            "the content column shrank to fit a window that cannot hold it, so its \
             height is no longer the height it needs"
        );
    });
    assert_eq!(
        cramped.settled_height(cx),
        roomy_height,
        "a window too small for its content settled somewhere else"
    );
}

/// The window is shown with a frame that was painted at the height it settled on.
///
/// Showing a window at one height with a frame laid out for another is the same
/// flash as showing it at the wrong size, one frame later: for a changelog the
/// actions would be below the fold of a window tall enough to hold them.
#[gpui_kit::test]
fn the_window_is_painted_at_the_height_it_settles_on_before_it_is_shown(cx: &mut TestAppContext) {
    // A changelog, because the height moves the most for it, and a frame laid out
    // for the wrong height is the most visibly wrong there.
    let phase = UpdatePhase::Available {
        release: release(Some(&long_notes())),
    };
    // Both directions: growing into a tall phase, and shrinking out of the ceiling.
    // The shrink is the one that used to flash, because the window was created at
    // the ceiling and came down from it.
    for opened_at in [WINDOW_MIN_HEIGHT, WINDOW_MAX_HEIGHT] {
        let harness = harness_sized(cx, phase.clone(), size(px(560.), px(opened_at)));
        harness.paint(cx, |window, app| {
            // The two steps priming takes, in the order it takes them.
            let content_height = harness.view.read(app).content_height.clone();
            let target = content_height
                .target(window.viewport_size().height)
                .expect("a window opened at another height has one to move to");
            let width = window.viewport_size().width;
            window.resize(size(width, target));
            finish_sizing(window, app);

            let viewport = window.viewport_size().height;
            assert!(
                content_height.target(viewport).is_none(),
                "a window opened at {opened_at} was left at a height its content \
                 does not need"
            );
            let actions = window
                .try_find("update-footer")
                .expect("the actions render");
            let bottom = actions.bounds().origin.y + actions.bounds().size.height;
            assert!(
                bottom <= viewport,
                "a window opened at {opened_at} settled at {viewport:?} but the \
                 frame it would be shown with ends its actions at {bottom:?}"
            );
        });
    }
}

/// The height measurement applies the top padding to both sides, so the root
/// has to keep padding symmetrically.
///
/// Nothing else in the product depends on it, so a layout change that made the
/// padding asymmetric would quietly put the window a padding's worth off its
/// content rather than fail anywhere else.
#[gpui_kit::test]
fn the_window_pads_its_content_symmetrically(cx: &mut TestAppContext) {
    let harness = harness(cx, UpdatePhase::Idle);
    harness.settled_height(cx);
    harness.paint(cx, |window, _| {
        let content = window
            .try_find("update-content")
            .expect("the content column renders");
        let footer = window
            .try_find("update-footer")
            .expect("the actions render");
        let viewport_height = window.viewport_size().height;
        let top = content.bounds().origin.y;
        let bottom = viewport_height - (footer.bounds().origin.y + footer.bounds().size.height);
        assert_eq!(top, bottom, "the window pads its content unevenly");
        assert!(top > px(0.), "the window has no padding at all");
    });
}
