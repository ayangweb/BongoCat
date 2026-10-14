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
        let seconds = self.remaining_ms.div_ceil(1000);
        let text = if self.remaining_ms == 0 {
            "Ready / 完成".into()
        } else {
            format!("{:02}:{:02}", seconds / 60, seconds % 60)
        };
        let model = PluginView {
            anchor: Anchor::TopRight,
            nodes: vec![
                UiNode::Text { text },
                UiNode::Button {
                    id: "toggle".into(),
                    label: if self.running {
                        "Pause / 暂停"
                    } else {
                        "Start / 开始"
                    }
                    .into(),
                },
                UiNode::Button {
                    id: "restart".into(),
                    label: "Restart / 重新计时".into(),
                },
            ],
        };
        Ok(PluginOutput {
            model,
            settings: PluginView {
                nodes: vec![UiNode::Integer {
                    key: "minutes".into(),
                    label: "Minutes / 分钟".into(),
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
                }
                self.running = false;
                self.last_tick = None;
                self.configure(&config)?;
            }
            HostMessage::Configure { config } => self.configure(&config)?,
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
                        self.remaining_ms = self.minutes as u64 * 60_000;
                    }
                    self.running = !self.running;
                }
                "restart" => {
                    self.remaining_ms = self.minutes as u64 * 60_000;
                    self.running = true;
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
