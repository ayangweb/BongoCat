//! BongoCat 插件 SDK
//!
//! 为插件开发提供核心类型、trait 和工具函数。

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// 插件元数据
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PluginMetadata {
    /// 插件唯一标识符，使用反向域名格式（例如 com.example.my-plugin）
    pub id: String,
    /// 插件名称
    pub name: String,
    /// 插件简短描述
    pub description: String,
    /// 插件版本
    pub version: String,
    /// 插件作者
    pub author: String,
    /// 插件图标（Emoji）
    pub icon: String,
    /// 支持的 BongoCat 最小版本
    pub min_app_version: String,
    /// 插件主页 URL（可选）
    pub homepage: Option<String>,
    /// 插件仓库 URL（可选）
    pub repository: Option<String>,
}

/// 插件配置定义
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PluginConfigSchema {
    /// 配置项定义
    pub fields: Vec<ConfigField>,
}

/// 配置字段定义
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConfigField {
    /// 字段键名
    pub key: String,
    /// 字段显示名称
    pub label: String,
    /// 字段类型
    pub field_type: ConfigFieldType,
    /// 默认值（JSON 格式）
    pub default_value: serde_json::Value,
    /// 字段描述（可选）
    pub description: Option<String>,
}

/// 配置字段类型
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ConfigFieldType {
    /// 布尔值
    Boolean,
    /// 字符串
    String,
    /// 整数
    Integer { min: Option<i64>, max: Option<i64> },
    /// 浮点数
    Float { min: Option<f64>, max: Option<f64> },
    /// 枚举选择
    Enum { options: Vec<String> },
}

/// 输入事件类型
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum InputEvent {
    /// 键盘按下
    KeyDown { key: String },
    /// 键盘释放
    KeyUp { key: String },
    /// 鼠标按钮按下
    MouseButtonDown { button: MouseButton },
    /// 鼠标按钮释放
    MouseButtonUp { button: MouseButton },
    /// 鼠标移动
    MouseMove { delta_x: f32, delta_y: f32 },
    /// 鼠标滚轮
    MouseWheel { delta: f32 },
}

/// 鼠标按钮
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MouseButton {
    Left,
    Right,
    Middle,
    Other(u8),
}

/// 插件渲染位置
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RenderPosition {
    /// 模型窗口左上角
    TopLeft,
    /// 模型窗口右上角
    TopRight,
    /// 模型窗口左下角
    BottomLeft,
    /// 模型窗口右下角
    BottomRight,
    /// 模型窗口中心
    Center,
}

/// 插件渲染内容
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum RenderContent {
    /// 文本内容
    Text {
        content: String,
        font_size: f32,
        color: (u8, u8, u8, u8),
    },
    /// 自定义 UI（暂时保留，未来扩展）
    Custom { data: serde_json::Value },
}

/// 插件到主程序的消息
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum PluginMessage {
    /// 请求播放音频文件
    PlayAudio { file_path: String },
    /// 请求更新渲染内容
    UpdateRender {
        position: RenderPosition,
        content: Option<RenderContent>,
    },
    /// 请求显示通知
    ShowNotification { title: String, message: String },
    /// 请求保存持久化数据
    SaveData { key: String, value: String },
    /// 请求读取持久化数据
    LoadData { key: String },
}

/// 主程序到插件的消息
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum HostMessage {
    /// 输入事件
    InputEvent { event: InputEvent },
    /// 配置变更
    ConfigChanged {
        config: HashMap<String, serde_json::Value>,
    },
    /// 数据加载响应
    DataLoaded { key: String, value: Option<String> },
}

/// 插件错误
#[derive(Debug, thiserror::Error)]
pub enum PluginError {
    #[error("初始化失败: {0}")]
    InitializationFailed(String),
    #[error("配置无效: {0}")]
    InvalidConfig(String),
    #[error("运行时错误: {0}")]
    RuntimeError(String),
}

/// 插件生命周期 trait
pub trait Plugin: Send + Sync {
    /// 获取插件元数据
    fn metadata(&self) -> PluginMetadata;

    /// 获取配置 schema（可选）
    fn config_schema(&self) -> Option<PluginConfigSchema> {
        None
    }

    /// 初始化插件
    fn initialize(&mut self, config: HashMap<String, serde_json::Value>)
    -> Result<(), PluginError>;

    /// 处理来自主程序的消息
    fn handle_message(&mut self, message: HostMessage) -> Result<Vec<PluginMessage>, PluginError>;

    /// 关闭插件
    fn shutdown(&mut self) -> Result<(), PluginError>;
}

/// 插件构造器类型
pub type PluginConstructor = fn() -> Box<dyn Plugin>;

/// 声明插件入口点的宏
#[macro_export]
macro_rules! declare_plugin {
    ($plugin_type:ty, $constructor:path) => {
        #[unsafe(no_mangle)]
        pub extern "C" fn _plugin_create() -> *mut ::std::ffi::c_void {
            let constructor: $crate::PluginConstructor = $constructor;
            let boxed: Box<dyn $crate::Plugin> = constructor();
            Box::into_raw(boxed) as *mut ::std::ffi::c_void
        }

        #[unsafe(no_mangle)]
        pub extern "C" fn _plugin_destroy(ptr: *mut ::std::ffi::c_void) {
            if !ptr.is_null() {
                unsafe {
                    drop(Box::from_raw(ptr as *mut $plugin_type));
                }
            }
        }
    };
}
