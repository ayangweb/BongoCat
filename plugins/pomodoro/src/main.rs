//! Standalone Pomodoro plugin. The application knows no timer business rules.
#![forbid(unsafe_code)]
use bongocat_plugin_sdk::{
    ActionIcon, Anchor, ConfigValue, Configuration, HostMessage, Plugin, PluginOutput, PluginView,
    UiNode,
};
use serde::{Deserialize, Serialize};

/// Every word this plugin publishes, in one language.
///
/// The copy lives here rather than in the host's catalog because the plugin owns
/// its own controls: a host that translated them would be guessing at business
/// rules it deliberately does not have, and a plugin update could not correct a
/// mistranslation without shipping a new application build.
struct Copy {
    start: &'static str,
    pause: &'static str,
    restart: &'static str,
    minutes: &'static str,
}

const ENGLISH: Copy = Copy {
    start: "Start",
    pause: "Pause",
    restart: "Restart",
    minutes: "Minutes",
};
const SIMPLIFIED_CHINESE: Copy = Copy {
    start: "开始",
    pause: "暂停",
    restart: "重新计时",
    minutes: "时长（分钟）",
};

/// The copy for a catalog tag, falling back to English.
///
/// The host sends a tag it already resolved, so this only has to pick a
/// language out of the tags it knows; an unknown tag reads English rather than
/// showing a raw locale to the user.
fn copy_for(locale: &str) -> &'static Copy {
    if bongocat_plugin_sdk::plugin_locale(locale) == "zh-CN" {
        &SIMPLIFIED_CHINESE
    } else {
        &ENGLISH
    }
}

#[derive(Serialize, Deserialize)]
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
    /// The catalog tag the host last named, or `None` before it names one.
    ///
    /// It is deliberately skipped by `serde`: the language a user is reading is
    /// a property of the running session, not of the saved timer, and persisting
    /// it would make a state file depend on the language it was written in.
    #[serde(skip)]
    locale: Option<String>,
}
impl Default for Pomodoro {
    fn default() -> Self {
        Self {
            remaining_ms: 0,
            running: false,
            started: false,
            last_tick: None,
            minutes: default_minutes(),
            locale: None,
        }
    }
}
fn default_minutes() -> i64 {
    25
}
impl Pomodoro {
    /// The copy the panel and the settings panel should draw right now.
    fn copy(&self) -> &'static Copy {
        self.locale.as_deref().map_or(&ENGLISH, copy_for)
    }
}
impl Pomodoro {
    fn configure(&mut self, config: &Configuration) -> Result<(), String> {
        if config.keys().any(|key| key != "minutes") {
            return Err("Unknown configuration key".into());
        }
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
        let copy = self.copy();
        let text = match (self.started, self.remaining_ms) {
            (false, _) => format_clock(self.session_ms()),
            (_, remaining) => format_clock(remaining),
        };
        let model = PluginView {
            anchor: Anchor::TopRight,
            nodes: vec![
                UiNode::Text { text },
                // The icon names what the button does in this frame, so the
                // control does not read as one fixed glyph whatever the timer is
                // doing: pause while it counts, start while it does not.
                UiNode::Button {
                    id: "toggle".into(),
                    label: if self.running { copy.pause } else { copy.start }.into(),
                    icon: Some(if self.running {
                        ActionIcon::Pause
                    } else {
                        ActionIcon::Play
                    }),
                    loading: false,
                },
                UiNode::Button {
                    id: "restart".into(),
                    label: copy.restart.into(),
                    icon: Some(ActionIcon::Restart),
                    loading: false,
                },
            ],
        };
        Ok(PluginOutput {
            model,
            settings: PluginView {
                nodes: vec![UiNode::Integer {
                    key: "minutes".into(),
                    label: copy.minutes.into(),
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

/// The label one panel button carries, by the action it publishes.
#[cfg(test)]
fn button_label<'a>(output: &'a PluginOutput, id: &str) -> &'a str {
    match output.model.nodes.iter().find_map(|node| match node {
        UiNode::Button {
            id: node_id, label, ..
        } if node_id == id => Some(label.as_str()),
        _ => None,
    }) {
        Some(label) => label,
        None => panic!("the panel always publishes the {id} button"),
    }
}

/// The glyph one panel button asks the host to draw.
#[cfg(test)]
fn button_icon(output: &PluginOutput, id: &str) -> Option<ActionIcon> {
    output
        .model
        .nodes
        .iter()
        .find_map(|node| match node {
            UiNode::Button {
                id: node_id, icon, ..
            } if node_id == id => Some(*icon),
            _ => None,
        })
        .unwrap_or_else(|| panic!("the panel always publishes the {id} button"))
}

/// The label of the single numeric setting the settings panel publishes.
#[cfg(test)]
fn integer_label(output: &PluginOutput) -> &str {
    match output.settings.nodes.iter().find_map(|node| match node {
        UiNode::Integer { label, .. } => Some(label.as_str()),
        _ => None,
    }) {
        Some(label) => label,
        None => panic!("the settings panel always publishes the minutes field"),
    }
}
impl Plugin for Pomodoro {
    fn id(&self) -> &'static str {
        "com.bongocat.pomodoro"
    }
    fn handle(&mut self, message: HostMessage) -> Result<PluginOutput, String> {
        match message {
            HostMessage::Start { mut config, state } => {
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
                // Earlier configurations ignored unknown keys. Keep those saved
                // sessions readable while rejecting new edits with unknown keys.
                config.retain(|key, _| key == "minutes");
                self.configure(&config)?;
            }
            HostMessage::Configure { config } => self.configure(&config)?,
            // The pomodoro counts time, not keys: both activity messages say
            // only that some device moved, which is nothing a timer needs.
            HostMessage::KeyboardActivity { .. } | HostMessage::InputActivity { .. } => {}
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
            // The host resolved the tag before sending it, and the plugin only
            // republishes its own views, so nothing about the timer's own state
            // changes: the clock, the remaining time and the running flag stay
            // exactly as they were.
            HostMessage::Locale { locale } => self.locale = Some(locale),
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
                    locale: self.locale.take(),
                    ..Default::default()
                };
            }
            HostMessage::Handshake { .. } => return Err("Handshake belongs to SDK".into()),
        }
        self.output()
    }
}
fn main() -> std::io::Result<()> {
    bongocat_plugin_sdk::run(&mut Pomodoro::default())
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
        assert!(
            p.configure(&[("unknown".into(), ConfigValue::Integer(1))].into())
                .is_err()
        );
        assert_eq!(p.minutes, 25);
    }

    #[test]
    fn defaults_and_reset_are_valid_and_keep_the_session_language() {
        let mut p = Pomodoro::default();
        assert!(p.output().unwrap().validate());
        p.handle(HostMessage::Locale {
            locale: "zh-CN".into(),
        })
        .unwrap();
        p.configure(&[("minutes".into(), ConfigValue::Integer(5))].into())
            .unwrap();
        let output = p.handle(HostMessage::Reset).unwrap();
        assert!(output.validate());
        assert_eq!(p.minutes, 25);
        assert_eq!(button_label(&output, "toggle"), "开始");
    }

    #[test]
    fn saved_configuration_keeps_the_previous_unknown_key_tolerance() {
        let output = Pomodoro::default()
            .handle(HostMessage::Start {
                config: [
                    ("minutes".into(), ConfigValue::Integer(5)),
                    ("unused".into(), ConfigValue::Boolean(true)),
                ]
                .into(),
                state: Vec::new(),
            })
            .unwrap();
        assert_eq!(clock_text(&output), "05:00");
        assert_eq!(output.config.len(), 1);
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

    /// The panel's controls are its own copy, so switching language re-labels
    /// them without touching the timer the user is watching.
    #[test]
    fn the_host_locale_relabels_the_controls_and_preserves_the_timer() {
        let mut p = Pomodoro {
            minutes: 25,
            ..Default::default()
        };
        p.handle(HostMessage::Tick { elapsed_ms: 0 }).unwrap();
        p.handle(HostMessage::Action {
            id: "toggle".into(),
        })
        .unwrap();
        p.handle(HostMessage::Tick { elapsed_ms: 5_000 }).unwrap();
        assert_eq!(clock_text(&p.output().unwrap()), "24:55");
        assert_eq!(button_label(&p.output().unwrap(), "toggle"), "Pause");

        let localized = p
            .handle(HostMessage::Locale {
                locale: "zh-CN".into(),
            })
            .unwrap();
        // The running timer and the clock it shows are the plugin's own state,
        // so a language change cannot disturb them.
        assert_eq!(clock_text(&localized), "24:55");
        assert_eq!(button_label(&localized, "toggle"), "暂停");
        assert_eq!(button_label(&localized, "restart"), "重新计时");
        let minutes = integer_label(&localized);
        assert_eq!(minutes, "时长（分钟）");

        for locale in ["zh_CN", "ZH-cn"] {
            let localized = p
                .handle(HostMessage::Locale {
                    locale: locale.into(),
                })
                .unwrap();
            assert_eq!(button_label(&localized, "restart"), "重新计时", "{locale}");
        }

        for locale in [
            "en-US", "en-GB", "zh-TW", "zh-HK", "zh-MO", "zh-Hant", "zh", "ar-SA", "vi-VN",
            "pt-BR", "ko-KR", "fr-FR", "de-DE", "",
        ] {
            let fallback = p
                .handle(HostMessage::Locale {
                    locale: locale.into(),
                })
                .unwrap();
            assert_eq!(button_label(&fallback, "toggle"), "Pause", "{locale}");
            assert_eq!(button_label(&fallback, "restart"), "Restart", "{locale}");
            assert_eq!(integer_label(&fallback), "Minutes", "{locale}");
            assert_eq!(clock_text(&fallback), "24:55", "{locale}");
            assert!(p.running);
        }
    }

    /// The locale the host named is not part of the saved timer.
    ///
    /// A state file belongs to the timer, not to the language it was written in:
    /// persisting the tag would make one user's language part of another
    /// machine's saved session.
    #[test]
    fn the_locale_is_not_persisted_with_the_timer() {
        let mut p = Pomodoro {
            minutes: 25,
            ..Default::default()
        };
        p.handle(HostMessage::Locale {
            locale: "zh-CN".into(),
        })
        .unwrap();
        let state = p.output().unwrap().state;
        assert!(
            !String::from_utf8_lossy(&state).contains("zh-CN"),
            "the saved state must not carry the display language"
        );
    }

    /// The button glyph follows what the control does in this frame, rather than
    /// staying one fixed icon whatever the timer is doing.
    #[test]
    fn the_toggle_button_names_the_action_it_offers() {
        use bongocat_plugin_sdk::ActionIcon;
        let mut p = Pomodoro {
            minutes: 25,
            ..Default::default()
        };
        assert_eq!(
            button_icon(&p.output().unwrap(), "toggle"),
            Some(ActionIcon::Play)
        );
        p.handle(HostMessage::Action {
            id: "toggle".into(),
        })
        .unwrap();
        assert_eq!(
            button_icon(&p.output().unwrap(), "toggle"),
            Some(ActionIcon::Pause)
        );
        assert_eq!(
            button_icon(&p.output().unwrap(), "restart"),
            Some(ActionIcon::Restart)
        );
    }
}
