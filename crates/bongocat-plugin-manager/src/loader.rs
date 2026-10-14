//! 插件动态库加载器

use bongocat_plugin_sdk::Plugin;
use std::path::Path;
use thiserror::Error;

#[cfg(any(target_os = "windows", target_os = "macos"))]
use libloading::{Library, Symbol};

/// 加载器错误
#[derive(Debug, Error)]
pub enum LoaderError {
    #[error("不支持的平台")]
    UnsupportedPlatform,
    #[cfg(any(target_os = "windows", target_os = "macos"))]
    #[error("动态库加载失败: {0}")]
    LibraryLoadFailed(#[from] libloading::Error),
    #[error("找不到插件入口点")]
    EntryPointNotFound,
}

/// 插件加载器
pub struct PluginLoader {
    #[cfg(any(target_os = "windows", target_os = "macos"))]
    libraries: Vec<Library>,
}

impl PluginLoader {
    /// 创建新的加载器
    pub fn new() -> Self {
        Self {
            #[cfg(any(target_os = "windows", target_os = "macos"))]
            libraries: Vec::new(),
        }
    }

    /// 加载插件
    ///
    /// # Safety
    ///
    /// 调用方必须确保：
    /// 1. 插件动态库来自可信源
    /// 2. 插件实现了正确的 ABI
    /// 3. _plugin_create 返回的指针指向有效的 Plugin trait object
    pub fn load(&mut self, path: &Path) -> Result<Box<dyn Plugin>, LoaderError> {
        #[cfg(any(target_os = "windows", target_os = "macos"))]
        {
            // SAFETY: 加载插件是 FFI 操作，需要 unsafe。
            // 要求插件必须导出符合 ABI 的 _plugin_create 函数。
            unsafe {
                let lib = Library::new(path)?;

                // 获取插件创建函数
                let constructor: Symbol<extern "C" fn() -> *mut dyn Plugin> =
                    lib.get(b"_plugin_create\0")?;

                // 调用构造函数
                let raw_ptr = constructor();

                if raw_ptr.is_null() {
                    return Err(LoaderError::EntryPointNotFound);
                }

                // 从原始指针重建 Box
                let plugin = Box::from_raw(raw_ptr);

                // 保持动态库加载状态，避免卸载
                self.libraries.push(lib);

                Ok(plugin)
            }
        }

        #[cfg(not(any(target_os = "windows", target_os = "macos")))]
        {
            let _ = path;
            Err(LoaderError::UnsupportedPlatform)
        }
    }
}

impl Default for PluginLoader {
    fn default() -> Self {
        Self::new()
    }
}
