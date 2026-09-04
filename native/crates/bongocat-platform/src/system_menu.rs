use std::fmt;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SystemMenuAction {
    OpenSettings,
    Quit,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SystemMenuError {
    WrongThread,
    WindowClassRegistrationFailed,
    WindowCreateFailed,
    MenuCreateFailed,
    MenuItemCreateFailed,
    StatusItemCreateFailed,
    StatusItemUpdateFailed,
    EventQueueClosed,
    ShutdownFailed,
}

impl fmt::Display for SystemMenuError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::WrongThread => "the system menu must be created on the platform UI thread",
            Self::WindowClassRegistrationFailed => {
                "the system menu window class could not be registered"
            }
            Self::WindowCreateFailed => "the system menu owner window could not be created",
            Self::MenuCreateFailed => "the system menu could not be created",
            Self::MenuItemCreateFailed => "a required system menu item could not be created",
            Self::StatusItemCreateFailed => "the platform status item could not be created",
            Self::StatusItemUpdateFailed => {
                "the platform status item visibility could not be changed"
            }
            Self::EventQueueClosed => "the system menu event consumer is no longer available",
            Self::ShutdownFailed => "the system menu did not shut down cleanly",
        })
    }
}

impl std::error::Error for SystemMenuError {}
