//! 输入统计插件
//!
//! 统计键盘按键次数和鼠标移动距离。

use bongocat_plugin_sdk::{
    ConfigField, ConfigFieldType, HostMessage, InputEvent, Plugin, PluginConfigSchema, PluginError,
    PluginMessage, PluginMetadata, RenderContent, RenderPosition,
};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// 统计数据
#[derive(Debug, Clone, Serialize, Deserialize)]
struct Statistics {
    /// 今日按键次数
    key_count: u64,
    /// 今日鼠标移动距离（像素）
    mouse_distance: f64,
    /// 最后更新日期（YYYY-MM-DD 格式）
    last_date: String,
}

impl Default for Statistics {
    fn default() -> Self {
        Self {
            key_count: 0,
            mouse_distance: 0.0,
            last_date: Self::current_date(),
        }
    }
}

impl Statistics {
    /// 获取当前日期字符串（YYYY-MM-DD）
    fn current_date() -> String {
        use std::time::{SystemTime, UNIX_EPOCH};
        let duration = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default();
        let days = duration.as_secs() / 86400;

        // 简单计算日期（1970-01-01 起）
        let total_days = days as i64;
        let mut year = 1970;
        let mut day_of_year = total_days;

        loop {
            let days_in_year = if Self::is_leap_year(year) { 366 } else { 365 };
            if day_of_year < days_in_year {
                break;
            }
            day_of_year -= days_in_year;
            year += 1;
        }

        let (month, day) = Self::day_of_year_to_month_day(year, day_of_year as u32);
        format!("{:04}-{:02}-{:02}", year, month, day)
    }

    /// 判断是否为闰年
    fn is_leap_year(year: i64) -> bool {
        (year % 4 == 0 && year % 100 != 0) || (year % 400 == 0)
    }

    /// 将年内天数转换为月日
    fn day_of_year_to_month_day(year: i64, day_of_year: u32) -> (u32, u32) {
        let days_in_months = if Self::is_leap_year(year) {
            [31, 29, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31]
        } else {
            [31, 28, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31]
        };

        let mut remaining = day_of_year + 1; // 转为 1-based
        for (month_idx, &days) in days_in_months.iter().enumerate() {
            if remaining <= days {
                return ((month_idx + 1) as u32, remaining);
            }
            remaining -= days;
        }
        (12, 31)
    }

    /// 检查并重置统计（如果日期已改变）
    fn check_and_reset(&mut self) {
        let current = Self::current_date();
        if self.last_date != current {
            self.key_count = 0;
            self.mouse_distance = 0.0;
            self.last_date = current;
        }
    }

    /// 增加按键计数
    fn add_key(&mut self) {
        self.check_and_reset();
        self.key_count += 1;
    }
}

/// 输入统计插件
pub struct InputStatsPlugin {
    /// 是否启用统计
    enabled: bool,
    /// 是否显示统计信息
    show_stats: bool,
    /// 统计数据
    stats: Statistics,
}

impl InputStatsPlugin {
    /// 创建新的插件实例
    fn create() -> Box<dyn Plugin> {
        Box::new(Self {
            enabled: true,
            show_stats: true,
            stats: Statistics::default(),
        })
    }
}

impl Plugin for InputStatsPlugin {
    fn metadata(&self) -> PluginMetadata {
        PluginMetadata {
            id: "com.bongocat.input-stats".to_string(),
            name: "输入统计".to_string(),
            description: "统计键盘按键次数和鼠标移动距离".to_string(),
            version: "1.0.0".to_string(),
            author: "BongoCat Team".to_string(),
            icon: "📊".to_string(),
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
                    label: "启用统计".to_string(),
                    field_type: ConfigFieldType::Boolean,
                    default_value: serde_json::json!(true),
                    description: Some("是否启用输入统计".to_string()),
                },
                ConfigField {
                    key: "show_stats".to_string(),
                    label: "显示统计信息".to_string(),
                    field_type: ConfigFieldType::Boolean,
                    default_value: serde_json::json!(true),
                    description: Some("是否在模型窗口显示统计信息".to_string()),
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

        if let Some(show_stats) = config.get("show_stats").and_then(|v| v.as_bool()) {
            self.show_stats = show_stats;
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
                        self.stats.add_key();

                        let mut messages = vec![
                            // 保存统计数据
                            PluginMessage::SaveData {
                                key: "statistics".to_string(),
                                value: serde_json::to_string(&self.stats)
                                    .map_err(|e| PluginError::RuntimeError(e.to_string()))?,
                            },
                        ];

                        // 如果启用显示，更新渲染
                        if self.show_stats {
                            messages.push(PluginMessage::UpdateRender {
                                position: RenderPosition::TopRight,
                                content: Some(RenderContent::Text {
                                    content: format!(
                                        "按键: {} 次\n鼠标: {:.2} km",
                                        self.stats.key_count,
                                        self.stats.mouse_distance / 1_000_000.0
                                    ),
                                    font_size: 14.0,
                                    color: (255, 255, 255, 200),
                                }),
                            });
                        }

                        Ok(messages)
                    }
                    InputEvent::MouseMove { delta_x, delta_y } => {
                        // 使用增量计算移动距离
                        let distance = ((delta_x * delta_x + delta_y * delta_y) as f64).sqrt();
                        self.stats.check_and_reset();
                        self.stats.mouse_distance += distance;

                        // 每移动一定距离保存一次（避免过于频繁）
                        let mut messages = Vec::new();
                        if self.stats.mouse_distance as u64 % 10000 < 100 {
                            messages.push(PluginMessage::SaveData {
                                key: "statistics".to_string(),
                                value: serde_json::to_string(&self.stats)
                                    .map_err(|e| PluginError::RuntimeError(e.to_string()))?,
                            });
                        }

                        // 如果启用显示，更新渲染
                        if self.show_stats {
                            messages.push(PluginMessage::UpdateRender {
                                position: RenderPosition::TopRight,
                                content: Some(RenderContent::Text {
                                    content: format!(
                                        "按键: {} 次\n鼠标: {:.2} km",
                                        self.stats.key_count,
                                        self.stats.mouse_distance / 1_000_000.0
                                    ),
                                    font_size: 14.0,
                                    color: (255, 255, 255, 200),
                                }),
                            });
                        }

                        Ok(messages)
                    }
                    _ => Ok(Vec::new()),
                }
            }
            HostMessage::ConfigChanged { config } => {
                self.initialize(config)?;
                Ok(Vec::new())
            }
            HostMessage::DataLoaded { key, value } => {
                if key == "statistics" {
                    if let Some(value) = value {
                        if let Ok(stats) = serde_json::from_str::<Statistics>(&value) {
                            self.stats = stats;
                            self.stats.check_and_reset(); // 检查日期
                        }
                    }
                }
                Ok(Vec::new())
            }
        }
    }

    fn shutdown(&mut self) -> Result<(), PluginError> {
        // 最后保存一次统计数据
        Ok(())
    }
}

bongocat_plugin_sdk::declare_plugin!(InputStatsPlugin, InputStatsPlugin::create);
