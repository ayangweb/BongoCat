//! Issue #90: playback policy and resources belong solely to this process.
//!
//! The plugin plays a press sound and a release sound for the keyboard and for
//! the mouse, out of sound sets embedded in this executable, so nothing it needs
//! is fetched at runtime and no audio dependency or key-sounds rule enters the
//! host. What it is told is device totals plus the physical special keys that
//! Thock addresses by name (ADR-0091/ADR-0093). Everything else — which set is
//! in use, how loud, how often, and what a set with no release sound does — is
//! decided here.
#![forbid(unsafe_code)]

mod packs;

use bongocat_plugin_sdk::{
    ConfigValue, Configuration, HostMessage, InputActivity, Plugin, PluginOutput, PluginView,
    SelectOption, UiNode,
};
use packs::{PACKS, PackAsset};
use rodio::{Source as _, buffer::SamplesBuffer};
use std::{
    collections::BTreeMap,
    io::Cursor,
    time::{Duration, Instant},
};

/// Which device a sound set is for.
///
/// It is the plugin's own split rather than the host's: the host reports a
/// keyboard edge and a mouse edge, and what may be played for each of them is
/// this plugin's business.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PackCategory {
    Keyboard,
    Mouse,
}

impl PackCategory {
    /// The sets embedded for this device, in the order the settings panel lists
    /// them.
    fn packs(self) -> impl Iterator<Item = &'static PackAsset> {
        PACKS.iter().filter(move |pack| pack.category == self)
    }

    /// The set this device plays when nothing else is chosen.
    fn default_pack(self) -> &'static PackAsset {
        self.packs()
            .next()
            .expect("the build embeds at least one set per device")
    }
}

/// One device edge a sound can be played for.
///
/// These four are the whole vocabulary the plugin reacts to. The sound set is
/// chosen per device, so a press and its release always play from the same set.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum Action {
    KeyboardDown,
    KeyboardUp,
    MouseDown,
    MouseUp,
}

/// The key group a keyboard edge plays from.
///
/// These names are the subset of Thock's `KeyMapper` whose physical keys cross
/// the plugin activity contract. Thock maps Backspace to `del`, not
/// `backspace`, and keeps the two Shift keys and left Control separate.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum KeyGroup {
    Default,
    Space,
    Enter,
    Del,
    Tab,
    ShiftLeft,
    ShiftRight,
    CtrlLeft,
}

impl KeyGroup {
    fn name(self) -> &'static str {
        match self {
            Self::Default => "default",
            Self::Space => "space",
            Self::Enter => "enter",
            Self::Del => "del",
            Self::Tab => "tab",
            Self::ShiftLeft => "shiftLeft",
            Self::ShiftRight => "shiftRight",
            Self::CtrlLeft => "ctrlLeft",
        }
    }

    /// The group a single-key batch selects, if the batch is exactly one of the
    /// special-key subsets.
    fn for_edge(activity: InputActivity, before: InputActivity, release: bool) -> Self {
        let total = if release {
            activity.keyboard_up.saturating_sub(before.keyboard_up)
        } else {
            activity.keyboard_down.saturating_sub(before.keyboard_down)
        };
        let special = [
            (
                Self::Space,
                activity.space_down,
                before.space_down,
                activity.space_up,
                before.space_up,
            ),
            (
                Self::Enter,
                activity.enter_down,
                before.enter_down,
                activity.enter_up,
                before.enter_up,
            ),
            (
                Self::Del,
                activity.backspace_down,
                before.backspace_down,
                activity.backspace_up,
                before.backspace_up,
            ),
            (
                Self::Tab,
                activity.tab_down,
                before.tab_down,
                activity.tab_up,
                before.tab_up,
            ),
            (
                Self::ShiftLeft,
                activity.shift_left_down,
                before.shift_left_down,
                activity.shift_left_up,
                before.shift_left_up,
            ),
            (
                Self::ShiftRight,
                activity.shift_right_down,
                before.shift_right_down,
                activity.shift_right_up,
                before.shift_right_up,
            ),
            (
                Self::CtrlLeft,
                activity.ctrl_left_down,
                before.ctrl_left_down,
                activity.ctrl_left_up,
                before.ctrl_left_up,
            ),
        ];
        special
            .into_iter()
            .find_map(|(group, down, before_down, up, before_up)| {
                let delta = if release {
                    up.saturating_sub(before_up)
                } else {
                    down.saturating_sub(before_down)
                };
                (delta == total && delta > 0).then_some(group)
            })
            .unwrap_or(Self::Default)
    }
}

impl Action {
    const ALL: [Self; 4] = [
        Self::KeyboardDown,
        Self::KeyboardUp,
        Self::MouseDown,
        Self::MouseUp,
    ];

    fn category(self) -> PackCategory {
        match self {
            Self::KeyboardDown | Self::KeyboardUp => PackCategory::Keyboard,
            Self::MouseDown | Self::MouseUp => PackCategory::Mouse,
        }
    }

    fn is_release(self) -> bool {
        matches!(self, Self::KeyboardUp | Self::MouseUp)
    }

    fn from_config_key(key: &str) -> Option<Self> {
        match key {
            "keyboard_pack" => Some(Self::KeyboardDown),
            "mouse_pack" => Some(Self::MouseDown),
            _ => None,
        }
    }
}

const DEFAULT_VOLUME: i64 = 100;
const MIN_VOLUME: i64 = 0;
const MAX_VOLUME: i64 = 100;
/// Thock's built-in rapid key filter.
///
/// Thock ignores keyboard events that arrive within 10 ms of the previous one
/// when its "ignore rapid key events" option is on. Key Sounds keeps that
/// behavior always on and applies the same window per action, so a release is
/// never swallowed by the press before it.
const MINIMUM_SOUND_GAP_MS: u64 = 10;
/// How many sounds may be audible at once.
///
/// A press and its release overlap by design, so this is a ceiling on a
/// pathological burst rather than a limit a typist reaches: the oldest sound is
/// dropped only once this many are still playing.
const MAX_CONCURRENT_SOUNDS: usize = 8;
/// How long the output device is kept after the last sound.
const DEVICE_IDLE: Duration = Duration::from_secs(2);

/// Every word this plugin publishes, in one language.
///
/// The copy lives here rather than in the host's catalog because the plugin owns
/// its own controls: the host has no key-sounds business rules to translate a
/// sound set's name from, and a plugin update could not correct a mistranslation
/// without shipping a new application build.
struct Copy {
    volume: &'static str,
    mouse_enabled: &'static str,
    keyboard_pack: &'static str,
    mouse_pack: &'static str,
    no_release: &'static str,
    decode_failed: &'static str,
    output_unavailable: &'static str,
}

const ENGLISH: Copy = Copy {
    volume: "Volume",
    mouse_enabled: "Play mouse sounds",
    keyboard_pack: "Keyboard sound",
    mouse_pack: "Mouse sound",
    no_release: "no release sound",
    decode_failed: "The sound effect could not be decoded.",
    output_unavailable: "No audio output device is available.",
};
const SIMPLIFIED_CHINESE: Copy = Copy {
    volume: "音量",
    mouse_enabled: "播放鼠标音效",
    keyboard_pack: "键盘音效",
    mouse_pack: "鼠标音效",
    no_release: "无弹起音",
    decode_failed: "无法解码音效。",
    output_unavailable: "没有可用的音频输出设备。",
};

/// The copy for the catalog tag the host sends.
///
/// The host delivers the tag this plugin's manifest declares a translation for,
/// so the plugin compares against that one tag instead of resolving tags it does
/// not serve.
fn copy_for(locale: &str) -> &'static Copy {
    if locale == "zh-CN" {
        &SIMPLIFIED_CHINESE
    } else {
        &ENGLISH
    }
}

/// Why the plugin could not play a sound.
///
/// It is an error code rather than a sentence because the sentence belongs to the
/// language on screen: the plugin renders its own text, so a message written
/// while playing a sound would be frozen in whatever language the plugin was
/// built with.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum AudioError {
    /// An embedded sound did not decode.
    Decode,
    /// The host has no default output device to play through.
    Output,
}

impl AudioError {
    fn copy(self, copy: &Copy) -> &'static str {
        match self {
            Self::Decode => copy.decode_failed,
            Self::Output => copy.output_unavailable,
        }
    }
}

/// One decoded sound set.
#[derive(Default)]
struct DecodedPack {
    down: Vec<SamplesBuffer>,
    up: Vec<SamplesBuffer>,
    /// Per-key groups keyed by the name Thock uses in a soundpack config.
    key_groups: BTreeMap<&'static str, DecodedKeyGroup>,
}

#[derive(Default)]
struct DecodedKeyGroup {
    down: Vec<SamplesBuffer>,
    up: Vec<SamplesBuffer>,
}

impl DecodedPack {
    /// The sounds one edge may draw from.
    ///
    /// A set with no release sounds yields an empty slice rather than reusing its
    /// press sounds: the release of a switch nobody recorded is silence, and a
    /// second press would be a sound that recording never contained.
    fn sounds(&self, action: Action) -> &[SamplesBuffer] {
        if action.is_release() {
            &self.up
        } else {
            &self.down
        }
    }

    /// The recordings one keyboard edge plays, resolved exactly as Thock does:
    /// the named group when the pack carries it, otherwise `default`. A group
    /// that exists but has no recording for this direction is silent; it does
    /// not fall back a second time.
    ///
    /// Mouse edges have no key group and keep the device-level lists.
    fn keyboard_sounds(&self, action: Action, group: KeyGroup) -> &[SamplesBuffer] {
        let Some(group) = self.key_groups.get(group.name()) else {
            return self.sounds(action);
        };
        if action.is_release() {
            &group.up
        } else {
            &group.down
        }
    }
}

/// The sound sets this process has decoded, keyed by the set's id.
///
/// A set is decoded the first time it is played and dropped when the
/// configuration stops naming it, so what a user's choices cost in memory is the
/// four sets those choices resolve to rather than every set that ships. A
/// failure is remembered with the same map: a sound that cannot be decoded would
/// otherwise be decoded again on every keystroke.
#[derive(Default)]
struct Library {
    decoded: BTreeMap<&'static str, Result<DecodedPack, AudioError>>,
}

impl Library {
    fn load(&mut self, pack: &'static PackAsset) -> Result<&DecodedPack, AudioError> {
        self.decoded
            .entry(pack.id)
            .or_insert_with(|| decode_pack(pack))
            .as_ref()
            .map_err(|error| *error)
    }

    /// Keep only the sets the given choices resolve to.
    fn retain(&mut self, keep: &[&'static str]) {
        self.decoded.retain(|id, _| keep.contains(id));
    }
}

/// Decode one embedded set.
///
/// The bytes are the files the set names, because the build embedded exactly
/// them, so a sound that will not decode is a build error rather than user data —
/// it is reported as a decode failure, which is what the panel says when a sound
/// cannot be prepared, instead of panicking inside a process the host would then
/// have to restart.
fn decode_pack(pack: &'static PackAsset) -> Result<DecodedPack, AudioError> {
    let mut decoded = DecodedPack::default();
    for (release, files) in [(false, pack.down), (true, pack.up)] {
        for bytes in files {
            let sound = decode_sound(bytes)?;
            if release {
                decoded.up.push(sound);
            } else {
                decoded.down.push(sound);
            }
        }
    }
    for group in pack.key_groups {
        let mut sounds = DecodedKeyGroup::default();
        for (release, files) in [(false, group.down), (true, group.up)] {
            for bytes in files {
                let sound = decode_sound(bytes)?;
                if release {
                    sounds.up.push(sound);
                } else {
                    sounds.down.push(sound);
                }
            }
        }
        decoded.key_groups.insert(group.name, sounds);
    }
    Ok(decoded)
}

/// Decode one sound file into the samples a player can be handed.
fn decode_sound(bytes: &[u8]) -> Result<SamplesBuffer, AudioError> {
    let decoder =
        rodio::Decoder::try_from(Cursor::new(bytes.to_vec())).map_err(|_| AudioError::Decode)?;
    Ok(SamplesBuffer::new(
        decoder.channels(),
        decoder.sample_rate(),
        decoder.collect::<Vec<_>>(),
    ))
}

/// The audio output, and what has been played through it.
#[derive(Default)]
struct Playback {
    device: Option<rodio::MixerDeviceSink>,
    /// Sounds still audible, oldest first.
    players: Vec<rodio::Player>,
    /// When each action last played, on the monotonic clock the host sends.
    ///
    /// It is per action rather than one timestamp because a press and its release
    /// are the two sounds a user most expects to hear in sequence: one clock
    /// shared by both would swallow the release of every quick keystroke.
    last_played: BTreeMap<Action, u64>,
    last_audible: Option<Instant>,
    previewing: bool,
}

impl Playback {
    /// Play one sound, keeping at most [`MAX_CONCURRENT_SOUNDS`] audible.
    fn play(&mut self, sound: SamplesBuffer, volume: i64) -> Result<(), AudioError> {
        if self.device.is_none() {
            let mut device =
                rodio::DeviceSinkBuilder::open_default_sink().map_err(|_| AudioError::Output)?;
            device.log_on_drop(false);
            self.device = Some(device);
        }
        let mixer = self
            .device
            .as_ref()
            .ok_or(AudioError::Output)?
            .mixer()
            .clone();
        let player = rodio::Player::connect_new(&mixer);
        player.set_volume(volume as f32 / 100.0);
        player.append(sound);
        if self.players.len() >= MAX_CONCURRENT_SOUNDS {
            self.players.remove(0);
        }
        self.players.push(player);
        self.last_audible = Some(Instant::now());
        Ok(())
    }

    /// Whether anything is still audible.
    fn playing(&self) -> bool {
        self.players.iter().any(|player| !player.empty())
    }

    fn clear_voices(&mut self) {
        self.players.clear();
        self.previewing = false;
    }

    fn preview_active(&mut self) -> bool {
        if self.previewing && !self.playing() {
            self.previewing = false;
        }
        self.previewing
    }

    fn stop(&mut self) {
        self.clear_voices();
        self.device.take();
        self.last_played.clear();
        self.last_audible = None;
    }

    /// Drop finished players, and release the device once nothing has played for
    /// a while.
    fn idle(&mut self) {
        self.players.retain(|player| !player.empty());
        if self.players.is_empty() {
            self.previewing = false;
        }
        if !self.playing()
            && self
                .last_audible
                .is_some_and(|time| time.elapsed() >= DEVICE_IDLE)
        {
            self.stop();
        }
    }
}

/// The sound sets a user chose, one per device.
struct Choices {
    keyboard: String,
    mouse: String,
}

impl Choices {
    fn from_config(config: &Configuration) -> Self {
        let text = |key: &str, default: &'static str| match config.get(key) {
            Some(ConfigValue::Text(value)) => value.clone(),
            _ => default.to_owned(),
        };
        Self {
            keyboard: text("keyboard_pack", PackCategory::Keyboard.default_pack().id),
            mouse: text("mouse_pack", PackCategory::Mouse.default_pack().id),
        }
    }
}

/// The sets one user's choices resolve to.
struct Selection {
    keyboard: &'static PackAsset,
    mouse: &'static PackAsset,
}

impl Selection {
    /// Resolve the stored choices, falling back to each device's default set.
    ///
    /// A stored id this build does not carry resolves to the default rather than
    /// failing the exchange, and the resolved id is what the plugin publishes, so
    /// a value that no longer names anything repairs itself on the first frame.
    fn resolve(choices: &Choices) -> Self {
        let device = |category: PackCategory, stored: &str| {
            pack_by_id(stored)
                .filter(|pack| pack.category == category)
                .unwrap_or_else(|| category.default_pack())
        };
        let keyboard = device(PackCategory::Keyboard, &choices.keyboard);
        let mouse = device(PackCategory::Mouse, &choices.mouse);
        Self { keyboard, mouse }
    }

    /// The set one action plays from.
    fn pack(&self, action: Action) -> &'static PackAsset {
        if action.category() == PackCategory::Keyboard {
            self.keyboard
        } else {
            self.mouse
        }
    }

    /// Every set this selection can play, for pruning the decoded cache.
    fn ids(&self) -> Vec<&'static str> {
        vec![self.keyboard.id, self.mouse.id]
    }
}

/// The set a stored id names, if this build carries it.
fn pack_by_id(id: &str) -> Option<&'static PackAsset> {
    PACKS.iter().find(|pack| pack.id == id)
}

/// A random-enough source for picking among the sounds of a set.
///
/// Choosing among a set's variants is the only thing randomness is for here, so
/// this is a `SplitMix64` step rather than a dependency: the clock seeds it, and
/// nothing about the result needs to be unpredictable.
struct Pick(u64);

impl Pick {
    fn new() -> Self {
        let seed = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |elapsed| elapsed.as_nanos() as u64);
        Self(seed ^ 0x9E37_79B9_7F4A_7C15)
    }

    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut value = self.0;
        value = (value ^ (value >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        value = (value ^ (value >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        value ^ (value >> 31)
    }

    /// One item of a slice, if it has any.
    fn pick<'a, T>(&mut self, items: &'a [T]) -> Option<&'a T> {
        if items.is_empty() {
            return None;
        }
        items.get((self.next() % items.len() as u64) as usize)
    }
}

struct KeySounds {
    volume: i64,
    mouse_enabled: bool,
    choices: Choices,
    selection: Selection,
    /// The counters from the last activity message after the host established
    /// the initial baseline. `None` prevents cumulative counters that predate
    /// this process from being replayed as fresh key edges.
    activity: Option<InputActivity>,
    paused: bool,
    library: Library,
    playback: Playback,
    pick: Pick,
    error: Option<AudioError>,
    /// The catalog tag the host last named, or `None` before it names one.
    ///
    /// Nothing here is persisted: the language a user is reading is a property of
    /// the running session, not of the sound settings.
    locale: Option<String>,
}

impl Default for KeySounds {
    fn default() -> Self {
        let choices = Choices::from_config(&Configuration::new());
        let selection = Selection::resolve(&choices);
        Self {
            volume: DEFAULT_VOLUME,
            mouse_enabled: true,
            choices,
            selection,
            activity: None,
            paused: false,
            library: Library::default(),
            playback: Playback::default(),
            pick: Pick::new(),
            error: None,
            locale: None,
        }
    }
}

impl KeySounds {
    /// The copy the panels should draw right now.
    fn copy(&self) -> &'static Copy {
        self.locale.as_deref().map_or(&ENGLISH, copy_for)
    }

    fn configure(&mut self, config: &Configuration) -> Result<(), String> {
        if config.keys().any(|key| !is_known_key(key)) {
            return Err("Unknown configuration key".into());
        }
        let integer = |key: &str, default, min, max| match config.get(key) {
            None => Ok(default),
            Some(ConfigValue::Integer(value)) if (min..=max).contains(value) => Ok(*value),
            _ => Err(format!("Invalid {key}")),
        };
        let boolean = |key: &str, default| match config.get(key) {
            None => Ok(default),
            Some(ConfigValue::Boolean(value)) => Ok(*value),
            _ => Err(format!("Invalid {key}")),
        };
        // A set id is text, and this plugin is the one that decides which ids it
        // carries: an id this build does not know is accepted here and resolved
        // to the device's default set when playback asks for it. Refusing it
        // would turn a set dropped by an update into a plugin that will not
        // start.
        if let Some(key) = ["keyboard_pack", "mouse_pack"].into_iter().find(|key| {
            config
                .get(*key)
                .is_some_and(|value| !matches!(value, ConfigValue::Text(text) if text.len() <= 128))
        }) {
            return Err(format!("Invalid {key}"));
        }
        let volume = integer("volume", DEFAULT_VOLUME, MIN_VOLUME, MAX_VOLUME)?;
        let mouse_enabled = boolean("mouse_enabled", true)?;

        if self.playback.previewing {
            self.playback.clear_voices();
        }
        self.volume = volume;
        self.mouse_enabled = mouse_enabled;
        self.choices = Choices::from_config(config);
        self.selection = Selection::resolve(&self.choices);
        self.library.retain(&self.selection.ids());
        if volume == 0 {
            self.playback.stop();
        }
        Ok(())
    }

    /// Whether one action may be heard right now.
    fn admit(&mut self, action: Action, now_ms: u64) -> bool {
        if self.paused
            || self.playback.preview_active()
            || self.volume == 0
            || (action.category() == PackCategory::Mouse && !self.mouse_enabled)
        {
            return false;
        }
        if self
            .playback
            .last_played
            .get(&action)
            .is_some_and(|last| now_ms.saturating_sub(*last) < MINIMUM_SOUND_GAP_MS)
        {
            return false;
        }
        self.playback.last_played.insert(action, now_ms);
        true
    }

    /// Play one sound from a resolved pack.
    fn play_from_pack(
        &mut self,
        action: Action,
        key_group: Option<KeyGroup>,
        pack: &'static PackAsset,
    ) {
        // The cache is asked for the set, and one sound is taken out of it: the
        // borrow ends before playback starts, so only the sound being played is
        // copied out of the cache.
        let chosen = match self.library.load(pack) {
            Ok(decoded) => {
                let sounds = match key_group {
                    Some(group) => decoded.keyboard_sounds(action, group),
                    None => decoded.sounds(action),
                };
                self.pick.pick(sounds).cloned()
            }
            Err(error) => {
                self.error = Some(error);
                return;
            }
        };
        // A set with no release sounds is silent on release rather than wrong.
        let Some(sound) = chosen else {
            return;
        };
        self.error = self.playback.play(sound, self.volume).err();
    }

    fn play_keyboard(&mut self, action: Action, key_group: KeyGroup) {
        self.play_from_pack(action, Some(key_group), self.selection.pack(action));
    }

    fn play(&mut self, action: Action) {
        self.play_from_pack(action, None, self.selection.pack(action));
    }

    /// Resolve a published device option to the edge whose preview it plays.
    fn preview_choice(
        &self,
        key: &str,
        value: &str,
    ) -> Result<(Action, &'static PackAsset), String> {
        let action =
            Action::from_config_key(key).ok_or_else(|| "Unknown preview key".to_owned())?;
        let pack = pack_by_id(value)
            .filter(|pack| pack.category == action.category())
            .ok_or_else(|| "Unknown preview option".to_owned())?;
        Ok((action, pack))
    }

    /// A preview owns the voices until its one-shot sample finishes. Normal
    /// input counters still advance, but cannot mix with or replay after it.
    fn preview(&mut self, key: &str, value: &str) -> Result<(), String> {
        let (action, pack) = self.preview_choice(key, value)?;
        // Reuse the output device and preserve normal input cooldowns.
        self.playback.clear_voices();
        self.error = None;
        if !self.paused && self.volume > 0 {
            self.playback.previewing = true;
            let key_group = matches!(action, Action::KeyboardDown | Action::KeyboardUp)
                .then_some(KeyGroup::Default);
            self.play_from_pack(action, key_group, pack);
            let mut keep = self.selection.ids();
            keep.push(pack.id);
            self.library.retain(&keep);
        }
        Ok(())
    }

    /// Play whichever edges moved since the counters were last read.
    ///
    /// The counters are compared rather than treated as events because that is
    /// what the host's signal is: a coalesced edge is one sound, an edge that
    /// arrived between two polls is not lost, and a device nobody touched costs
    /// nothing.
    fn play_edges(&mut self, elapsed_ms: u64, activity: InputActivity) {
        let Some(before) = self.activity.replace(activity) else {
            // InputActivity is cumulative and the host sends one baseline after
            // starting this process. Never replay activity that happened before
            // that baseline as a new press or release.
            return;
        };
        for action in Action::ALL {
            let (current, previous) = match action {
                Action::KeyboardDown => (activity.keyboard_down, before.keyboard_down),
                Action::KeyboardUp => (activity.keyboard_up, before.keyboard_up),
                Action::MouseDown => (activity.mouse_down, before.mouse_down),
                Action::MouseUp => (activity.mouse_up, before.mouse_up),
            };
            if current > previous && self.admit(action, elapsed_ms) {
                let group = match action {
                    Action::KeyboardDown => KeyGroup::for_edge(activity, before, false),
                    Action::KeyboardUp => KeyGroup::for_edge(activity, before, true),
                    Action::MouseDown | Action::MouseUp => {
                        self.play(action);
                        continue;
                    }
                };
                self.play_keyboard(action, group);
            }
        }
    }

    /// One select node for a device's own sound.
    fn device_select(&self, category: PackCategory, label: &str) -> UiNode {
        let options = category
            .packs()
            .map(|pack| SelectOption {
                value: pack.id.into(),
                // A set with no release recording still has press sounds, but
                // choosing it makes the release edge silent.
                label: if !pack.supports_key_up {
                    format!("{} · {}", pack.label, self.copy().no_release)
                } else {
                    pack.label.into()
                },
                group: Some(pack.brand.into()),
            })
            .collect();
        let (key, value) = match category {
            PackCategory::Keyboard => ("keyboard_pack", self.selection.keyboard.id),
            PackCategory::Mouse => ("mouse_pack", self.selection.mouse.id),
        };
        UiNode::Select {
            key: key.into(),
            label: label.into(),
            value: value.into(),
            options,
            preview: true,
        }
    }

    fn output(&self) -> PluginOutput {
        let copy = self.copy();
        let nodes = vec![
            UiNode::Integer {
                key: "volume".into(),
                label: copy.volume.into(),
                value: self.volume,
                min: MIN_VOLUME,
                max: MAX_VOLUME,
            },
            UiNode::Toggle {
                key: "mouse_enabled".into(),
                label: copy.mouse_enabled.into(),
                value: self.mouse_enabled,
            },
            self.device_select(PackCategory::Keyboard, copy.keyboard_pack),
            self.device_select(PackCategory::Mouse, copy.mouse_pack),
        ];
        let config = Configuration::from([
            ("volume".into(), ConfigValue::Integer(self.volume)),
            (
                "mouse_enabled".into(),
                ConfigValue::Boolean(self.mouse_enabled),
            ),
            (
                "keyboard_pack".into(),
                ConfigValue::Text(self.selection.keyboard.id.into()),
            ),
            (
                "mouse_pack".into(),
                ConfigValue::Text(self.selection.mouse.id.into()),
            ),
        ]);
        PluginOutput {
            errors: self
                .error
                .map(|error| bongocat_plugin_sdk::PluginError {
                    id: match error {
                        AudioError::Decode => "audio_decode",
                        AudioError::Output => "audio_output",
                    }
                    .into(),
                    message: error.copy(copy).into(),
                })
                .into_iter()
                .collect(),
            settings: PluginView {
                nodes,
                ..Default::default()
            },
            config,
            ..Default::default()
        }
    }
}

/// Every configuration key this plugin accepts.
///
/// The list is closed so a typo is refused rather than stored.
fn is_known_key(key: &str) -> bool {
    matches!(
        key,
        "volume" | "mouse_enabled" | "keyboard_pack" | "mouse_pack"
    )
}

impl Plugin for KeySounds {
    fn id(&self) -> &'static str {
        "com.bongocat.key-sounds"
    }
    fn handle(&mut self, message: HostMessage) -> Result<PluginOutput, String> {
        match message {
            HostMessage::Start { config, .. } => {
                self.configure(&config)?;
                self.paused = false;
                self.activity = None;
            }
            HostMessage::Configure { config } => self.configure(&config)?,
            HostMessage::InputActivity {
                elapsed_ms,
                activity,
            } => self.play_edges(elapsed_ms, activity),
            HostMessage::Tick { .. } => self.playback.idle(),
            HostMessage::Preview { key, value } => self.preview(&key, &value)?,
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
                self.activity = None;
            }
            HostMessage::Reset => {
                self.playback.stop();
                *self = Self {
                    locale: self.locale.take(),
                    ..Self::default()
                };
            }
            HostMessage::Handshake { .. } => return Err("Handshake belongs to SDK".into()),
        }
        Ok(self.output())
    }
}

fn main() -> std::io::Result<()> {
    bongocat_plugin_sdk::run(&mut KeySounds::default())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeSet;

    #[test]
    fn resetting_settings_keeps_the_application_language() {
        let mut plugin = KeySounds::default();
        plugin
            .handle(HostMessage::Locale {
                locale: "zh-CN".into(),
            })
            .unwrap();
        plugin
            .configure(&[("volume".into(), ConfigValue::Integer(80))].into())
            .unwrap();
        let output = plugin.handle(HostMessage::Reset).unwrap();
        assert!(output.validate());
        assert_eq!(plugin.volume, 100);
        assert!(output.settings.nodes.iter().any(|node| {
            matches!(node, UiNode::Integer { label, .. } if label == SIMPLIFIED_CHINESE.volume)
        }));
    }

    /// Every embedded set has to be playable, because nothing else can repair it
    /// after the fact: a set that ships a file this build cannot decode would
    /// fail at the user's first keystroke rather than here.
    #[test]
    fn every_embedded_set_decodes_and_declares_its_release_honestly() {
        let mut ids = BTreeSet::new();
        for pack in PACKS {
            assert!(ids.insert(pack.id), "{} is embedded twice", pack.id);
            assert!(!pack.down.is_empty(), "{} carries no press sound", pack.id);
            assert_eq!(
                pack.supports_key_up,
                !pack.up.is_empty(),
                "{} disagrees with itself about its release sound",
                pack.id
            );
            let decoded = decode_pack(pack).unwrap_or_else(|_| panic!("{} decodes", pack.id));
            assert_eq!(decoded.down.len(), pack.down.len());
            assert_eq!(decoded.up.len(), pack.up.len());
            assert_eq!(decoded.key_groups.len(), pack.key_groups.len());
            for sound in decoded
                .down
                .iter()
                .chain(&decoded.up)
                .chain(decoded.key_groups.values().flat_map(|group| &group.down))
                .chain(decoded.key_groups.values().flat_map(|group| &group.up))
            {
                assert!(
                    sound.clone().count() > 0,
                    "{} carries an empty sound",
                    pack.id
                );
            }
        }
        for category in [PackCategory::Keyboard, PackCategory::Mouse] {
            assert!(
                category.packs().count() > 0,
                "{category:?} has no set to fall back to"
            );
        }
    }

    /// Thock resolves a named key group first and falls back to `default` only
    /// when the pack has no such group. A group that exists but has no recording
    /// for this direction is silent instead of falling back again.
    #[test]
    fn keyboard_edges_use_thocks_key_group_fallback() {
        let with_space = PACKS
            .iter()
            .find(|pack| pack.key_groups.iter().any(|group| group.name == "space"))
            .expect("the registry carries a space group");
        let decoded = decode_pack(with_space).unwrap();
        let space = decoded.key_groups.get("space").unwrap();
        assert_eq!(
            decoded
                .keyboard_sounds(Action::KeyboardDown, KeyGroup::Space)
                .len(),
            space.down.len()
        );
        assert_eq!(
            decoded
                .keyboard_sounds(Action::KeyboardUp, KeyGroup::Space)
                .len(),
            space.up.len()
        );
        assert_eq!(
            decoded
                .keyboard_sounds(Action::KeyboardDown, KeyGroup::Default)
                .len(),
            decoded.down.len()
        );

        let no_space_up = PACKS
            .iter()
            .find(|pack| {
                pack.category == PackCategory::Keyboard
                    && pack
                        .key_groups
                        .iter()
                        .any(|group| group.name == "space" && group.up.is_empty())
            })
            .expect("the registry carries a space group without release recordings");
        let decoded = decode_pack(no_space_up).unwrap();
        assert!(
            decoded
                .keyboard_sounds(Action::KeyboardUp, KeyGroup::Space)
                .is_empty(),
            "an existing group with no release recording must stay silent"
        );

        let no_space = PACKS
            .iter()
            .find(|pack| {
                pack.category == PackCategory::Keyboard
                    && pack.key_groups.iter().all(|group| group.name != "space")
            })
            .expect("the registry carries a keyboard set without a space group");
        let decoded = decode_pack(no_space).unwrap();
        assert_eq!(
            decoded
                .keyboard_sounds(Action::KeyboardDown, KeyGroup::Space)
                .len(),
            decoded.down.len()
        );
    }

    /// A special-key group is selected only when its counter accounts for the
    /// whole keyboard batch; a mixed batch still plays the default sound once,
    /// rather than guessing which of several keys it was.
    #[test]
    fn special_key_edges_select_their_groups_without_double_playing() {
        let before = InputActivity::default();
        for (group, activity) in [
            (
                KeyGroup::Space,
                InputActivity {
                    keyboard_down: 1,
                    space_down: 1,
                    ..InputActivity::default()
                },
            ),
            (
                KeyGroup::Enter,
                InputActivity {
                    keyboard_down: 1,
                    enter_down: 1,
                    ..InputActivity::default()
                },
            ),
            (
                KeyGroup::Del,
                InputActivity {
                    keyboard_down: 1,
                    backspace_down: 1,
                    ..InputActivity::default()
                },
            ),
            (
                KeyGroup::Tab,
                InputActivity {
                    keyboard_down: 1,
                    tab_down: 1,
                    ..InputActivity::default()
                },
            ),
            (
                KeyGroup::ShiftLeft,
                InputActivity {
                    keyboard_down: 1,
                    shift_left_down: 1,
                    ..InputActivity::default()
                },
            ),
            (
                KeyGroup::ShiftRight,
                InputActivity {
                    keyboard_down: 1,
                    shift_right_down: 1,
                    ..InputActivity::default()
                },
            ),
            (
                KeyGroup::CtrlLeft,
                InputActivity {
                    keyboard_down: 1,
                    ctrl_left_down: 1,
                    ..InputActivity::default()
                },
            ),
        ] {
            assert_eq!(KeyGroup::for_edge(activity, before, false), group);
        }
        assert_eq!(
            KeyGroup::for_edge(
                InputActivity {
                    keyboard_down: 2,
                    enter_down: 2,
                    ..InputActivity::default()
                },
                before,
                false,
            ),
            KeyGroup::Enter,
            "main Enter and keypad Enter share the same Thock group"
        );
        for (group, activity) in [
            (
                KeyGroup::Space,
                InputActivity {
                    keyboard_up: 1,
                    space_up: 1,
                    ..InputActivity::default()
                },
            ),
            (
                KeyGroup::Enter,
                InputActivity {
                    keyboard_up: 1,
                    enter_up: 1,
                    ..InputActivity::default()
                },
            ),
            (
                KeyGroup::Del,
                InputActivity {
                    keyboard_up: 1,
                    backspace_up: 1,
                    ..InputActivity::default()
                },
            ),
            (
                KeyGroup::Tab,
                InputActivity {
                    keyboard_up: 1,
                    tab_up: 1,
                    ..InputActivity::default()
                },
            ),
            (
                KeyGroup::ShiftLeft,
                InputActivity {
                    keyboard_up: 1,
                    shift_left_up: 1,
                    ..InputActivity::default()
                },
            ),
            (
                KeyGroup::ShiftRight,
                InputActivity {
                    keyboard_up: 1,
                    shift_right_up: 1,
                    ..InputActivity::default()
                },
            ),
            (
                KeyGroup::CtrlLeft,
                InputActivity {
                    keyboard_up: 1,
                    ctrl_left_up: 1,
                    ..InputActivity::default()
                },
            ),
        ] {
            assert_eq!(KeyGroup::for_edge(activity, before, true), group);
        }
        assert_eq!(
            KeyGroup::for_edge(
                InputActivity {
                    keyboard_up: 2,
                    enter_up: 2,
                    ..InputActivity::default()
                },
                before,
                true,
            ),
            KeyGroup::Enter,
            "main Enter and keypad Enter share the same Thock group"
        );

        assert_eq!(
            KeyGroup::for_edge(
                InputActivity {
                    keyboard_down: 2,
                    space_down: 1,
                    enter_down: 1,
                    ..InputActivity::default()
                },
                before,
                false,
            ),
            KeyGroup::Default,
            "two special keys in one batch are not guessed"
        );
        assert_eq!(
            KeyGroup::for_edge(
                InputActivity {
                    keyboard_down: 2,
                    space_down: 1,
                    ..InputActivity::default()
                },
                before,
                false,
            ),
            KeyGroup::Default,
            "a special key mixed with an ordinary key is not guessed"
        );

        let mut plugin = KeySounds {
            activity: Some(InputActivity::default()),
            ..Default::default()
        };
        plugin.play_edges(
            1,
            InputActivity {
                keyboard_down: 1,
                space_down: 1,
                ..InputActivity::default()
            },
        );
        assert_eq!(plugin.activity.unwrap().keyboard_down, 1);
        assert_eq!(plugin.playback.last_played.len(), 1);

        plugin.playback.last_played.clear();
        plugin.play_edges(
            100,
            InputActivity {
                keyboard_down: 3,
                space_down: 2,
                ..InputActivity::default()
            },
        );
        assert_eq!(
            plugin.playback.last_played.len(),
            1,
            "a mixed batch is still one keyboard edge batch"
        );
    }

    /// The configuration vocabulary is closed, and a value outside a control's
    /// range is refused rather than clamped: the host has to be told the edit did
    /// not take, not shown a number it never stored.
    #[test]
    fn the_configuration_vocabulary_is_closed_and_bounded() {
        let mut plugin = KeySounds::default();
        let output = plugin
            .handle(HostMessage::Start {
                config: Configuration::new(),
                state: Vec::new(),
            })
            .unwrap();
        assert!(output.validate());
        let volume = output.settings.nodes.iter().find_map(|node| match node {
            UiNode::Integer {
                key,
                value: value @ 100,
                ..
            } if key == "volume" => Some(*value),
            _ => None,
        });
        assert_eq!(volume, Some(100));
        assert!(plugin.mouse_enabled);

        for invalid in [
            ("volume".to_owned(), ConfigValue::Integer(101)),
            ("mouse_enabled".to_owned(), ConfigValue::Text("yes".into())),
            ("secret".to_owned(), ConfigValue::Boolean(true)),
            // A key this plugin does not declare is a typo, not a preference.
            ("cooldown_ms".to_owned(), ConfigValue::Integer(0)),
            (
                "keyboard_down_pack".to_owned(),
                ConfigValue::Text("alpaca".into()),
            ),
        ] {
            assert!(
                plugin
                    .configure(&Configuration::from([invalid.clone()]))
                    .is_err(),
                "{invalid:?} was accepted"
            );
        }
        assert_eq!(plugin.volume, 100);

        plugin
            .configure(&Configuration::from([
                ("volume".into(), ConfigValue::Integer(80)),
                ("mouse_enabled".into(), ConfigValue::Boolean(true)),
            ]))
            .unwrap();
        assert_eq!(plugin.volume, 80);
        assert!(plugin.mouse_enabled);

        plugin
            .configure(&Configuration::from([(
                "keyboard_pack".into(),
                ConfigValue::Text("skcm-blue".into()),
            )]))
            .unwrap();
        assert_eq!(plugin.selection.keyboard.id, "skcm-blue");
        assert_eq!(plugin.selection.pack(Action::KeyboardUp).id, "skcm-blue");
    }

    #[test]
    fn mouse_sounds_default_to_on_and_preserve_a_saved_opt_out() {
        let mut plugin = KeySounds::default();
        assert!(plugin.mouse_enabled);

        for enabled in [false, true] {
            let output = plugin
                .handle(HostMessage::Start {
                    config: Configuration::from([(
                        "mouse_enabled".into(),
                        ConfigValue::Boolean(enabled),
                    )]),
                    state: Vec::new(),
                })
                .unwrap();
            assert_eq!(plugin.mouse_enabled, enabled);
            assert_eq!(
                output.config["mouse_enabled"],
                ConfigValue::Boolean(enabled)
            );
            assert!(output.settings.nodes.iter().any(|node| matches!(
                node,
                UiNode::Toggle { key, value, .. } if key == "mouse_enabled" && *value == enabled
            )));
        }

        plugin
            .configure(&Configuration::from([(
                "mouse_enabled".into(),
                ConfigValue::Boolean(false),
            )]))
            .unwrap();
        let output = plugin.handle(HostMessage::Reset).unwrap();
        assert_eq!(output.config["mouse_enabled"], ConfigValue::Boolean(true));

        plugin
            .handle(HostMessage::Start {
                config: Configuration::from([("volume".into(), ConfigValue::Integer(80))]),
                state: Vec::new(),
            })
            .unwrap();
        assert!(plugin.mouse_enabled);
    }

    /// A stored set id this build no longer carries resolves to the default
    /// rather than failing the exchange.
    #[test]
    fn a_stored_set_id_this_build_lost_repairs_itself_to_the_default() {
        let mut plugin = KeySounds::default();
        plugin
            .configure(&Configuration::from([(
                "keyboard_pack".into(),
                ConfigValue::Text("gone".into()),
            )]))
            .unwrap();

        let keyboard = PackCategory::Keyboard.default_pack();
        assert_eq!(plugin.selection.keyboard.id, keyboard.id);
        // The published configuration is the resolved value, so the panel and the
        // stored session never drift from what is actually playing.
        let published = plugin.output().config;
        assert_eq!(
            published.get("keyboard_pack"),
            Some(&ConfigValue::Text(keyboard.id.into()))
        );
    }

    /// Both edges of a device use that device's selected set.
    #[test]
    fn one_device_set_serves_both_of_its_edges() {
        let pinned = PackCategory::Keyboard
            .packs()
            .nth(1)
            .expect("the keyboard ships more than one set");
        let mut plugin = KeySounds::default();
        plugin
            .configure(&Configuration::from([(
                "keyboard_pack".into(),
                ConfigValue::Text(pinned.id.into()),
            )]))
            .unwrap();

        assert_eq!(plugin.selection.pack(Action::KeyboardDown).id, pinned.id);
        assert_eq!(plugin.selection.pack(Action::KeyboardUp).id, pinned.id);
    }

    /// A set with no release recording is silent on release, and both device
    /// selectors mark it so the choice is not mistaken for complete coverage.
    #[test]
    fn a_set_without_a_release_sound_is_silent_on_release() {
        let silent = PackCategory::Keyboard
            .packs()
            .find(|pack| !pack.supports_key_up)
            .expect("the registry carries sets with no release recording");
        assert!(silent.up.is_empty());

        let copy = &ENGLISH;
        let mut plugin = KeySounds::default();
        let output = plugin.output();
        let chosen = select_option(&output, "keyboard_pack", silent.id);
        assert!(chosen.label.ends_with(copy.no_release));

        plugin
            .configure(&Configuration::from([(
                "keyboard_pack".into(),
                ConfigValue::Text(silent.id.into()),
            )]))
            .unwrap();
        assert_eq!(plugin.selection.pack(Action::KeyboardUp).id, silent.id);
        let decoded = decode_pack(silent).unwrap();
        assert!(decoded.sounds(Action::KeyboardUp).is_empty());
        assert!(!decoded.sounds(Action::KeyboardDown).is_empty());
    }

    /// Cumulative totals already include input from before a plugin was started:
    /// the first snapshot is a baseline, then down and up are independent edges.
    #[test]
    fn initial_activity_is_baselined_and_edges_play_separately() {
        let mut plugin = KeySounds::default();
        plugin
            .handle(HostMessage::InputActivity {
                elapsed_ms: 0,
                activity: InputActivity {
                    keyboard_down: 40,
                    keyboard_up: 39,
                    ..InputActivity::default()
                },
            })
            .unwrap();
        assert!(plugin.playback.last_played.is_empty());

        plugin
            .handle(HostMessage::InputActivity {
                elapsed_ms: 1,
                activity: InputActivity {
                    keyboard_down: 41,
                    keyboard_up: 39,
                    ..InputActivity::default()
                },
            })
            .unwrap();
        assert_eq!(
            plugin
                .playback
                .last_played
                .keys()
                .copied()
                .collect::<Vec<_>>(),
            vec![Action::KeyboardDown]
        );

        plugin.playback.last_played.clear();
        plugin
            .handle(HostMessage::InputActivity {
                elapsed_ms: 2,
                activity: InputActivity {
                    keyboard_down: 41,
                    keyboard_up: 40,
                    ..InputActivity::default()
                },
            })
            .unwrap();
        assert_eq!(
            plugin
                .playback
                .last_played
                .keys()
                .copied()
                .collect::<Vec<_>>(),
            vec![Action::KeyboardUp]
        );
    }

    /// Every edge the host reports is one sound, and an edge nobody performed is
    /// no sound at all — the counters are compared, not replayed.
    #[test]
    fn each_device_edge_plays_once_and_only_when_its_counter_moves() {
        let mut plugin = KeySounds::default();
        plugin
            .configure(&Configuration::from([(
                "mouse_enabled".into(),
                ConfigValue::Boolean(true),
            )]))
            .unwrap();
        plugin
            .handle(HostMessage::InputActivity {
                elapsed_ms: 0,
                activity: InputActivity::default(),
            })
            .unwrap();
        plugin
            .handle(HostMessage::InputActivity {
                elapsed_ms: 1,
                activity: InputActivity {
                    keyboard_down: 1,
                    keyboard_up: 1,
                    mouse_down: 1,
                    mouse_up: 1,
                    ..InputActivity::default()
                },
            })
            .unwrap();
        assert_eq!(
            plugin
                .playback
                .last_played
                .keys()
                .copied()
                .collect::<Vec<_>>(),
            Action::ALL
        );

        // A repeat of the same counters is not activity, and one counter moving
        // plays that action alone.
        plugin.playback.last_played.clear();
        plugin
            .handle(HostMessage::InputActivity {
                elapsed_ms: 100,
                activity: InputActivity {
                    keyboard_down: 1,
                    keyboard_up: 1,
                    mouse_down: 1,
                    mouse_up: 1,
                    ..InputActivity::default()
                },
            })
            .unwrap();
        assert!(plugin.playback.last_played.is_empty());
        plugin
            .handle(HostMessage::InputActivity {
                elapsed_ms: 200,
                activity: InputActivity {
                    keyboard_up: 2,
                    ..plugin.activity.unwrap_or_default()
                },
            })
            .unwrap();
        assert_eq!(
            plugin
                .playback
                .last_played
                .keys()
                .copied()
                .collect::<Vec<_>>(),
            vec![Action::KeyboardUp]
        );
    }

    /// Turning mouse sounds off silences both mouse edges
    /// without touching the keyboard.
    #[test]
    fn the_mouse_switch_gates_both_mouse_edges() {
        let activity = InputActivity {
            keyboard_down: 1,
            keyboard_up: 1,
            mouse_down: 1,
            mouse_up: 1,
            ..InputActivity::default()
        };
        let mut plugin = KeySounds::default();
        plugin
            .configure(&Configuration::from([(
                "mouse_enabled".into(),
                ConfigValue::Boolean(false),
            )]))
            .unwrap();
        plugin
            .handle(HostMessage::InputActivity {
                elapsed_ms: 0,
                activity: InputActivity::default(),
            })
            .unwrap();
        plugin
            .handle(HostMessage::InputActivity {
                elapsed_ms: 1,
                activity,
            })
            .unwrap();
        assert_eq!(
            plugin
                .playback
                .last_played
                .keys()
                .copied()
                .collect::<Vec<_>>(),
            vec![Action::KeyboardDown, Action::KeyboardUp]
        );

        plugin.playback.last_played.clear();
        plugin
            .configure(&Configuration::from([(
                "mouse_enabled".into(),
                ConfigValue::Boolean(true),
            )]))
            .unwrap();
        plugin
            .handle(HostMessage::InputActivity {
                elapsed_ms: 100,
                activity: InputActivity {
                    mouse_down: 2,
                    mouse_up: 2,
                    ..activity
                },
            })
            .unwrap();
        assert_eq!(
            plugin
                .playback
                .last_played
                .keys()
                .copied()
                .collect::<Vec<_>>(),
            vec![Action::MouseDown, Action::MouseUp]
        );
    }

    /// Pausing, resuming and a zero volume stop playback; the fixed Thock-derived
    /// gap is per action, so a release is never swallowed by the press before it.
    #[test]
    fn cooldown_pause_and_mute_are_plugin_policy() {
        let mut plugin = KeySounds::default();
        assert!(plugin.admit(Action::KeyboardDown, 0));
        assert!(!plugin.admit(Action::KeyboardDown, MINIMUM_SOUND_GAP_MS - 1));
        assert!(plugin.admit(Action::KeyboardDown, MINIMUM_SOUND_GAP_MS));
        assert!(
            plugin.admit(Action::KeyboardUp, MINIMUM_SOUND_GAP_MS),
            "the release keeps its own clock"
        );

        plugin.handle(HostMessage::Pause).unwrap();
        assert!(!plugin.admit(Action::KeyboardDown, 10_000));
        plugin.handle(HostMessage::Resume).unwrap();
        assert!(plugin.admit(Action::KeyboardDown, 10_001));

        plugin
            .configure(&Configuration::from([(
                "volume".into(),
                ConfigValue::Integer(0),
            )]))
            .unwrap();
        assert!(!plugin.admit(Action::KeyboardDown, 20_000));
    }

    #[test]
    fn every_sound_select_publishes_localized_categories_without_changing_ids() {
        let mut plugin = KeySounds::default();
        let original_config = plugin.output().config;
        for locale in ["en-US", "zh-CN", "de-DE"] {
            let output = plugin
                .handle(HostMessage::Locale {
                    locale: locale.into(),
                })
                .unwrap();
            let copy = copy_for(locale);
            assert_eq!(output.config, original_config);
            assert!(output.settings.validate());
            for node in output.settings.nodes {
                let UiNode::Select { key, options, .. } = node else {
                    continue;
                };
                let category = if key.starts_with("keyboard_") {
                    PackCategory::Keyboard
                } else {
                    PackCategory::Mouse
                };
                for option in options {
                    let pack = category
                        .packs()
                        .find(|pack| pack.id == option.value)
                        .unwrap();
                    assert_eq!(option.group.as_deref(), Some(pack.brand));
                    let expected_label = if !pack.supports_key_up {
                        format!("{} · {}", pack.label, copy.no_release)
                    } else {
                        pack.label.to_owned()
                    };
                    assert_eq!(option.label, expected_label);
                }
            }
        }
    }

    /// The settings panel is the plugin's own copy, so switching language
    /// re-labels it without touching what the user has configured.
    #[test]
    fn the_host_locale_relabels_the_settings_panel() {
        let mut plugin = KeySounds::default();
        let english = plugin.output();
        assert_eq!(integer_label(&plugin.output(), "volume"), "Volume");
        assert_eq!(
            select_label(&plugin.output(), "keyboard_pack"),
            "Keyboard sound"
        );
        assert_eq!(
            select_option_group(&english, "mouse_pack", "pixabay-unknown-unknown"),
            "Unknown"
        );
        assert_eq!(
            select_option_group(
                &plugin.output(),
                "keyboard_pack",
                PackCategory::Keyboard.default_pack().id
            ),
            PackCategory::Keyboard.default_pack().brand
        );

        let localized = plugin
            .handle(HostMessage::Locale {
                locale: "zh-CN".into(),
            })
            .unwrap();
        assert_eq!(integer_label(&localized, "volume"), "音量");
        assert_eq!(select_label(&localized, "mouse_pack"), "鼠标音效");
        assert_eq!(
            select_option_group(&localized, "mouse_pack", "pixabay-unknown-unknown"),
            "Unknown"
        );
        assert_eq!(
            select_option_group(
                &localized,
                "mouse_pack",
                PackCategory::Mouse.default_pack().id
            ),
            PackCategory::Mouse.default_pack().brand
        );
        assert_eq!(plugin.volume, DEFAULT_VOLUME);

        // A language this plugin does not carry reads English: the host only ever
        // sends the two tags the manifest declares, so nothing here has to resolve
        // a tag the plugin does not serve.
        for locale in ["en-US", "zh-TW", "ar-SA", "fr-FR", ""] {
            let fallback = plugin
                .handle(HostMessage::Locale {
                    locale: locale.into(),
                })
                .unwrap();
            assert_eq!(
                select_label(&fallback, "mouse_pack"),
                "Mouse sound",
                "{locale}"
            );
            assert_eq!(plugin.volume, DEFAULT_VOLUME);
            assert_eq!(fallback.settings, english.settings, "{locale}");
            assert_eq!(fallback.config, english.config, "{locale}");
            assert_eq!(fallback.state, english.state, "{locale}");
        }
    }

    /// A playback failure is reported in the language on screen, and a language
    /// change re-reads the message rather than leaving the old one behind.
    #[test]
    fn a_playback_failure_reads_the_language_on_screen() {
        // The failure is injected rather than provoked: this asserts the copy a
        // failure reads as, not that this machine has (or lacks) an output device.
        for (error, english, chinese) in [
            (
                AudioError::Decode,
                "The sound effect could not be decoded.",
                "无法解码音效。",
            ),
            (
                AudioError::Output,
                "No audio output device is available.",
                "没有可用的音频输出设备。",
            ),
        ] {
            let mut plugin = KeySounds {
                error: Some(error),
                ..Default::default()
            };
            assert_eq!(error_text(&plugin.output()), english);

            let localized = plugin
                .handle(HostMessage::Locale {
                    locale: "zh-CN".into(),
                })
                .unwrap();
            assert_eq!(error_text(&localized), chinese);

            for locale in [
                "en-US", "zh-TW", "zh-HK", "ar-SA", "vi-VN", "pt-BR", "ko-KR", "fr-FR", "de-DE", "",
            ] {
                let fallback = plugin
                    .handle(HostMessage::Locale {
                        locale: locale.into(),
                    })
                    .unwrap();
                assert_eq!(error_text(&fallback), english, "{locale}");
            }
        }
    }

    /// Auditioning is attached to each option rather than taking a separate
    /// action row in the settings panel.
    #[test]
    fn the_settings_panel_has_no_preview_button() {
        let plugin = KeySounds::default();
        let output = plugin.output();
        assert!(
            !output
                .settings
                .nodes
                .iter()
                .any(|node| matches!(node, UiNode::Button { id, .. } if id == "preview"))
        );
    }

    #[test]
    fn hovering_a_sound_option_previews_without_committing_configuration() {
        let mut plugin = KeySounds::default();
        plugin
            .configure(&Configuration::from([(
                "volume".into(),
                ConfigValue::Integer(0),
            )]))
            .unwrap();
        let before = plugin.output().config;
        let output = plugin
            .handle(HostMessage::Preview {
                key: "keyboard_pack".into(),
                value: PACKS
                    .iter()
                    .find(|pack| pack.category == PackCategory::Keyboard)
                    .unwrap()
                    .id
                    .into(),
            })
            .unwrap();
        assert_eq!(output.config, before);
        assert!(plugin.error.is_none());
        assert!(plugin.playback.device.is_none());
        assert!(plugin.library.decoded.is_empty());
        assert!(
            plugin
                .handle(HostMessage::Preview {
                    key: "mouse_pack".into(),
                    value: PackCategory::Keyboard.default_pack().id.into(),
                })
                .is_err()
        );
        assert!(
            plugin
                .handle(HostMessage::Preview {
                    key: "keyboard_pack".into(),
                    value: "missing-pack".into(),
                })
                .is_err()
        );
    }

    #[test]
    fn every_published_preview_resolves_to_its_device() {
        let plugin = KeySounds::default();
        for node in plugin.output().settings.nodes {
            if let UiNode::Select {
                key,
                options,
                preview,
                ..
            } = node
            {
                assert!(preview);
                let expected = Action::from_config_key(&key).unwrap();
                for option in options {
                    let (action, pack) = plugin.preview_choice(&key, &option.value).unwrap();
                    assert_eq!(action, expected);
                    assert_eq!(pack.category, action.category());
                }
            }
        }
        assert!(plugin.preview_choice("keyboard_pack", "auto").is_err());
        assert!(
            plugin
                .preview_choice("keyboard_down_pack", "skcm-blue")
                .is_err(),
            "only the keys this plugin publishes are previewable"
        );
        assert!(plugin.preview_choice("volume", "skcm-blue").is_err());
        assert!(plugin.preview_choice("mouse_pack", "skcm-blue").is_err());
    }

    fn install_test_voice(plugin: &mut KeySounds, preview: bool) {
        let (player, _output) = rodio::Player::new();
        player.append(rodio::source::SineWave::new(440.0).take_duration(Duration::from_secs(1)));
        plugin.playback.players.push(player);
        plugin.playback.previewing = preview;
    }

    #[test]
    fn previews_suppress_input_without_replaying_edges_or_resetting_cooldown() {
        let mut plugin = KeySounds {
            activity: Some(InputActivity::default()),
            ..Default::default()
        };
        plugin.playback.last_played.insert(Action::KeyboardDown, 10);
        install_test_voice(&mut plugin, true);
        assert!(plugin.playback.preview_active());
        let activity = InputActivity {
            keyboard_down: 1,
            keyboard_up: 1,
            mouse_down: 1,
            mouse_up: 1,
            ..InputActivity::default()
        };
        plugin.play_edges(100, activity);
        assert_eq!(plugin.activity, Some(activity));
        assert_eq!(plugin.playback.players.len(), 1);
        assert_eq!(plugin.playback.last_played[&Action::KeyboardDown], 10);
        assert!(plugin.error.is_none());
        plugin.playback.clear_voices();
        plugin.play_edges(200, activity);
        assert!(plugin.playback.players.is_empty());
        assert!(plugin.admit(Action::KeyboardDown, 200));
    }

    #[test]
    fn finished_preview_releases_ownership_before_the_next_input_batch() {
        let mut plugin = KeySounds::default();
        let (player, mut output) = rodio::Player::new();
        player.append(rodio::source::SineWave::new(440.0).take_duration(Duration::from_millis(1)));
        plugin.playback.players.push(player);
        plugin.playback.previewing = true;
        for _ in 0..100_000 {
            if !plugin.playback.playing() {
                break;
            }
            output.next();
        }
        assert!(!plugin.playback.playing());
        assert!(plugin.admit(Action::KeyboardDown, 100));
        assert!(!plugin.playback.previewing);
        install_test_voice(&mut plugin, false);
        assert!(
            plugin.admit(Action::KeyboardUp, 100),
            "normal press audio must not be mistaken for a preview"
        );
    }

    #[test]
    fn preview_replaces_old_voice_and_pause_mute_reset_stop_are_safe() {
        let mut plugin = KeySounds::default();
        install_test_voice(&mut plugin, true);
        plugin.preview("keyboard_pack", "skcm-blue").unwrap();
        assert!(plugin.playback.playing());
        assert!(plugin.error.is_none());
        for message in [HostMessage::Pause, HostMessage::Reset, HostMessage::Stop] {
            install_test_voice(&mut plugin, true);
            plugin.handle(message).unwrap();
            assert!(!plugin.playback.preview_active());
            assert!(plugin.playback.players.is_empty());
        }
        plugin.handle(HostMessage::Pause).unwrap();
        plugin.preview("keyboard_pack", "skcm-blue").unwrap();
        assert!(
            plugin.playback.device.is_none(),
            "paused previews never open output"
        );
    }

    #[test]
    #[ignore = "requires a real audio output device; run explicitly on macOS/Windows"]
    fn rapid_previews_replace_voices_reuse_output_and_recover_normal_input() {
        let mut plugin = KeySounds {
            activity: Some(InputActivity::default()),
            ..Default::default()
        };
        plugin.playback.last_played.insert(Action::KeyboardDown, 10);
        plugin.preview("keyboard_pack", "skcm-blue").unwrap();
        assert!(plugin.error.is_none());
        assert!(plugin.playback.preview_active());
        let device = std::ptr::from_ref(plugin.playback.device.as_ref().unwrap());
        for pack in PackCategory::Keyboard.packs().take(4) {
            plugin.preview("keyboard_pack", pack.id).unwrap();
            assert!(plugin.error.is_none());
            assert_eq!(plugin.playback.players.len(), 1);
            assert_eq!(
                device,
                std::ptr::from_ref(plugin.playback.device.as_ref().unwrap())
            );
            assert_eq!(plugin.playback.last_played[&Action::KeyboardDown], 10);
            plugin.play_edges(
                100,
                InputActivity {
                    keyboard_down: 1,
                    ..Default::default()
                },
            );
            assert_eq!(plugin.playback.players.len(), 1);
        }
        let deadline = Instant::now() + Duration::from_secs(5);
        while plugin.playback.preview_active() && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(5));
        }
        assert!(!plugin.playback.preview_active());
        assert!(plugin.admit(Action::KeyboardDown, 200));
        plugin.play(Action::KeyboardDown);
        assert!(plugin.playback.playing());
        plugin.handle(HostMessage::Stop).unwrap();
        assert!(plugin.playback.device.is_none());
    }

    /// The whole panel is one frame the host has to be able to draw: every select
    /// publishes a value out of its own list, and the panel stays inside the node
    /// budget the protocol gives it.
    #[test]
    fn the_published_panel_is_valid_and_bounded() {
        let plugin = KeySounds::default();
        let view = plugin.output().settings;
        assert!(view.validate());
        assert!(view.nodes.len() <= 16);
        for node in &view.nodes {
            if let UiNode::Select {
                key,
                value,
                options,
                ..
            } = node
            {
                assert!(
                    options.iter().any(|option| &option.value == value),
                    "{key} publishes a value its own list does not hold"
                );
            }
        }
    }

    /// A reset returns the process to a fresh plugin, and the sound it plays
    /// afterwards is a new one rather than a stale edge.
    #[test]
    fn reset_clears_what_the_counters_had_seen() {
        let mut plugin = KeySounds::default();
        plugin
            .handle(HostMessage::InputActivity {
                elapsed_ms: 0,
                activity: InputActivity {
                    keyboard_down: 4,
                    ..InputActivity::default()
                },
            })
            .unwrap();
        plugin.handle(HostMessage::Reset).unwrap();
        assert_eq!(plugin.activity, None);
        plugin
            .handle(HostMessage::InputActivity {
                elapsed_ms: 10,
                activity: InputActivity::default(),
            })
            .unwrap();
        assert!(plugin.playback.last_played.is_empty());
        plugin
            .handle(HostMessage::InputActivity {
                elapsed_ms: 20,
                activity: InputActivity {
                    keyboard_down: 5,
                    ..InputActivity::default()
                },
            })
            .unwrap();
        assert_eq!(
            plugin
                .playback
                .last_played
                .keys()
                .copied()
                .collect::<Vec<_>>(),
            vec![Action::KeyboardDown]
        );
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

    /// The label of one dropdown the settings panel publishes.
    fn select_label<'a>(output: &'a PluginOutput, key: &str) -> &'a str {
        match output.settings.nodes.iter().find_map(|node| match node {
            UiNode::Select {
                key: node_key,
                label,
                ..
            } if node_key == key => Some(label.as_str()),
            _ => None,
        }) {
            Some(label) => label,
            None => panic!("the settings panel always publishes the {key} dropdown"),
        }
    }

    /// The option one dropdown publishes for a value.
    fn select_option<'a>(output: &'a PluginOutput, key: &str, value: &str) -> &'a SelectOption {
        match output.settings.nodes.iter().find_map(|node| match node {
            UiNode::Select {
                key: node_key,
                options,
                ..
            } if node_key == key => options.iter().find(|option| option.value == value),
            _ => None,
        }) {
            Some(option) => option,
            None => panic!("the settings panel always publishes the {key} dropdown"),
        }
    }

    /// The category heading one dropdown publishes for an option.
    fn select_option_group<'a>(output: &'a PluginOutput, key: &str, value: &str) -> &'a str {
        match output.settings.nodes.iter().find_map(|node| match node {
            UiNode::Select {
                key: node_key,
                options,
                ..
            } if node_key == key => options
                .iter()
                .find(|option| option.value == value)
                .and_then(|option| option.group.as_deref()),
            _ => None,
        }) {
            Some(group) => group,
            None => panic!("the settings panel always groups the {key} option {value}"),
        }
    }

    /// The one failure message the settings panel publishes, if it published one.
    fn error_text(output: &PluginOutput) -> &str {
        output
            .errors
            .first()
            .map(|error| error.message.as_str())
            .expect("the panel publishes the failure it is reporting")
    }
}
