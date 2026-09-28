//! Where the settings window is, and how small it may be.
//!
//! The bounds are a protocol constant rather than a view default because both
//! ends have to agree: the service persists a box, and a box smaller than the
//! minimum would be one no window can honour.

use super::*;

pub const MIN_SETTINGS_WINDOW_WIDTH: u32 = 640;

pub const MIN_SETTINGS_WINDOW_HEIGHT: u32 = 480;

pub const MAX_SETTINGS_WINDOW_DIMENSION: u32 = 16_384;

pub const MAX_SETTINGS_WINDOW_COORDINATE: i32 = 1_000_000;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SettingsWindowPlacement {
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
    pub maximized: bool,
}

impl SettingsWindowPlacement {
    pub fn new(x: i32, y: i32, width: u32, height: u32, maximized: bool) -> Option<Self> {
        if !(-MAX_SETTINGS_WINDOW_COORDINATE..=MAX_SETTINGS_WINDOW_COORDINATE).contains(&x)
            || !(-MAX_SETTINGS_WINDOW_COORDINATE..=MAX_SETTINGS_WINDOW_COORDINATE).contains(&y)
            || !(MIN_SETTINGS_WINDOW_WIDTH..=MAX_SETTINGS_WINDOW_DIMENSION).contains(&width)
            || !(MIN_SETTINGS_WINDOW_HEIGHT..=MAX_SETTINGS_WINDOW_DIMENSION).contains(&height)
        {
            return None;
        }
        Some(Self {
            x,
            y,
            width,
            height,
            maximized,
        })
    }
}

#[derive(Clone, Default)]
pub struct SettingsWindowState {
    pub(crate) placement: Arc<Mutex<Option<SettingsWindowPlacement>>>,
    pub(crate) change_revision: Arc<AtomicU64>,
    pub(crate) commands: Option<Sender<SettingsCommand>>,
}

impl SettingsWindowState {
    pub fn new(placement: Option<SettingsWindowPlacement>) -> Self {
        Self {
            placement: Arc::new(Mutex::new(placement)),
            change_revision: Arc::new(AtomicU64::new(0)),
            commands: None,
        }
    }

    pub(crate) fn tracked(
        placement: Option<SettingsWindowPlacement>,
        commands: Sender<SettingsCommand>,
    ) -> Self {
        Self {
            placement: Arc::new(Mutex::new(placement)),
            change_revision: Arc::new(AtomicU64::new(0)),
            commands: Some(commands),
        }
    }

    pub fn placement(&self) -> Option<SettingsWindowPlacement> {
        *self
            .placement
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    pub fn update(&self, placement: SettingsWindowPlacement) -> Option<u64> {
        let mut current = self
            .placement
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if *current == Some(placement) {
            return None;
        }
        *current = Some(placement);
        Some(self.change_revision.fetch_add(1, Ordering::AcqRel) + 1)
    }

    pub fn request_persist_if_current(&self, revision: u64) -> bool {
        if self.change_revision.load(Ordering::Acquire) != revision {
            return true;
        }
        if let Some(commands) = self.commands.as_ref() {
            return match commands.try_send(SettingsCommand::SettingsWindowPlacementChanged) {
                Ok(()) | Err(async_channel::TrySendError::Closed(_)) => true,
                Err(async_channel::TrySendError::Full(_)) => false,
            };
        }
        true
    }
}
