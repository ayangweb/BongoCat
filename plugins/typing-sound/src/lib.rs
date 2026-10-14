//! 打字音效插件
//!
//! 监听键盘输入事件并播放打字音效。

use bongocat_plugin_sdk::{
    ConfigField, ConfigFieldType, HostMessage, InputEvent, Plugin, PluginConfigSchema, PluginError,
    PluginMessage, PluginMetadata,
};
use std::collections::HashMap;

/// 打字音效插件
pub struct TypingSoundPlugin {
    /// 是否启用
    enabled: bool,
    /// 音效文件路径
    sound_file: String,
    /// 音量（0.0 - 1.0）
    volume: f64,
}

impl TypingSoundPlugin {
    /// 创建新的插件实例
    pub fn new() -> Box<dyn Plugin> {
        Box::new(Self {
            enabled: true,
            sound_file: String::new(),
            volume: 0.5,
        })
    }
}

impl Plugin for TypingSoundPlugin {
    fn metadata(&self) -> PluginMetadata {
        PluginMetadata {
            id: "com.bongocat.typing-sound".to_string(),
            name: "打字音效".to_string(),
            description: "为键盘输入添加打字音效".to_string(),
            version: "1.0.0".to_string(),
            author: "BongoCat Team".to_string(),
            icon: "⌨️".to_string(),
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
                    key: "sound_file".to_string(),
                    label: "音效文件".to_string(),
                    field_type: ConfigFieldType::String,
                    default_value: serde_json::json!(""),
                    description: Some("打字音效文件路径（相对于插件目录）".to_string()),
                },
                ConfigField {
                    key: "volume".to_string(),
                    label: "音量".to_string(),
                    field_type: ConfigFieldType::Float {
                        min: Some(0.0),
                        max: Some(1.0),
                    },
                    default_value: serde_json::json!(0.5),
                    description: Some("音效音量（0.0 - 1.0）".to_string()),
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

        if let Some(sound_file) = config.get("sound_file").and_then(|v| v.as_str()) {
            self.sound_file = sound_file.to_string();
        }

        if let Some(volume) = config.get("volume").and_then(|v| v.as_f64()) {
            self.volume = volume.clamp(0.0, 1.0);
        }

        Ok(())
    }

    fn handle_message(&mut self, message: HostMessage) -> Result<Vec<PluginMessage>, PluginError> {
        match message {
            HostMessage::InputEvent { event } => {
                if !self.enabled || self.sound_file.is_empty() {
                    return Ok(Vec::new());
                }

                // 仅响应键盘按下事件
                if matches!(event, InputEvent::KeyDown { .. }) {
                    return Ok(vec![PluginMessage::PlayAudio {
                        file_path: self.sound_file.clone(),
                    }]);
                }

                Ok(Vec::new())
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

// 声明插件入口点
bongocat_plugin_sdk::declare_plugin!(TypingSoundPlugin, TypingSoundPlugin::new);
