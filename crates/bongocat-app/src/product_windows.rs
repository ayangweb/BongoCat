//! The product's windows: opening the settings and update windows, and reading
//! back what the overlay and the taskbar icon are actually showing.
//!
//! Each helper answers one question the run loop asks. A failure is reported
//! through the shared failure list rather than by unwinding, because a product
//! that cannot show a window must still shut down in order.

use super::*;
use product_shutdown::finish_product_quit;

/// Bring the stored overlay scale to the one a right-button resize drag settled
/// on.
///
/// The drag already resized the native window, so this only aligns the
/// configuration — and with it the settings page — with what the user sees. A
/// failed snapshot read, a configuration that already matches, and a missing
/// config revision are all non-errors: the window keeps working either way, and
/// the placement write the frame loop already performs is what makes the new
/// size survive a restart.
pub(crate) async fn publish_overlay_scale(client: &SettingsClient, scale_percent: u16) {
    let Ok(snapshot) = client.read_snapshot().await else {
        return;
    };
    let Some(config_revision) = snapshot.config_revision else {
        return;
    };
    if snapshot.overlay.scale_percent == scale_percent {
        return;
    }
    let settings = SettingsOverlay {
        scale_percent,
        ..snapshot.overlay
    };
    let _ = client.set_overlay_settings(config_revision, settings).await;
}

/// Run one update against the settings view, retrying while GPUI cannot hand the
/// window over.
///
/// A settings close destroys the window, so callers must stop using an old handle
/// after they request close; what can still fail is `AsyncApp::update` returning
/// `Err` while the platform is inside a window callback of its own.
pub(crate) async fn update_settings_window<R>(
    cx: &mut AsyncApp,
    window_handle: &SettingsWindowHandle,
    mut update: impl FnMut(
        &mut SettingsView,
        &mut Window,
        &mut Context<SettingsView>,
    ) -> Result<R, String>,
) -> Result<R, String> {
    const MAX_ATTEMPTS: u32 = 200;
    for attempt in 0..MAX_ATTEMPTS {
        match window_handle.update(cx, |view, window, cx| update(view, window, cx)) {
            Ok(Ok(result)) => return Ok(result),
            Ok(Err(error)) => return Err(error),
            Err(_) if attempt + 1 < MAX_ATTEMPTS => {
                Timer::after(Duration::from_millis(5)).await;
            }
            Err(error) => {
                return Err(format!(
                    "settings window remained unavailable after {MAX_ATTEMPTS} attempts: {error}"
                ));
            }
        }
    }
    unreachable!("the bounded settings update loop always returns")
}

pub(crate) fn ensure_settings_window(cx: &mut App) -> Result<SettingsWindowHandle, String> {
    let (existing, taskbar_icon_visible) = cx
        .try_global::<ProductCoordinator>()
        .map(|coordinator| {
            (
                coordinator.settings_window.clone(),
                #[cfg(target_os = "windows")]
                coordinator.taskbar_icon_visible,
                #[cfg(not(target_os = "windows"))]
                true,
            )
        })
        .unwrap_or((None, true));
    if let Some(window_handle) = existing {
        if window_handle.is_open() {
            return match window_handle.update(cx, |view, window, cx| {
                #[cfg(target_os = "windows")]
                bongocat_platform::set_taskbar_icon_visible(window, taskbar_icon_visible)
                    .map_err(|error| error.to_string())?;
                view.reopen(window, cx)
            }) {
                Ok(Ok(())) => {
                    cx.activate(true);
                    Ok(window_handle)
                }
                Ok(Err(error)) => Err(error),
                Err(error) => Err(format!(
                    "settings window is temporarily unavailable to reopen: {error}"
                )),
            };
        }
        // Only a released view entity is replaced. A transient GPUI borrow
        // failure above is returned to the caller instead of stacking a second
        // settings window.
        cx.global_mut::<ProductCoordinator>().settings_window = None;
    }

    let (settings_client, window_state, seed, navigation_memory) = cx
        .try_global::<ProductCoordinator>()
        .and_then(|coordinator| {
            coordinator.settings_service.as_ref().map(|service| {
                (
                    service.client(),
                    service.window_state(),
                    SettingsWindowSeed {
                        language: coordinator.product_language,
                        appearance_theme: coordinator.product_appearance_theme,
                    },
                    coordinator.settings_navigation_memory.clone(),
                )
            })
        })
        .ok_or_else(|| "settings service owner is unavailable".to_owned())?;
    let window_handle = open_settings_window(
        settings_client,
        window_state,
        seed,
        navigation_memory,
        taskbar_icon_visible,
        finish_product_quit,
        open_update_window_and_check,
        cx,
    )?;
    cx.global_mut::<ProductCoordinator>().settings_window = Some(window_handle.clone());
    Ok(window_handle)
}

/// The open update window, opening it first when there is none.
///
/// The window is a singleton like the settings window: the system menu, the About
/// page and an automatic check all route through here, so a second request focuses
/// the existing window instead of stacking another one.
///
/// `start` only applies to a window that has to be opened. An already-open window is
/// left showing what it is showing, and the caller decides afterwards whether to ask
/// it for something.
pub(crate) fn ensure_update_window(
    cx: &mut App,
    start: bongocat_ui::UpdateWindowStart,
) -> Result<bongocat_ui::UpdateWindowHandle, String> {
    let (existing, update_client, settings_client, language, appearance_theme) = {
        let coordinator = cx
            .try_global::<ProductCoordinator>()
            .ok_or_else(|| "product coordinator is unavailable".to_owned())?;
        let update_service = coordinator
            .update_service
            .as_ref()
            .ok_or_else(|| "update service is unavailable".to_owned())?;
        let settings_client = coordinator
            .settings_service
            .as_ref()
            .ok_or_else(|| "settings service is unavailable".to_owned())?
            .client();
        (
            coordinator.update_window.clone(),
            update_service.client(),
            settings_client,
            coordinator.product_language,
            coordinator.product_appearance_theme,
        )
    };
    if let Some(window_handle) = existing
        && window_handle.activate(cx).is_ok()
    {
        cx.activate(true);
        return Ok(window_handle);
    }
    let window_handle = bongocat_ui::open_update_window(
        update_client,
        settings_client,
        language,
        appearance_theme,
        start,
        cx,
    )?;
    cx.global_mut::<ProductCoordinator>().update_window = Some(window_handle.clone());
    Ok(window_handle)
}

/// Open the update window and start a check in it.
///
/// A window created for this request asked for the check while it was being built, so
/// it opens onto that check rather than onto the result of the last one. The call below
/// is what asks a window that was already open, and it is a no-op for the one just
/// created: a view does not have two checks in flight.
pub(crate) fn open_update_window_and_check(cx: &mut App) {
    let start = bongocat_ui::UpdateWindowStart::Check;
    let result = ensure_update_window(cx, start).and_then(|window_handle| {
        window_handle
            .update(cx, |view, _, cx| view.check(cx))
            .map_err(|error| error.to_string())
    });
    if let Err(error) = result {
        record_update_window_failure(cx, error);
    }
}

/// Show the update window without starting a check.
///
/// The automatic check already ran, so opening the window here only surfaces its
/// result; asking for another check would repeat the request the user did not make.
pub(crate) fn show_update_window(cx: &mut App) {
    if let Err(error) = ensure_update_window(cx, bongocat_ui::UpdateWindowStart::Current) {
        record_update_window_failure(cx, error);
    }
}

pub(crate) fn record_update_window_failure(cx: &App, error: String) {
    if let Some(failures) = cx
        .try_global::<ProductCoordinator>()
        .map(|coordinator| Arc::clone(&coordinator.failures))
    {
        record_failure(&failures, error);
    }
}

/// Whether the update window is currently on screen.
pub(crate) fn update_window_is_open(cx: &mut App) -> bool {
    cx.try_global::<ProductCoordinator>()
        .and_then(|coordinator| coordinator.update_window.as_ref())
        .is_some_and(bongocat_ui::UpdateWindowHandle::is_open)
}

/// The phase the update worker is currently publishing.
pub(crate) fn published_update_phase(cx: &mut App) -> Option<bongocat_ui_protocol::UpdatePhase> {
    cx.try_global::<ProductCoordinator>()
        .and_then(|coordinator| coordinator.update_service.as_ref())
        .map(|service| service.state().phase())
}

/// Ask the update worker for a check, unless this build cannot update at all.
pub(crate) fn request_update_check(cx: &mut App) -> bool {
    let Some(client) = cx
        .try_global::<ProductCoordinator>()
        .and_then(|coordinator| coordinator.update_service.as_ref())
        .map(bongocat_app::ApplicationUpdateService::client)
    else {
        return false;
    };
    if matches!(
        client.snapshot().phase,
        bongocat_ui_protocol::UpdatePhase::Unavailable { .. }
    ) {
        return false;
    }
    client.request_check().is_ok()
}

/// Whether the update window asked for the process to be replaced.
pub(crate) fn take_update_restart_request(cx: &mut App) -> bool {
    cx.try_global::<ProductCoordinator>()
        .and_then(|coordinator| coordinator.update_service.as_ref())
        .is_some_and(bongocat_app::ApplicationUpdateService::take_restart_request)
}

/// How long a completed install stays visible before the process is replaced.
///
/// The install already succeeded at this point and the running build is executing the
/// previous release's files, so the delay exists only to let the window show what
/// happened before it disappears.
#[cfg(target_os = "macos")]
pub(crate) const UPDATE_RESTART_DELAY: Duration = Duration::from_millis(1200);

/// Whether a completed install has been on screen long enough to restart into it.
///
/// Split out from the poll so the boundary is testable without a running product.
#[cfg(target_os = "macos")]
pub(crate) fn restart_delay_elapsed(observed_at: Instant, now: Instant) -> bool {
    now.saturating_duration_since(observed_at) >= UPDATE_RESTART_DELAY
}

/// Start the post-install restart once, from either the window's request or the
/// observed install.
///
/// The window asks for the restart, but it can be closed, and a closed window would
/// leave the process running the previous release's deleted files. Watching the
/// published phase here means the restart happens whether or not anyone is looking.
/// Returns whether the restart was started, which ends the calling loop.
#[cfg(target_os = "macos")]
pub(crate) fn poll_update_restart(cx: &mut App) -> bool {
    if !cx.has_global::<ProductCoordinator>() {
        return false;
    }
    let requested = take_update_restart_request(cx);
    let install_completed = matches!(
        published_update_phase(cx),
        Some(bongocat_ui_protocol::UpdatePhase::Installed {
            restart_required: true,
            ..
        })
    );
    if !requested && !install_completed {
        return false;
    }
    {
        let coordinator = cx.global_mut::<ProductCoordinator>();
        if coordinator.update_restart_started {
            return false;
        }
        if !requested {
            let now = Instant::now();
            let observed_at = *coordinator.update_installed_since.get_or_insert(now);
            if !restart_delay_elapsed(observed_at, now) {
                return false;
            }
        }
        coordinator.update_restart_started = true;
    }
    restart_after_update(cx);
    true
}

/// Replace this process with the build that was just installed.
///
/// The install already deleted the previous release from disk, so the running process
/// is executing files that no longer exist and everything that reads the installation
/// lazily would fail. The product is therefore shut down in the documented order
/// first, and only then is the process image replaced; the new build starts from a
/// complete, quiesced state.
#[cfg(target_os = "macos")]
pub(crate) fn restart_after_update(cx: &mut App) {
    if !cx.has_global::<ProductCoordinator>() {
        return;
    }
    let shutdown = begin_product_shutdown(cx);
    cx.spawn(async move |_| {
        let failures = shutdown.finish().await;
        {
            let failures = failures
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            for failure in failures.iter() {
                let mut stderr = io::stderr().lock();
                let _ = writeln!(stderr, "bongocat: {failure}");
            }
        }
        // `exec` replaces the process image and only returns on failure, so reaching
        // the next line means the new build could not be started.
        let _ = bongocat_update::UpdateRuntime::for_current_build(
            bongocat_app::BUILD_ENVIRONMENT,
            bongocat_app::PRODUCT_VERSION,
            bongocat_update::UpdateDiagnosticsTracker::default(),
        )
        .restart();
        let mut stderr = io::stderr().lock();
        let _ = writeln!(
            stderr,
            "bongocat: the updated build was installed but could not be started"
        );
        std::process::exit(1);
    })
    .detach();
}

#[cfg(target_os = "windows")]
pub(crate) fn apply_taskbar_icon_visibility(
    cx: &mut App,
    visible: bool,
) -> Result<(), SettingsError> {
    if !cx.has_global::<ProductCoordinator>() {
        return Err(SettingsError::new(
            SettingsErrorCode::TaskbarIconUpdateFailed,
        ));
    }
    let Some(window_handle) = cx
        .try_global::<ProductCoordinator>()
        .and_then(|coordinator| coordinator.settings_window.clone())
    else {
        // The settings window is intentionally destroyed on close. The native
        // taskbar button does not exist while it is absent; retain the desired
        // value and apply it when the next window is created.
        cx.global_mut::<ProductCoordinator>().taskbar_icon_visible = visible;
        return Ok(());
    };
    let result = window_handle
        .update(cx, |_, window, _| {
            bongocat_platform::set_taskbar_icon_visible(window, visible)
        })
        .map_err(|_| SettingsError::new(SettingsErrorCode::TaskbarIconUpdateFailed))?
        .map_err(|_| SettingsError::new(SettingsErrorCode::TaskbarIconUpdateFailed));
    if result.is_err() && window_handle.is_open() {
        return Err(SettingsError::new(
            SettingsErrorCode::TaskbarIconUpdateFailed,
        ));
    }
    cx.global_mut::<ProductCoordinator>().taskbar_icon_visible = visible;
    Ok(())
}

#[cfg(target_os = "windows")]
pub(crate) fn product_taskbar_icon_state(cx: &mut App) -> Result<(bool, bool), String> {
    let window_handle = cx
        .try_global::<ProductCoordinator>()
        .and_then(|coordinator| coordinator.settings_window.clone())
        .ok_or_else(|| "settings window is unavailable".to_owned())?;
    window_handle
        .update(cx, |view, window, _| {
            bongocat_platform::taskbar_icon_is_visible(window)
                .map(|visible| (visible, view.window_hidden()))
                .map_err(|error| error.to_string())
        })
        .map_err(|error| error.to_string())?
}

pub(crate) fn product_overlay_state(cx: &mut App) -> Result<(u64, bool), String> {
    let coordinator = cx
        .try_global::<ProductCoordinator>()
        .ok_or_else(|| "product coordinator is unavailable".to_owned())?;
    #[cfg(target_os = "macos")]
    let overlay = coordinator
        .overlay
        .as_ref()
        .ok_or_else(|| "product overlay is unavailable".to_owned())?;
    #[cfg(target_os = "windows")]
    let overlay = coordinator.overlay.borrow();
    #[cfg(target_os = "windows")]
    let overlay = overlay
        .as_ref()
        .ok_or_else(|| "product overlay is unavailable".to_owned())?;
    Ok((overlay.model_generation(), overlay.is_visible()))
}

pub(crate) fn toggle_settings_window(cx: &mut App) -> Result<(), String> {
    let existing = cx
        .try_global::<ProductCoordinator>()
        .and_then(|coordinator| coordinator.settings_window.clone());
    let Some(window_handle) = existing else {
        ensure_settings_window(cx)?;
        return Ok(());
    };

    // Closing settings destroys the current GPUI window. The coordinator clears
    // the handle from `on_window_closed`; the process-local navigation memory
    // remains and is supplied to the next window.
    match window_handle.update(cx, |view, window, cx| view.close(window, cx)) {
        Ok(result) => result?,
        Err(_) => {
            if !window_handle.is_open() {
                cx.global_mut::<ProductCoordinator>().settings_window = None;
                return Ok(());
            }
            return Err("settings window is temporarily unavailable to close".to_owned());
        }
    }

    Ok(())
}

pub(crate) fn handle_shortcut_toggle_settings(cx: &mut App) {
    let requested = cx
        .try_global::<ProductCoordinator>()
        .is_some_and(|coordinator| coordinator.main_thread_signals.take_open_settings_request());
    if !requested {
        return;
    }
    if let Err(error) = toggle_settings_window(cx)
        && let Some(failures) = cx
            .try_global::<ProductCoordinator>()
            .map(|coordinator| Arc::clone(&coordinator.failures))
    {
        record_failure(&failures, error);
    }
}
