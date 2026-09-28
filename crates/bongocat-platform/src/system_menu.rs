/// Localized text and revisioned state used by the two native menu surfaces.
///
/// The platform owns native menu handles only. The application supplies this
/// value from its settings snapshot so platform code never becomes a second
/// source of configuration or localization state.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SystemMenuPresentation {
    pub title: String,
    pub tooltip: String,
    pub open_settings: String,
    pub model_window: String,
    /// The shared “hide model window” label for the visibility check item.
    /// The check state means "the model window is hidden"; there is no
    /// separate Show label.
    pub hide_overlay: String,
    pub click_through: String,
    pub always_on_top: String,
    pub hide_on_pointer_hover: String,
    pub check_for_updates: String,
    pub quit: String,
    pub overlay_visible: bool,
    pub click_through_enabled: bool,
    pub always_on_top_enabled: bool,
    pub hide_on_pointer_hover_enabled: bool,
    pub update_check_available: bool,
}

/// A product action emitted by either native menu surface.
///
/// Actions are deliberately small and stable: the tray and model-window
/// context surfaces share the same menu set, while the application remains
/// the only owner of what each action means.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SystemMenuAction {
    OpenSettings,
    /// Toggle the session-only model-window visibility. Native menus render
    /// this as a check item whose checked state means hidden.
    ToggleOverlayVisibility,
    ToggleClickThrough,
    ToggleAlwaysOnTop,
    ToggleHideOnPointerHover,
    CheckForUpdates,
    Quit,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, thiserror::Error)]
pub enum SystemMenuError {
    #[error("the system menu must be created on the platform UI thread")]
    WrongThread,
    #[error("the system menu window class could not be registered")]
    WindowClassRegistrationFailed,
    #[error("the system menu owner window could not be created")]
    WindowCreateFailed,
    #[error("the context menu window handle is unavailable")]
    WindowHandleUnavailable,
    #[error("the context menu window handle is not supported on this platform")]
    UnsupportedWindowHandle,
    #[error("the system menu could not be created")]
    MenuCreateFailed,
    #[error("a required system menu item could not be created")]
    MenuItemCreateFailed,
    #[error("the platform status item could not be created")]
    StatusItemCreateFailed,
    #[error("the platform status icon image could not be loaded")]
    StatusIconImageLoadFailed,
    #[error("the platform status item visibility could not be changed")]
    StatusItemUpdateFailed,
    #[error("the system menu event consumer is no longer available")]
    EventQueueClosed,
    #[error("the system menu did not shut down cleanly")]
    ShutdownFailed,
}
