//! 插件注册表

use bongocat_plugin_sdk::{HostMessage, Plugin, PluginConfigSchema, PluginMessage, PluginMetadata};
use std::collections::HashMap;
use std::path::PathBuf;
use thiserror::Error;

/// 注册表错误
#[derive(Debug, Error)]
pub enum RegistryError {
    #[error("插件未找到: {0}")]
    PluginNotFound(String),
    #[error("插件初始化失败: {0}")]
    InitializationFailed(String),
    #[error("插件运行时错误: {0}")]
    RuntimeError(String),
}

/// 插件状态
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PluginState {
    /// 已禁用
    Disabled,
    /// 已启用
    Enabled,
}

/// 插件注册信息
struct PluginEntry {
    /// 插件实例
    plugin: Box<dyn Plugin>,
    /// 插件状态
    state: PluginState,
    /// 插件路径
    #[allow(dead_code)]
    path: PathBuf,
    /// 插件配置
    config: HashMap<String, serde_json::Value>,
}

/// 插件注册表
pub struct PluginRegistry {
    plugins: HashMap<String, PluginEntry>,
}

impl PluginRegistry {
    /// 创建新的注册表
    pub fn new() -> Self {
        Self {
            plugins: HashMap::new(),
        }
    }

    /// 注册插件
    pub fn register(
        &mut self,
        id: String,
        plugin: Box<dyn Plugin>,
        path: PathBuf,
    ) -> Result<(), RegistryError> {
        let entry = PluginEntry {
            plugin,
            state: PluginState::Disabled,
            path,
            config: HashMap::new(),
        };

        self.plugins.insert(id, entry);
        Ok(())
    }

    /// 取消注册插件
    pub fn unregister(&mut self, id: &str) -> Result<(), RegistryError> {
        if let Some(mut entry) = self.plugins.remove(id) {
            let _ = entry.plugin.shutdown();
            Ok(())
        } else {
            Err(RegistryError::PluginNotFound(id.to_string()))
        }
    }

    /// 启用插件
    pub fn enable(&mut self, id: &str) -> Result<(), RegistryError> {
        let entry = self
            .plugins
            .get_mut(id)
            .ok_or_else(|| RegistryError::PluginNotFound(id.to_string()))?;

        if entry.state == PluginState::Disabled {
            entry
                .plugin
                .initialize(entry.config.clone())
                .map_err(|e| RegistryError::InitializationFailed(e.to_string()))?;
            entry.state = PluginState::Enabled;
        }

        Ok(())
    }

    /// 禁用插件
    pub fn disable(&mut self, id: &str) -> Result<(), RegistryError> {
        let entry = self
            .plugins
            .get_mut(id)
            .ok_or_else(|| RegistryError::PluginNotFound(id.to_string()))?;

        if entry.state == PluginState::Enabled {
            let _ = entry.plugin.shutdown();
            entry.state = PluginState::Disabled;
        }

        Ok(())
    }

    /// 获取插件
    pub fn contains(&self, id: &str) -> bool {
        self.plugins.contains_key(id)
    }

    /// 列出所有插件
    pub fn list(&self) -> Vec<(String, PluginMetadata, PluginState)> {
        self.plugins
            .iter()
            .map(|(id, entry)| (id.clone(), entry.plugin.metadata(), entry.state))
            .collect()
    }

    /// 向所有启用的插件广播消息
    pub fn broadcast_message(&mut self, message: HostMessage) -> Vec<(String, Vec<PluginMessage>)> {
        let mut results = Vec::new();

        for (id, entry) in &mut self.plugins {
            if entry.state == PluginState::Enabled {
                match entry.plugin.handle_message(message.clone()) {
                    Ok(messages) if !messages.is_empty() => {
                        results.push((id.clone(), messages));
                    }
                    Err(e) => {
                        eprintln!("插件 {} 处理消息时出错: {}", id, e);
                    }
                    _ => {}
                }
            }
        }

        results
    }

    /// 向特定插件发送消息
    pub fn send_message(
        &mut self,
        id: &str,
        message: HostMessage,
    ) -> Result<Vec<PluginMessage>, RegistryError> {
        let entry = self
            .plugins
            .get_mut(id)
            .ok_or_else(|| RegistryError::PluginNotFound(id.to_string()))?;

        if entry.state != PluginState::Enabled {
            return Ok(Vec::new());
        }

        entry
            .plugin
            .handle_message(message)
            .map_err(|e| RegistryError::RuntimeError(e.to_string()))
    }

    /// 获取插件配置 schema
    pub fn get_config_schema(&self, id: &str) -> Option<Option<PluginConfigSchema>> {
        self.plugins
            .get(id)
            .map(|entry| entry.plugin.config_schema())
    }

    /// 更新插件配置
    pub fn update_config(
        &mut self,
        id: &str,
        config: HashMap<String, serde_json::Value>,
    ) -> Result<(), RegistryError> {
        let entry = self
            .plugins
            .get_mut(id)
            .ok_or_else(|| RegistryError::PluginNotFound(id.to_string()))?;

        entry.config = config.clone();

        if entry.state == PluginState::Enabled {
            let message = HostMessage::ConfigChanged { config };
            entry
                .plugin
                .handle_message(message)
                .map_err(|e| RegistryError::RuntimeError(e.to_string()))?;
        }

        Ok(())
    }

    /// 关闭所有插件
    pub fn shutdown_all(&mut self) {
        for entry in self.plugins.values_mut() {
            if entry.state == PluginState::Enabled {
                entry.plugin.shutdown().ok();
            }
        }
    }
}

impl Default for PluginRegistry {
    fn default() -> Self {
        Self::new()
    }
}
