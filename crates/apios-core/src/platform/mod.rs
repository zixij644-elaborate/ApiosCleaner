//! 平台适配层 —— 统一接口 + 条件编译的平台实现
//!
//! 架构原则（README "Platform adapters"）：
//! - 核心引擎（matcher/search/orphan/format/scan）只依赖本模块的 trait，零 OS API
//! - 每个平台一个实现，用 `cfg(target_os)` 在编译期选择（`Adapter` 类型别名，零虚表开销）
//! - 各平台实现可自由使用原生 API 做最优实现（macOS: codesign；Linux: desktop 文件；Windows: 注册表/卸载项）
//!
//! 当前实现：
//! - macOS: codesign 元数据、darwin 缓存目录、~/Library 布局、~/.Trash
//! - 其他平台: fallback 基础版（XDG 目录约定），待各平台专业适配

use std::path::{Path, PathBuf};

use crate::model::{AppInfo, Sensitivity};

/// 全盘索引补充查询（原版 spotlightSupplementalPaths，AppPathsFetch.swift:490-614）
///
/// 路径表逐目录扫描有盲区（路径表外的深层残留），macOS 用 Spotlight 索引（mdfind）
/// 做补充；其他平台暂无等价索引服务，返回空。
pub trait SpotlightIndex {
    fn spotlight_supplemental_paths(
        &self,
        app_name: &str,
        bundle_id: &str,
        sensitivity: Sensitivity,
    ) -> Vec<PathBuf>;
}

/// 系统目录布局：应用搜索路径与用户目录
pub trait SystemPaths {
    fn home(&self) -> String;
    fn user_cache_dir(&self) -> String;
    fn user_temp_dir(&self) -> String;
    /// 应用相关文件搜索路径（原 LocationManager.appsPaths）
    fn apps_paths(&self) -> Vec<String>;
    /// 反向（孤儿）搜索路径（原 LocationManager.reversePaths）
    fn reverse_paths(&self) -> Vec<String>;
    /// 应用支持目录下的子目录列表（深度搜索）
    fn app_support_subdirs(&self) -> Vec<String>;
}

/// 应用元数据提取（每平台机制不同：macOS codesign / Linux desktop 文件 / Windows 注册表）
pub trait AppMetadata {
    /// entitlements（application-groups / iCloud 容器标识；其他平台可无此概念）
    fn entitlements(&self, app_path: &Path) -> Option<Vec<String>>;
    /// 团队标识符（macOS codesign 概念；其他平台返回 None）
    fn team_identifier(&self, app_path: &Path) -> Option<String>;
}

/// 回收站语义（macOS ~/.Trash / Linux XDG trash / Windows 回收站）
pub trait Trash {
    fn trash_dir(&self) -> PathBuf;
}

/// 卸载前终止运行中的应用（原版 GUI 的 killApp；每平台机制不同：
/// macOS pgrep/killall、Linux pkill、Windows taskkill）
pub trait ProcessControl {
    /// 终止应用进程，返回被终止的进程数（0 = 无运行实例）
    fn kill_running_app(&self, app: &AppInfo) -> u32;
}

/// 开发环境路径表（`dev-clean` 用；每平台目录布局差异大：
/// macOS ~/Library 布局 vs Linux ~/.cache / ~/.config 变体）
pub trait DevEnvPaths {
    /// 可清理的开发环境路径（收紧原则：只列可再生缓存，不列工具本体/配置/用户数据）
    fn dev_envs(&self) -> Vec<crate::dev_env::DevEnv>;
}

#[cfg(not(target_os = "macos"))]
mod fallback;
#[cfg(target_os = "macos")]
mod macos;

/// 当前平台的适配器类型（cfg 编译期选择）
#[cfg(target_os = "macos")]
pub type Adapter = macos::MacOsAdapter;
#[cfg(not(target_os = "macos"))]
pub type Adapter = fallback::FallbackAdapter;

/// 当前平台的适配器实例
pub fn adapter() -> Adapter {
    Adapter::new()
}
