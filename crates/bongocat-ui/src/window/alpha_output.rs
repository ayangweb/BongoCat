//! Linux Alpha output controls and explicitly refreshed system state.

use super::*;
use crate::{
    SettingsAlphaInstallationStatus as Installation, SettingsAlphaManagementAction as Action,
    SettingsAlphaManagementError as Error, SettingsAlphaManagementUnavailable as Unavailable,
    SettingsAlphaOutputStatus as Output, SettingsAlphaSystemStatus,
};
use gpui_kit::component::Sizable as _;

fn text(language: SettingsLanguage, suffix: &'static str) -> SharedString {
    bongocat_i18n::text(language.catalog_locale(), suffix).into()
}

pub(super) fn error_text(language: SettingsLanguage, error: Error) -> SharedString {
    let key = match error {
        Error::ServiceUnavailable => "settings.alpha.error.service",
        Error::SnapshotOutdated => "settings.alpha.error.stale",
        Error::Busy => "settings.alpha.error.busy",
        Error::OutputStopFailed => "settings.alpha.error.stop",
        Error::ConfigPersistFailed => "settings.alpha.error.config",
        Error::AuthorizationDenied => "settings.alpha.error.authorization",
        Error::DriverMissing => "settings.alpha.error.driver",
        Error::UnsupportedSystem => "settings.alpha.error.unsupported",
        Error::OwnershipConflict => "settings.alpha.error.conflict",
        Error::PermissionDenied => "settings.alpha.error.permission",
        Error::RollbackFailed => "settings.alpha.error.rollback",
        Error::OperationFailed => "settings.alpha.error.operation",
    };
    text(language, key)
}

impl SettingsView {
    pub(super) fn refresh_alpha_status(&mut self, cx: &mut Context<Self>) {
        if self.alpha_operation_pending {
            return;
        }
        self.alpha_status_checked = true;
        self.alpha_operation_pending = true;
        let client = self.client.clone();
        cx.spawn(async move |this, cx| {
            let result = client.read_alpha_system_status().await;
            let _ = this.update(cx, |view, cx| {
                view.alpha_operation_pending = false;
                match result {
                    Ok(status) => view.alpha_system_status = Some(status),
                    Err(error) => view.pending_notification = Some(error),
                }
                cx.notify();
            });
        })
        .detach();
    }

    pub(super) fn manage_alpha_camera(&mut self, action: Action, cx: &mut Context<Self>) {
        if self.alpha_operation_pending || self.pending.is_some() {
            return;
        }
        let Some(revision) = self
            .snapshot
            .as_ref()
            .and_then(|snapshot| snapshot.config_revision)
        else {
            return;
        };
        self.alpha_operation_pending = true;
        let client = self.client.clone();
        cx.notify();
        cx.spawn(async move |this, cx| {
            let result = client.manage_alpha_camera(revision, action).await;
            let status = client.read_alpha_system_status().await;
            let snapshot = client.read_snapshot().await;
            let _ = this.update(cx, |view, cx| {
                view.alpha_operation_pending = false;
                if let Err(error) = result {
                    view.alpha_management_error = Some(error);
                }
                if let Ok(status) = status {
                    view.alpha_system_status = Some(status);
                }
                if let Ok(snapshot) = snapshot
                    && accepts_snapshot_revision(
                        view.snapshot.as_ref().map(|value| value.revision),
                        snapshot.revision,
                    )
                {
                    view.snapshot = Some(snapshot);
                }
                cx.notify();
            });
        })
        .detach();
    }
}

fn status_row(language: SettingsLanguage, title: &'static str, value: &'static str) -> SettingItem {
    let value = text(language, value);
    SettingItem::new(
        text(language, title),
        SettingField::element(move |_: &RenderOptions, _: &mut Window, _: &mut App| {
            div().text_sm().child(value.clone()).into_any_element()
        }),
    )
}

fn button_row(
    view: &Entity<SettingsView>,
    language: SettingsLanguage,
    title: &'static str,
    action_key: &'static str,
    id: &'static str,
    disabled: bool,
    callback: fn(&mut SettingsView, &mut Context<SettingsView>),
) -> SettingItem {
    let view = view.clone();
    SettingItem::new(
        text(language, title),
        SettingField::element(
            move |options: &RenderOptions, _: &mut Window, _: &mut App| {
                let view = view.clone();
                Button::new(id)
                    .label(text(language, action_key))
                    .with_size(options.size())
                    .disabled(disabled || options.is_disabled())
                    .on_click(move |_, _, app| {
                        view.update(app, callback);
                    })
                    .into_any_element()
            },
        ),
    )
    .disabled(disabled)
}

pub(super) fn group(
    view: &Entity<SettingsView>,
    snapshot: Option<&SettingsSnapshot>,
    language: SettingsLanguage,
    status: Option<SettingsAlphaSystemStatus>,
    busy: bool,
) -> SettingGroup {
    let unknown = if busy && status.is_none() {
        "settings.alpha.value.checking"
    } else {
        "settings.alpha.value.unknown"
    };
    let driver = status
        .and_then(|s| s.driver_installed)
        .map_or(unknown, |value| {
            if value {
                "settings.alpha.value.installed"
            } else {
                "settings.alpha.value.driver_missing"
            }
        });
    let module = status
        .and_then(|s| s.module_loaded)
        .map_or(unknown, |value| {
            if value {
                "settings.alpha.value.loaded"
            } else {
                "settings.alpha.value.not_loaded"
            }
        });
    let camera = status.map_or(unknown, |s| match s.camera {
        Installation::NotInstalled => "settings.alpha.value.not_installed",
        Installation::Installed => "settings.alpha.value.installed",
        Installation::RecoveryRequired => "settings.alpha.value.recovery",
        Installation::Conflict => "settings.alpha.value.conflict",
        Installation::Unknown | Installation::Unsupported => unknown,
    });
    let output = snapshot.map_or(unknown, |s| match s.alpha_output_status {
        Output::Disabled => "settings.alpha.value.off",
        Output::Starting => "settings.alpha.value.starting",
        Output::Running => "settings.alpha.value.running",
        Output::Stopping => "settings.alpha.value.stopping",
        Output::DeviceUnavailable => "settings.alpha.value.unavailable",
        Output::PermissionDenied => "settings.alpha.value.permission",
        Output::UnsupportedFormat => "settings.alpha.value.format",
        Output::Failed => "settings.alpha.value.failed",
    });
    let available = status.is_some_and(|s| s.management_available);
    let installable = status.is_some_and(|s| s.driver_installed == Some(true));
    let uninstallable = status.is_some_and(|s| s.camera != Installation::NotInstalled);
    let retry = snapshot.is_some_and(|s| {
        s.alpha_output_enabled
            && matches!(
                s.alpha_output_status,
                Output::DeviceUnavailable
                    | Output::PermissionDenied
                    | Output::UnsupportedFormat
                    | Output::Failed
            )
    });
    let read = view.clone();
    let write = view.clone();
    SettingGroup::new()
        .title(text(language, "settings.alpha.title"))
        .items([
            status_row(language, "settings.alpha.driver", driver),
            status_row(language, "settings.alpha.module", module),
            status_row(
                language,
                "settings.alpha.management",
                status.map_or(unknown, |s| match s.management_unavailable {
                    None if s.management_available => "settings.alpha.value.available",
                    Some(Unavailable::Authorization) => "settings.alpha.value.pkexec_missing",
                    Some(Unavailable::LoopbackControl) => "settings.alpha.value.control_missing",
                    Some(Unavailable::Udev) => "settings.alpha.value.udev_missing",
                    Some(Unavailable::ModuleTools) => "settings.alpha.value.module_tools_missing",
                    None => "settings.alpha.value.unsupported",
                }),
            ),
            status_row(language, "settings.alpha.camera", camera),
            status_row(language, "settings.alpha.output", output),
            SettingItem::new(
                text(language, "settings.alpha.enable"),
                SettingField::switch(
                    move |app| {
                        read.read(app)
                            .snapshot
                            .as_ref()
                            .is_some_and(|s| s.alpha_output_enabled)
                    },
                    move |enabled, app| {
                        write.update(app, |view, cx| view.set_alpha_output_enabled(enabled, cx));
                    },
                ),
            )
            .disabled(busy || snapshot.is_none()),
            button_row(
                view,
                language,
                "settings.alpha.refresh",
                if busy {
                    "settings.alpha.value.checking"
                } else {
                    "settings.alpha.refresh"
                },
                "alpha-refresh",
                busy,
                SettingsView::refresh_alpha_status,
            ),
            button_row(
                view,
                language,
                "settings.alpha.install",
                "settings.alpha.install",
                "alpha-install",
                busy || !available || !installable,
                |view, cx| view.manage_alpha_camera(Action::Install, cx),
            ),
            button_row(
                view,
                language,
                "settings.alpha.uninstall",
                "settings.alpha.uninstall",
                "alpha-uninstall",
                busy || !available || !uninstallable,
                |view, cx| view.manage_alpha_camera(Action::Uninstall, cx),
            ),
            button_row(
                view,
                language,
                "settings.alpha.retry",
                "settings.alpha.retry",
                "alpha-retry",
                busy || !retry,
                |view, cx| view.set_alpha_output_enabled(true, cx),
            ),
        ])
}
