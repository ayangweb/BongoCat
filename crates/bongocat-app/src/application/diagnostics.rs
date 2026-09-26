//! The anonymous diagnostics and the process panic hook the application exposes.
//!
//! Everything here is counted rather than named: the diagnostics providers are
//! closures owned by the product, so no log line, path or version string is
//! assembled in a place that could record it.

use super::Application;
use crate::app_log::{
    ApplicationLogDiagnostics, ApplicationLogEvent, ApplicationLogHandle, CoreLogDiagnostics,
};
use bongocat_log::LogSettingsController;
use bongocat_update::{UpdateDiagnostics, UpdateDiagnosticsTracker};
use std::{path::Path, sync::Arc};

impl Application {
    pub fn logs_directory(&self) -> &Path {
        &self.config_store.layout().logs
    }

    pub fn application_log_diagnostics(&self) -> ApplicationLogDiagnostics {
        self.application_log.diagnostics()
    }

    pub fn log_settings_controller(&self) -> LogSettingsController {
        self.application_log.settings_controller()
    }

    pub fn set_core_log_diagnostics_provider(
        &mut self,
        provider: impl Fn() -> CoreLogDiagnostics + Send + Sync + 'static,
    ) {
        self.core_log_diagnostics = Some(Arc::new(provider));
    }

    pub fn core_log_diagnostics(&self) -> Option<CoreLogDiagnostics> {
        self.core_log_diagnostics
            .as_ref()
            .map(|provider| provider())
    }

    pub fn set_update_diagnostics_provider(
        &mut self,
        provider: impl Fn() -> UpdateDiagnostics + Send + Sync + 'static,
    ) {
        self.update_diagnostics = Some(Arc::new(provider));
    }

    /// Register an app-owned tracker that update workers can share safely.
    pub fn set_update_diagnostics_tracker(&mut self, tracker: UpdateDiagnosticsTracker) {
        self.set_update_diagnostics_provider(move || tracker.snapshot());
    }

    pub fn update_diagnostics(&self) -> Option<UpdateDiagnostics> {
        self.update_diagnostics
            .as_ref()
            .map(|provider| provider().sanitized())
    }

    pub fn log_handle(&self) -> ApplicationLogHandle {
        self.application_log.clone()
    }

    pub fn record_log(&self, event: ApplicationLogEvent) {
        self.application_log.record(event);
    }

    pub fn record_log_once(&self, event: ApplicationLogEvent) {
        self.application_log.record_once(event);
    }

    pub fn install_process_panic_hook(&mut self) {
        if self.panic_hook.is_none() {
            self.panic_hook = Some(self.application_log.install_panic_hook());
        }
    }
}
