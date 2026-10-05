//! Settings-window ownership shared by the platform hosts.
use bongocat_ui::{
    SettingsNavigationMemory, SettingsWindowHandle, SettingsWindowSeed, open_settings_window,
};
use bongocat_ui_protocol::{SettingsClient, SettingsWindowState};
use gpui_kit::App;

pub(crate) struct ProductSettingsWindow {
    pub(crate) window: Option<SettingsWindowHandle>,
    pub(crate) navigation: SettingsNavigationMemory,
    pub(crate) seed: SettingsWindowSeed,
}

impl ProductSettingsWindow {
    pub(crate) fn new(seed: SettingsWindowSeed) -> Self {
        Self {
            window: None,
            navigation: SettingsNavigationMemory::new(),
            seed,
        }
    }

    pub(crate) fn ensure(
        &mut self,
        client: SettingsClient,
        window_state: SettingsWindowState,
        request_quit: impl Fn(&mut App) + 'static,
        request_update: impl Fn(&mut App) + 'static,
        cx: &mut App,
    ) -> Result<SettingsWindowHandle, String> {
        if let Some(handle) = self.window.as_ref().filter(|handle| handle.is_open()) {
            handle
                .update(cx, |view, window, cx| view.reopen(window, cx))
                .map_err(|error| error.to_string())??;
            cx.activate(true);
            return Ok(handle.clone());
        }
        self.window = None;
        let handle = open_settings_window(
            client,
            window_state,
            self.seed,
            self.navigation.clone(),
            request_quit,
            request_update,
            cx,
        )?;
        self.window = Some(handle.clone());
        Ok(handle)
    }
}
