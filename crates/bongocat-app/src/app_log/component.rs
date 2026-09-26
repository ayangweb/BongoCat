//! Which part of the application an event came from.
//!
//! The component is part of the code rather than free text, so a log line says
//! which subsystem emitted it without anyone having to type a prefix. That is what
//! makes a filter usable: a user can turn off one noisy subsystem rather than the
//! whole application.

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum ApplicationLogComponent {
    Application,
    Configuration,
    Filesystem,
    Input,
    Logging,
    Model,
    Network,
    Parser,
    Renderer,
    Runtime,
    Service,
    Settings,
    Ui,
    Update,
    Window,
}

impl ApplicationLogComponent {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Application => "application",
            Self::Configuration => "configuration",
            Self::Filesystem => "filesystem",
            Self::Input => "input",
            Self::Logging => "logging",
            Self::Model => "model",
            Self::Network => "network",
            Self::Parser => "parser",
            Self::Renderer => "renderer",
            Self::Runtime => "runtime",
            Self::Service => "service",
            Self::Settings => "settings",
            Self::Ui => "ui",
            Self::Update => "update",
            Self::Window => "window",
        }
    }
}
