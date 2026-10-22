//! Map the runtime's privacy-filtered edges into the process protocol.
use bongocat_plugin_service::{InputEventControl, PluginInputEvent};
use bongocat_runtime::{ActivityControl, ActivitySubscription, InputEdge};

pub(crate) struct PluginActivity(pub(crate) ActivitySubscription);

impl bongocat_plugin_service::PluginInputEvents for PluginActivity {
    fn set_waker(&self, thread: std::thread::Thread) {
        self.0.set_waker(thread);
    }

    fn drain(&mut self) -> (Vec<PluginInputEvent>, u64) {
        let (events, overflow) = self.0.drain();
        let events = events
            .into_iter()
            .map(|event| PluginInputEvent {
                control: match event.control {
                    ActivityControl::Keyboard => InputEventControl::Keyboard,
                    ActivityControl::Space => InputEventControl::Space,
                    ActivityControl::Enter => InputEventControl::Enter,
                    ActivityControl::Backspace => InputEventControl::Backspace,
                    ActivityControl::Tab => InputEventControl::Tab,
                    ActivityControl::ShiftLeft => InputEventControl::ShiftLeft,
                    ActivityControl::ShiftRight => InputEventControl::ShiftRight,
                    ActivityControl::CtrlLeft => InputEventControl::CtrlLeft,
                    ActivityControl::MouseLeft => InputEventControl::MouseLeft,
                    ActivityControl::MouseRight => InputEventControl::MouseRight,
                },
                pressed: event.edge == InputEdge::Down,
                captured_ms: event.at.value(),
            })
            .collect();
        (events, overflow)
    }
}
