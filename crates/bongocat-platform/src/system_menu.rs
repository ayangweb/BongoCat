/// Localized text and revisioned state used by both native menu surfaces.
///
/// The platform owns native menu handles only. The application supplies this
/// value from its settings snapshot so platform code never becomes a second
/// source of configuration or localization state.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SystemMenuPresentation {
    pub title: String,
    pub tooltip: String,
    pub open_settings: String,
    pub show_overlay: String,
    pub hide_overlay: String,
    pub click_through: String,
    pub check_for_updates: String,
    pub open_source: String,
    pub restart: String,
    pub quit: String,
    pub version: String,
    pub overlay_visible: bool,
    pub click_through_enabled: bool,
    pub update_check_available: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SystemMenuAction {
    OpenSettings,
    ToggleOverlayVisibility,
    ToggleClickThrough,
    CheckForUpdates,
    OpenSource,
    Restart,
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
