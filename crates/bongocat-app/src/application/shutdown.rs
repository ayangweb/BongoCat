//! Stopping the runtime and the motion audio service, and reporting both.
//!
//! Shutdown never hides one failure behind the other: the aggregate error names
//! both, so a stuck audio thread is as visible as a stuck runtime.

use super::Application;
use crate::app_log::{ApplicationLogContext, ApplicationLogEvent};
use crate::{ApplicationError, RUNTIME_TIMEOUT, combine_shutdown_results};
use bongocat_runtime::RuntimeSnapshot;

impl Application {
    pub fn shutdown(self) -> Result<RuntimeSnapshot, ApplicationError> {
        self.application_log
            .record(ApplicationLogEvent::shutdown_started());
        let marker_start = self.run_marker.mark_shutdown_started();
        let runtime_result = self.runtime.shutdown(RUNTIME_TIMEOUT);
        let audio_result = self
            .motion_audio
            .map(|service| service.shutdown(RUNTIME_TIMEOUT))
            .transpose()
            .map(|_| ());
        match combine_shutdown_results(runtime_result, audio_result) {
            Ok(stopped) => {
                let marker_complete = self.run_marker.complete();
                let marker_error = marker_start.err().or(marker_complete.err());
                if let Some(error) = marker_error {
                    self.application_log.record(
                        ApplicationLogEvent::shutdown_failed()
                            .with_context(ApplicationLogContext::Reason("run_marker_failed")),
                    );
                    return Err(ApplicationError::ApplicationLog(error));
                }
                self.application_log
                    .record(ApplicationLogEvent::shutdown_completed());
                Ok(stopped)
            }
            Err(error) => {
                drop(self.run_marker);
                self.application_log.record(
                    ApplicationLogEvent::shutdown_failed()
                        .with_context(ApplicationLogContext::Reason(error.stable_code())),
                );
                Err(error)
            }
        }
    }
}
