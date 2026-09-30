//! The input door: from the runtime's stream to the plugins that asked for it.
//!
//! Three things live here, and none of them is a plugin's business:
//!
//! * **A subscription.** The runtime offers one to anybody who wants every event rather
//!   than a summary, and this is the only subscriber.
//! * **A translation.** The runtime names a control the way the model window's artwork
//!   does (`KeyA`, `LeftShift`, `KpEnter`); a plugin showing a key to a person needs the
//!   name on a keycap (`A`, `Shift`, `Enter`). The wire carries the stable name and the
//!   protocol carries the spelling, so a plugin never has to keep a table of a hundred
//!   key names and a host never has to guess what a plugin meant by one.
//! * **A thread.** The runtime's events arrive on the platform layer's thread, and the
//!   worker is a channel; translating on the platform thread would make every keystroke
//!   wait for a plugin's queue.
//!
//! What is *not* here is any decision about what a plugin does with an event. #905 counts
//! keystrokes, #74 shows the keys you are holding and #90 plays a sound: three plugins,
//! three behaviours, and none of them is a line in this file.

use bongocat_input::{InputControl, InputEdge, InputEvent, InputSource, MouseButton};
use bongocat_plugin::{InputEvent as PluginInputEvent, PluginInputSink, PluginWorkerEndpoint};
use bongocat_runtime::Wait;

/// How many runtime events one translation pass takes.
///
/// Bounded because the runtime's queue is the one being kept short: a pass that drained
/// without a limit would hold a consumer's attention for as long as the queue is long, and
/// the next pass would find the queue no shorter. Sixty is well past one frame's worth of
/// typing on any keyboard.
const MAXIMUM_EVENTS_PER_PASS: usize = 64;

/// How long the translation thread waits for the shutdown flag between passes.
///
/// Not a poll interval in the usual sense — it is how long a thread with nothing to do
/// sleeps — so it is short enough that quitting does not wait on it and long enough that an
/// idle product does not wake sixty times a second for nothing.
const IDLE_WAIT: std::time::Duration = std::time::Duration::from_millis(50);

/// The thread's stop flag.
#[derive(Clone, Default)]
pub struct InputForwarderStop(std::sync::Arc<std::sync::atomic::AtomicBool>);

impl InputForwarderStop {
    /// Ask the thread to stop.
    pub fn stop(&self) {
        self.0.store(true, std::sync::atomic::Ordering::Release);
    }

    fn is_stopped(&self) -> bool {
        self.0.load(std::sync::atomic::Ordering::Acquire)
    }
}

/// Start forwarding the runtime's input to the plugins that asked for it.
///
/// Returns the handle that stops it. The thread is named, so a hang names the thing that
/// hung — which for a thread whose whole job is moving events is the only clue available.
pub fn start(
    runtime: bongocat_runtime::RuntimeClient,
    endpoint: PluginWorkerEndpoint,
) -> InputForwarderStop {
    let subscription = runtime.subscribe_input();
    let sink = endpoint.input_sink();
    let stop = InputForwarderStop::default();
    let flag = stop.clone();
    let _ = std::thread::Builder::new()
        .name("bongocat-plugin-input".to_owned())
        .spawn(move || serve(subscription, sink, flag));
    stop
}

/// Read the runtime's events and publish them, until asked to stop.
fn serve(
    subscription: bongocat_runtime::InputSubscription,
    sink: PluginInputSink,
    stop: InputForwarderStop,
) {
    while !stop.is_stopped() {
        // A reset when nothing arrived, rather than a timeout: the alternative is peeking
        // at the queue to find out whether it was empty, and a peek that consumed an event
        // would be a keystroke lost to a scheduling question.
        match subscription.wait_for(MAXIMUM_EVENTS_PER_PASS, IDLE_WAIT) {
            Wait::Nothing => continue,
            Wait::Closed => return,
            Wait::Events(events) => {
                let events: Vec<PluginInputEvent> = events.iter().filter_map(translate).collect();
                if !events.is_empty() {
                    sink.publish(events);
                }
            }
        }
    }
}

/// One runtime event as a plugin event, or `None` when there is nothing to say.
///
/// The `None` cases are the interesting ones and they are all the same case: an event the
/// protocol has no word for. A gamepad connecting is the platform layer telling the
/// runtime about a device; a reconciliation is the runtime correcting itself after a
/// device vanished, and a plugin that wanted to know would rather hear a `Reset` than
/// nothing. Anything the protocol does grow later is one arm here, and this file is the
/// only place that has to learn it.
pub fn translate(event: &InputEvent) -> Option<PluginInputEvent> {
    match event {
        InputEvent::Edge { control, edge, .. } => match (control, edge) {
            (InputControl::Key(key), edge) => {
                let control = key_name(key.hid_usage())?;
                Some(match edge {
                    InputEdge::Down => PluginInputEvent::KeyDown {
                        control,
                        repeat: false,
                    },
                    InputEdge::Up => PluginInputEvent::KeyUp { control },
                })
            }
            (InputControl::Mouse(button), edge) => Some(PluginInputEvent::MouseButton {
                button: mouse_button_name(*button),
                pressed: matches!(edge, InputEdge::Down),
            }),
            // A gamepad button is not in the protocol yet, and a plugin that is counting
            // gamepad presses would be told about some and not others — which is worse
            // than being told about none. So none is the honest answer until the protocol
            // grows the arm, and this is the comment that says so.
            (InputControl::Gamepad(_), _) => None,
        },
        // A reconciliation is a `Reset`: the platform has re-read the real pressed set and
        // it is not what the plugin was told, so the plugin's own tally is wrong in a way
        // it cannot detect on its own.
        InputEvent::Reconcile { .. } | InputEvent::Reset { .. } => Some(PluginInputEvent::Reset {
            reason: reset_reason(event).to_owned(),
        }),
        InputEvent::GamepadConnected { .. } | InputEvent::GamepadDisconnected { .. } => None,
    }
}

/// Why the pressed set was cleared, in the protocol's own words.
///
/// A closed vocabulary rather than a formatted debug string, because a plugin reacts to
/// these: one that only wanted to forget its keys does not care, and one that wanted to
/// tell the user the keyboard was unplugged does.
fn reset_reason(event: &InputEvent) -> &'static str {
    match event {
        InputEvent::Reset {
            reason: bongocat_input::InputResetReason::SessionLock,
            ..
        } => "session_lock",
        InputEvent::Reset {
            reason: bongocat_input::InputResetReason::Sleep,
            ..
        } => "sleep",
        InputEvent::Reset {
            reason: bongocat_input::InputResetReason::DeviceRemoved,
            ..
        } => "device_removed",
        InputEvent::Reset {
            reason: bongocat_input::InputResetReason::ServiceRestart,
            ..
        } => "service_restart",
        InputEvent::Reset {
            reason: bongocat_input::InputResetReason::QueueOverflow,
            ..
        } => "queue_overflow",
        InputEvent::Reset {
            reason: bongocat_input::InputResetReason::PermissionChanged,
            ..
        } => "permission_changed",
        // A reconciliation has no reason of its own: it is the platform correcting itself,
        // and the honest word for that is the one that covers every cause.
        _ => "reconciled",
    }
}

/// The stable name of a key, as the model window's artwork names it.
///
/// The same names the shipped models' key images use, taken from the one place that owns
/// them, so a plugin and a model agree on what a key is called without either owning a
/// table. `None` for a key with no artwork name, which is a key no model can draw and a
/// plugin has no use for either.
fn key_name(hid_usage: u16) -> Option<String> {
    bongocat_live2d_render::key_name_candidates(hid_usage)
        .into_iter()
        .next()
        .map(str::to_owned)
}

/// A mouse button's name, in the protocol's own spelling.
fn mouse_button_name(button: MouseButton) -> String {
    match button {
        MouseButton::Left => "left",
        MouseButton::Right => "right",
        MouseButton::Middle => "middle",
        MouseButton::Back => "back",
        MouseButton::Forward => "forward",
        // A button this build does not name is named by its number, which is a fact rather
        // than a guess: a plugin that shows `Other 3` is showing what the platform said.
        MouseButton::Other(index) => return format!("other_{index}"),
    }
    .to_owned()
}

/// Whether an event came from a real device rather than from the runtime correcting itself.
///
/// Not used by the translation today, and not exported: it exists so a future arm that has
/// to tell the two apart is reading the field rather than inventing a second meaning for
/// it.
#[allow(
    dead_code,
    reason = "read by a protocol arm that has not been written yet"
)]
fn is_capture(event: &InputEvent) -> bool {
    matches!(
        event,
        InputEvent::Edge {
            source: InputSource::Capture,
            ..
        }
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use bongocat_input::{GamepadButton, GamepadButtonKey, GamepadConnection, PhysicalKey};

    fn edge(usage: u16, edge: InputEdge) -> InputEvent {
        InputEvent::Edge {
            control: InputControl::Key(PhysicalKey::from_hid_usage(usage)),
            edge,
            source: InputSource::Capture,
            at: bongocat_input::MonotonicMillis::new(1),
        }
    }

    #[test]
    fn a_key_goes_on_the_wire_under_the_name_the_model_window_uses() {
        assert_eq!(
            translate(&edge(0x04, InputEdge::Down)),
            Some(PluginInputEvent::KeyDown {
                control: "KeyA".to_owned(),
                repeat: false
            })
        );
        assert_eq!(
            translate(&edge(0x04, InputEdge::Up)),
            Some(PluginInputEvent::KeyUp {
                control: "KeyA".to_owned()
            })
        );
        assert_eq!(
            translate(&edge(0xe1, InputEdge::Down)),
            Some(PluginInputEvent::KeyDown {
                control: "ShiftLeft".to_owned(),
                repeat: false
            }),
            "and a modifier is named as the artwork names it, so a plugin and a model agree \
             without either of them owning a table"
        );
        assert_eq!(
            translate(&edge(0x58, InputEdge::Down)),
            Some(PluginInputEvent::KeyDown {
                control: "KpEnter".to_owned(),
                repeat: false
            }),
        );
    }

    #[test]
    fn a_mouse_button_carries_which_edge_it_was() {
        for (button, expected) in [
            (MouseButton::Left, "left"),
            (MouseButton::Right, "right"),
            (MouseButton::Middle, "middle"),
            (MouseButton::Back, "back"),
            (MouseButton::Forward, "forward"),
        ] {
            let event = InputEvent::Edge {
                control: InputControl::Mouse(button),
                edge: InputEdge::Down,
                source: InputSource::Capture,
                at: bongocat_input::MonotonicMillis::new(1),
            };
            assert_eq!(
                translate(&event),
                Some(PluginInputEvent::MouseButton {
                    button: expected.to_owned(),
                    pressed: true
                })
            );
            let InputEvent::Edge {
                control,
                source,
                at,
                ..
            } = event
            else {
                panic!("the event is an edge");
            };
            let up = InputEvent::Edge {
                control,
                edge: InputEdge::Up,
                source,
                at,
            };
            assert_eq!(
                translate(&up),
                Some(PluginInputEvent::MouseButton {
                    button: expected.to_owned(),
                    pressed: false
                }),
                "because a button that is only ever reported as pressed is a button that can \\
                 never be released"
            );
        }
    }

    #[test]
    fn a_button_this_build_does_not_name_is_named_by_its_number_rather_than_dropped() {
        // The platform said there is a button and the product does not have a word for it.
        // Showing nothing would be a plugin whose tally silently misses a button; showing
        // the number is a fact the plugin can show and the user can recognise.
        let event = InputEvent::Edge {
            control: InputControl::Mouse(MouseButton::Other(7)),
            edge: InputEdge::Down,
            source: InputSource::Capture,
            at: bongocat_input::MonotonicMillis::new(1),
        };
        assert_eq!(
            translate(&event),
            Some(PluginInputEvent::MouseButton {
                button: "other_7".to_owned(),
                pressed: true
            })
        );
    }

    #[test]
    fn a_gamepad_button_is_not_sent_because_the_protocol_has_no_arm_for_one() {
        // Sending some gamepad presses and not others would be worse than sending none: a
        // tally would be quietly short with nothing to show for it.
        let event = InputEvent::Edge {
            control: InputControl::Gamepad(GamepadButtonKey {
                connection: GamepadConnection {
                    device_id: 0,
                    generation: 1,
                },
                button: GamepadButton::South,
            }),
            edge: InputEdge::Down,
            source: InputSource::Capture,
            at: bongocat_input::MonotonicMillis::new(1),
        };
        assert_eq!(translate(&event), None);
    }

    #[test]
    fn a_gamepad_connecting_is_not_input() {
        // The product telling the runtime about a device is not a thing a person pressed.
        let connection = GamepadConnection {
            device_id: 0,
            generation: 1,
        };
        assert_eq!(
            translate(&InputEvent::GamepadConnected {
                connection,
                at: bongocat_input::MonotonicMillis::new(1)
            }),
            None
        );
        assert_eq!(
            translate(&InputEvent::GamepadDisconnected {
                connection,
                at: bongocat_input::MonotonicMillis::new(1)
            }),
            None
        );
    }

    #[test]
    fn a_cleared_pressed_set_reaches_a_plugin_as_a_reset_with_a_reason() {
        // A plugin keeping its own tally cannot detect that the platform forgot a key, and
        // a tally that keeps a key the platform has already forgotten never balances.
        for (reason, expected) in [
            (
                bongocat_input::InputResetReason::SessionLock,
                "session_lock",
            ),
            (bongocat_input::InputResetReason::Sleep, "sleep"),
            (
                bongocat_input::InputResetReason::DeviceRemoved,
                "device_removed",
            ),
            (
                bongocat_input::InputResetReason::ServiceRestart,
                "service_restart",
            ),
            (
                bongocat_input::InputResetReason::QueueOverflow,
                "queue_overflow",
            ),
            (
                bongocat_input::InputResetReason::PermissionChanged,
                "permission_changed",
            ),
        ] {
            let event = InputEvent::Reset {
                reason,
                at: bongocat_input::MonotonicMillis::new(1),
            };
            assert_eq!(
                translate(&event),
                Some(PluginInputEvent::Reset {
                    reason: expected.to_owned()
                })
            );
        }
    }

    #[test]
    fn a_reconciliation_is_a_reset_because_a_plugin_cannot_detect_one() {
        let event = InputEvent::Reconcile {
            pressed: Default::default(),
            at: bongocat_input::MonotonicMillis::new(1),
        };
        assert_eq!(
            translate(&event),
            Some(PluginInputEvent::Reset {
                reason: "reconciled".to_owned()
            }),
            "the platform re-read the real pressed set and it is not what the plugin was told"
        );
    }

    #[test]
    fn a_key_with_no_artwork_name_is_not_sent_as_an_empty_string() {
        // 0xffff is not a key on any keyboard. An empty `control` would be a key that no
        // pressed set could ever match, so a plugin's tally would grow by one for a key it
        // could not name and could never remove.
        assert_eq!(translate(&edge(0xffff, InputEdge::Down)), None);
    }
}
