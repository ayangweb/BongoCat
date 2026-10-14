//! 打字音效插件
//!
//! 为键盘输入添加打字音效。

use bongocat_plugin_sdk::{
    ConfigField, ConfigFieldType, HostMessage, InputEvent, Plugin, PluginConfigSchema, PluginError,
    PluginMessage, PluginMetadata,
};
use std::collections::HashMap;

/// 打字音效插件
pub struct TypingSoundsPlugin {
    /// 是否启用音效
    enabled: bool,
    /// 音量 (0.0 - 1.0)
    volume: f32,
    /// 音效类型
    sound_type: String,
}

impl TypingSoundsPlugin {
    /// 创建新的插件实例
    fn create() -> Box<dyn Plugin> {
        Box::new(Self {
            enabled: true,
            volume: 0.5,
            sound_type: "mechanical".to_string(),
        })
    }
}

impl Plugin for TypingSoundsPlugin {
    fn metadata(&self) -> PluginMetadata {
        PluginMetadata {
            id: "com.bongocat.typing-sounds".to_string(),
            name: "打字音效".to_string(),
            description: "为键盘输入添加打字音效".to_string(),
            version: "1.0.0".to_string(),
            author: "BongoCat Team".to_string(),
            icon: "🔊".to_string(),
            min_app_version: "2.3.0".to_string(),
            homepage: None,
            repository: Some("https://github.com/ayangweb/BongoCat".to_string()),
        }
    }

    fn config_schema(&self) -> Option<PluginConfigSchema> {
        Some(PluginConfigSchema {
            fields: vec![
                ConfigField {
                    key: "enabled".to_string(),
                    label: "启用音效".to_string(),
                    field_type: ConfigFieldType::Boolean,
                    default_value: serde_json::json!(true),
                    description: Some("是否启用打字音效".to_string()),
                },
                ConfigField {
                    key: "volume".to_string(),
                    label: "音量".to_string(),
                    field_type: ConfigFieldType::Float {
                        min: Some(0.0),
                        max: Some(1.0),
                    },
                    default_value: serde_json::json!(0.5),
                    description: Some("音效音量 (0.0 - 1.0)".to_string()),
                },
                ConfigField {
                    key: "sound_type".to_string(),
                    label: "音效类型".to_string(),
                    field_type: ConfigFieldType::Enum {
                        options: vec![
                            "mechanical".to_string(),
                            "typewriter".to_string(),
                            "soft".to_string(),
                        ],
                    },
                    default_value: serde_json::json!("mechanical"),
                    description: Some("打字音效类型".to_string()),
                },
            ],
        })
    }

    fn initialize(
        &mut self,
        config: HashMap<String, serde_json::Value>,
    ) -> Result<(), PluginError> {
        if let Some(enabled) = config.get("enabled").and_then(|v| v.as_bool()) {
            self.enabled = enabled;
        }

        if let Some(volume) = config.get("volume").and_then(|v| v.as_f64()) {
            self.volume = (volume as f32).clamp(0.0, 1.0);
        }

        if let Some(sound_type) = config.get("sound_type").and_then(|v| v.as_str()) {
            self.sound_type = sound_type.to_string();
        }

        Ok(())
    }

    fn handle_message(&mut self, message: HostMessage) -> Result<Vec<PluginMessage>, PluginError> {
        match message {
            HostMessage::InputEvent { event } => {
                if !self.enabled {
                    return Ok(Vec::new());
                }

                match event {
                    InputEvent::KeyDown { .. } => {
                        // 播放打字音效
                        let audio_file = format!("sounds/{}.wav", self.sound_type);
                        Ok(vec![PluginMessage::PlayAudio {
                            file_path: audio_file,
                        }])
                    }
                    _ => Ok(Vec::new()),
                }
            }
            HostMessage::ConfigChanged { config } => {
                self.initialize(config)?;
                Ok(Vec::new())
            }
            _ => Ok(Vec::new()),
        }
    }

    fn shutdown(&mut self) -> Result<(), PluginError> {
        Ok(())
    }
}

bongocat_plugin_sdk::declare_plugin!(TypingSoundsPlugin, TypingSoundsPlugin::create);
