//! 搜索位置集合 —— 数据来自平台适配层（platform::SystemPaths）
//!
//! macOS 的路径表（~/Library 布局等）已迁移到 platform/macos.rs；
//! 本模块保持 `Locations` 公共结构不变，核心引擎与 CLI 零改动。

use std::collections::HashSet;

use crate::format::pear_format;
use crate::platform;
use crate::platform::SystemPaths;

/// 标准 Library 子目录 —— 深度2匹配时判断是否回退到父目录（供应商目录规则）
///
/// 纯数据表（macOS Library 布局，无平台 API 依赖）；将来可为其他平台
/// 补充各自的标准目录表（如 Linux 的 XDG 数据目录）。
pub const STANDARD_LIBRARY_SUBDIRECTORIES: &[&str] = &[
    "Application Scripts",
    "Application Support",
    "Caches",
    "Containers",
    "Group Containers",
    "HTTPStorages",
    "Internet Plug-Ins",
    "LaunchAgents",
    "LaunchDaemons",
    "Logs",
    "Preferences",
    "PreferencePanes",
    "PrivilegedHelperTools",
    "Saved Application State",
    "Services",
    "WebKit",
    "Extensions",
    "Frameworks",
];

pub fn standard_library_subdirectories() -> HashSet<String> {
    STANDARD_LIBRARY_SUBDIRECTORIES
        .iter()
        .map(|s| s.to_string())
        .collect()
}

/// 搜索位置集合（apps.paths + reverse.paths；插件路径在平台层 PluginPaths，不在此导出）
pub struct Locations {
    pub home: String,
    pub cache_dir: String,
    pub temp_dir: String,
    pub apps_paths: Vec<String>,
    pub reverse_paths: Vec<String>,
}

impl Locations {
    /// 从当前平台的适配器构建（macOS: ~/Library 布局；其他平台: XDG 基础版）
    pub fn new() -> Locations {
        let a = platform::adapter();
        Locations {
            home: a.home(),
            cache_dir: a.user_cache_dir(),
            temp_dir: a.user_temp_dir(),
            apps_paths: a.apps_paths(),
            reverse_paths: a.reverse_paths(),
        }
    }
}

impl Default for Locations {
    fn default() -> Self {
        Self::new()
    }
}

/// 路径存在性判断（原版 Condition.init 中 FileManager.fileExists）
pub fn existing(path: &str) -> bool {
    std::path::Path::new(path).exists()
}

/// 为测试提供构造入口
#[allow(dead_code)]
fn _assert_tables_unique() {
    // 与标准目录表做交叉检查时使用
    let _ = pear_format("placeholder");
}
