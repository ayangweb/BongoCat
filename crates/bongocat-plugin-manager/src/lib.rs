//! BongoCat 插件管理器
//!
//! 负责插件的发现、加载、卸载和生命周期管理。

use bongocat_plugin_sdk::{HostMessage, PluginError, PluginMessage, PluginMetadata};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use thiserror::Error;

mod loader;
mod registry;

pub use loader::{LoaderError, PluginLoader};
pub use registry::{PluginRegistry, PluginState, RegistryError};

/// 插件管理器错误
#[derive(Debug, Error)]
pub enum PluginManagerError {
    #[error("插件加载失败: {0}")]
    LoadFailed(String),
    #[error("插件未找到: {0}")]
    PluginNotFound(String),
    #[error("插件已存在: {0}")]
    PluginAlreadyExists(String),
    #[error("插件错误: {0}")]
    PluginError(#[from] PluginError),
    #[error("加载器错误: {0}")]
    LoaderError(#[from] LoaderError),
    #[error("注册表错误: {0}")]
    RegistryError(#[from] RegistryError),
    #[error("IO 错误: {0}")]
    IoError(#[from] std::io::Error),
    #[error("序列化错误: {0}")]
    SerializationError(#[from] serde_json::Error),
}

/// 插件清单（用于生产环境从 GitHub 获取）
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PluginManifest {
    /// 插件列表
    pub plugins: Vec<PluginManifestEntry>,
}

/// 插件清单条目
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PluginManifestEntry {
    /// 插件 ID
    pub id: String,
    /// 插件名称
    pub name: String,
    /// 插件描述
    pub description: String,
    /// 插件版本
    pub version: String,
    /// 插件作者
    pub author: String,
    /// 插件图标（Emoji）
    pub icon: String,
    /// 最小应用版本
    pub min_app_version: String,
    /// 下载 URL（按平台）
    pub download_urls: HashMap<String, String>,
    /// 插件主页
    pub homepage: Option<String>,
    /// 插件仓库
    pub repository: Option<String>,
}

/// 插件管理器
pub struct PluginManager {
    /// 插件注册表
    registry: PluginRegistry,
    /// 插件加载器
    loader: PluginLoader,
    /// 插件目录
    plugin_dir: PathBuf,
}

impl PluginManager {
    /// 创建新的插件管理器
    pub fn new(plugin_dir: PathBuf) -> Result<Self, PluginManagerError> {
        // 确保插件目录存在
        if !plugin_dir.exists() {
            bongocat_storage::create_private_dir_all(&plugin_dir)?;
        }

        Ok(Self {
            registry: PluginRegistry::new(),
            loader: PluginLoader::new(),
            plugin_dir,
        })
    }

    /// 获取插件目录
    pub fn plugin_dir(&self) -> &Path {
        &self.plugin_dir
    }

    /// 扫描并加载所有可用插件
    pub fn discover_plugins(&mut self) -> Result<(), PluginManagerError> {
        if !self.plugin_dir.exists() {
            return Ok(());
        }

        for entry in walkdir::WalkDir::new(&self.plugin_dir)
            .max_depth(2)
            .into_iter()
            .filter_map(|e| e.ok())
        {
            let path = entry.path();
            if path.is_file() {
                let extension = path.extension().and_then(|s| s.to_str());
                let is_plugin = if cfg!(target_os = "windows") {
                    extension == Some("dll")
                } else if cfg!(target_os = "macos") {
                    extension == Some("dylib")
                } else {
                    false
                };

                if is_plugin {
                    self.load_plugin(path).ok();
                }
            }
        }

        Ok(())
    }

    /// 加载插件
    fn load_plugin(&mut self, path: &Path) -> Result<(), PluginManagerError> {
        let plugin = self.loader.load(path)?;
        let metadata = plugin.metadata();
        let id = metadata.id.clone();

        if self.registry.contains(&id) {
            return Err(PluginManagerError::PluginAlreadyExists(id));
        }

        self.registry.register(id, plugin, path.to_path_buf())?;
        Ok(())
    }

    /// 获取所有插件元数据
    pub fn list_plugins(&self) -> Vec<(String, PluginMetadata, PluginState)> {
        self.registry.list()
    }

    /// 启用插件
    pub fn enable_plugin(&mut self, id: &str) -> Result<(), PluginManagerError> {
        Ok(self.registry.enable(id)?)
    }

    /// 禁用插件
    pub fn disable_plugin(&mut self, id: &str) -> Result<(), PluginManagerError> {
        Ok(self.registry.disable(id)?)
    }

    /// 卸载插件
    pub fn uninstall_plugin(&mut self, id: &str) -> Result<(), PluginManagerError> {
        Ok(self.registry.unregister(id)?)
    }

    /// 向所有启用的插件广播消息
    pub fn broadcast_message(&mut self, message: HostMessage) -> Vec<(String, Vec<PluginMessage>)> {
        self.registry.broadcast_message(message)
    }

    /// 向特定插件发送消息
    pub fn send_message_to_plugin(
        &mut self,
        id: &str,
        message: HostMessage,
    ) -> Result<Vec<PluginMessage>, PluginManagerError> {
        Ok(self.registry.send_message(id, message)?)
    }

    /// 获取插件配置 schema
    pub fn get_plugin_config_schema(
        &self,
        id: &str,
    ) -> Result<Option<bongocat_plugin_sdk::PluginConfigSchema>, PluginManagerError> {
        self.registry
            .get_config_schema(id)
            .ok_or_else(|| PluginManagerError::PluginNotFound(id.to_string()))
    }

    /// 更新插件配置
    pub fn update_plugin_config(
        &mut self,
        id: &str,
        config: HashMap<String, serde_json::Value>,
    ) -> Result<(), PluginManagerError> {
        Ok(self.registry.update_config(id, config)?)
    }

    /// 关闭所有插件
    pub fn shutdown(&mut self) -> Result<(), PluginManagerError> {
        self.registry.shutdown_all();
        Ok(())
    }
}
