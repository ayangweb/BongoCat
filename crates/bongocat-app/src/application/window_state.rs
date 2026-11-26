//! Persisting where the settings and overlay windows were left.
//!
//! Window placement is a convenience, not configuration: it survives a restart
//! without ever making a bad value fatal, so a failed commit is reported and the
//! previous placement is kept.

use super::Application;
use crate::ApplicationError;
use bongocat_config::{OverlayWindowPlacement, WindowPlacement};
use std::path::Path;

impl Application {
    pub const fn settings_window_placement(&self) -> Option<WindowPlacement> {
        self.window_state.settings_window
    }

    pub const fn overlay_window_placement(&self) -> Option<OverlayWindowPlacement> {
        self.window_state.overlay_window
    }

    pub fn device_overlay_window_placements(
        &self,
    ) -> &std::collections::BTreeMap<String, OverlayWindowPlacement> {
        &self.window_state.device_overlay_windows
    }

    pub fn persist_settings_window_placement(
        &mut self,
        placement: Option<WindowPlacement>,
    ) -> Result<(), ApplicationError> {
        if self.window_state.settings_window == placement {
            return Ok(());
        }
        let mut window_state = self.window_state.clone();
        window_state.settings_window = placement;
        self.window_state_store.commit(&window_state)?;
        self.window_state = window_state;
        Ok(())
    }

    pub fn persist_overlay_window_placement(
        &mut self,
        placement: OverlayWindowPlacement,
        device: Option<&str>,
    ) -> Result<(), ApplicationError> {
        let mut window_state = self.window_state.clone();
        if let Some(device) = device {
            if window_state.device_overlay_windows.get(device) == Some(&placement) {
                return Ok(());
            }
            window_state
                .device_overlay_windows
                .insert(device.to_owned(), placement);
        } else {
            if window_state.overlay_window == Some(placement) {
                return Ok(());
            }
            window_state.overlay_window = Some(placement);
        }
        self.window_state_store.commit(&window_state)?;
        self.window_state = window_state;
        Ok(())
    }

    pub(crate) fn config_backup_directory(&self) -> &Path {
        &self.config_store.layout().backups
    }
}
