//! Projecting the persisted configuration onto the runtime, logging and settings
//! protocol types, and back.
//!
//! These conversions are the only place the two vocabularies meet, so a field
//! added on one side and forgotten on the other is a compile error here rather
//! than a setting that quietly stops doing anything.

use bongocat_config::{
    ConfigError, LoggingConfig, LoggingLevel, NativeConfig, NetworkClientConfig,
    NetworkClientSource, NetworkConfig, NetworkDeviceConfig, NetworkListenMode,
    NetworkServerConfig, RandomBehaviorMode,
};
use bongocat_input::GamepadAxisSettings;
use bongocat_log::{LogLevel as RuntimeLogLevel, LogSettings as RuntimeLogSettings};
use bongocat_runtime::{
    CursorSettings, ModelSettings, OverlaySettings,
    RandomBehaviorMode as RuntimeRandomBehaviorMode, RandomBehaviorSettings,
};
use bongocat_ui_protocol::{
    SettingsNetwork, SettingsNetworkClient, SettingsNetworkClientSource, SettingsNetworkDevice,
    SettingsNetworkListenMode, SettingsNetworkServer, SettingsRandomBehaviorMode,
};

pub(crate) const fn runtime_log_level(level: LoggingLevel) -> RuntimeLogLevel {
    match level {
        LoggingLevel::Error => RuntimeLogLevel::Error,
        LoggingLevel::Warn => RuntimeLogLevel::Warn,
        LoggingLevel::Info => RuntimeLogLevel::Info,
        LoggingLevel::Debug => RuntimeLogLevel::Debug,
        LoggingLevel::Trace => RuntimeLogLevel::Trace,
    }
}

pub(crate) fn runtime_log_settings(config: &LoggingConfig) -> RuntimeLogSettings {
    RuntimeLogSettings {
        level: runtime_log_level(config.level),
        retention_days: u64::from(config.retention_days),
    }
}

pub(crate) const fn logging_config_from_settings(
    settings: bongocat_ui_protocol::SettingsLogging,
) -> LoggingConfig {
    LoggingConfig {
        level: match settings.level {
            bongocat_ui_protocol::SettingsLogLevel::Error => LoggingLevel::Error,
            bongocat_ui_protocol::SettingsLogLevel::Warn => LoggingLevel::Warn,
            bongocat_ui_protocol::SettingsLogLevel::Info => LoggingLevel::Info,
            bongocat_ui_protocol::SettingsLogLevel::Debug => LoggingLevel::Debug,
            bongocat_ui_protocol::SettingsLogLevel::Trace => LoggingLevel::Trace,
        },
        retention_days: settings.retention_days,
    }
}

pub(crate) const fn settings_logging_from_config(
    config: &LoggingConfig,
) -> bongocat_ui_protocol::SettingsLogging {
    bongocat_ui_protocol::SettingsLogging {
        level: match config.level {
            LoggingLevel::Error => bongocat_ui_protocol::SettingsLogLevel::Error,
            LoggingLevel::Warn => bongocat_ui_protocol::SettingsLogLevel::Warn,
            LoggingLevel::Info => bongocat_ui_protocol::SettingsLogLevel::Info,
            LoggingLevel::Debug => bongocat_ui_protocol::SettingsLogLevel::Debug,
            LoggingLevel::Trace => bongocat_ui_protocol::SettingsLogLevel::Trace,
        },
        retention_days: config.retention_days,
    }
}

pub(crate) fn overlay_settings_from_config(config: &NativeConfig) -> OverlaySettings {
    OverlaySettings {
        window_mode: config.overlay.window_mode,
        window_background_color: config.overlay.window_background_color,
        click_through: config.overlay.click_through,
        hold_modifier_to_interact: config.overlay.hold_modifier_to_interact,
        always_on_top: config.overlay.always_on_top,
        scale_percent: config.overlay.scale_percent,
        opacity_percent: config.overlay.opacity_percent,
        corner_radius_percent: config.overlay.corner_radius_percent,
        hide_on_pointer_hover: config.overlay.hide_on_pointer_hover,
        hide_on_pointer_hover_delay_seconds: config.overlay.hide_on_pointer_hover_delay_seconds,
        hide_on_idle: config.overlay.hide_on_idle,
        hide_on_idle_delay_seconds: config.overlay.hide_on_idle_delay_seconds,
        keep_inside_screen: config.overlay.keep_inside_screen,
    }
}

pub(crate) const fn cursor_settings_from_config(config: &NativeConfig) -> CursorSettings {
    CursorSettings {
        force_move: config.input.mouse.force_move,
    }
}

pub(crate) const fn model_settings_from_config(config: &NativeConfig) -> ModelSettings {
    ModelSettings {
        mirror: config.model.mirror,
        mirror_pointer_tracking_horizontal: config.model.mirror_pointer_tracking_horizontal,
        mirror_pointer_tracking_vertical: config.model.mirror_pointer_tracking_vertical,
        ignore_keyboard: config.model.ignore_keyboard,
        ignore_gamepad: config.model.ignore_gamepad,
        show_all_pressed_keys: config.model.show_all_pressed_keys,
        toggle_repeated_expression: config.model.toggle_repeated_expression,
        allow_motion_overlap: config.model.allow_motion_overlap,
        ignore_pointer: config.model.ignore_pointer,
    }
}

// The random-behavior mode exists in three vocabularies, the same way the log
// level does. Each is owned by the layer that has a reason for it: the document
// persists it, the runtime filters candidates with it, and the window needs a
// catalogue it can render as a dropdown without importing either. The four
// conversions below are the only places the three meet, so a variant added on one
// side and forgotten on another is a compile error here rather than a mode the
// user can pick and the product ignores.

pub(crate) const fn random_behavior_mode_to_runtime(
    mode: RandomBehaviorMode,
) -> RuntimeRandomBehaviorMode {
    match mode {
        RandomBehaviorMode::Off => RuntimeRandomBehaviorMode::Off,
        RandomBehaviorMode::Expressions => RuntimeRandomBehaviorMode::Expressions,
        RandomBehaviorMode::Motions => RuntimeRandomBehaviorMode::Motions,
        RandomBehaviorMode::MotionsAndExpressions => {
            RuntimeRandomBehaviorMode::MotionsAndExpressions
        }
    }
}

pub(crate) const fn random_behavior_mode_to_config(
    mode: RuntimeRandomBehaviorMode,
) -> RandomBehaviorMode {
    match mode {
        RuntimeRandomBehaviorMode::Off => RandomBehaviorMode::Off,
        RuntimeRandomBehaviorMode::Expressions => RandomBehaviorMode::Expressions,
        RuntimeRandomBehaviorMode::Motions => RandomBehaviorMode::Motions,
        RuntimeRandomBehaviorMode::MotionsAndExpressions => {
            RandomBehaviorMode::MotionsAndExpressions
        }
    }
}

pub(crate) const fn random_behavior_mode_to_runtime_settings(
    mode: SettingsRandomBehaviorMode,
) -> RuntimeRandomBehaviorMode {
    match mode {
        SettingsRandomBehaviorMode::Off => RuntimeRandomBehaviorMode::Off,
        SettingsRandomBehaviorMode::Expressions => RuntimeRandomBehaviorMode::Expressions,
        SettingsRandomBehaviorMode::Motions => RuntimeRandomBehaviorMode::Motions,
        SettingsRandomBehaviorMode::MotionsAndExpressions => {
            RuntimeRandomBehaviorMode::MotionsAndExpressions
        }
    }
}

pub(crate) const fn random_behavior_mode_from_runtime(
    mode: RuntimeRandomBehaviorMode,
) -> SettingsRandomBehaviorMode {
    match mode {
        RuntimeRandomBehaviorMode::Off => SettingsRandomBehaviorMode::Off,
        RuntimeRandomBehaviorMode::Expressions => SettingsRandomBehaviorMode::Expressions,
        RuntimeRandomBehaviorMode::Motions => SettingsRandomBehaviorMode::Motions,
        RuntimeRandomBehaviorMode::MotionsAndExpressions => {
            SettingsRandomBehaviorMode::MotionsAndExpressions
        }
    }
}

pub(crate) const fn random_behavior_settings_from_config(
    config: &NativeConfig,
) -> RandomBehaviorSettings {
    RandomBehaviorSettings {
        mode: random_behavior_mode_to_runtime(config.model.random_behavior.mode),
        interval_seconds: config.model.random_behavior.interval_seconds,
    }
}

pub(crate) fn gamepad_axis_settings_from_config(
    config: &NativeConfig,
) -> Result<GamepadAxisSettings, ConfigError> {
    let stick_dead_zone = runtime_dead_zone(
        config.input.gamepad.stick_dead_zone,
        "input.gamepad.stick_dead_zone",
    )?;
    let trigger_dead_zone = runtime_dead_zone(
        config.input.gamepad.trigger_dead_zone,
        "input.gamepad.trigger_dead_zone",
    )?;
    GamepadAxisSettings::new(stick_dead_zone, trigger_dead_zone)
        .ok_or(ConfigError::InvalidValue("input.gamepad"))
}

pub(crate) fn runtime_dead_zone(value: f64, field: &'static str) -> Result<f32, ConfigError> {
    let value = value as f32;
    if value.is_finite() && (0.0..1.0).contains(&value) {
        Ok(value)
    } else {
        Err(ConfigError::InvalidValue(field))
    }
}

// The multiplayer section lives in three vocabularies for the same reason the
// log level does: the document persists it, the relay obeys it, and the window
// has to render a dropdown without importing either. The conversions below are
// the only places the three meet, so a variant added on one side and forgotten
// on another is a compile error here rather than a mode the user can pick and
// the product ignores.

pub(crate) const fn network_listen_mode_to_config(
    mode: SettingsNetworkListenMode,
) -> NetworkListenMode {
    match mode {
        SettingsNetworkListenMode::LocalOnly => NetworkListenMode::LocalOnly,
        SettingsNetworkListenMode::Ipv4 => NetworkListenMode::Ipv4,
        SettingsNetworkListenMode::DualStack => NetworkListenMode::DualStack,
        SettingsNetworkListenMode::Custom => NetworkListenMode::Custom,
    }
}

pub(crate) const fn network_listen_mode_to_settings(
    mode: NetworkListenMode,
) -> SettingsNetworkListenMode {
    match mode {
        NetworkListenMode::LocalOnly => SettingsNetworkListenMode::LocalOnly,
        NetworkListenMode::Ipv4 => SettingsNetworkListenMode::Ipv4,
        NetworkListenMode::DualStack => SettingsNetworkListenMode::DualStack,
        NetworkListenMode::Custom => SettingsNetworkListenMode::Custom,
    }
}

pub(crate) const fn network_client_source_to_config(
    source: SettingsNetworkClientSource,
) -> NetworkClientSource {
    match source {
        SettingsNetworkClientSource::FollowLocal => NetworkClientSource::FollowLocal,
        SettingsNetworkClientSource::Manual => NetworkClientSource::Manual,
    }
}

pub(crate) const fn network_client_source_to_settings(
    source: NetworkClientSource,
) -> SettingsNetworkClientSource {
    match source {
        NetworkClientSource::FollowLocal => SettingsNetworkClientSource::FollowLocal,
        NetworkClientSource::Manual => SettingsNetworkClientSource::Manual,
    }
}

pub(crate) fn network_config_from_settings(settings: SettingsNetwork) -> NetworkConfig {
    NetworkConfig {
        device: NetworkDeviceConfig {
            id: settings.device.id,
            name: settings.device.name,
        },
        client: NetworkClientConfig {
            enabled: settings.client.enabled,
            source: network_client_source_to_config(settings.client.source),
            url: settings.client.url,
            token: settings.client.token,
            publish: settings.client.publish,
            display: settings.client.display,
        },
        server: NetworkServerConfig {
            enabled: settings.server.enabled,
            listen: network_listen_mode_to_config(settings.server.listen),
            address: settings.server.address,
            port: settings.server.port,
            token: settings.server.token,
        },
    }
}

pub(crate) fn settings_network_from_config(config: &NetworkConfig) -> SettingsNetwork {
    SettingsNetwork {
        device: SettingsNetworkDevice {
            id: config.device.id.clone(),
            name: config.device.name.clone(),
        },
        client: SettingsNetworkClient {
            enabled: config.client.enabled,
            source: network_client_source_to_settings(config.client.source),
            url: config.client.url.clone(),
            token: config.client.token.clone(),
            publish: config.client.publish,
            display: config.client.display.clone(),
        },
        server: SettingsNetworkServer {
            enabled: config.server.enabled,
            listen: network_listen_mode_to_settings(config.server.listen),
            address: config.server.address.clone(),
            port: config.server.port,
            token: config.server.token.clone(),
        },
    }
}

pub(crate) fn persistent_dead_zone(value: f32) -> f64 {
    value.to_string().parse().unwrap_or(f64::NAN)
}
