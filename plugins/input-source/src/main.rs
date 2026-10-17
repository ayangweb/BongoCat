//! A read-only input source indicator. Policy belongs here, not in the host.
#![forbid(unsafe_code)]

use bongocat_platform::{InputSource, InputSourceError, InputSourceMonitor};
use bongocat_plugin_sdk::{
    Anchor, ConfigValue, Configuration, HostMessage, Plugin, PluginOutput, PluginView,
    SelectOption, StatusIcon, UiNode,
};

struct Copy {
    mode: &'static str,
    always: &'static str,
    on_switch: &'static str,
    seconds: &'static str,
    unavailable: &'static str,
    paused: &'static str,
}

const ENGLISH: Copy = Copy {
    mode: "Display mode",
    always: "Always show",
    on_switch: "Show after switching",
    seconds: "Display duration (seconds)",
    unavailable: "Current input source is unavailable. Retrying automatically.",
    paused: "Input source observation is paused.",
};
const CHINESE: Copy = Copy {
    mode: "显示方式",
    always: "始终显示",
    on_switch: "切换后显示",
    seconds: "显示时长（秒）",
    unavailable: "暂时无法获取当前输入法，正在自动重试。",
    paused: "输入法检测已暂停。",
};

struct InputSourceIndicator {
    monitor: Option<InputSourceMonitor>,
    switch_only: bool,
    seconds: i64,
    locale: String,
    source: Option<InputSource>,
    now_ms: u64,
    visible_until: Option<u64>,
    paused: bool,
}

impl Default for InputSourceIndicator {
    fn default() -> Self {
        Self {
            monitor: None,
            switch_only: false,
            seconds: 2,
            locale: String::new(),
            source: None,
            now_ms: 0,
            visible_until: None,
            paused: false,
        }
    }
}

impl InputSourceIndicator {
    fn configure(&mut self, config: &Configuration) -> Result<(), String> {
        if config
            .keys()
            .any(|key| !matches!(key.as_str(), "mode" | "seconds"))
        {
            return Err("Unknown configuration key".into());
        }
        let switch_only = match config.get("mode") {
            None => false,
            Some(ConfigValue::Text(value)) if value == "always" => false,
            Some(ConfigValue::Text(value)) if value == "switch" => true,
            _ => return Err("Invalid display mode".into()),
        };
        let seconds = match config.get("seconds") {
            None => 2,
            Some(ConfigValue::Integer(value)) if (1..=10).contains(value) => *value,
            _ => return Err("Display duration must be between 1 and 10 seconds".into()),
        };
        if switch_only != self.switch_only {
            self.visible_until = None;
        }
        self.switch_only = switch_only;
        self.seconds = seconds;
        Ok(())
    }

    fn read_source(&mut self) {
        if self.monitor.is_none() {
            self.monitor = InputSourceMonitor::new().ok();
        }
        let source = self
            .monitor
            .as_mut()
            .ok_or(InputSourceError)
            .and_then(InputSourceMonitor::snapshot);
        self.observe(source);
    }

    /// An unavailable interval breaks the baseline. Recovery is not a switch,
    /// and repeated snapshots/locale changes must never extend a transient.
    fn observe(&mut self, sample: Result<InputSource, InputSourceError>) {
        let source = sample.ok().and_then(|mut source| {
            source.name = source
                .name
                .chars()
                .filter(|c| !c.is_control())
                .take(120)
                .collect();
            (!source.id.is_empty() && !source.name.trim().is_empty()).then_some(source)
        });
        match (&self.source, &source) {
            (Some(previous), Some(current)) if previous.id != current.id => {
                self.visible_until = Some(self.now_ms.saturating_add(self.seconds as u64 * 1000));
            }
            (_, None) => self.visible_until = None,
            _ => {}
        }
        self.source = source;
    }

    fn status(&self) -> Option<UiNode> {
        let source = self.source.as_ref()?;
        let icon = source
            .icon_rgba
            .as_ref()
            .filter(|pixels| pixels.len() == 4096)
            .map(|pixels| StatusIcon::Rgba(pixels.clone()))
            .unwrap_or_else(|| StatusIcon::Badge(source.name.trim().chars().take(2).collect()));
        Some(UiNode::IconText {
            icon,
            text: source.name.clone(),
        })
    }

    fn output(&self) -> PluginOutput {
        let copy = if bongocat_plugin_sdk::plugin_locale(&self.locale) == "zh-CN" {
            &CHINESE
        } else {
            &ENGLISH
        };
        let mode = if self.switch_only { "switch" } else { "always" };
        let status = self.status();
        let visible = !self.paused
            && (!self.switch_only || self.visible_until.is_some_and(|end| self.now_ms < end));
        let mut settings = vec![
            status.clone().unwrap_or_else(|| UiNode::Text {
                text: if self.paused {
                    copy.paused
                } else {
                    copy.unavailable
                }
                .into(),
            }),
            UiNode::Select {
                key: "mode".into(),
                label: copy.mode.into(),
                value: mode.into(),
                options: [("always", copy.always), ("switch", copy.on_switch)]
                    .into_iter()
                    .map(|(value, label)| SelectOption {
                        value: value.into(),
                        label: label.into(),
                        group: None,
                    })
                    .collect(),
                preview: false,
            },
        ];
        if self.switch_only {
            settings.push(UiNode::Integer {
                key: "seconds".into(),
                label: copy.seconds.into(),
                value: self.seconds,
                min: 1,
                max: 10,
            });
        }
        PluginOutput {
            model: PluginView {
                anchor: Anchor::BottomRight,
                nodes: if visible {
                    status.into_iter().collect()
                } else {
                    vec![]
                },
            },
            settings: PluginView {
                nodes: settings,
                ..Default::default()
            },
            config: [
                ("mode".into(), ConfigValue::Text(mode.into())),
                ("seconds".into(), ConfigValue::Integer(self.seconds)),
            ]
            .into(),
            state: vec![],
        }
    }
}

impl Plugin for InputSourceIndicator {
    fn id(&self) -> &'static str {
        "com.bongocat.input-source"
    }

    fn handle(&mut self, message: HostMessage) -> Result<PluginOutput, String> {
        match message {
            HostMessage::Start { config, .. } => {
                self.configure(&config)?;
                self.source = None;
                self.visible_until = None;
                self.paused = false;
                self.read_source();
            }
            HostMessage::Configure { config } => self.configure(&config)?,
            HostMessage::Tick { elapsed_ms } => {
                self.now_ms = self.now_ms.max(elapsed_ms);
                if !self.paused {
                    self.read_source();
                }
            }
            HostMessage::Locale { locale } => self.locale = locale,
            HostMessage::Reset => {
                self.configure(&Configuration::new())?;
                self.visible_until = None;
            }
            HostMessage::Stop => {
                self.monitor = None;
                self.source = None;
                self.visible_until = None;
            }
            HostMessage::Pause => {
                self.paused = true;
                self.monitor = None;
                self.source = None;
                self.visible_until = None;
            }
            HostMessage::Resume => {
                self.paused = false;
                self.source = None;
                self.visible_until = None;
                self.read_source();
            }
            HostMessage::Handshake { .. } => return Err("Unexpected handshake".into()),
            HostMessage::Action { .. } | HostMessage::Preview { .. } => {
                return Err("Unknown action".into());
            }
            HostMessage::KeyboardActivity { .. } | HostMessage::InputActivity { .. } => {}
        }
        Ok(self.output())
    }
}

fn main() -> std::io::Result<()> {
    bongocat_plugin_sdk::run(&mut InputSourceIndicator::default())
}

#[cfg(test)]
mod tests;
