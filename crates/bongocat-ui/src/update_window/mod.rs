//! The update window: the handle the application holds and the view behind it.
//!
//! The window is small on purpose. It is opened only to show one phase of an
//! update and to offer that phase's action, so the rest of this module's
//! concerns — how tall the window has to be, what each phase says, how the
//! frame is built — are the modules beside it.

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

mod height;
mod message;
mod pending;
mod render;
#[cfg(test)]
mod tests;
mod view;

// Every module reaches its neighbours through this one prelude rather than
// naming each of them: the window's items are one vocabulary, and a list per
// module would be the same list six times.
pub(crate) use height::*;
pub(crate) use message::*;
pub(crate) use pending::*;

#[derive(Clone)]
pub struct UpdateWindowHandle {
    pub(crate) window: WindowHandle<Root>,
    pub(crate) view: WeakEntity<UpdateView>,
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

/// The update window view.
pub struct UpdateView {
    pub(crate) client: UpdateClient,
    pub(crate) settings_client: SettingsClient,
    pub(crate) language: SettingsLanguage,
    /// The appearance the product is configured with, kept in sync by
    /// [`Self::start_settings_polling`]. Seeded at open so the first frame already
    /// matches, instead of clearing the override the settings window installed.
    pub(crate) appearance_theme: SettingsTheme,
    pub(crate) snapshot: UpdateSnapshot,
    pub(crate) applied_theme: Option<SettingsTheme>,
    pub(crate) observed_revision: Option<u64>,
    pub(crate) pending_check: PendingCheck,
    pub(crate) content_height: Rc<ContentHeight>,
    pub(crate) primary_focus: FocusHandle,
    pub(crate) close_focus: FocusHandle,
    pub(crate) link_focus: FocusHandle,
}

impl Focusable for UpdateView {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.primary_focus.clone()
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
