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
use bongocat_plugin::{PluginCommand, PluginId};
use std::collections::BTreeMap;

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
    plugins: Option<PluginWorkerReader>,
) {
    let mut clock = SettingsSnapshotClock::new(application.config_revision(), plugins);
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
            //
            // It is also the one thing that runs for the whole product lifetime without
            // the user having done anything, which is what the remembered per-model
            // expression needs: a shortcut reaches the runtime directly, so nothing the
            // user does in the settings window announces it. The common answer is a
            // sequence comparison and no write.
            SettingsCommand::ReadSnapshotRevision { reply } => {
                application.persist_user_expression_memory();
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
                let result =
                    check_revision(&application, expected_config_revision).and_then(|()| {
                        application
                            .set_language(config_language(language))
                            .map_err(map_application_error)
                    });
                if result.is_ok() {
                    // A plugin resolves its own strings against the locale the host
                    // hands it, so a user who switches language expects the panels
                    // beside the cat to switch with them. Sent here rather than only at
                    // startup because the worker outlives a language change.
                    send_plugin_locale(
                        &clock,
                        &settings_language(application.effective_language()),
                    );
                }
                let _ = reply.respond(
                    result
                        .map(|()| snapshot(&application, &mut clock, false, startup_item.state())),
                );
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
            SettingsCommand::SetDockIconVisible {
                expected_config_revision,
                visible,
                reply,
            } => {
                let result = check_revision(&application, expected_config_revision)
                    .and_then(|()| {
                        let previous = application.config().system.show_dock_icon;
                        if previous == visible {
                            return Ok(());
                        }
                        visibility.dock_icon.set_visible(visible)?;
                        if let Err(error) = application.set_dock_icon_visible(visible) {
                            if visibility.dock_icon.set_visible(previous).is_err() {
                                return Err(SettingsError::new(
                                    SettingsErrorCode::DockIconUpdateFailed,
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
            SettingsCommand::SetRandomBehaviorSettings {
                expected_config_revision,
                settings,
                reply,
            } => {
                let runtime_settings = RandomBehaviorSettings {
                    mode: random_behavior_mode_to_runtime_settings(settings.mode),
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
                    mirror_pointer_tracking_horizontal: settings.mirror_pointer_tracking_horizontal,
                    mirror_pointer_tracking_vertical: settings.mirror_pointer_tracking_vertical,
                    ignore_keyboard: settings.ignore_keyboard,
                    ignore_gamepad: settings.ignore_gamepad,
                    show_all_pressed_keys: settings.show_all_pressed_keys,
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
            SettingsCommand::SetRememberLastExpression {
                expected_config_revision,
                enabled,
                reply,
            } => {
                let result = check_revision(&application, expected_config_revision)
                    .and_then(|()| {
                        application
                            .set_remember_last_expression(enabled)
                            .map_err(map_application_error)
                    })
                    .map(|_| snapshot(&application, &mut clock, false, startup_item.state()));
                if let Err(error) = &result {
                    application.record_log_once(
                        ApplicationLogEvent::new(ApplicationLogCode::SettingsCommandFailed)
                            .with_context(ApplicationLogContext::Operation(
                                "remember_last_expression",
                            ))
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
            SettingsCommand::RefreshPluginCatalog => {
                // No revision guard and no reply: reading a catalog changes no
                // configuration, and the answer is the snapshot the worker publishes,
                // which the revision poll already watches.
                let _ = send_plugin_command(&clock, PluginCommand::RefreshCatalog);
            }
            SettingsCommand::InstallPlugin { plugin, reply } => {
                let result = with_plugin_id(&clock, &plugin, |id| {
                    send_plugin_command(&clock, PluginCommand::Install(id))
                })
                .map(|()| snapshot(&application, &mut clock, false, startup_item.state()));
                let _ = reply.respond(result);
            }
            SettingsCommand::UninstallPlugin { plugin, reply } => {
                let result = with_plugin_id(&clock, &plugin, |id| {
                    send_plugin_command(&clock, PluginCommand::Uninstall(id))
                })
                .map(|()| snapshot(&application, &mut clock, false, startup_item.state()));
                let _ = reply.respond(result);
            }
            SettingsCommand::SetPluginEnabled {
                plugin,
                enabled,
                reply,
            } => {
                // Configuration first, then the worker: a persisted switch with no
                // panel is a switch the next launch honours, while a panel with no
                // persisted switch is one the user has to turn off again every run.
                let result = with_plugin_id(&clock, &plugin, |id| {
                    application
                        .set_plugin_enabled(id.as_str(), enabled)
                        .map_err(map_plugin_error)?;
                    send_plugin_command(&clock, PluginCommand::SetEnabled { id, enabled })
                })
                .map(|()| snapshot(&application, &mut clock, false, startup_item.state()));
                let _ = reply.respond(result);
            }
            SettingsCommand::SetPluginConfig {
                plugin,
                config,
                reply,
            } => {
                // The document travels to the plugin, which writes its own file. The
                // host checks each value against the field the plugin declared and the
                // plugin decides what it means — so nothing here interprets a value,
                // and `config.json` has no plugin section to drift out of step.
                //
                // No snapshot is built, and that is the point of this command not having
                // one: the page is drawing the draft it already holds, and the answer it
                // will show next comes from the plugin's own reply through the revision
                // poll. Building one would have walked the model store on disk for every
                // keystroke in a plugin's settings form.
                let result =
                    with_plugin_id(&clock, &plugin, |id| send_plugin_config(&clock, id, config));
                let _ = reply.respond(result);
            }
            SettingsCommand::SetPluginPosition {
                plugin,
                position,
                reply,
            } => {
                // Configuration first, then the worker, for the same reason the enabled
                // switch does it in this order: a position the user chose and the window
                // forgets is one they chose twice, while a panel that moved and the file did
                // not is one that moves back on the next launch.
                //
                // The window is told only whether the move was made. The position the
                // plugin actually got is on the snapshot it polls for, which is also the
                // only place it could be drawn from — a press and a whole model-store scan
                // to learn something the next poll carries anyway.
                let result = with_plugin_id(&clock, &plugin, |id| {
                    application
                        .set_plugin_position(id.as_str(), Some(&position))
                        .map_err(map_plugin_error)?;
                    send_plugin_position(&clock, id, &position)
                });
                let _ = reply.respond(result);
            }
            SettingsCommand::ClearPluginPosition { plugin, reply } => {
                let result = with_plugin_id(&clock, &plugin, |id| {
                    application
                        .set_plugin_position(id.as_str(), None)
                        .map_err(map_plugin_error)?;
                    // Nothing is sent to the worker: forgetting is the absence of a
                    // preference, and the worker already answers every request it makes
                    // with the position it actually got — which is the plugin's own corner
                    // again the moment the preference is gone.
                    Ok(())
                });
                let _ = reply.respond(result);
            }
            SettingsCommand::PressPluginAction {
                plugin,
                action,
                reply,
            } => {
                // The id is the plugin's own and the host checks it against what the
                // plugin is currently offering — a button on a stale snapshot is a click
                // that did not register, which is reported as a refusal rather than
                // delivered as a press the plugin cannot interpret.
                let result = with_plugin_id(&clock, &plugin, |id| {
                    press_plugin_action(&clock, id, &action)
                })
                .map(|()| snapshot(&application, &mut clock, false, startup_item.state()));
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
                            clock.input_capability(),
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

/// Send one command to the plugin worker, reporting a host that is not running.
///
/// A dropped command is a settings error rather than a silent no-op: the window's
/// button would otherwise appear to do nothing at all, which is the failure mode
/// this exists to avoid.
pub(super) fn send_plugin_command(
    clock: &SettingsSnapshotClock,
    command: PluginCommand,
) -> Result<(), SettingsError> {
    clock
        .plugin_reader()
        .ok_or_else(|| SettingsError::new(SettingsErrorCode::PluginHostUnavailable))
        .and_then(|reader| {
            if reader.send(command) {
                Ok(())
            } else {
                Err(SettingsError::new(SettingsErrorCode::PluginHostBusy))
            }
        })
}

/// Move one plugin's panel, with the position named the way the protocol names it.
///
/// The name is checked here as well as in the configuration, because the two are the same
/// fact in two places: the window sends what the menu it drew carried, and a menu from a
/// stale snapshot could name a position this build has no corner for. Refused at the press
/// rather than written, so a press that could not happen does not leave a file behind.
pub(super) fn send_plugin_position(
    clock: &SettingsSnapshotClock,
    id: PluginId,
    position: &str,
) -> Result<(), SettingsError> {
    let Some(anchor) = bongocat_plugin::parse_anchor(position) else {
        return Err(SettingsError::new(SettingsErrorCode::PluginNotFound));
    };
    send_plugin_command(clock, PluginCommand::SetPosition { id, anchor })
}

/// Tell every plugin which language the user now reads.
///
/// Best-effort in the strict sense: a queue that is full means the worker is busy
/// with something the user asked for first, and the next tick carries the locale
/// anyway — so a dropped command costs a panel one tick of English, not a panel stuck
/// in the wrong language. Failing the language change over it would be worse than the
/// panel the user is already looking at.
fn send_plugin_locale(clock: &SettingsSnapshotClock, language: &SettingsLanguage) {
    let Some(reader) = clock.plugin_reader() else {
        return;
    };
    let _ = reader.send(PluginCommand::SetLocale {
        locale: language.catalog_locale().to_string(),
    });
}

/// Hand one plugin the settings the user just changed.
///
/// The values are fitted to the schema the plugin *running* declared before they go
/// out, so the host's check is made against the same field the window drew rather than
/// against the archive's metadata — and a plugin that improved its settings in a later
/// version is configured against the version that is actually running.
///
/// The schema is read through [`PluginWorkerReader::schema_of`] rather than by taking a
/// snapshot. This runs once per keystroke in a plugin's settings form, and a snapshot is
/// every plugin's manifest, descriptor, configuration document and log line: the cost of
/// checking one number's range was the cost of copying the whole plugin center.
pub(super) fn send_plugin_config(
    clock: &SettingsSnapshotClock,
    id: PluginId,
    values: BTreeMap<String, SettingsFieldValue>,
) -> Result<(), SettingsError> {
    let reader = clock
        .plugin_reader()
        .ok_or_else(|| SettingsError::new(SettingsErrorCode::PluginHostUnavailable))?;
    let schema = reader.schema_of(&id).ok_or_else(|| {
        // A plugin that is listed but has no running process has not declared a
        // schema this host could draw — its archive's `plugin.json` carries metadata
        // only, because a *running* plugin is what sends its settings. So the command
        // refuses rather than guessing a form from the archive.
        SettingsError::new(SettingsErrorCode::PluginNotFound)
    })?;
    let mut document = schema.defaults();
    for (key, value) in values {
        let Some(field) = schema.field(&key) else {
            // A key the running plugin does not declare is dropped rather than
            // refused: it is a field a newer version of that same plugin wrote, and
            // the plugin is the only side that can decide what to do about it.
            continue;
        };
        let Ok(value) = field.fit(&protocol_value(&value)) else {
            continue;
        };
        document.0.insert(key, value);
    }
    send_plugin_command(
        clock,
        PluginCommand::SetConfig {
            id,
            config: document,
        },
    )
}

/// Hand one plugin a press of a control it offered for the host to draw.
///
/// Checked against the list the plugin is *currently* offering, in the settings service
/// rather than only in the worker, and that is deliberate: the window builds its card
/// from a snapshot, so a button can outlive the list it was drawn from by however long a
/// poll takes. Refusing here means the page learns the press did nothing, rather than
/// sending it to a plugin that has withdrawn the control and forgotten what the id
/// meant.
pub(super) fn press_plugin_action(
    clock: &SettingsSnapshotClock,
    id: PluginId,
    action: &str,
) -> Result<(), SettingsError> {
    let reader = clock
        .plugin_reader()
        .ok_or_else(|| SettingsError::new(SettingsErrorCode::PluginHostUnavailable))?;
    if !reader.offers_action(&id, action) {
        return Err(SettingsError::new(SettingsErrorCode::PluginNotFound));
    }
    send_plugin_command(
        clock,
        PluginCommand::PressAction {
            id,
            action: action.to_string(),
        },
    )
}

/// The window's value, as the protocol's own.
///
/// The one conversion at this boundary, and it is exhaustive on purpose: a control
/// produces one of four kinds and the window's enum has exactly those four, so a fifth
/// would be a compile error here rather than a value silently read as something else.
fn protocol_value(value: &SettingsFieldValue) -> bongocat_plugin::ConfigValue {
    match value {
        SettingsFieldValue::Bool(value) => bongocat_plugin::ConfigValue::Bool(*value),
        SettingsFieldValue::Integer(value) => bongocat_plugin::ConfigValue::Integer(*value),
        SettingsFieldValue::Decimal(value) => bongocat_plugin::ConfigValue::Decimal(*value),
        SettingsFieldValue::Text(value) => bongocat_plugin::ConfigValue::Text(value.clone()),
    }
}

/// Resolve the plugin id a command named, and do the work for it.
///
/// The name is validated here rather than in the window because a plugin id is the
/// host's vocabulary: the window sends what the row it rendered carried, and a row
/// that named something else must fail as a refused command, not as a panic inside
/// the host's id type.
pub(super) fn with_plugin_id<T>(
    clock: &SettingsSnapshotClock,
    plugin: &str,
    work: impl FnOnce(PluginId) -> Result<T, SettingsError>,
) -> Result<T, SettingsError> {
    if clock.plugin_reader().is_none() {
        return Err(SettingsError::new(SettingsErrorCode::PluginHostUnavailable));
    }
    // A plugin id is the host's vocabulary, so it is validated here: the window sends
    // what the row it rendered carried, and a row that named something else has to
    // fail as a refused command rather than reach the host's id type.
    let id =
        PluginId::new(plugin).map_err(|_| SettingsError::new(SettingsErrorCode::PluginNotFound))?;
    work(id)
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
