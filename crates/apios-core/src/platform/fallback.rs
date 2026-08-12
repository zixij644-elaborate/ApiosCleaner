//! 非 macOS 基础适配器 —— XDG 目录约定的最小实现
//!
//! 目标：保证非 macOS 平台可编译、有合理默认行为（不谎报能力）。
//! TODO（平台专业适配，每平台一个发行版）：
//! - Linux: desktop 文件元数据（.desktop）、XDG trash 规范（info 文件 + 同名冲突处理）
//! - Windows: 卸载注册表 / 开始菜单 / 回收站 API
//! - 各平台应用目录布局的细化（flatpak / snap / brew 前缀等）

use std::path::{Path, PathBuf};

use super::{AppMetadata, SpotlightIndex, SystemPaths, Trash};
use crate::model::Sensitivity;

/// 非 macOS 平台的基础适配器（当前按 Linux XDG 约定提供默认值）
pub struct FallbackAdapter {
    home: String,
    cache_dir: String,
    temp_dir: String,
}

impl FallbackAdapter {
    pub fn new() -> Self {
        let home = std::env::var("HOME").unwrap_or_default();
        // XDG Base Directory 规范
        let cache_dir = std::env::var("XDG_CACHE_HOME")
            .unwrap_or_else(|_| format!("{home}/.cache"));
        let temp_dir = std::env::temp_dir().to_string_lossy().to_string();
        FallbackAdapter {
            home,
            cache_dir,
            temp_dir,
        }
    }
}

impl SystemPaths for FallbackAdapter {
    fn home(&self) -> String {
        self.home.clone()
    }

    fn user_cache_dir(&self) -> String {
        self.cache_dir.clone()
    }

    fn user_temp_dir(&self) -> String {
        self.temp_dir.clone()
    }

    /// 常见 Linux 应用入口目录（TODO: flatpak/snap/brew 前缀细化）
    fn apps_paths(&self) -> Vec<String> {
        let home = &self.home;
        vec![
            format!("{home}"),
            format!("{home}/.config"),
            format!("{home}/Applications"),
            format!("{home}/.local/share/applications"),
            format!("{home}/.local/share/flatpak/exports/share/applications"),
            format!("{home}/.var/app"),
            "/usr/local/share/applications".to_string(),
            "/usr/share/applications".to_string(),
            "/var/lib/flatpak/exports/share/applications".to_string(),
        ]
    }

    fn reverse_paths(&self) -> Vec<String> {
        let home = &self.home;
        vec![
            format!("{home}/.config"),
            format!("{home}/.local/share"),
            format!("{home}/.local/state"),
            format!("{home}/.cache"),
        ]
    }

    /// 未细化：平台适配 TODO（Linux 可扫 ~/.local/share/ 下的供应商目录）
    fn app_support_subdirs(&self) -> Vec<String> {
        Vec::new()
    }
}

impl AppMetadata for FallbackAdapter {
    /// 无 codesign 概念；TODO: Linux 解析 .desktop 文件的 Exec/Icon，Windows 读注册表卸载项
    fn entitlements(&self, _app_path: &Path) -> Option<Vec<String>> {
        None
    }

    fn team_identifier(&self, _app_path: &Path) -> Option<String> {
        None
    }
}

impl Trash for FallbackAdapter {
    /// XDG trash 目录（规范中 home trash 固定位于 ~/.local/share/Trash）
    fn trash_dir(&self) -> PathBuf {
        PathBuf::from(format!("{}/.local/share/Trash", self.home))
    }
}

impl SpotlightIndex for FallbackAdapter {
    /// 无等价索引服务（TODO: Linux 可考虑 baloo/recoll 等，属平台专业适配）
    fn spotlight_supplemental_paths(
        &self,
        _app_name: &str,
        _bundle_id: &str,
        _sensitivity: Sensitivity,
    ) -> Vec<PathBuf> {
        Vec::new()
    }
}
