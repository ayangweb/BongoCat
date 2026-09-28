//! Persisting where the settings and overlay windows were left.
//!
//! Window placement is a convenience, not configuration: it survives a restart
//! without ever making a bad value fatal, so a failed commit is reported and the
//! previous placement is kept.

use super::Application;
use crate::ApplicationError;
use bongocat_config::{OverlayWindowPlacement, WindowPlacement, WindowState};
use std::path::Path;

impl Application {
    pub const fn settings_window_placement(&self) -> Option<WindowPlacement> {
        self.window_state.settings_window
    }

    pub const fn overlay_window_placement(&self) -> Option<OverlayWindowPlacement> {
        self.window_state.overlay_window
    }

    pub fn persist_settings_window_placement(
        &mut self,
        placement: Option<WindowPlacement>,
    ) -> Result<(), ApplicationError> {
        if self.window_state.settings_window == placement {
            return Ok(());
        }
        let window_state = WindowState::with_windows(placement, self.window_state.overlay_window);
        self.window_state_store.commit(&window_state)?;
        self.window_state = window_state;
        Ok(())
    }

    pub fn persist_overlay_window_placement(
        &mut self,
        placement: OverlayWindowPlacement,
    ) -> Result<(), ApplicationError> {
        if self.window_state.overlay_window == Some(placement) {
            return Ok(());
        }
        let window_state =
            WindowState::with_windows(self.window_state.settings_window, Some(placement));
        self.window_state_store.commit(&window_state)?;
        self.window_state = window_state;
        Ok(())
    }

    pub(crate) fn config_backup_directory(&self) -> &Path {
        &self.config_store.layout().backups
    }
}
