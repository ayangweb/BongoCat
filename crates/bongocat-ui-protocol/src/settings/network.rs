//! How this machine takes part in a room, and what that is doing right now.
//!
//! The settings page shows three parts — what this machine calls itself, what it
//! joins, what it serves — and it never names a `bongocat-net` type, so the three
//! travel in this module's vocabulary. `bongocat-config` keeps the same three as
//! its `network` section, and `bongocat-app` is the only place the two meet.
//!
//! The status half is kept apart from the settings half on purpose: the settings
//! are what the user chose, the status is what the process is doing, and a page
//! that has to redraw the second every second must not have to re-read the first.

/// Which addresses a relay served from this machine binds.
///
/// The four values are the four answers a desktop user can actually want: only
/// this machine, only the local network, both families, or one interface they
/// name themselves. They mirror the values the relay accepts, so a server
/// started from the page and one started from the command line are the same
/// server.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum SettingsNetworkListenMode {
    /// The loopback interface only.
    LocalOnly,
    /// The IPv4 interfaces only.
    Ipv4,
    /// Loopback and both address families, which is what a room on a home
    /// network needs.
    #[default]
    DualStack,
    /// Exactly one interface, named by [`SettingsNetworkServer::address`].
    Custom,
}

impl SettingsNetworkListenMode {
    pub const ALL: [Self; 4] = [Self::LocalOnly, Self::Ipv4, Self::DualStack, Self::Custom];

    /// Whether this mode is meaningless without an interface to bind.
    ///
    /// The page greys the address row out for every other mode rather than
    /// hiding it, so the rule that only `Custom` reads the field is written once
    /// here instead of as a comparison in the render path.
    pub const fn needs_address(self) -> bool {
        matches!(self, Self::Custom)
    }
}

/// Where a client's relay address comes from.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum SettingsNetworkClientSource {
    /// Join the relay this product serves on this machine.
    #[default]
    FollowLocal,
    /// Join the relay named by [`SettingsNetworkClient::url`].
    Manual,
}

impl SettingsNetworkClientSource {
    pub const ALL: [Self; 2] = [Self::FollowLocal, Self::Manual];

    /// Whether this source is meaningless without a typed address.
    pub const fn needs_url(self) -> bool {
        matches!(self, Self::Manual)
    }
}

/// What this machine calls itself in a room.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SettingsNetworkDevice {
    /// The address other machines see.
    ///
    /// It is a user choice rather than the host name so two machines that
    /// report the same host name can still be told apart.
    pub id: String,
    /// The label a viewer shows for this machine.
    pub name: String,
}

/// This machine as a viewer and publisher.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SettingsNetworkClient {
    /// Whether this machine joins a room at all.
    pub enabled: bool,
    /// Where the relay address comes from.
    pub source: SettingsNetworkClientSource,
    /// The relay to join when `source` is
    /// [`SettingsNetworkClientSource::Manual`].
    pub url: Option<String>,
    /// The room key presented to the relay.
    pub token: Option<String>,
    /// Whether this machine's own input is sent to the room.
    ///
    /// A viewer that only draws another machine sets this to `false`; a machine
    /// whose pet stays off screen keeps it `true`.
    pub publish: bool,
    /// Which devices this machine draws, by device id.
    ///
    /// An empty list is a valid state and means "draw nothing": a machine may
    /// publish into a room without showing any of it.
    pub display: Vec<String>,
}

/// What this machine serves.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SettingsNetworkServer {
    /// Whether this machine runs a relay.
    pub enabled: bool,
    /// Which addresses the relay binds.
    pub listen: SettingsNetworkListenMode,
    /// The interface to bind when `listen` is
    /// [`SettingsNetworkListenMode::Custom`].
    pub address: Option<String>,
    pub port: u16,
    /// The room key clients must present.
    pub token: Option<String>,
}

/// The whole multiplayer section.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct SettingsNetwork {
    pub device: SettingsNetworkDevice,
    pub client: SettingsNetworkClient,
    pub server: SettingsNetworkServer,
}

/// What the room connection this process opened is doing.
///
/// "Off" is a state rather than the absence of a status so the page can say
/// "not connected" without having to invent a sentence for a machine that was
/// never asked to join.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum SettingsNetworkClientState {
    /// Nothing is trying to join a room.
    #[default]
    Off,
    /// The first connection has not completed yet.
    Connecting,
    /// In the room, with this many other machines in it.
    Connected { peers: u16 },
    /// Lost the room; a retry is scheduled.
    Reconnecting,
    /// Stopped trying.
    Failed,
}

/// The relay this process is serving, if it is serving one.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SettingsNetworkServerStatus {
    /// What the relay is bound to, as a URL a peer could dial.
    pub url: String,
}

/// The room this process has joined, if it has joined one.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SettingsNetworkClientStatus {
    pub state: SettingsNetworkClientState,
    /// The peer whose input this pet is drawing.
    ///
    /// `None` until the first frame arrives: the room can hold several
    /// publishers and one pet draws one of them, so the binding is a fact about
    /// what has been seen rather than about what was configured.
    pub bound: Option<String>,
}

/// What the multiplayer side of this process is doing right now.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct SettingsNetworkStatus {
    /// The relay this process serves, if it is serving one.
    pub server: Option<SettingsNetworkServerStatus>,
    /// The room this process has joined, if it has joined one.
    pub client: Option<SettingsNetworkClientStatus>,
}

/// The device id that means "the pet running on this machine".
///
/// It is a reserved word rather than a real id because the id is the user's
/// choice: the entry in a display list that names this machine cannot be a value
/// another machine might also pick.
pub const NETWORK_SELF_DEVICE: &str = "self";

/// The port a relay listens on unless the user picks another one.
///
/// The same number as `bongocat-config`'s, kept as a literal here because this
/// crate cannot see the configuration crate. A test in `bongocat-app` pins the
/// two together.
pub const DEFAULT_NETWORK_PORT: u16 = 8787;

/// The device id a fresh configuration gives this machine.
const DEFAULT_NETWORK_DEVICE_ID: &str = "pc-01";

/// The device name a fresh configuration gives this machine.
const DEFAULT_NETWORK_DEVICE_NAME: &str = "PC";

impl Default for SettingsNetworkDevice {
    fn default() -> Self {
        Self {
            id: DEFAULT_NETWORK_DEVICE_ID.to_owned(),
            name: DEFAULT_NETWORK_DEVICE_NAME.to_owned(),
        }
    }
}

impl Default for SettingsNetworkClient {
    fn default() -> Self {
        Self {
            enabled: false,
            source: SettingsNetworkClientSource::FollowLocal,
            url: None,
            token: None,
            publish: true,
            display: vec![NETWORK_SELF_DEVICE.to_owned()],
        }
    }
}

impl Default for SettingsNetworkServer {
    fn default() -> Self {
        Self {
            enabled: false,
            listen: SettingsNetworkListenMode::DualStack,
            address: None,
            port: DEFAULT_NETWORK_PORT,
            token: None,
        }
    }
}
