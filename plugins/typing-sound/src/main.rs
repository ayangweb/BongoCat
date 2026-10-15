//! Issue #90: playback policy and resources belong solely to this process.
#![forbid(unsafe_code)]
use bongocat_plugin_sdk::{
    ConfigValue, Configuration, HostMessage, Plugin, PluginOutput, PluginView, UiNode,
};
use rodio::{Source, buffer::SamplesBuffer};
use std::{
    io::Cursor,
    time::{Duration, Instant},
};

const AUDIO: &[u8] = include_bytes!("../resources/typing.mp3");

#[derive(Default)]
struct Playback {
    samples: Option<SamplesBuffer>,
    device: Option<rodio::MixerDeviceSink>,
    player: Option<rodio::Player>,
    last_played: Option<Instant>,
}
impl Playback {
    fn play(&mut self, volume: i64) -> Result<(), String> {
        if volume == 0 {
            return Ok(());
        }
        if self.samples.is_none() {
            let decoder = rodio::Decoder::try_from(Cursor::new(AUDIO))
                .map_err(|_| "Cannot decode typing sound")?;
            self.samples = Some(SamplesBuffer::new(
                decoder.channels(),
                decoder.sample_rate(),
                decoder.collect::<Vec<_>>(),
            ));
        }
        if self.device.is_none() {
            let mut device = rodio::DeviceSinkBuilder::open_default_sink()
                .map_err(|_| "Audio output unavailable")?;
            device.log_on_drop(false);
            self.device = Some(device);
        }
        // Replace, never enqueue: rapid typing cannot accumulate an audio backlog.
        self.player.take();
        let player = rodio::Player::connect_new(
            self.device
                .as_ref()
                .ok_or("Audio output unavailable")?
                .mixer(),
        );
        player.set_volume(volume as f32 / 100.0);
        player.append(self.samples.as_ref().ok_or("Audio is unavailable")?.clone());
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
    error: Option<String>,
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
        }
    }
}
impl TypingSound {
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
        let mut nodes = vec![
            UiNode::Integer {
                key: "volume".into(),
                label: "Volume / 音量".into(),
                value: self.volume,
                min: 0,
                max: 100,
            },
            UiNode::Integer {
                key: "cooldown_ms".into(),
                label: "Cooldown (ms) / 间隔（毫秒）".into(),
                value: self.cooldown_ms,
                min: 50,
                max: 2000,
            },
            UiNode::Button {
                id: "preview".into(),
                label: "Preview / 试听".into(),
            },
        ];
        if self
            .playback
            .player
            .as_ref()
            .is_some_and(|player| !player.empty())
        {
            nodes.push(UiNode::Text {
                text: "Playing / 正在播放".into(),
            });
        }
        if let Some(error) = &self.error {
            nodes.push(UiNode::Text {
                text: error.clone(),
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
}
