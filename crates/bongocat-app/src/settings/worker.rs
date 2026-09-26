//! The settings worker thread: one blocking receive, one typed command, one reply.
//!
//! The loop is deliberately a single match over `SettingsCommand`. Every arm checks
//! the configuration revision the caller saw, applies exactly one change and
//! answers with a fresh snapshot, so the ordering guarantees the settings window
//! depends on stay visible in one place instead of being spread across handlers.

// The settings vocabulary. `super` already imports every type this module's code
// names; what follows are the sibling modules whose values it reads.
use super::*;

use super::capabilities::*;
use super::error_mapping::*;
use super::model_projection::*;
use super::projection::*;
use super::snapshot::*;

#[allow(clippy::too_many_arguments)]
pub(super) fn run_service(
    mut application: Application,
    endpoint: SettingsServiceEndpoint,
    startup_item: Arc<dyn StartupItemCapability>,
    visibility: VisibilityCapabilities,
    backup_location: Arc<dyn BackupLocationCapability>,
    diagnostics_export: Arc<dyn DiagnosticsExportCapability>,
    model_location: Arc<dyn ModelLocationCapability>,
    log_location: Arc<dyn LogLocationCapability>,
    window_state: SettingsWindowState,
    signals: Option<ApplicationMainThreadSignals>,
) {
    let mut clock = SettingsSnapshotClock::new(application.config_revision());
    loop {
        let Ok(command) = endpoint.recv_blocking() else {
            if persist_window_state(&mut application, &window_state).is_err() {
                application.record_log_once(
                    ApplicationLogEvent::new(ApplicationLogCode::StatePersistFailed)
                        .with_context(ApplicationLogContext::State("window_state"))
                        .with_context(ApplicationLogContext::Operation("service_shutdown"))
                        .with_context(ApplicationLogContext::Reason("window_state_persist_failed")),
                );
            }
            application.record_log_once(
                ApplicationLogEvent::new(ApplicationLogCode::UiTransportFailed)
                    .with_context(ApplicationLogContext::Operation("settings_endpoint")),
            );
            let _ = application.shutdown();
            break;
        };
        match command {
            SettingsCommand::SettingsWindowPlacementChanged => {
                if persist_window_state(&mut application, &window_state).is_err() {
                    application.record_log_once(
                        ApplicationLogEvent::new(ApplicationLogCode::StatePersistFailed)
                            .with_context(ApplicationLogContext::State("window_state"))
                            .with_context(ApplicationLogContext::Operation("persist"))
                            .with_context(ApplicationLogContext::Reason(
                                "window_state_persist_failed",
                            )),
                    );
                } else {
                    application.record_log_once(
                        ApplicationLogEvent::new(ApplicationLogCode::WindowStatePersisted)
                            .with_context(ApplicationLogContext::Operation("settings")),
                    );
                }
            }
            SettingsCommand::OverlayWindowPlacementChanged {
                x,
                y,
                width,
                height,
            } => match OverlayWindowPlacement::new(x, y, width, height) {
                Ok(placement) => {
                    if application
                        .persist_overlay_window_placement(placement)
                        .is_err()
                    {
                        application.record_log_once(
                            ApplicationLogEvent::new(ApplicationLogCode::StatePersistFailed)
                                .with_context(ApplicationLogContext::State("window_state"))
                                .with_context(ApplicationLogContext::Operation("persist_overlay"))
                                .with_context(ApplicationLogContext::Reason(
                                    "window_state_persist_failed",
                                )),
                        );
                    } else {
                        application.record_log_once(
                            ApplicationLogEvent::new(ApplicationLogCode::WindowStatePersisted)
                                .with_context(ApplicationLogContext::Operation("overlay")),
                        );
                    }
                }
                Err(_) => application.record_log_once(
                    ApplicationLogEvent::new(ApplicationLogCode::ParsingFailed)
                        .with_context(ApplicationLogContext::Operation("overlay_placement"))
                        .with_context(ApplicationLogContext::Reason("invalid_window_placement")),
                ),
            },
            SettingsCommand::TriggerApplicationShortcut { command } => {
                if let Err(error) = apply_application_shortcut(&mut application, command) {
                    application.record_log_once(
                        ApplicationLogEvent::new(ApplicationLogCode::RuntimeUnavailable)
                            .with_context(ApplicationLogContext::Operation("application_shortcut")),
                    );
                    let _ = error;
                }
            }
            SettingsCommand::ReadSnapshot { reply } => {
                let _ = reply.respond(Ok(snapshot(
                    &application,
                    &mut clock,
                    false,
                    startup_item.state(),
                )));
            }
            // The application polls this from its system menu loop. It answers "did anything
            // change?" and stops there on purpose: building the snapshot also scans the model
            // catalog, so polling the whole thing twenty times a second spends milliseconds of
            // filesystem work per tick on a value the poller only compares for equality.
            SettingsCommand::ReadSnapshotRevision { reply } => {
                let _ =
                    observe_snapshot_state(&application, &mut clock, startup_item.state(), false);
                let _ = reply.respond(clock.revision);
            }
            SettingsCommand::ReadAutomaticUpdateSettings { reply } => {
                let application_config = &application.config().updates;
                let _ = reply.respond(Ok(AutomaticUpdateSettings {
                    enabled: application_config.check_automatically,
                    interval_hours: application_config.check_interval_hours,
                }));
            }
            SettingsCommand::SetOverlayVisible {
                expected_config_revision,
                visible,
                reply,
            } => {
                let result = check_revision(&application, expected_config_revision)
                    .and_then(|()| {
                        application
                            .set_overlay_visible(visible)
                            .map(|_| ())
                            .map_err(map_application_error)
                    })
                    .map(|_| snapshot(&application, &mut clock, false, startup_item.state()));
                match &result {
                    Ok(_) => application.record_log(
                        ApplicationLogEvent::new(ApplicationLogCode::WindowVisibilityChanged)
                            .with_context(ApplicationLogContext::Result(if visible {
                                "visible"
                            } else {
                                "hidden"
                            })),
                    ),
                    Err(error) => application.record_log_once(
                        ApplicationLogEvent::new(ApplicationLogCode::SettingsCommandFailed)
                            .with_context(ApplicationLogContext::Operation("overlay_visibility"))
                            .with_context(ApplicationLogContext::Reason(error.code().as_str())),
                    ),
                }
                let _ = reply.respond(result);
            }
            SettingsCommand::SetAppearanceTheme {
                expected_config_revision,
                theme,
                reply,
            } => {
                let result = check_revision(&application, expected_config_revision)
                    .and_then(|()| {
                        application
                            .set_appearance_theme(config_theme(theme))
                            .map_err(map_application_error)
                    })
                    .map(|_| snapshot(&application, &mut clock, false, startup_item.state()));
                let _ = reply.respond(result);
            }
            SettingsCommand::SetLanguage {
                expected_config_revision,
                language,
                reply,
            } => {
                let result = check_revision(&application, expected_config_revision)
                    .and_then(|()| {
                        application
                            .set_language(config_language(language))
                            .map_err(map_application_error)
                    })
                    .map(|_| snapshot(&application, &mut clock, false, startup_item.state()));
                let _ = reply.respond(result);
            }
            SettingsCommand::SetStatusIconVisible {
                expected_config_revision,
                visible,
                reply,
            } => {
                let result = check_revision(&application, expected_config_revision)
                    .and_then(|()| {
                        let previous = application.config().system.show_status_icon;
                        if previous == visible {
                            return Ok(());
                        }
                        visibility.status_icon.set_visible(visible)?;
                        if let Err(error) = application.set_status_icon_visible(visible) {
                            if visibility.status_icon.set_visible(previous).is_err() {
                                return Err(SettingsError::new(
                                    SettingsErrorCode::StatusIconUpdateFailed,
                                ));
                            }
                            return Err(map_application_error(error));
                        }
                        Ok(())
                    })
                    .map(|_| snapshot(&application, &mut clock, false, startup_item.state()));
                let _ = reply.respond(result);
            }
            SettingsCommand::SetTaskbarIconVisible {
                expected_config_revision,
                visible,
                reply,
            } => {
                let result = check_revision(&application, expected_config_revision)
                    .and_then(|()| {
                        let previous = application.config().system.show_taskbar_icon;
                        if previous == visible {
                            return Ok(());
                        }
                        visibility.taskbar_icon.set_visible(visible)?;
                        if let Err(error) = application.set_taskbar_icon_visible(visible) {
                            if visibility.taskbar_icon.set_visible(previous).is_err() {
                                return Err(SettingsError::new(
                                    SettingsErrorCode::TaskbarIconUpdateFailed,
                                ));
                            }
                            return Err(map_application_error(error));
                        }
                        Ok(())
                    })
                    .map(|_| snapshot(&application, &mut clock, false, startup_item.state()));
                let _ = reply.respond(result);
            }
            SettingsCommand::SetCheckForUpdatesAutomatically {
                expected_config_revision,
                enabled,
                reply,
            } => {
                let result = check_revision(&application, expected_config_revision)
                    .and_then(|()| {
                        application
                            .set_check_for_updates_automatically(enabled)
                            .map_err(map_application_error)
                    })
                    .map(|_| snapshot(&application, &mut clock, false, startup_item.state()));
                let _ = reply.respond(result);
            }
            SettingsCommand::SetCheckForUpdatesIntervalHours {
                expected_config_revision,
                interval_hours,
                reply,
            } => {
                let result = check_revision(&application, expected_config_revision)
                    .and_then(|()| {
                        application
                            .set_check_for_updates_interval_hours(interval_hours)
                            .map_err(map_application_error)
                    })
                    .map(|_| snapshot(&application, &mut clock, false, startup_item.state()));
                let _ = reply.respond(result);
            }
            SettingsCommand::SetOverlaySettings {
                expected_config_revision,
                settings,
                reply,
            } => {
                let runtime_settings = OverlaySettings {
                    click_through: settings.click_through,
                    always_on_top: settings.always_on_top,
                    scale_percent: settings.scale_percent,
                    opacity_percent: settings.opacity_percent,
                    corner_radius_percent: settings.corner_radius_percent,
                    hide_on_pointer_hover: settings.hide_on_pointer_hover,
                    hide_on_pointer_hover_delay_seconds: settings
                        .hide_on_pointer_hover_delay_seconds,
                    keep_inside_screen: settings.keep_inside_screen,
                };
                let result = check_revision(&application, expected_config_revision)
                    .and_then(|()| {
                        application
                            .set_overlay_settings(runtime_settings)
                            .map(|_| ())
                            .map_err(map_application_error)
                    })
                    .map(|_| snapshot(&application, &mut clock, false, startup_item.state()));
                let _ = reply.respond(result);
            }
            SettingsCommand::SetMotionAudioEnabled {
                expected_config_revision,
                enabled,
                reply,
            } => {
                let result = check_revision(&application, expected_config_revision)
                    .and_then(|()| {
                        application
                            .set_motion_audio_enabled(enabled)
                            .map(|_| ())
                            .map_err(map_application_error)
                    })
                    .map(|_| snapshot(&application, &mut clock, false, startup_item.state()));
                let _ = reply.respond(result);
            }
            SettingsCommand::SetBehaviorShortcutsEnabled {
                expected_config_revision,
                enabled,
                reply,
            } => {
                let result = check_revision(&application, expected_config_revision)
                    .and_then(|()| {
                        application
                            .set_behavior_shortcuts_enabled(enabled)
                            .map(|_| ())
                            .map_err(map_application_error)
                    })
                    .map(|_| snapshot(&application, &mut clock, false, startup_item.state()));
                let _ = reply.respond(result);
            }
            SettingsCommand::SetCommandShortcutsEnabled {
                expected_config_revision,
                enabled,
                reply,
            } => {
                let result = check_revision(&application, expected_config_revision)
                    .and_then(|()| {
                        application
                            .set_command_shortcuts_enabled(enabled)
                            .map(|_| ())
                            .map_err(map_application_error)
                    })
                    .map(|_| snapshot(&application, &mut clock, false, startup_item.state()));
                let _ = reply.respond(result);
            }
            SettingsCommand::SetMaximumFps {
                expected_config_revision,
                maximum_fps,
                reply,
            } => {
                let result = check_revision(&application, expected_config_revision)
                    .and_then(|()| {
                        application
                            .set_maximum_fps(maximum_fps)
                            .map(|_| ())
                            .map_err(map_application_error)
                    })
                    .map(|_| snapshot(&application, &mut clock, false, startup_item.state()));
                let _ = reply.respond(result);
            }
            SettingsCommand::SetReleaseFallbackTimeout {
                expected_config_revision,
                timeout_ms,
                reply,
            } => {
                let result = check_revision(&application, expected_config_revision)
                    .and_then(|()| {
                        application
                            .set_release_fallback_timeout(timeout_ms)
                            .map(|_| ())
                            .map_err(map_application_error)
                    })
                    .map(|_| snapshot(&application, &mut clock, false, startup_item.state()));
                let _ = reply.respond(result);
            }
            SettingsCommand::SetRandomBehaviorSettings {
                expected_config_revision,
                settings,
                reply,
            } => {
                let runtime_settings = RandomBehaviorSettings {
                    enabled: settings.enabled,
                    interval_seconds: settings.interval_seconds,
                };
                let result = check_revision(&application, expected_config_revision)
                    .and_then(|()| {
                        application
                            .set_random_behavior_settings(runtime_settings)
                            .map(|_| ())
                            .map_err(map_application_error)
                    })
                    .map(|_| snapshot(&application, &mut clock, false, startup_item.state()));
                let _ = reply.respond(result);
            }
            SettingsCommand::SetModelSettings {
                expected_config_revision,
                settings,
                reply,
            } => {
                let runtime_settings = ModelSettings {
                    mirror: settings.mirror,
                    mirror_pointer_tracking: settings.mirror_pointer_tracking,
                    ignore_keyboard: settings.ignore_keyboard,
                    ignore_gamepad: settings.ignore_gamepad,
                    ignore_pointer: settings.ignore_pointer,
                };
                let result = check_revision(&application, expected_config_revision)
                    .and_then(|()| {
                        application
                            .set_model_settings(runtime_settings)
                            .map(|_| ())
                            .map_err(map_application_error)
                    })
                    .map(|_| snapshot(&application, &mut clock, false, startup_item.state()));
                let _ = reply.respond(result);
            }
            SettingsCommand::SetGamepadAxisSettings {
                expected_config_revision,
                settings,
                reply,
            } => {
                let result = check_revision(&application, expected_config_revision)
                    .and_then(|()| {
                        let runtime_settings = bongocat_input::GamepadAxisSettings::new(
                            f32::from(settings.stick_dead_zone_percent.min(99)) / 100.0,
                            f32::from(settings.trigger_dead_zone_percent.min(99)) / 100.0,
                        )
                        .expect("bounded gamepad percentages are below 100");
                        application
                            .set_gamepad_axis_settings(runtime_settings)
                            .map(|_| ())
                            .map_err(map_application_error)
                    })
                    .map(|_| snapshot(&application, &mut clock, false, startup_item.state()));
                let _ = reply.respond(result);
            }
            SettingsCommand::SetGamepadAutoSwitch {
                expected_config_revision,
                settings,
                reply,
            } => {
                let result = check_revision(&application, expected_config_revision)
                    .and_then(|()| {
                        application
                            .set_gamepad_auto_switch(settings)
                            .map_err(map_application_error)
                    })
                    .map(|_| snapshot(&application, &mut clock, false, startup_item.state()));
                let _ = reply.respond(result);
            }
            SettingsCommand::GamepadConnectionChanged => {
                if let Err(error) = application.apply_gamepad_auto_switch() {
                    // The current model stays on screen and the reason is
                    // anonymous, exactly as it is for a selection the user made.
                    application.record_log_once(
                        ApplicationLogEvent::new(ApplicationLogCode::ModelActivationFailed)
                            .with_context(ApplicationLogContext::Operation("gamepad_auto_switch"))
                            .with_context(ApplicationLogContext::Reason(error.stable_code())),
                    );
                }
            }
            SettingsCommand::SetLoggingSettings {
                expected_config_revision,
                settings,
                reply,
            } => {
                let result = check_revision(&application, expected_config_revision)
                    .and_then(|()| {
                        application
                            .set_logging_settings(settings)
                            .map_err(map_application_error)
                    })
                    .map(|_| snapshot(&application, &mut clock, false, startup_item.state()));
                if let Err(error) = &result {
                    application.record_log_once(
                        ApplicationLogEvent::new(ApplicationLogCode::SettingsCommandFailed)
                            .with_context(ApplicationLogContext::Operation("logging_settings"))
                            .with_context(ApplicationLogContext::Reason(error.code().as_str())),
                    );
                }
                let _ = reply.respond(result);
            }
            SettingsCommand::SetShortcuts {
                expected_config_revision,
                shortcuts,
                reply,
            } => {
                let result =
                    check_revision(&application, expected_config_revision).and_then(|()| {
                        application
                            .set_shortcuts(shortcuts)
                            .map(|_| ())
                            .map_err(map_application_error)
                    });
                if result.is_err() {
                    let _ = application.resume_shortcut_capture();
                }
                let result =
                    result.map(|_| snapshot(&application, &mut clock, false, startup_item.state()));
                let _ = reply.respond(result);
            }
            SettingsCommand::SuspendShortcutCapture {
                expected_config_revision,
                shortcuts_without_capture_target,
                reply,
            } => {
                let result = check_revision(&application, expected_config_revision)
                    .and_then(|()| {
                        application
                            .suspend_shortcut_capture(shortcuts_without_capture_target)
                            .map_err(map_application_error)
                    })
                    .map(|()| snapshot(&application, &mut clock, false, startup_item.state()));
                let _ = reply.respond(result);
            }
            SettingsCommand::ResumeShortcutCapture { reply } => {
                let result = application
                    .resume_shortcut_capture()
                    .map_err(map_application_error)
                    .map(|()| snapshot(&application, &mut clock, false, startup_item.state()));
                let _ = reply.respond(result);
            }
            SettingsCommand::SetStartupItemEnabled { enabled, reply } => {
                let result = startup_item.set_enabled(enabled).map(|state| {
                    snapshot(
                        &application,
                        &mut clock,
                        false,
                        SettingsStartupItemStatus::State(state),
                    )
                });
                let _ = reply.respond(result);
            }
            SettingsCommand::SelectModel {
                expected_config_revision,
                model,
                reply,
            } => {
                let result = check_revision(&application, expected_config_revision)
                    .and_then(|()| {
                        application
                            .select_model(model_origin_from_settings(model.origin), model.id)
                            .map(|_| ())
                            .map_err(map_application_error)
                    })
                    .map(|_| snapshot(&application, &mut clock, false, startup_item.state()));
                let _ = reply.respond(result);
            }
            SettingsCommand::PreviewModelBehavior {
                model,
                behavior,
                reply,
            } => {
                let result = preview_model_behavior(&application, &model, behavior)
                    .map(|()| snapshot(&application, &mut clock, false, startup_item.state()));
                let _ = reply.respond(result);
            }
            SettingsCommand::SetModelTitle {
                expected_config_revision,
                model,
                title,
                reply,
            } => {
                let result = check_revision(&application, expected_config_revision)
                    .and_then(|()| {
                        application
                            .set_model_title(
                                model_origin_from_settings(model.origin),
                                model.id,
                                title,
                            )
                            .map_err(map_model_metadata_error)
                    })
                    .map(|()| snapshot(&application, &mut clock, false, startup_item.state()));
                let _ = reply.respond(result);
            }
            SettingsCommand::SetModelCover {
                model,
                source,
                reply,
            } => {
                let result = application
                    .set_model_cover(model_origin_from_settings(model.origin), model.id, source)
                    .map(|_| ())
                    .map_err(map_model_cover_error)
                    .map(|()| snapshot(&application, &mut clock, false, startup_item.state()));
                let _ = reply.respond(result);
            }
            SettingsCommand::ReplaceModelCover { model, png, reply } => {
                // The capture that produced these bytes already knows the model is
                // installed, so a rejection here means the model disappeared between
                // the import that queued the capture and the write, or that the
                // encoder produced something the package contract refuses.
                let result = application
                    .set_model_cover_bytes(model_origin_from_settings(model.origin), model.id, &png)
                    .map(|_| ())
                    .map_err(map_model_cover_error)
                    .map(|()| snapshot(&application, &mut clock, false, startup_item.state()));
                let _ = reply.respond(result);
            }
            SettingsCommand::OpenModelLocation { model, reply } => {
                let result = match application
                    .model_directory(model_origin_from_settings(model.origin), &model.id)
                {
                    Some(directory) => model_location.open(&directory),
                    None => Err(SettingsError::new(
                        SettingsErrorCode::ModelLocationOpenFailed,
                    )),
                }
                .map(|()| snapshot(&application, &mut clock, false, startup_item.state()));
                let _ = reply.respond(result);
            }
            SettingsCommand::InspectModelSource { source_root, reply } => {
                let result = application
                    .inspect_model_source(source_root)
                    .map_err(map_model_import_error);
                let _ = reply.respond(result);
            }
            SettingsCommand::ImportModel {
                request,
                operation,
                reply,
            } => {
                let progress = operation.clone();
                let cancellation = operation.clone();
                let result = application
                    .import_models_with_selected_modes_with_observer(
                        request.title,
                        request.source_root,
                        request
                            .selected_mver_modes
                            .into_iter()
                            .map(model_mver_input_mode)
                            .collect(),
                        move |update| {
                            let _ = progress.report_progress(settings_import_progress(update));
                        },
                        move || cancellation.is_cancelled(),
                    )
                    .map(|installed| {
                        // A freshly imported model gets a cover rendered from the
                        // model itself rather than the one its source shipped,
                        // which for a converted BongoCatMver model is the same
                        // placeholder in every mode. The worker only queues the
                        // work: rendering it needs a native window, and this thread
                        // does not own one.
                        if let Some(signals) = signals.as_ref() {
                            for model in installed {
                                let key = SettingsModelKey {
                                    id: model.id().as_str().to_owned(),
                                    origin: settings_origin_from_model(ModelOrigin::Installed),
                                };
                                signals
                                    .request_model_cover_capture(key, CommittedModel::from(model));
                            }
                        }
                    })
                    .map(|()| snapshot(&application, &mut clock, true, startup_item.state()))
                    .map_err(map_model_import_error);
                let _ = reply.respond(result);
            }
            SettingsCommand::DeleteModel { model, reply } => {
                let result = application
                    .delete_model(model_origin_from_settings(model.origin), model.id)
                    .map(|_| snapshot(&application, &mut clock, true, startup_item.state()))
                    .map_err(map_model_delete_error);
                let _ = reply.respond(result);
            }
            SettingsCommand::OpenConfigBackupLocation { reply } => {
                let result = backup_location
                    .open()
                    .map(|()| snapshot(&application, &mut clock, false, startup_item.state()));
                let _ = reply.respond(result);
            }
            SettingsCommand::ExportDiagnostics { reply } => {
                let result = {
                    let current = snapshot(&application, &mut clock, false, startup_item.state());
                    diagnostics_export
                        .export(
                            &current,
                            application.application_log_diagnostics(),
                            application.core_log_diagnostics(),
                            application.update_diagnostics(),
                        )
                        .map(|status| {
                            clock.observe_diagnostics_export(status);
                            snapshot(&application, &mut clock, false, startup_item.state())
                        })
                };
                if result.is_err() {
                    application.record_log(
                        ApplicationLogEvent::new(ApplicationLogCode::DiagnosticsExportFailed)
                            .with_context(ApplicationLogContext::Operation("export")),
                    );
                }
                let _ = reply.respond(result);
            }
            SettingsCommand::OpenLogsLocation { reply } => {
                let result = log_location
                    .open()
                    .map(|()| snapshot(&application, &mut clock, false, startup_item.state()));
                let _ = reply.respond(result);
            }
            SettingsCommand::Shutdown { reply } => {
                let before_shutdown =
                    snapshot(&application, &mut clock, false, startup_item.state());
                let state_result = persist_window_state(&mut application, &window_state);
                let shutdown_result = application.shutdown();
                let result = match (state_result, shutdown_result) {
                    (Ok(()), Ok(stopped)) => {
                        clock.mark_changed();
                        let mut stopped_snapshot = SettingsSnapshot {
                            revision: clock.revision,
                            runtime_health: RuntimeHealth::Stopped,
                            ..before_shutdown
                        };
                        stopped_snapshot.runtime_diagnostics =
                            settings_runtime_diagnostics(&stopped);
                        stopped_snapshot.input_diagnostics = settings_input_diagnostics(
                            &stopped.input,
                            stopped.platform_input,
                            clock.input_monitoring_permission(),
                        );
                        Ok(stopped_snapshot)
                    }
                    (Err(_), Ok(_)) => Err(SettingsError::new(
                        SettingsErrorCode::WindowStatePersistFailed,
                    )),
                    (_, Err(_)) => Err(SettingsError::new(SettingsErrorCode::ShutdownFailed)),
                };
                let _ = reply.respond(result);
                break;
            }
        }
    }
}

pub(super) fn settings_window_placement(
    placement: WindowPlacement,
) -> Option<SettingsWindowPlacement> {
    SettingsWindowPlacement::new(
        placement.x,
        placement.y,
        placement.width,
        placement.height,
        placement.maximized,
    )
}

pub(super) fn persist_window_state(
    application: &mut Application,
    window_state: &SettingsWindowState,
) -> Result<(), ApplicationError> {
    let placement = window_state
        .placement()
        .map(|placement| {
            WindowPlacement::new(
                placement.x,
                placement.y,
                placement.width,
                placement.height,
                placement.maximized,
            )
        })
        .transpose()?;
    match application.persist_settings_window_placement(placement) {
        Err(ApplicationError::WindowState(WindowStateError::UnsupportedSchema(_))) => Ok(()),
        result => result,
    }
}
