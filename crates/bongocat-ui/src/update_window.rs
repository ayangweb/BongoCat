//! The update window.
//!
//! One window renders the whole update flow: check, download with progress,
//! verification, install and failure recovery. It never performs any of those
//! steps itself — it sends typed commands to the update worker and renders the
//! state the worker publishes. Closing the window therefore does not cancel
//! anything, which is why the close action stays available while the worker is
//! busy.

use std::{
    cell::{Cell, RefCell},
    rc::Rc,
    time::Duration,
};

use crate::{
    SettingsClient, SettingsLanguage, SettingsTheme, UpdateClient, UpdateErrorCode,
    UpdateFailureStage, UpdatePhase, UpdateProgressInfo, UpdateSnapshot, UpdateUnavailableReason,
    window::{Tokens, apply_component_theme},
};
use bongocat_i18n::{format_text, text};
use gpui_kit::component::{Disableable, Root, Theme, button::Button, progress::Progress};
use gpui_kit::{
    Anchor, App, AppContext, Bounds, Context, Div, FocusHandle, Focusable, Pixels, Render,
    SharedString, TitlebarOptions, WeakEntity, Window, WindowBounds, WindowHandle, WindowOptions,
    div, prelude::*, px, size,
};
// `test_support()` registers elements for the headless window tests; without
// `gpui-kit`'s `test-support` feature it is the identity function.
use gpui_kit::base::TestSupportExt;

const WINDOW_WIDTH: f32 = 560.0;
const WINDOW_MIN_WIDTH: f32 = 460.0;

/// The height the window is created at, before it has been laid out once.
///
/// The guess is invisible: the window is created hidden, laid out once to find out
/// what its content needs, resized to that, and only shown once a frame has been
/// painted at it. The floor is the guess because most phases are compact, so it is the
/// one that needs no second step.
const WINDOW_INITIAL_HEIGHT: f32 = WINDOW_MIN_HEIGHT;

/// The floor, and the height every content-sized phase collapses to when the phase
/// renders less than this.
///
/// It is set low enough that a one-line status is snug. At 200% scaling the text is
/// twice as tall in logical pixels, so the window follows the content up rather
/// than clipping it.
const WINDOW_MIN_HEIGHT: f32 = 180.0;

/// The ceiling. The window is as tall as its content asks for, and this is as tall
/// as it is ever allowed to get.
const WINDOW_MAX_HEIGHT: f32 = 460.0;

/// How much room the changelog may claim before it starts scrolling.
///
/// The window is sized to its content, so this is also what bounds the window: the
/// tallest window is the tallest phase's chrome plus this. Sizing it so that still
/// lands inside the ceiling is what keeps the ceiling reachable, and
/// `every_phase_fits_inside_the_ceiling` is what checks that against a real layout
/// instead of against a comment.
const NOTES_MAX_HEIGHT: f32 = 200.0;

/// How far a platform's content height may sit from the one that was asked for before
/// the difference counts as the window needing to move.
///
/// A platform rounds a content size to whole pixels, so asking for 410.5 arrives as 411.
/// Left alone, the frame after would measure 410.5 against a 411 viewport and ask for
/// 410.5 again, and the one after that the same — the platform not moving, the window
/// asking, once per frame. Half a pixel is under what anyone can see, so it is not
/// chased.
const HEIGHT_MATCH_TOLERANCE: f32 = 0.5;

/// How often the update window re-reads the shared update state while it is open.
const UPDATE_STATE_POLL_INTERVAL: Duration = Duration::from_millis(250);

/// How often the window re-reads the settings snapshot for the display language and the
/// appearance.
const SETTINGS_POLL_INTERVAL: Duration = Duration::from_secs(1);

fn update_error_message_key(code: UpdateErrorCode) -> &'static str {
    match code {
        UpdateErrorCode::NotConfigured => "update.error.not_configured",
        UpdateErrorCode::EnvironmentDisabled => "update.error.environment_disabled",
        UpdateErrorCode::SignatureKeyMissing => "update.error.signature_key_missing",
        UpdateErrorCode::ReleaseFetchFailed => "update.error.release_fetch_failed",
        UpdateErrorCode::ReleaseManifestInvalid => "update.error.release_manifest_invalid",
        UpdateErrorCode::NoMatchingAsset => "update.error.no_matching_asset",
        UpdateErrorCode::DownloadTransportFailed => "update.error.download_transport_failed",
        UpdateErrorCode::ChecksumMismatch => "update.error.checksum_mismatch",
        UpdateErrorCode::SignatureInvalid => "update.error.signature_invalid",
        UpdateErrorCode::ArchiveInvalid => "update.error.archive_invalid",
        UpdateErrorCode::InstallPathNotWritable => "update.error.install_path_not_writable",
        UpdateErrorCode::InstallFailed => "update.error.install_failed",
        UpdateErrorCode::RestartFailed => "update.error.restart_failed",
        UpdateErrorCode::Internal => "update.error.internal",
    }
}

#[derive(Clone)]
pub struct UpdateWindowHandle {
    window: WindowHandle<Root>,
    view: WeakEntity<UpdateView>,
}

impl UpdateWindowHandle {
    pub fn update<C, R>(
        &self,
        cx: &mut C,
        update: impl FnOnce(&mut UpdateView, &mut Window, &mut Context<UpdateView>) -> R,
    ) -> gpui_kit::Result<R>
    where
        C: AppContext,
    {
        self.window.update(cx, |_, window, cx| {
            self.view.update(cx, |view, cx| update(view, window, cx))
        })?
    }

    pub fn view(&self) -> &WeakEntity<UpdateView> {
        &self.view
    }

    pub fn window(&self) -> &WindowHandle<Root> {
        &self.window
    }

    /// Bring the window to the front.
    pub fn activate(&self, cx: &mut App) -> gpui_kit::Result<()> {
        self.window
            .update(cx, |_, window, _| window.activate_window())
    }

    /// Whether the window still exists.
    ///
    /// A handle outlives the window it was opened for, so callers that need to know
    /// whether the user still has it on screen ask here rather than assuming.
    pub fn is_open(&self) -> bool {
        self.view.upgrade().is_some()
    }
}

impl PartialEq for UpdateWindowHandle {
    fn eq(&self, other: &Self) -> bool {
        self.window == other.window
    }
}

impl Eq for UpdateWindowHandle {}

/// What a freshly opened update window opens onto.
///
/// The system menu and the About page ask for a check, and the check they ask for is
/// what that window shows: opening onto whatever the worker last published would put
/// the previous answer on screen for as long as the new one took to arrive, which is
/// the one thing a person who just asked for an answer is not waiting for.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum UpdateWindowStart {
    /// Show the state the worker has published.
    Current,
    /// Ask the worker for a check and show it from the window's first frame.
    Check,
}

/// A check this view asked for, and the published revision it asked from.
///
/// The worker is the only writer of the published state, and it publishes `Checking`
/// when it takes the command. The command is a message to another thread, so from the
/// request until that publish the published state still describes the *previous*
/// check. Rendering that is what made a window opened for a new check show the last
/// result for a moment before its progress bar appeared.
///
/// The revision the request was made at is what makes the locally rendered phase
/// safe rather than a guess. A worker that has not answered has not moved the
/// revision, so the stale phase is not adopted; a worker that answers answers with a
/// revision that differs, so the answer always wins. A build that cannot update is
/// excluded before this is ever set — see [`asks_for_a_new_check`].
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
struct PendingCheck {
    asked_at: Option<u64>,
}

impl PendingCheck {
    /// Whether a check has been asked for and not answered yet.
    fn is_pending(&self) -> bool {
        self.asked_at.is_some()
    }

    /// Render the check this view asked for instead of the last one's result.
    fn begin(&mut self, snapshot: &mut UpdateSnapshot) {
        self.asked_at = Some(snapshot.revision);
        snapshot.phase = UpdatePhase::Checking;
    }

    /// Whether a published revision is the worker answering.
    fn answers(&self, revision: u64) -> bool {
        self.asked_at != Some(revision)
    }

    /// The worker answered; what is published is the current fact again.
    fn settle(&mut self) {
        self.asked_at = None;
    }
}

/// Whether a check asked for now would start one.
///
/// Pure because it is a decision about two facts rather than about a window. It is
/// what keeps a second request from being sent while one is outstanding, and it is
/// what keeps a build that cannot update out of a progress bar that would never
/// resolve: such a build's worker republishes the phase it already had, which advances
/// no revision and therefore would never answer.
const fn asks_for_a_new_check(phase: &UpdatePhase, pending: bool) -> bool {
    !pending && !phase.is_busy() && !matches!(phase, UpdatePhase::Unavailable { .. })
}

/// The height the rendered content needs, read off one laid-out frame.
///
/// The window's root has three children: the content column, which is sized by its
/// content and never shrinks, a spacer that collects whatever height is left over,
/// and the actions. Reading the height off those three is what keeps the window
/// honest — the column reports the height it needs, and the gaps around the spacer
/// are real distances the frame reports rather than constants this file has to keep
/// in step with the style.
///
/// The measurement is exact even when the window is too short for its content, which
/// is what lets the window go straight to the right size instead of growing to the
/// ceiling and then settling back down. Two things make that hold: the column does
/// not shrink, so it still reports its content height when it overflows, and the
/// spacer collapses to nothing, so both gaps around it are still the gaps.
/// `the_height_does_not_depend_on_which_height_the_window_opened_at` is what checks
/// it end to end; a layout change that broke it would send the window to the wrong
/// size rather than merely a slow one.
///
/// `children` is the root's `on_children_prepainted` bounds, in order.
fn required_height(children: &[Bounds<Pixels>]) -> Option<Pixels> {
    let [content, spacer, actions] = children else {
        return None;
    };
    // The padding is read off the top and applied to both sides, which is what `p_4`
    // means. It is the one number the frame cannot report: a window too small for its
    // content squeezes its own bottom padding rather than pushing the actions off
    // the bottom.
    let padding = content.origin.y;
    let above_spacer = spacer.origin.y - (content.origin.y + content.size.height);
    let below_spacer = actions.origin.y - (spacer.origin.y + spacer.size.height);
    // The spacer's own height is left out on purpose. It is the leftover, so counting
    // it would make an oversized window a fixed point and it would never come back
    // down to its content.
    Some(
        padding + content.size.height + above_spacer + below_spacer + actions.size.height + padding,
    )
}

/// The height the window should be, once a frame has said what its content needs.
///
/// The height is only knowable *after* layout, so the layout pass records it here and
/// whoever can act on it does: the opener, before the window is shown, and the
/// following frame once it is.
#[derive(Default)]
struct ContentHeight {
    required: Cell<Option<Pixels>>,
}

impl ContentHeight {
    fn record(&self, children: &[Bounds<Pixels>]) {
        if let Some(required) = required_height(children) {
            self.required.set(Some(required));
        }
    }

    /// The height the window should have, or `None` when it already has it.
    ///
    /// The floor is what a compact phase collapses to, and the ceiling is what a long
    /// changelog stops at rather than growing past. "Already has it" allows for the
    /// platform rounding what it was asked for, so a window is not asked to move to a
    /// height it cannot hold.
    fn target(&self, viewport_height: Pixels) -> Option<Pixels> {
        let required = self.required.get()?;
        let target = required.clamp(px(WINDOW_MIN_HEIGHT), px(WINDOW_MAX_HEIGHT));
        let drift = f32::from(target - viewport_height).abs();
        (drift > HEIGHT_MATCH_TOLERANCE).then_some(target)
    }
}

/// The update window view.
pub struct UpdateView {
    client: UpdateClient,
    settings_client: SettingsClient,
    language: SettingsLanguage,
    /// The appearance the product is configured with, kept in sync by
    /// [`Self::start_settings_polling`]. Seeded at open so the first frame already
    /// matches, instead of clearing the override the settings window installed.
    appearance_theme: SettingsTheme,
    snapshot: UpdateSnapshot,
    applied_theme: Option<SettingsTheme>,
    observed_revision: Option<u64>,
    pending_check: PendingCheck,
    content_height: Rc<ContentHeight>,
    primary_focus: FocusHandle,
    close_focus: FocusHandle,
    link_focus: FocusHandle,
}

impl Focusable for UpdateView {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.primary_focus.clone()
    }
}

impl UpdateView {
    fn new(
        client: UpdateClient,
        settings_client: SettingsClient,
        language: SettingsLanguage,
        appearance_theme: SettingsTheme,
        start: UpdateWindowStart,
        cx: &mut Context<Self>,
    ) -> Self {
        let snapshot = client.snapshot();
        let mut view = Self {
            client,
            settings_client,
            language,
            appearance_theme,
            observed_revision: Some(snapshot.revision),
            pending_check: PendingCheck::default(),
            snapshot,
            applied_theme: None,
            content_height: Rc::new(ContentHeight::default()),
            primary_focus: cx.focus_handle().tab_index(1).tab_stop(true),
            close_focus: cx.focus_handle().tab_index(2).tab_stop(true),
            link_focus: cx.focus_handle().tab_index(3).tab_stop(true),
        };
        // The request is made here, while the window is still being built. A window
        // that asked for its check after the first frame had been painted is a window
        // whose first frame shows the answer to the previous question.
        if start == UpdateWindowStart::Check {
            view.check(cx);
        }
        view
    }

    /// The state currently rendered.
    pub fn snapshot(&self) -> &UpdateSnapshot {
        &self.snapshot
    }

    pub fn phase(&self) -> &UpdatePhase {
        &self.snapshot.phase
    }

    pub fn language(&self) -> SettingsLanguage {
        self.language
    }

    /// Start a check from outside the window (the system menu and the About page), or
    /// from the window's own action.
    ///
    /// Asking does not wait for the worker, and it does not have to: the view renders
    /// the check it asked for until the worker publishes a newer revision, so the
    /// progress is there from the next frame rather than a poll later.
    pub fn check(&mut self, cx: &mut Context<Self>) {
        if !asks_for_a_new_check(&self.snapshot.phase, self.pending_check.is_pending()) {
            return;
        }
        if self.client.request_check().is_err() {
            // The worker cannot take the command, so there is no check to show, and a
            // check nobody is running must not be rendered as one that is.
            return;
        }
        // The revision this request is keyed on is read from the shared state now, not
        // reused from the snapshot this view last polled. The worker may have published
        // something in between, and a request keyed on a revision it has already moved
        // past would be answered by that stale result instead of by its own.
        let mut snapshot = self.client.snapshot();
        self.pending_check.begin(&mut snapshot);
        self.observed_revision = Some(snapshot.revision);
        self.snapshot = snapshot;
        cx.notify();
    }

    fn install(&mut self, cx: &mut Context<Self>) {
        if !self.snapshot.phase.offers_install() {
            return;
        }
        let _ = self.client.request_install();
        cx.notify();
    }

    fn restart(&mut self, cx: &mut Context<Self>) {
        let _ = self.client.request_restart();
        cx.notify();
    }

    fn close(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        window.remove_window();
        cx.notify();
    }

    fn start_polling(&self, cx: &mut Context<Self>) {
        let executor = cx.background_executor().clone();
        cx.spawn(async move |this, cx| {
            loop {
                executor.timer(UPDATE_STATE_POLL_INTERVAL).await;
                if this.update(cx, |view, cx| view.poll_state(cx)).is_err() {
                    break;
                }
            }
        })
        .detach();
    }

    /// Keeps the window on the settings the product is configured with.
    ///
    /// The window opens on the values the application cached, and this poll is what
    /// makes a change made while it is open take effect: the display language and the
    /// appearance, which the update window used to read as a constant.
    fn start_settings_polling(&self, cx: &mut Context<Self>) {
        let executor = cx.background_executor().clone();
        cx.spawn(async move |this, cx| {
            loop {
                executor.timer(SETTINGS_POLL_INTERVAL).await;
                let client = match this.update(cx, |view, _| view.settings_client.clone()) {
                    Ok(client) => client,
                    Err(_) => break,
                };
                let Ok(snapshot) = client.read_snapshot().await else {
                    continue;
                };
                if this
                    .update(cx, |view, cx| {
                        let language_changed = view.language != snapshot.resolved_language;
                        let theme_changed = view.appearance_theme != snapshot.appearance_theme;
                        if language_changed || theme_changed {
                            view.language = snapshot.resolved_language;
                            view.appearance_theme = snapshot.appearance_theme;
                            cx.notify();
                        }
                    })
                    .is_err()
                {
                    break;
                }
            }
        })
        .detach();
    }

    fn poll_state(&mut self, cx: &mut Context<Self>) {
        let snapshot = self.client.snapshot();
        if self.pending_check.is_pending() {
            // A check this view asked for is the newer fact, so the result of the
            // previous one is not adopted until the worker answers. The worker has not
            // answered while the revision is the one it was asked from.
            if !self.pending_check.answers(snapshot.revision) {
                return;
            }
            self.pending_check.settle();
        } else if self.observed_revision == Some(snapshot.revision) {
            return;
        }
        self.observed_revision = Some(snapshot.revision);
        self.snapshot = snapshot;
        // A restart-requiring install is *not* driven from here: the window can be
        // closed, and then nothing would replace the process. The application watches
        // the published phase itself, and the restart action below only shortens the
        // wait for a user who is looking at it.
        cx.notify();
    }

    fn sync_component_theme(
        &mut self,
        theme: SettingsTheme,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.applied_theme == Some(theme) {
            return;
        }
        apply_component_theme(theme, window, cx);
        self.applied_theme = Some(theme);
    }
}

impl Render for UpdateView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let viewport = window.viewport_size();
        if viewport.width <= px(0.) || viewport.height <= px(0.) {
            return div().size_full().into_any_element();
        }
        let language = self.language;
        let locale = language.catalog_locale();
        window.set_window_title(text(locale, "update.window.title"));
        self.sync_component_theme(self.appearance_theme, window, cx);

        let tokens = Tokens::from_theme(cx);
        let phase = self.snapshot.phase.clone();
        let current_version = self.snapshot.current_version.clone();

        let body = match &phase {
            UpdatePhase::Unavailable { reason } => div()
                .flex()
                .flex_col()
                .gap_2()
                .child(status_line(unavailable_message(locale, *reason), tokens)),
            UpdatePhase::Idle => div()
                .flex()
                .flex_col()
                .gap_2()
                .child(status_line(text(locale, "update.status.idle"), tokens)),
            UpdatePhase::Checking => div()
                .flex()
                .flex_col()
                .gap_3()
                .child(status_line(text(locale, "update.status.checking"), tokens))
                .child(
                    Progress::new("update-check-progress")
                        .loading(true)
                        .w_full(),
                ),
            UpdatePhase::UpToDate => div().flex().flex_col().gap_2().child(status_line(
                format_text(
                    locale,
                    "update.status.up_to_date",
                    &[("version", current_version.clone())],
                ),
                tokens,
            )),
            UpdatePhase::Available { release } => {
                div().flex().flex_col().gap_2().child(status_line(
                    format_text(
                        locale,
                        "update.status.available",
                        &[("version", release.version.clone())],
                    ),
                    tokens,
                ))
            }
            UpdatePhase::Downloading { release, progress } => div()
                .flex()
                .flex_col()
                .gap_3()
                .child(status_line(download_message(locale, *progress), tokens))
                .child(
                    Progress::new("update-download-progress")
                        .value(progress.fraction().map_or(0.0, |fraction| fraction * 100.0))
                        .loading(progress.fraction().is_none())
                        .w_full(),
                )
                .child(hint_line(
                    format_text(
                        locale,
                        "update.status.downloading_detail",
                        &[
                            ("version", release.version.clone()),
                            ("downloaded", human_bytes(progress.downloaded_bytes)),
                            (
                                "total",
                                progress
                                    .total_bytes
                                    .map_or_else(|| "?".to_owned(), human_bytes),
                            ),
                        ],
                    ),
                    tokens,
                )),
            UpdatePhase::Verifying { release } => div()
                .flex()
                .flex_col()
                .gap_3()
                .child(status_line(
                    format_text(
                        locale,
                        "update.status.verifying",
                        &[("version", release.version.clone())],
                    ),
                    tokens,
                ))
                .child(
                    Progress::new("update-verify-progress")
                        .loading(true)
                        .w_full(),
                ),
            UpdatePhase::Installing { release } => div()
                .flex()
                .flex_col()
                .gap_3()
                .child(status_line(
                    format_text(
                        locale,
                        "update.status.installing",
                        &[("version", release.version.clone())],
                    ),
                    tokens,
                ))
                .child(
                    Progress::new("update-install-progress")
                        .loading(true)
                        .w_full(),
                ),
            UpdatePhase::Installed {
                version,
                restart_required,
            } => {
                let key = if *restart_required {
                    "update.status.installed_restarting"
                } else {
                    "update.status.installed_relaunching"
                };
                div().flex().flex_col().gap_2().child(status_line(
                    format_text(locale, key, &[("version", version.clone())]),
                    tokens,
                ))
            }
            UpdatePhase::Failed {
                stage,
                code,
                release,
            } => {
                // Before a check succeeds there is no release version to name, so the
                // message falls back to the running build's version.
                let version = release.as_ref().map_or_else(
                    || current_version.clone(),
                    |release| release.version.clone(),
                );
                div()
                    .flex()
                    .flex_col()
                    .gap_2()
                    .child(status_line(
                        format_text(locale, stage_message_key(*stage), &[("version", version)]),
                        tokens,
                    ))
                    .child(hint_line(
                        text(locale, update_error_message_key(*code)),
                        tokens,
                    ))
            }
        };

        let release = phase.release().cloned();
        let notes = release
            .as_ref()
            .and_then(|release| release.notes.as_ref())
            .cloned();
        let release_page = release
            .as_ref()
            .and_then(|release| release.release_page_url.clone());

        let mut footer = div()
            .id("update-footer")
            .test_support()
            .flex()
            .flex_row()
            .items_center()
            .gap_2()
            .w_full();
        if phase.offers_check() {
            // Idle 是首次检查；UpToDate 和 Failed 都意味着至少检查过一次，动作是"再来一次"。
            // 用"重新检查"而不是"重试"：点击走的是完整的 check → available → 重新下载，
            // 不是续传，措辞必须与真实动作一致。
            let check_key = if matches!(phase, UpdatePhase::UpToDate | UpdatePhase::Failed { .. }) {
                "update.action.recheck"
            } else {
                "update.action.check"
            };
            footer = footer.child(
                command_button(
                    text(locale, check_key),
                    "update-check",
                    &self.primary_focus,
                    tokens,
                    false,
                )
                .on_click(cx.listener(|view, _, _, cx| view.check(cx))),
            );
        }
        if phase.offers_install() {
            footer = footer.child(
                command_button(
                    text(locale, "update.action.install"),
                    "update-install",
                    &self.primary_focus,
                    tokens,
                    false,
                )
                .on_click(cx.listener(|view, _, _, cx| view.install(cx))),
            );
        }
        if phase.offers_restart() {
            footer = footer.child(
                command_button(
                    text(locale, "update.action.restart"),
                    "update-restart",
                    &self.primary_focus,
                    tokens,
                    false,
                )
                .on_click(cx.listener(|view, _, _, cx| view.restart(cx))),
            );
        }
        if let Some(url) = release_page
            && matches!(
                phase,
                UpdatePhase::Available { .. }
                    | UpdatePhase::Downloading { .. }
                    | UpdatePhase::Verifying { .. }
                    | UpdatePhase::Installing { .. }
            )
        {
            footer = footer.child(
                command_button(
                    text(locale, "update.action.view_on_github"),
                    "update-release-notes",
                    &self.link_focus,
                    tokens,
                    false,
                )
                .on_click(move |_, _window, _cx| {
                    let _ = bongocat_platform::open_external_url(&url);
                }),
            );
        }
        footer = footer.child(div().flex_1()).child(
            command_button(
                text(locale, "actions.close"),
                "update-close",
                &self.close_focus,
                tokens,
                false,
            )
            .on_click(cx.listener(|view, _, window, cx| view.close(window, cx))),
        );

        let measure = self.content_height.clone();
        div()
            .size_full()
            .flex()
            .flex_col()
            .gap_3()
            .p_4()
            .bg(tokens.canvas)
            .text_color(tokens.text)
            // The height this window should have is only knowable once the frame has
            // laid out, so the layout pass records it and the following frame acts on
            // it. Resizing from inside the layout pass would change the very size the
            // frame is being laid out against.
            .on_children_prepainted({
                let measure = measure.clone();
                move |children, window, _| {
                    measure.record(&children);
                    let Some(target) = measure.target(window.viewport_size().height) else {
                        return;
                    };
                    window.on_next_frame(move |window, _| {
                        let viewport = window.viewport_size();
                        if viewport.height == target {
                            return;
                        }
                        window.resize(size(viewport.width, target));
                    });
                }
            })
            .child(
                // `flex_shrink_0` is what makes the measurement honest: the column
                // keeps the height its content needs even when the window is too
                // short, which is what lets the frame report that the window is too
                // short instead of quietly compressing the changelog to fit.
                div()
                    .id("update-content")
                    .test_support()
                    .flex_shrink_0()
                    .flex()
                    .flex_col()
                    .gap_3()
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .gap_1()
                            .child(
                                div()
                                    .text_lg()
                                    .child(text(locale, "update.heading.title").to_owned()),
                            )
                            .child(hint_line(
                                format_text(
                                    locale,
                                    "update.current_version",
                                    &[("version", current_version.clone())],
                                ),
                                tokens,
                            )),
                    )
                    .child(body)
                    .child(notes_section(locale, notes, tokens, cx)),
            )
            // The leftover height collects here rather than stretching the
            // changelog, so the actions stay where a dialog's actions belong
            // whether the window is oversized, exact, or shrunk by the user — and
            // so the gap below the content is a real distance the frame can report
            // rather than a constant this file has to keep in step with the style.
            .child(div().flex_1())
            .child(footer)
            .into_any_element()
    }
}

fn status_line(message: impl Into<SharedString>, tokens: Tokens) -> Div {
    div()
        .text_sm()
        .text_color(tokens.text)
        .child(message.into())
}

fn hint_line(message: impl Into<SharedString>, tokens: Tokens) -> Div {
    div()
        .text_xs()
        .text_color(tokens.muted)
        .child(message.into())
}

/// The changelog, rendered from the Markdown the release manifest announced.
///
/// The notes are untrusted input. [`crate::update_markdown`] bounds them and hands them to
/// `gpui-kit` with the plugins that keep a manifest from making this application fetch
/// anything or offer a target it will not open.
///
/// The scroll box is sized by its content up to [`NOTES_MAX_HEIGHT`] and no
/// further, which is what makes the window's own height measurable: the height the
/// box reports is the changelog's intrinsic height capped at a constant, so it does
/// not depend on how tall the window happens to be this frame. Past the cap the box
/// holds its height and scrolls. Sizing it to the window instead is what would make
/// the two feed each other and never settle.
fn notes_section(locale: &str, notes: Option<String>, tokens: Tokens, _cx: &App) -> Div {
    let Some(notes) = notes else {
        return div();
    };
    if notes.trim().is_empty() {
        return div();
    }
    div()
        .flex()
        .flex_col()
        .gap_1()
        .child(
            div()
                .text_xs()
                .text_color(tokens.muted)
                .child(text(locale, "update.notes.title").to_owned()),
        )
        .child(
            div()
                .id("update-release-notes-body")
                .test_support()
                .w_full()
                .h_auto()
                .min_h_0()
                .max_h(px(NOTES_MAX_HEIGHT))
                .overflow_y_scroll()
                .child(crate::update_markdown::render(&notes)),
        )
}

/// A footer action.
///
/// `test_support()` registers the element for the headless window tests without
/// changing anything in a normal build: the trait's method is the identity function
/// when `gpui-kit`'s `test-support` feature is off.
fn command_button(
    label: &'static str,
    id: &'static str,
    focus: &FocusHandle,
    _tokens: Tokens,
    disabled: bool,
) -> impl IntoElement + StatefulInteractiveElement {
    div()
        .key_context("UpdateControl")
        .track_focus(focus)
        // The wrapper owns the queryable id; the inner control gets a derived one so
        // the two registrations cannot be ambiguous.
        .child(
            Button::new(SharedString::from(format!("{id}-control")))
                .label(label)
                .disabled(disabled),
        )
        .id(id)
        // `test_support()` needs the element to already have an id.
        .test_support()
}

fn stage_message_key(stage: UpdateFailureStage) -> &'static str {
    match stage {
        UpdateFailureStage::Check => "update.error.stage.check",
        UpdateFailureStage::Download => "update.error.stage.download",
        UpdateFailureStage::Verify => "update.error.stage.verify",
        UpdateFailureStage::Install => "update.error.stage.install",
    }
}

fn unavailable_message(locale: &str, reason: UpdateUnavailableReason) -> String {
    let key = match reason {
        UpdateUnavailableReason::DevelopmentBuild => "update.unavailable.development_build",
        UpdateUnavailableReason::SigningKeyMissing => "update.unavailable.signing_key_missing",
    };
    text(locale, key).to_owned()
}

fn download_message(locale: &str, progress: UpdateProgressInfo) -> String {
    match progress.percent() {
        Some(percent) => format_text(
            locale,
            "update.status.downloading",
            &[("percent", percent.to_string())],
        ),
        None => text(locale, "update.status.downloading_unknown").to_owned(),
    }
}

/// Render a byte count without pulling in a formatting dependency.
fn human_bytes(bytes: u64) -> String {
    const UNITS: [&str; 4] = ["B", "KiB", "MiB", "GiB"];
    let mut value = bytes as f64;
    let mut unit = 0;
    while value >= 1024.0 && unit + 1 < UNITS.len() {
        value /= 1024.0;
        unit += 1;
    }
    if unit == 0 {
        format!("{bytes} {}", UNITS[0])
    } else {
        format!("{value:.1} {}", UNITS[unit])
    }
}

/// Open the update window.
///
/// `language` and `appearance_theme` are the display language and the appearance at open
/// time; the window keeps itself in sync with the settings snapshot afterwards, so
/// neither a language nor a theme change needs the caller to reopen it.
///
/// `start` is what the window opens onto. [`UpdateWindowStart::Check`] asks for a check
/// here, before the window is painted for the first time, so the first frame on screen
/// is the check that was asked for rather than the answer to the previous one.
pub fn open_update_window(
    client: UpdateClient,
    settings_client: SettingsClient,
    language: SettingsLanguage,
    appearance_theme: SettingsTheme,
    start: UpdateWindowStart,
    cx: &mut App,
) -> Result<UpdateWindowHandle, String> {
    let bounds = WindowBounds::Windowed(gpui_kit::Bounds::centered(
        None,
        size(px(WINDOW_WIDTH), px(WINDOW_INITIAL_HEIGHT)),
        cx,
    ));
    let view_slot = Rc::new(RefCell::new(None));
    let opened_view = Rc::clone(&view_slot);
    let handle = cx
        .open_window(
            WindowOptions {
                window_bounds: Some(bounds),
                window_min_size: Some(size(px(WINDOW_MIN_WIDTH), px(WINDOW_MIN_HEIGHT))),
                titlebar: Some(TitlebarOptions {
                    title: Some(text(language.catalog_locale(), "update.window.title").into()),
                    ..Default::default()
                }),
                focus: true,
                show: false,
                // The window's height is its content's height. A user who could drag
                // the edge would be choosing a height the next phase change then
                // overrode, so the window is created without a resize affordance
                // rather than allowing a drag it would have to undo.
                is_resizable: false,
                ..Default::default()
            },
            move |window, cx| {
                if !cx.has_global::<Theme>() {
                    gpui_kit::init(cx);
                }
                Theme::global_mut(cx).notification.placement = Anchor::BottomRight;
                apply_component_theme(appearance_theme, window, cx);
                let view = cx.new(|cx| {
                    let view = UpdateView::new(
                        client,
                        settings_client,
                        language,
                        appearance_theme,
                        start,
                        cx,
                    );
                    view.start_polling(cx);
                    view.start_settings_polling(cx);
                    view
                });
                opened_view.borrow_mut().replace(view.clone());
                let appearance_view = view.downgrade();
                window
                    .observe_window_appearance(move |window, cx| {
                        // The callback is the system announcing that it changed. That is only
                        // the product's business while the product follows the system: a pinned
                        // preference was already handed to the platform, and it does not move
                        // because the system did.
                        let theme = appearance_view
                            .upgrade()
                            .map_or(SettingsTheme::System, |view| view.read(cx).appearance_theme);
                        if theme == SettingsTheme::System {
                            apply_component_theme(theme, window, cx);
                        }
                    })
                    .detach();
                let close_view = view.downgrade();
                window.on_window_should_close(cx, move |_, cx| {
                    let _ = close_view.update(cx, |view, cx| {
                        view.applied_theme = None;
                        cx.notify();
                    });
                    true
                });
                let focus = view.read(cx).primary_focus.clone();
                window.focus(&focus, cx);
                cx.new(|cx| Root::new(view, window, cx))
            },
        )
        .map_err(|error| error.to_string())?;
    let view = view_slot
        .borrow_mut()
        .take()
        .ok_or_else(|| "update view was not created".to_owned())?;
    // The window is still hidden here. It is sized and painted before it is shown, so
    // the first thing on screen is a frame made at the height its content needs rather
    // than one made at whatever height it was created with.
    prime_the_height(&view, handle, cx)?;
    cx.activate(true);
    Ok(UpdateWindowHandle {
        window: handle,
        view: view.downgrade(),
    })
}

/// Measure the hidden window's content, set the height it needs, and show it once that
/// height has been painted.
///
/// Three things have to happen in this order, and none of them can be left to the frame
/// loop. A content-sized window's height is only knowable after a frame has laid it out.
/// The window is not being sent frames while it is hidden — macOS only runs a display
/// link, and so only asks for frames, for a window whose occlusion state says it is on
/// screen. And the paint the window is shown with has to be one made at the final height,
/// not the one made while measuring.
///
/// So the window is laid out here, resized, and then painted and shown from a task
/// queued behind the one that applies the resize. That ordering is the whole guarantee:
/// applying a resize is itself a task on the same foreground executor, so a task spawned
/// after it cannot run before it. Nothing polls and nothing times out.
///
/// The handle is the untyped one on purpose. A typed `WindowHandle::update` leases the
/// root view for the duration of its closure, and drawing re-leases it, which is a
/// double-lease panic. The frame loop makes the same call, through the untyped handle,
/// for the same reason.
fn prime_the_height(
    view: &gpui_kit::Entity<UpdateView>,
    window: gpui_kit::WindowHandle<Root>,
    cx: &mut App,
) -> Result<(), String> {
    let showing: gpui_kit::AnyWindowHandle = window.into();
    cx.update_window(showing, |_, window, cx| {
        window.refresh();
        window.draw(cx).clear(cx);
        let content_height = view.read(cx).content_height.clone();
        let Some(target) = content_height.target(window.viewport_size().height) else {
            window.activate_window();
            return;
        };
        let width = window.viewport_size().width;
        window.resize(size(width, target));
        cx.spawn(async move |cx| {
            cx.update_window(showing, |_, window, cx| finish_sizing(window, cx))
                .ok();
        })
        .detach();
    })
    .map_err(|error| error.to_string())
}

/// The tail of priming: the resize has been applied, so paint at the height the window is
/// now and show it.
fn finish_sizing(window: &mut Window, cx: &mut App) {
    // The platform's own resize callback is what normally tells the window how big it
    // now is, and it need not have arrived yet. Reading the size from the platform here
    // is what makes the frame below the one the window is shown with.
    window.bounds_changed(cx);
    window.refresh();
    window.draw(cx).clear(cx);
    window.activate_window();
}

#[cfg(test)]
mod tests {
    use super::{
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
}

/// Headless rendering tests.
///
/// Everything above this point is exercised through pure functions and the protocol
/// types; these are the only tests that actually **render** the window. They run
/// against `gpui-kit`'s test platform, so a window is opened, laid out and painted
/// without a display, and elements registered with `test_support()` can be queried and
/// clicked. The point is to catch what pure logic cannot: a phase whose render branch
/// panics, or offers an action the phase does not allow.
#[cfg(test)]
mod render_tests {
    use super::*;
    use crate::tests::every_renderable_phase;
    use crate::{
        SettingsClient, SettingsLanguage, SettingsServiceEndpoint, UpdateClient, UpdateErrorCode,
        UpdateFailureStage, UpdatePhase, UpdateReleaseInfo, UpdateServiceEndpoint, UpdateSnapshot,
        UpdateStateHandle,
    };
    use gpui_kit::test::{TestAppContextExt, TestWindowExt};
    use gpui_kit::{AppContext, Entity, Pixels, Size, TestAppContext, px, size};
    use std::{cell::RefCell, rc::Rc, time::Duration};

    /// Matches the production default, so assertions run at the real window's size.
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
    fn the_window_is_painted_at_the_height_it_settles_on_before_it_is_shown(
        cx: &mut TestAppContext,
    ) {
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
}
