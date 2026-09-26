//! Projecting the persisted configuration onto the runtime, logging and settings
//! protocol types, and back.
//!
//! These conversions are the only place the two vocabularies meet, so a field
//! added on one side and forgotten on the other is a compile error here rather
//! than a setting that quietly stops doing anything.

use bongocat_config::{ConfigError, LoggingConfig, LoggingLevel, NativeConfig};
use bongocat_input::GamepadAxisSettings;
use bongocat_log::{LogLevel as RuntimeLogLevel, LogSettings as RuntimeLogSettings};
use bongocat_runtime::{ModelSettings, OverlaySettings, RandomBehaviorSettings};

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
        click_through: config.overlay.click_through,
        always_on_top: config.overlay.always_on_top,
        scale_percent: config.overlay.scale_percent,
        opacity_percent: config.overlay.opacity_percent,
        corner_radius_percent: config.overlay.corner_radius_percent,
        hide_on_pointer_hover: config.overlay.hide_on_pointer_hover,
        hide_on_pointer_hover_delay_seconds: config.overlay.hide_on_pointer_hover_delay_seconds,
        keep_inside_screen: config.overlay.keep_inside_screen,
    }
}

pub(crate) const fn model_settings_from_config(config: &NativeConfig) -> ModelSettings {
    ModelSettings {
        mirror: config.model.mirror,
        mirror_pointer_tracking: config.model.mirror_pointer_tracking,
        ignore_keyboard: config.model.ignore_keyboard,
        ignore_gamepad: config.model.ignore_gamepad,
        ignore_pointer: config.model.ignore_pointer,
    }
}

pub(crate) const fn random_behavior_settings_from_config(
    config: &NativeConfig,
) -> RandomBehaviorSettings {
    RandomBehaviorSettings {
        enabled: config.model.random_behavior.enabled,
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

pub(crate) fn persistent_dead_zone(value: f32) -> f64 {
    value.to_string().parse().unwrap_or(f64::NAN)
}
