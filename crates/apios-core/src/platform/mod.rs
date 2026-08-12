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
    /// POSIX 归档目录（Windows 回收站无目录模型，恒不用）
    fn trash_dir(&self) -> PathBuf;

    /// 动作级：把文件移入回收站。默认实现 = POSIX 归档式（move_to_trash_dir，
    /// 归档目录/重名 -N/跨卷 copy 回退）；Windows 覆写走 SHFileOperationW。
    fn move_to_trash(
        &self,
        urls: &[PathBuf],
        bundle_name: Option<&str>,
    ) -> crate::trash::DeleteResult {
        crate::trash::move_to_trash_dir(urls, bundle_name, self.trash_dir())
    }
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

/// 单个包管理器（`pkg` 命令用；卸载包本体，区别于 dev-clean 的缓存清理）。
/// 每平台可注册多个（macOS: brew/MacPorts/nix…，Linux: apt/snap/flatpak…），
/// 故用 `dyn` 集合而非零虚表 cfg 别名 —— 各实现独立 cfg 文件，零跨平台耦合。
pub trait PackageManager {
    /// 选择器名（CLI 输入），如 "brew"
    fn name(&self) -> &str;
    /// 已安装的某类包列表（name + version + kind）
    fn list_installed(&self, kind: crate::pkg::PkgKind)
        -> Result<Vec<crate::pkg::PkgInfo>, String>;
    /// 依赖该包、且已安装的包（brew: `uses --installed`；公式与 cask 都返回）
    fn dependents(&self, name: &str, kind: crate::pkg::PkgKind) -> Result<Vec<String>, String>;
    /// 卸载单个包。ignore_deps=true → 忽略被依赖方（brew: --ignore-dependencies）；
    /// zap 仅对 cask 生效（删除用户配置，不可恢复）
    fn uninstall(
        &self,
        name: &str,
        kind: crate::pkg::PkgKind,
        zap: bool,
        ignore_deps: bool,
    ) -> Result<(), String>;
    /// autoremove 预演（dry-run），返回将卸载的包名（不执行任何删除）
    fn autoremove_dry_run(&self) -> Result<Vec<String>, String>;
    /// 真正执行 autoremove（移除仅作为依赖安装、现已无用的包）
    fn autoremove(&self) -> Result<(), String>;
}

/// 插件分类路径表（`plugins` 命令用；原版 Locations.plugins.subcategories）。
/// macOS 18 个分类全表；其他平台暂无可移植的等价目录结构，返回空。
pub trait PluginPaths {
    fn plugin_categories(&self) -> Vec<crate::plugin::PluginCategory>;
}

/// 已安装应用发现（每平台机制不同：macOS walk .app / Windows 注册表卸载项 + 开始菜单 /
/// Linux desktop 文件——TODO）。scan.rs 的 walk 逻辑保留给 macOS/Fallback 实现复用。
pub trait AppDiscovery {
    fn discover_installed_apps(&self) -> Vec<AppInfo>;
}

/// 适配器暴露本平台支持的包管理器（多包管理器入口）
pub trait PackageManagers {
    fn package_managers(&self) -> Vec<Box<dyn PackageManager>>;

    /// 按选择器名查找（大小写不敏感）
    fn package_manager(&self, name: &str) -> Option<Box<dyn PackageManager>> {
        self.package_managers()
            .into_iter()
            .find(|pm| pm.name().eq_ignore_ascii_case(name))
    }
}

#[cfg(not(any(target_os = "macos", target_os = "windows")))]
mod fallback;
#[cfg(target_os = "macos")]
mod homebrew;
#[cfg(target_os = "macos")]
pub mod lipo;
#[cfg(target_os = "macos")]
mod macos;
#[cfg(target_os = "windows")]
mod win_registry;
#[cfg(target_os = "windows")]
mod win_trash;
#[cfg(target_os = "windows")]
mod windows;
#[cfg(target_os = "windows")]
mod winget;

/// 当前平台的适配器类型（cfg 编译期选择）
#[cfg(target_os = "macos")]
pub type Adapter = macos::MacOsAdapter;
#[cfg(target_os = "windows")]
pub type Adapter = windows::WindowsAdapter;
#[cfg(not(any(target_os = "macos", target_os = "windows")))]
pub type Adapter = fallback::FallbackAdapter;

/// 当前平台的适配器实例
pub fn adapter() -> Adapter {
    Adapter::new()
}
