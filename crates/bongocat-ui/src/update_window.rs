//! The update window.
//!
//! One window renders the whole update flow: check, download with progress,
//! verification, install and failure recovery. It never performs any of those
//! steps itself — it sends typed commands to the update worker and renders the
//! state the worker publishes. Closing the window therefore does not cancel
//! anything, which is why the close action stays available while the worker is
//! busy.

use std::{cell::RefCell, rc::Rc, time::Duration};

use crate::{
    SettingsClient, SettingsLanguage, SettingsTheme, UPDATE_STATE_POLL_INTERVAL, UpdateClient,
    UpdateFailureStage, UpdatePhase, UpdateProgressInfo, UpdateSnapshot, UpdateUnavailableReason,
    window::{Tokens, apply_component_theme, sync_system_component_theme},
};
use bongocat_i18n::{format_text, text};
use gpui_kit::component::{Disableable, Root, Theme, button::Button, progress::Progress};
use gpui_kit::{
    Anchor, App, AppContext, Context, Div, FocusHandle, Focusable, Render, SharedString,
    TitlebarOptions, WeakEntity, Window, WindowBounds, WindowHandle, WindowOptions, div,
    prelude::*, px, size,
};
// `test_support()` registers elements for the headless window tests; without
// `gpui-kit`'s `test-support` feature it is the identity function.
use gpui_kit::base::TestSupportExt;

const WINDOW_WIDTH: f32 = 560.0;
const WINDOW_HEIGHT: f32 = 460.0;
const WINDOW_MIN_WIDTH: f32 = 460.0;
const WINDOW_MIN_HEIGHT: f32 = 340.0;

/// How often the window re-reads the settings snapshot for the display language.
const LANGUAGE_POLL_INTERVAL: Duration = Duration::from_secs(1);

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

/// The update window view.
pub struct UpdateView {
    client: UpdateClient,
    settings_client: SettingsClient,
    language: SettingsLanguage,
    snapshot: UpdateSnapshot,
    applied_theme: Option<SettingsTheme>,
    observed_revision: Option<u64>,
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
        cx: &mut Context<Self>,
    ) -> Self {
        let snapshot = client.snapshot();
        Self {
            client,
            settings_client,
            language,
            observed_revision: Some(snapshot.revision),
            snapshot,
            applied_theme: None,
            primary_focus: cx.focus_handle().tab_index(1).tab_stop(true),
            close_focus: cx.focus_handle().tab_index(2).tab_stop(true),
            link_focus: cx.focus_handle().tab_index(3).tab_stop(true),
        }
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

    /// Start a check from outside the window (the system menu and the About page).
    pub fn check(&mut self, cx: &mut Context<Self>) {
        if self.snapshot.phase.is_busy() {
            return;
        }
        let _ = self.client.request_check();
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

    fn start_language_polling(&self, cx: &mut Context<Self>) {
        let executor = cx.background_executor().clone();
        cx.spawn(async move |this, cx| {
            loop {
                executor.timer(LANGUAGE_POLL_INTERVAL).await;
                let client = match this.update(cx, |view, _| view.settings_client.clone()) {
                    Ok(client) => client,
                    Err(_) => break,
                };
                let Ok(snapshot) = client.read_snapshot().await else {
                    continue;
                };
                if this
                    .update(cx, |view, cx| {
                        if view.language != snapshot.resolved_language {
                            view.language = snapshot.resolved_language;
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
        if self.observed_revision == Some(snapshot.revision) {
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
        self.sync_component_theme(SettingsTheme::System, window, cx);

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
                    .child(hint_line(text(locale, code.message_key()), tokens))
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

        let mut footer = div().flex().flex_row().items_center().gap_2().w_full();
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

        div()
            .size_full()
            .flex()
            .flex_col()
            .gap_3()
            .p_4()
            .bg(tokens.canvas)
            .text_color(tokens.text)
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
            .child(
                div()
                    .flex_1()
                    .flex()
                    .flex_col()
                    .gap_3()
                    .child(body)
                    .child(notes_section(locale, notes, tokens, cx)),
            )
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
/// The notes are untrusted input, so they are parsed into
/// [`crate::update_markdown`]'s representation and rendered from that — never
/// interpreted as markup.
fn notes_section(locale: &str, notes: Option<String>, tokens: Tokens, cx: &App) -> Div {
    let Some(notes) = notes else {
        return div();
    };
    if notes.trim().is_empty() {
        return div();
    }
    let blocks = crate::update_markdown::blocks(&notes);
    div()
        .flex()
        .flex_col()
        .gap_1()
        .min_h_0()
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
                .flex_1()
                .min_h_0()
                .overflow_y_scroll()
                .text_xs()
                .child(crate::update_markdown::render(&blocks, tokens, cx)),
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
        UpdateUnavailableReason::UnsupportedHost => "update.unavailable.unsupported_host",
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
/// `language` is the display language at open time; the window keeps itself in sync
/// with the settings snapshot afterwards, so a language change does not need the
/// caller to reopen it.
pub fn open_update_window(
    client: UpdateClient,
    settings_client: SettingsClient,
    language: SettingsLanguage,
    cx: &mut App,
) -> Result<UpdateWindowHandle, String> {
    let bounds = WindowBounds::Windowed(gpui_kit::Bounds::centered(
        None,
        size(px(WINDOW_WIDTH), px(WINDOW_HEIGHT)),
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
                ..Default::default()
            },
            move |window, cx| {
                if !cx.has_global::<Theme>() {
                    gpui_kit::init(cx);
                }
                Theme::global_mut(cx).notification.placement = Anchor::BottomRight;
                sync_system_component_theme(window, cx);
                let view = cx.new(|cx| {
                    let view = UpdateView::new(client, settings_client, language, cx);
                    view.start_polling(cx);
                    view.start_language_polling(cx);
                    view
                });
                opened_view.borrow_mut().replace(view.clone());
                let appearance_view = view.downgrade();
                window
                    .observe_window_appearance(move |window, cx| {
                        if appearance_view.upgrade().is_none_or(|view| {
                            view.read(cx).applied_theme == Some(SettingsTheme::System)
                        }) {
                            sync_system_component_theme(window, cx);
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
                window.activate_window();
                cx.new(|cx| Root::new(view, window, cx))
            },
        )
        .map_err(|error| error.to_string())?;
    let view = view_slot
        .borrow_mut()
        .take()
        .ok_or_else(|| "update view was not created".to_owned())?;
    cx.activate(true);
    Ok(UpdateWindowHandle {
        window: handle,
        view: view.downgrade(),
    })
}

#[cfg(test)]
mod tests {
    use super::{human_bytes, stage_message_key};
    use crate::UpdateFailureStage;

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
    fn byte_counts_are_readable() {
        assert_eq!(human_bytes(0), "0 B");
        assert_eq!(human_bytes(512), "512 B");
        assert_eq!(human_bytes(1024), "1.0 KiB");
        assert_eq!(human_bytes(1024 * 1024 * 3 / 2), "1.5 MiB");
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
    use crate::update::every_renderable_phase;
    use crate::{
        SettingsClient, SettingsLanguage, SettingsServiceEndpoint, UpdateClient, UpdateErrorCode,
        UpdateFailureStage, UpdatePhase, UpdateReleaseInfo, UpdateSnapshot, UpdateStateHandle,
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
    }

    fn harness(cx: &mut TestAppContext, phase: UpdatePhase) -> Harness {
        cx.update(gpui_kit::init);
        let (raw_client, _update_endpoint) = UpdateClient::bounded(8);
        let state = UpdateStateHandle::new(UpdateSnapshot::new("1.0.0", phase));
        let client = raw_client.track_state(state.clone());
        let (settings_client, settings_endpoint) = SettingsClient::bounded(4);
        let created: Rc<RefCell<Option<Entity<UpdateView>>>> = Rc::new(RefCell::new(None));
        let captured = Rc::clone(&created);
        let handle = cx.open_window(test_window_size(), |_window, cx| {
            let view = cx.new(|cx| {
                UpdateView::new(
                    client,
                    settings_client,
                    SettingsLanguage::EnglishUnitedStates,
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

    /// A Markdown changelog renders, and its `https` links become real controls.
    ///
    /// The parser has its own tests for what each syntax produces; this one proves the
    /// other half — that Markdown written by the release reaches the painted window and
    /// that a link ends up as a registered element a user can click.
    ///
    /// The changelog is deliberately short: the notes area scrolls, and an element
    /// scrolled out of view is not registered, so a long document would make this test
    /// depend on how much of it happens to fit.
    #[gpui_kit::test]
    fn a_markdown_changelog_renders_with_clickable_links(cx: &mut TestAppContext) {
        let markdown = "## Fixes\n\n- fixed [the issue](https://example.com/issues/47)\n";
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
                "the changelog area must render"
            );
            let link = window
                .try_find("update-notes-link:0:https://example.com/issues/47")
                .expect("an https link must render as a registered, clickable element");
            assert!(link.visible(), "the link must be on screen");
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

    /// A link this product will not open must not become a control.
    ///
    /// The parser rejects it; this pins that the rejection survives all the way to the
    /// rendered window, which is where it would actually matter.
    #[gpui_kit::test]
    fn a_non_https_link_does_not_become_a_control(cx: &mut TestAppContext) {
        let markdown = "[click me](javascript:alert(1))\n";
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
                "the text must still render"
            );
            assert!(
                window
                    .try_find("update-notes-link:0:javascript:alert(1)")
                    .is_none(),
                "a javascript: target must not be rendered as a link"
            );
        });
    }
}
