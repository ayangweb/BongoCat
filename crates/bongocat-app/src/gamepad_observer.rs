//! Turning the frame source's gamepad connectivity notices into settings commands.
//!
//! The runtime owns the connected set, so a notice is a fact to forward rather
//! than a model choice to make here. A notice the settings service could not
//! accept stays pending and is retried, because the next transition is not
//! something the product can afford to miss.

use super::*;

/// Tell the settings service when gamepad connectivity changes.
///
/// The frame source already reads the runtime snapshot every frame, and the
/// runtime owns the connected set, so this is where the product learns about a
/// plug or an unplug without a second input transport. It only notices a
/// *transition*: the first frame only seeds the last observed value, and a
/// change between two frames is queued at most once.
///
/// A notice the service could not accept stays pending and is retried on the next
/// frame instead of being dropped, because the next transition is not something
/// the product can afford to miss. Nothing is queued while the service is gone:
/// the frame source is stopped before it during shutdown.
#[derive(Default)]
pub(crate) struct GamepadConnectionObserver {
    connected: Option<bool>,
    pending: bool,
}

impl GamepadConnectionObserver {
    /// Queue a notice when the connected count crossed between "none" and "at
    /// least one" since the previous frame, and retry one the service could not
    /// take.
    pub(crate) fn observe(&mut self, connected_gamepad_count: usize, client: &SettingsClient) {
        let connected = connected_gamepad_count > 0;
        if self.connected != Some(connected) {
            // The first frame only establishes the baseline. A gamepad that is
            // already attached is announced by the input service after startup,
            // which is a real transition against that baseline.
            if self.connected.is_some() {
                self.pending = true;
            }
            self.connected = Some(connected);
        }
        if self.pending && client.notify_gamepad_connection_changed().is_ok() {
            self.pending = false;
        }
    }
}
