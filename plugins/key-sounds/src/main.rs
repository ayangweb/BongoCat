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
mod views;

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
mod tests;
