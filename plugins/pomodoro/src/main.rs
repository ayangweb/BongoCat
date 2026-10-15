//! Standalone Pomodoro plugin. The application knows no timer business rules.
#![forbid(unsafe_code)]
use bongocat_plugin_sdk::{
    Anchor, ConfigValue, Configuration, HostMessage, Plugin, PluginOutput, PluginView, UiNode,
};
use serde::{Deserialize, Serialize};

#[derive(Default, Serialize, Deserialize)]
struct Pomodoro {
    remaining_ms: u64,
    running: bool,
    /// Whether this session has ever run. A timer that has not run previews the
    /// whole session, and one that has run out reports that it is done instead
    /// of silently looking like a fresh one.
    #[serde(default)]
    started: bool,
    #[serde(skip)]
    last_tick: Option<u64>,
    #[serde(default = "default_minutes")]
    minutes: i64,
}
fn default_minutes() -> i64 {
    25
}
impl Pomodoro {
    fn configure(&mut self, config: &Configuration) -> Result<(), String> {
        let minutes = match config.get("minutes") {
            None => default_minutes(),
            Some(ConfigValue::Integer(value)) if (1..=180).contains(value) => *value,
            _ => return Err("Minutes must be between 1 and 180".into()),
        };
        self.minutes = minutes;
        Ok(())
    }
    fn output(&self) -> Result<PluginOutput, String> {
        // A timer that has not run yet previews the whole session it is about to
        // count down, so the duration is visible before the user presses Start.
        // Once it has run the panel shows what is left, and a finished session
        // stays at zero instead of reading as a fresh one.
        let text = match (self.started, self.remaining_ms) {
            (false, _) => format_clock(self.session_ms()),
            (_, remaining) => format_clock(remaining),
        };
        let model = PluginView {
            anchor: Anchor::TopRight,
            nodes: vec![
                UiNode::Text { text },
                UiNode::Button {
                    id: "toggle".into(),
                    label: if self.running { "Pause" } else { "Start" }.into(),
                },
                UiNode::Button {
                    id: "restart".into(),
                    label: "Restart".into(),
                },
            ],
        };
        Ok(PluginOutput {
            model,
            settings: PluginView {
                nodes: vec![UiNode::Integer {
                    key: "minutes".into(),
                    label: "Minutes".into(),
                    value: self.minutes,
                    min: 1,
                    max: 180,
                }],
                ..Default::default()
            },
            config: [("minutes".into(), ConfigValue::Integer(self.minutes))].into(),
            state: serde_json::to_vec(self).map_err(|_| "Cannot save pomodoro")?,
        })
    }
    /// The full session this timer will count down from.
    ///
    /// A timer that has not started shows the clock shape it is about to count
    /// down from, so the duration is readable before the first tick rather than
    /// after it: `25:00` says what the user is about to start, which a
    /// placeholder word cannot.
    fn session_ms(&self) -> u64 {
        self.minutes.max(0) as u64 * 60_000
    }
}

/// `MM:SS` for a remaining duration, and for a timer that has not started.
///
/// The host draws a `digits:digits` string at its large value size, so the
/// preview and a running countdown share one shape and one type size.
fn format_clock(remaining_ms: u64) -> String {
    let seconds = remaining_ms.div_ceil(1000);
    format!("{:02}:{:02}", seconds / 60, seconds % 60)
}

/// The clock the model-window panel displays.
#[cfg(test)]
fn clock_text(output: &PluginOutput) -> &str {
    match output.model.nodes.iter().find_map(|node| match node {
        UiNode::Text { text } => Some(text.as_str()),
        _ => None,
    }) {
        Some(text) => text,
        None => panic!("the panel always publishes a clock"),
    }
}
impl Plugin for Pomodoro {
    fn id(&self) -> &'static str {
        "com.bongocat.pomodoro"
    }
    fn handle(&mut self, message: HostMessage) -> Result<PluginOutput, String> {
        match message {
            HostMessage::Start { config, state } => {
                if !state.is_empty() {
                    *self = serde_json::from_slice(&state).map_err(|_| "Invalid saved pomodoro")?;
                    // State saved before `started` existed has no such field, so
                    // a remainder it did save is itself evidence that the
                    // session had already been running. Reading it back as a
                    // fresh timer would show the full length over the time the
                    // user had actually left.
                    self.started |= self.remaining_ms > 0;
                }
                self.running = false;
                self.last_tick = None;
                self.configure(&config)?;
            }
            HostMessage::Configure { config } => self.configure(&config)?,
            HostMessage::KeyboardActivity { .. } => {}
            HostMessage::Tick { elapsed_ms } => {
                if self.running
                    && let Some(previous) = self.last_tick
                {
                    self.remaining_ms = self
                        .remaining_ms
                        .saturating_sub(elapsed_ms.saturating_sub(previous));
                    if self.remaining_ms == 0 {
                        self.running = false;
                    }
                }
                self.last_tick = Some(elapsed_ms);
            }
            HostMessage::Action { id } => match id.as_str() {
                "toggle" => {
                    if self.remaining_ms == 0 {
                        self.remaining_ms = self.session_ms();
                    }
                    self.running = !self.running;
                    self.started = true;
                }
                "restart" => {
                    self.remaining_ms = self.session_ms();
                    self.running = true;
                    self.started = true;
                }
                _ => return Err("Unknown pomodoro action".into()),
            },
            HostMessage::Pause | HostMessage::Stop => {
                self.running = false;
                self.last_tick = None;
            }
            HostMessage::Resume => {
                self.running = self.remaining_ms > 0;
                self.last_tick = None;
            }
            HostMessage::Reset => {
                *self = Self {
                    minutes: default_minutes(),
                    ..Default::default()
                };
            }
            HostMessage::Handshake { .. } => return Err("Handshake belongs to SDK".into()),
        }
        self.output()
    }
}
fn main() -> std::io::Result<()> {
    bongocat_plugin_sdk::run(&mut Pomodoro {
        minutes: default_minutes(),
        ..Default::default()
    })
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn monotonic_pomodoro_pause_resume_and_finish() {
        let mut p = Pomodoro {
            minutes: 1,
            ..Default::default()
        };
        p.handle(HostMessage::Tick { elapsed_ms: 0 }).unwrap();
        p.handle(HostMessage::Action {
            id: "toggle".into(),
        })
        .unwrap();
        p.handle(HostMessage::Tick { elapsed_ms: 5000 }).unwrap();
        assert_eq!(p.remaining_ms, 55_000);
        p.handle(HostMessage::Pause).unwrap();
        p.handle(HostMessage::Tick { elapsed_ms: 9000 }).unwrap();
        assert_eq!(p.remaining_ms, 55_000);
        p.handle(HostMessage::Resume).unwrap();
        p.handle(HostMessage::Tick { elapsed_ms: 10_000 }).unwrap();
        p.handle(HostMessage::Tick { elapsed_ms: 70_000 }).unwrap();
        assert_eq!(p.remaining_ms, 0);
        assert!(!p.running);
    }
    #[test]
    fn invalid_configuration_keeps_previous_value() {
        let mut p = Pomodoro {
            minutes: 25,
            ..Default::default()
        };
        assert!(
            p.configure(&[("minutes".into(), ConfigValue::Integer(0))].into())
                .is_err()
        );
        assert_eq!(p.minutes, 25);
    }
    /// The panel shows the duration before the user presses Start.
    #[test]
    fn an_unstarted_timer_shows_the_session_it_is_about_to_count_down() {
        let mut p = Pomodoro {
            minutes: 25,
            ..Default::default()
        };
        assert_eq!(clock_text(&p.output().unwrap()), "25:00");

        // A configured session previews its own length, not the default.
        p.handle(HostMessage::Configure {
            config: [("minutes".into(), ConfigValue::Integer(45))].into(),
        })
        .unwrap();
        assert_eq!(clock_text(&p.output().unwrap()), "45:00");
    }
    /// The preview is not a promise that the timer is already counting, and a
    /// finished session does not read as a fresh one.
    #[test]
    fn a_finished_session_does_not_read_as_a_new_preview() {
        let p = Pomodoro {
            minutes: 25,
            remaining_ms: 0,
            running: false,
            started: true,
            ..Default::default()
        };
        assert_eq!(clock_text(&p.output().unwrap()), "00:00");
    }
    /// The countdown the user actually watches is the session minus what elapsed.
    #[test]
    fn the_preview_becomes_the_countdown_once_the_timer_runs() {
        let mut p = Pomodoro {
            minutes: 25,
            ..Default::default()
        };
        p.handle(HostMessage::Action {
            id: "toggle".into(),
        })
        .unwrap();
        assert_eq!(clock_text(&p.output().unwrap()), "25:00");
        // The first tick only seeds the monotonic clock the delta is measured
        // from, so the countdown begins against the next one.
        p.handle(HostMessage::Tick { elapsed_ms: 0 }).unwrap();
        p.handle(HostMessage::Tick { elapsed_ms: 5_000 }).unwrap();
        assert_eq!(clock_text(&p.output().unwrap()), "24:55");
    }
    /// State written before the preview existed still loads.
    ///
    /// The `started` flag was added when the panel began previewing the session
    /// length. State bytes saved by the previous version have no such field, so
    /// they must read back rather than fail into a reset.
    #[test]
    fn state_saved_before_the_preview_existed_still_loads() {
        let legacy = br#"{"remaining_ms":120000,"running":false,"minutes":30}"#;
        let mut p = Pomodoro {
            minutes: 25,
            ..Default::default()
        };
        p.handle(HostMessage::Start {
            config: [("minutes".into(), ConfigValue::Integer(30))].into(),
            state: legacy.to_vec(),
        })
        .unwrap();
        assert_eq!(p.remaining_ms, 120_000);
        assert_eq!(p.minutes, 30);
        assert!(
            p.started,
            "a saved remainder is evidence the session had been running"
        );
        assert_eq!(clock_text(&p.output().unwrap()), "02:00");
    }
    #[test]
    fn restart_is_paused_and_state_is_plugin_owned() {
        let mut p = Pomodoro {
            minutes: 25,
            remaining_ms: 500,
            running: true,
            ..Default::default()
        };
        let state = p.output().unwrap().state;
        p.handle(HostMessage::Start {
            config: Configuration::new(),
            state,
        })
        .unwrap();
        assert_eq!(p.remaining_ms, 500);
        assert!(!p.running);
    }
}
