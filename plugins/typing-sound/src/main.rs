//! Issue #90: playback policy and resources belong solely to this process.
#![forbid(unsafe_code)]
use bongocat_plugin_sdk::{
    ActionIcon, ConfigValue, Configuration, HostMessage, Plugin, PluginOutput, PluginView, UiNode,
};
use rodio::{Source, buffer::SamplesBuffer};
use std::{
    io::Cursor,
    time::{Duration, Instant},
};

const AUDIO: &[u8] = include_bytes!("../resources/typing.mp3");

/// Every word this plugin publishes, in one language.
///
/// The copy lives here rather than in the host's catalog because the plugin owns
/// its own controls: the host has no typing-sound business rules to translate a
/// preview button from, and a plugin update could not correct a mistranslation
/// without shipping a new application build.
struct Copy {
    volume: &'static str,
    cooldown: &'static str,
    preview: &'static str,
    decode_failed: &'static str,
    output_unavailable: &'static str,
}

const ENGLISH: Copy = Copy {
    volume: "Volume",
    cooldown: "Cooldown (ms)",
    preview: "Preview",
    decode_failed: "The typing sound could not be decoded.",
    output_unavailable: "No audio output device is available.",
};
const SIMPLIFIED_CHINESE: Copy = Copy {
    volume: "音量",
    cooldown: "间隔（毫秒）",
    preview: "试听",
    decode_failed: "无法解码打字音效。",
    output_unavailable: "没有可用的音频输出设备。",
};
const TRADITIONAL_CHINESE: Copy = Copy {
    volume: "音量",
    cooldown: "間隔（毫秒）",
    preview: "試聽",
    decode_failed: "無法解碼打字音效。",
    output_unavailable: "沒有可用的音訊輸出裝置。",
};

/// The copy for a catalog tag, falling back to English.
fn copy_for(locale: &str) -> &'static Copy {
    if locale
        .replace('_', "-")
        .to_ascii_lowercase()
        .starts_with("zh")
    {
        if bongocat_plugin_sdk::is_traditional_chinese(locale) {
            &TRADITIONAL_CHINESE
        } else {
            &SIMPLIFIED_CHINESE
        }
    } else {
        &ENGLISH
    }
}

/// Why the plugin could not audition the sound.
///
/// It is an error code rather than a sentence because the sentence belongs to
/// the language on screen: the plugin renders its own text, so a message written
/// in `play` would be frozen in whatever language the plugin was built with.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum AudioError {
    /// The bundled sample did not decode.
    Decode,
    /// The host has no default output device to play through.
    Output,
}

impl AudioError {
    /// The sentence this error reads as in one language.
    fn copy(self, copy: &Copy) -> &'static str {
        match self {
            Self::Decode => copy.decode_failed,
            Self::Output => copy.output_unavailable,
        }
    }
}

#[derive(Default)]
struct Playback {
    samples: Option<SamplesBuffer>,
    device: Option<rodio::MixerDeviceSink>,
    player: Option<rodio::Player>,
    last_played: Option<Instant>,
}
impl Playback {
    fn play(&mut self, volume: i64) -> Result<(), AudioError> {
        if volume == 0 {
            return Ok(());
        }
        if self.samples.is_none() {
            let decoder =
                rodio::Decoder::try_from(Cursor::new(AUDIO)).map_err(|_| AudioError::Decode)?;
            self.samples = Some(SamplesBuffer::new(
                decoder.channels(),
                decoder.sample_rate(),
                decoder.collect::<Vec<_>>(),
            ));
        }
        if self.device.is_none() {
            let mut device =
                rodio::DeviceSinkBuilder::open_default_sink().map_err(|_| AudioError::Output)?;
            device.log_on_drop(false);
            self.device = Some(device);
        }
        // Replace, never enqueue: rapid typing cannot accumulate an audio backlog.
        self.player.take();
        let player =
            rodio::Player::connect_new(self.device.as_ref().ok_or(AudioError::Output)?.mixer());
        player.set_volume(volume as f32 / 100.0);
        player.append(self.samples.as_ref().ok_or(AudioError::Decode)?.clone());
        self.player = Some(player);
        self.last_played = Some(Instant::now());
        Ok(())
    }
    fn stop(&mut self) {
        self.player.take();
        self.device.take();
        self.last_played = None;
    }
    fn idle(&mut self) {
        if self
            .last_played
            .is_some_and(|time| time.elapsed() >= Duration::from_secs(2))
        {
            self.stop();
        }
    }
}

struct TypingSound {
    volume: i64,
    cooldown_ms: i64,
    last_activity: Option<u64>,
    paused: bool,
    playback: Playback,
    error: Option<AudioError>,
    /// The catalog tag the host last named, or `None` before it names one.
    ///
    /// Nothing here is persisted: the language a user is reading is a property
    /// of the running session, not of the sound settings.
    locale: Option<String>,
}
impl Default for TypingSound {
    fn default() -> Self {
        Self {
            volume: 50,
            cooldown_ms: 150,
            last_activity: None,
            paused: false,
            playback: Playback::default(),
            error: None,
            locale: None,
        }
    }
}
impl TypingSound {
    /// The copy the panels should draw right now.
    fn copy(&self) -> &'static Copy {
        self.locale.as_deref().map_or(&ENGLISH, copy_for)
    }

    /// Whether the preview sound is still audible.
    ///
    /// The preview button reports this rather than the plugin publishing a
    /// "playing…" line beside it: the button is the control the user pressed, so
    /// it is the thing that has to show the action is still in flight.
    fn previewing(&self) -> bool {
        self.playback
            .player
            .as_ref()
            .is_some_and(|player| !player.empty())
    }

    fn configure(&mut self, config: &Configuration) -> Result<(), String> {
        if config
            .keys()
            .any(|key| key != "volume" && key != "cooldown_ms")
        {
            return Err("Unknown configuration key".into());
        }
        let integer = |key: &str, default, min, max| match config.get(key) {
            None => Ok(default),
            Some(ConfigValue::Integer(value)) if (min..=max).contains(value) => Ok(*value),
            _ => Err(format!("Invalid {key}")),
        };
        let volume = integer("volume", 50, 0, 100)?;
        let cooldown = integer("cooldown_ms", 150, 50, 2000)?;
        self.volume = volume;
        self.cooldown_ms = cooldown;
        if volume == 0 {
            self.playback.stop();
        }
        Ok(())
    }
    fn admit(&mut self, now: u64) -> bool {
        if self.paused
            || self.volume == 0
            || self
                .last_activity
                .is_some_and(|last| now.saturating_sub(last) < self.cooldown_ms as u64)
        {
            return false;
        }
        self.last_activity = Some(now);
        true
    }
    fn play(&mut self) {
        self.error = self.playback.play(self.volume).err();
    }
    fn output(&self) -> PluginOutput {
        let copy = self.copy();
        let mut nodes = vec![
            UiNode::Integer {
                key: "volume".into(),
                label: copy.volume.into(),
                value: self.volume,
                min: 0,
                max: 100,
            },
            UiNode::Integer {
                key: "cooldown_ms".into(),
                label: copy.cooldown.into(),
                value: self.cooldown_ms,
                min: 50,
                max: 2000,
            },
            UiNode::Button {
                id: "preview".into(),
                label: copy.preview.into(),
                // Auditioning is what this control is, whatever state it is in,
                // so the glyph is the speaker rather than a pointer that says
                // nothing about the action.
                icon: Some(ActionIcon::Volume),
                loading: self.previewing(),
            },
        ];
        if let Some(error) = &self.error {
            nodes.push(UiNode::Text {
                text: error.copy(copy).to_owned(),
            });
        }
        PluginOutput {
            settings: PluginView {
                nodes,
                ..Default::default()
            },
            config: Configuration::from([
                ("volume".into(), ConfigValue::Integer(self.volume)),
                ("cooldown_ms".into(), ConfigValue::Integer(self.cooldown_ms)),
            ]),
            ..Default::default()
        }
    }
}
impl Plugin for TypingSound {
    fn id(&self) -> &'static str {
        "com.bongocat.typing-sound"
    }
    fn handle(&mut self, message: HostMessage) -> Result<PluginOutput, String> {
        match message {
            HostMessage::Start { config, .. } => {
                self.configure(&config)?;
                self.paused = false;
                self.last_activity = None;
            }
            HostMessage::Configure { config } => self.configure(&config)?,
            HostMessage::KeyboardActivity { elapsed_ms } => {
                if self.admit(elapsed_ms) {
                    self.play();
                }
            }
            HostMessage::Tick { .. } => self.playback.idle(),
            HostMessage::Action { id } if id == "preview" => self.play(),
            HostMessage::Action { .. } => return Err("Unknown action".into()),
            // The host resolved the tag before sending it, and the plugin only
            // republishes its own labels, so the sound settings are untouched.
            HostMessage::Locale { locale } => self.locale = Some(locale),
            HostMessage::Stop | HostMessage::Pause => {
                self.paused = true;
                self.playback.stop();
            }
            HostMessage::Resume => {
                self.paused = false;
                self.last_activity = None;
            }
            HostMessage::Reset => {
                self.playback.stop();
                *self = Self::default();
            }
            HostMessage::Handshake { .. } => return Err("Handshake belongs to SDK".into()),
        }
        Ok(self.output())
    }
}
fn main() {
    let _ = bongocat_plugin_sdk::run(&mut TypingSound::default());
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn supplied_audio_decodes_and_is_bounded() {
        let decoder = rodio::Decoder::try_from(Cursor::new(AUDIO)).unwrap();
        assert!(
            decoder
                .total_duration()
                .is_some_and(|duration| duration < Duration::from_secs(2))
        );
        assert!(decoder.count() > 0);
    }
    #[test]
    fn cooldown_mute_pause_and_resume_are_plugin_policy() {
        let mut plugin = TypingSound::default();
        assert!(plugin.admit(0));
        assert!(!plugin.admit(149));
        assert!(plugin.admit(150));
        plugin.handle(HostMessage::Pause).unwrap();
        assert!(!plugin.admit(500));
        plugin.handle(HostMessage::Resume).unwrap();
        assert!(plugin.admit(501));
        plugin
            .configure(&Configuration::from([(
                "volume".into(),
                ConfigValue::Integer(0),
            )]))
            .unwrap();
        assert!(!plugin.admit(1000));
    }
    #[test]
    fn old_empty_configuration_loads_and_invalid_edits_do_not_mutate() {
        let mut plugin = TypingSound::default();
        assert!(
            plugin
                .handle(HostMessage::Start {
                    config: Configuration::new(),
                    state: Vec::new()
                })
                .unwrap()
                .validate()
        );
        assert!(
            plugin
                .configure(&Configuration::from([(
                    "volume".into(),
                    ConfigValue::Integer(101)
                )]))
                .is_err()
        );
        assert_eq!(plugin.volume, 50);
        assert!(
            plugin
                .configure(&Configuration::from([(
                    "secret".into(),
                    ConfigValue::Boolean(true)
                )]))
                .is_err()
        );
    }

    /// The settings panel is the plugin's own copy, so switching language
    /// re-labels it without touching what the user has configured.
    #[test]
    fn the_host_locale_relabels_the_settings_panel() {
        let mut plugin = TypingSound::default();
        assert_eq!(integer_label(&plugin.output(), "volume"), "Volume");
        assert_eq!(button_label(&plugin.output(), "preview"), "Preview");

        let localized = plugin
            .handle(HostMessage::Locale {
                locale: "zh-CN".into(),
            })
            .unwrap();
        assert_eq!(integer_label(&localized, "volume"), "音量");
        assert_eq!(integer_label(&localized, "cooldown_ms"), "间隔（毫秒）");
        assert_eq!(button_label(&localized, "preview"), "试听");
        assert_eq!(plugin.volume, 50);

        let traditional = plugin
            .handle(HostMessage::Locale {
                locale: "zh-TW".into(),
            })
            .unwrap();
        assert_eq!(button_label(&traditional, "preview"), "試聽");

        // A language this plugin does not carry reads English rather than the
        // raw tag the host happened to send.
        let unknown = plugin
            .handle(HostMessage::Locale {
                locale: "de-DE".into(),
            })
            .unwrap();
        assert_eq!(button_label(&unknown, "preview"), "Preview");
    }

    /// A playback failure is reported in the language on screen, and a language
    /// change re-reads the message rather than leaving the old one behind.
    #[test]
    fn a_playback_failure_reads_the_language_on_screen() {
        // The failure is injected rather than provoked: this asserts the copy a
        // failure reads as, not that this machine has (or lacks) an output device.
        let mut plugin = TypingSound {
            error: Some(AudioError::Output),
            ..Default::default()
        };
        let english = plugin.output();
        assert_eq!(error_text(&english), "No audio output device is available.");

        let localized = plugin
            .handle(HostMessage::Locale {
                locale: "zh-CN".into(),
            })
            .unwrap();
        assert_eq!(error_text(&localized), "没有可用的音频输出设备。");

        // A language the plugin does not carry reads English rather than the raw
        // tag the host sent, exactly as its own labels do.
        let unknown = plugin
            .handle(HostMessage::Locale {
                locale: "de-DE".into(),
            })
            .unwrap();
        assert_eq!(error_text(&unknown), "No audio output device is available.");
    }

    /// Auditioning is what the preview button is, so its glyph never changes,
    /// and the in-flight state is the button's own rather than a line of
    /// placeholder text beside it.
    #[test]
    fn the_preview_button_carries_its_own_glyph_and_progress() {
        use bongocat_plugin_sdk::ActionIcon;
        let plugin = TypingSound::default();
        let output = plugin.output();
        assert_eq!(button_icon(&output, "preview"), Some(ActionIcon::Volume));
        assert!(!button_loading(&output, "preview"));
        assert!(
            !output
                .settings
                .nodes
                .iter()
                .any(|node| matches!(node, UiNode::Text { .. })),
            "progress is the button's own state, not a published line"
        );
    }

    /// The label one settings button carries, by the action it publishes.
    fn button_label<'a>(output: &'a PluginOutput, id: &str) -> &'a str {
        match output.settings.nodes.iter().find_map(|node| match node {
            UiNode::Button {
                id: node_id, label, ..
            } if node_id == id => Some(label.as_str()),
            _ => None,
        }) {
            Some(label) => label,
            None => panic!("the settings panel always publishes the {id} button"),
        }
    }

    /// The glyph one settings button asks the host to draw.
    fn button_icon(output: &PluginOutput, id: &str) -> Option<ActionIcon> {
        output
            .settings
            .nodes
            .iter()
            .find_map(|node| match node {
                UiNode::Button {
                    id: node_id, icon, ..
                } if node_id == id => Some(*icon),
                _ => None,
            })
            .unwrap_or_else(|| panic!("the settings panel always publishes the {id} button"))
    }

    /// Whether one settings button is carrying out its action right now.
    fn button_loading(output: &PluginOutput, id: &str) -> bool {
        output
            .settings
            .nodes
            .iter()
            .find_map(|node| match node {
                UiNode::Button {
                    id: node_id,
                    loading,
                    ..
                } if node_id == id => Some(*loading),
                _ => None,
            })
            .unwrap_or_else(|| panic!("the settings panel always publishes the {id} button"))
    }

    /// The label of one numeric setting the settings panel publishes.
    fn integer_label<'a>(output: &'a PluginOutput, key: &str) -> &'a str {
        match output.settings.nodes.iter().find_map(|node| match node {
            UiNode::Integer {
                key: node_key,
                label,
                ..
            } if node_key == key => Some(label.as_str()),
            _ => None,
        }) {
            Some(label) => label,
            None => panic!("the settings panel always publishes the {key} field"),
        }
    }

    /// The one failure message the settings panel publishes, if it published one.
    fn error_text(output: &PluginOutput) -> &str {
        output
            .settings
            .nodes
            .iter()
            .find_map(|node| match node {
                UiNode::Text { text } => Some(text.as_str()),
                _ => None,
            })
            .expect("the panel publishes the failure it is reporting")
    }
}
