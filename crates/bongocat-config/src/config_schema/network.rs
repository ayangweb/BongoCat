//! How this machine takes part in a room: what it calls itself, what it joins,
//! and what it serves.
//!
//! The three parts are one section because they are one decision. A machine that
//! only watches sets `client.enabled` and leaves `server` alone; a machine that
//! only feeds a room does the opposite; the same window edits both. Keeping them
//! apart would let a document describe a client pointing at a server that no
//! longer exists in the same file.
//!
//! Every field carries `#[serde(default)]`, including the section itself, so a
//! `config.json` written before this section existed keeps loading: an absent
//! `network` object is the shipped default rather than a strict-parse failure.

use super::*;

/// The port a relay listens on unless the user picks another one.
///
/// It is the relay's own default as well, so "follow the local server" and
/// "serve locally" agree without either side repeating the number.
pub const DEFAULT_NETWORK_PORT: u16 = 8787;

/// The `display` entry that names this machine's own window.
///
/// Device ids are assigned by the user and by other machines, so the entry that
/// means "the pet running here" cannot be an id at all. It is this reserved
/// word instead, and [`NetworkClientConfig::display`] resolves it to
/// [`NetworkDeviceConfig::id`].
pub const NETWORK_SELF_DEVICE: &str = "self";

/// Longest device id a document may carry, in bytes.
///
/// The id is what other machines address this one by and what the settings page
/// lists, so it is bounded here rather than at the point of use.
pub const MAXIMUM_NETWORK_DEVICE_ID_BYTES: usize = 64;

/// Longest display name a document may carry, in bytes.
pub const MAXIMUM_NETWORK_DEVICE_NAME_BYTES: usize = 64;

/// The whole "multiplayer" section.
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
#[cfg_attr(any(test, feature = "schema-generation"), derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct NetworkConfig {
    #[serde(default)]
    pub device: NetworkDeviceConfig,
    #[serde(default)]
    pub client: NetworkClientConfig,
    #[serde(default)]
    pub server: NetworkServerConfig,
}

/// What this machine calls itself in a room.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[cfg_attr(any(test, feature = "schema-generation"), derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct NetworkDeviceConfig {
    /// The address other machines see, and the value `display` resolves `self`
    /// to. It is a user choice rather than the host name so two machines that
    /// report the same host name can still be told apart.
    #[serde(default = "default_device_id")]
    #[cfg_attr(
        any(test, feature = "schema-generation"),
        schemars(length(min = 1, max = 64))
    )]
    pub id: String,
    /// The human-readable label a viewer shows for this machine.
    #[serde(default = "default_device_name")]
    #[cfg_attr(any(test, feature = "schema-generation"), schemars(length(max = 64)))]
    pub name: String,
}

/// This machine as a viewer and publisher.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[cfg_attr(any(test, feature = "schema-generation"), derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct NetworkClientConfig {
    /// Whether this machine joins a room at all.
    #[serde(default)]
    pub enabled: bool,
    /// Where the relay address comes from.
    #[serde(default)]
    pub source: NetworkClientSource,
    /// The relay to join when `source` is [`NetworkClientSource::Manual`].
    ///
    /// `None` while the client follows the local server, and also while a user
    /// has enabled a manual client but not typed an address yet: the two are
    /// different states of one field, and the settings page shows an empty field
    /// in the second.
    #[serde(default)]
    pub url: Option<String>,
    /// The room key presented to the relay.
    ///
    /// `None` is "no token yet" — a relay started from this product always has
    /// one, and a client without it can only be refused, so the value is never
    /// defaulted to something that looks usable.
    #[serde(default)]
    pub token: Option<String>,
    /// Whether this machine's own input is sent to the room.
    ///
    /// A viewer that only draws another machine sets this to `false`; a machine
    /// whose pet stays off screen keeps it `true`.
    #[serde(default = "default_publish_input")]
    pub publish: bool,
    /// Which devices this machine draws, by device id.
    ///
    /// [`NETWORK_SELF_DEVICE`] is this machine; any other entry names a peer.
    /// An empty list is a valid state and means "draw nothing": a machine may
    /// publish into a room without showing any of it.
    #[serde(default = "default_display")]
    pub display: Vec<String>,
}

/// Where a client's relay address comes from.
#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
#[cfg_attr(any(test, feature = "schema-generation"), derive(JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum NetworkClientSource {
    /// Join the relay this product serves on this machine, at
    /// [`NetworkServerConfig::port`].
    #[default]
    FollowLocal,
    /// Join the relay named by [`NetworkClientConfig::url`].
    Manual,
}

/// What this machine serves.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[cfg_attr(any(test, feature = "schema-generation"), derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct NetworkServerConfig {
    /// Whether this machine runs a relay.
    #[serde(default)]
    pub enabled: bool,
    /// Which addresses the relay binds.
    #[serde(default)]
    pub listen: NetworkListenMode,
    /// The interface to bind when `listen` is [`NetworkListenMode::Custom`].
    #[serde(default)]
    pub address: Option<String>,
    #[serde(default = "default_listen_port")]
    #[cfg_attr(
        any(test, feature = "schema-generation"),
        schemars(range(min = 1, max = 65535))
    )]
    pub port: u16,
    /// The room key clients must present.
    ///
    /// `None` is "generate one on first start and keep it": a relay a user
    /// switched on must not be usable by whatever knows a published default.
    #[serde(default)]
    pub token: Option<String>,
}

/// Which addresses a relay binds.
///
/// The four values are the four answers a desktop user can actually want: only
/// this machine, only the local network, both families, or one interface they
/// name themselves. They mirror the `--listen` values the relay command line
/// accepts, so a server started either way is the same server.
#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
#[cfg_attr(any(test, feature = "schema-generation"), derive(JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum NetworkListenMode {
    /// The loopback interface only.
    LocalOnly,
    /// The IPv4 interfaces only.
    Ipv4,
    /// Loopback and both address families, which is what a room on a home
    /// network needs.
    #[default]
    DualStack,
    /// Exactly [`NetworkServerConfig::address`].
    Custom,
}

fn default_device_id() -> String {
    "pc-01".to_owned()
}

fn default_device_name() -> String {
    "PC".to_owned()
}

fn default_publish_input() -> bool {
    true
}

fn default_display() -> Vec<String> {
    vec![NETWORK_SELF_DEVICE.to_owned()]
}

fn default_listen_port() -> u16 {
    DEFAULT_NETWORK_PORT
}

impl Default for NetworkDeviceConfig {
    fn default() -> Self {
        Self {
            id: default_device_id(),
            name: default_device_name(),
        }
    }
}

impl Default for NetworkClientConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            source: NetworkClientSource::FollowLocal,
            url: None,
            token: None,
            publish: default_publish_input(),
            display: default_display(),
        }
    }
}

impl Default for NetworkServerConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            listen: NetworkListenMode::DualStack,
            address: None,
            port: default_listen_port(),
            token: None,
        }
    }
}

impl NetworkConfig {
    /// Cheap structural check of the values a document supplies.
    ///
    /// Only what can never work is refused: a device id that cannot be an
    /// address, a port nothing can bind, a manual client with no address, and a
    /// custom bind with no interface. Everything else — an unused token, an
    /// inactive section — is a state the product handles rather than a document
    /// to reject.
    pub fn validate(&self) -> Result<(), ConfigError> {
        let id = self.device.id.as_str();
        if id.trim().is_empty()
            || id.len() > MAXIMUM_NETWORK_DEVICE_ID_BYTES
            || id.chars().any(char::is_whitespace)
        {
            return Err(ConfigError::InvalidValue("network.device.id"));
        }
        if self.device.name.len() > MAXIMUM_NETWORK_DEVICE_NAME_BYTES {
            return Err(ConfigError::InvalidValue("network.device.name"));
        }
        if self.server.port == 0 {
            return Err(ConfigError::InvalidValue("network.server.port"));
        }
        if self.client.source == NetworkClientSource::Manual
            && self
                .client
                .url
                .as_deref()
                .is_none_or(|url| url.trim().is_empty())
        {
            return Err(ConfigError::InvalidValue("network.client.url"));
        }
        if self.server.listen == NetworkListenMode::Custom
            && self
                .server
                .address
                .as_deref()
                .is_none_or(|address| address.trim().is_empty())
        {
            return Err(ConfigError::InvalidValue("network.server.address"));
        }
        Ok(())
    }
}
