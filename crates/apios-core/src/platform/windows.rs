//! Windows 适配器 —— 注册表/开始菜单发现、%AppData% 路径表、回收站 API、taskkill、winget
//!
//! 结构（沿用 macOS 侧扁平文件风格）：
//! - windows.rs 本体：SystemPaths / AppMetadata / PluginPaths / SpotlightIndex / Trash /
//!   ProcessControl / DevEnvPaths / PackageManagers / AppDiscovery 的 Windows 实现
//! - win_registry.rs：注册表卸载项枚举（Reg*W FFI + 纯解析）
//! - win_trash.rs：SHFileOperationW FFI（FO_DELETE + FOF_ALLOWUNDO → 回收站）
//! - winget.rs：winget 包管理器包装
//!
//! Windows 平台特性：
//! - 应用发现 = 注册表卸载项 + 开始菜单 .lnk（无 macOS 式 .app bundle 与 Info.plist）
//! - 相关文件聚集地 = %APPDATA% / %LOCALAPPDATA% / %PROGRAMDATA% 下的供应商目录
//! - 回收站 = 系统 API（逐盘符，无目录模型），delete_files 的 POSIX 归档语义不适用

use std::path::{Path, PathBuf};

use super::{
    AppDiscovery, AppMetadata, DevEnvPaths, PackageManagers, PluginPaths, ProcessControl,
    SpotlightIndex, SystemPaths, Trash,
};
use crate::dev_env::DevEnv;
use crate::model::{AppInfo, Sensitivity};
use crate::plugin::PluginCategory;

/// 环境变量 → 值（缺失回退默认）。`%VAR%` 类变量名直接传原名。
fn env(name: &str, fallback: &str) -> String {
    std::env::var(name).unwrap_or_else(|_| fallback.to_string())
}

/// Windows 适配器（cfg(windows) 编译期选择；home=USERPROFILE）
pub struct WindowsAdapter {
    home: String,
    appdata_roaming: String,
    appdata_local: String,
    programdata: String,
    program_files: String,
    program_files_x86: String,
    temp_dir: String,
}

impl WindowsAdapter {
    pub fn new() -> Self {
        // USERPROFILE 缺失时回退 HOMEDRIVE+HOMEPATH（如 "C:" + "\Users\x"）
        let home = env("USERPROFILE", &{
            let d = env("HOMEDRIVE", "C:");
            let p = env("HOMEPATH", "\\Users");
            format!("{d}{p}")
        });
        WindowsAdapter {
            appdata_roaming: env("APPDATA", &format!("{home}\\AppData\\Roaming")),
            appdata_local: env("LOCALAPPDATA", &format!("{home}\\AppData\\Local")),
            programdata: env("ProgramData", "C:\\ProgramData"),
            program_files: env("ProgramFiles", "C:\\Program Files"),
            program_files_x86: env("ProgramFiles(x86)", "C:\\Program Files (x86)"),
            temp_dir: env("TEMP", &format!("{home}\\AppData\\Local\\Temp")),
            home,
        }
    }
}

impl SystemPaths for WindowsAdapter {
    fn home(&self) -> String {
        self.home.clone()
    }

    fn user_cache_dir(&self) -> String {
        // Windows 缓存均位于 %LOCALAPPDATA%（对齐 macOS cache 语义）
        self.appdata_local.clone()
    }

    fn user_temp_dir(&self) -> String {
        self.temp_dir.clone()
    }

    /// 相关文件搜索路径表：用户目录 + AppData 双根 + 开始菜单 + 常见安装目录。
    /// 条目过滤不存在项（目录没装就不扫，省时；与 macOS 全存在不同）。
    fn apps_paths(&self) -> Vec<String> {
        let mut out = vec![
            self.home.clone(),
            format!("{}\\Desktop", self.home),
            self.appdata_roaming.clone(),
            self.appdata_local.clone(),
            format!(
                "{}\\Microsoft\\Windows\\Start Menu\\Programs",
                self.appdata_roaming
            ),
            format!(
                "{}\\Microsoft\\Windows\\Start Menu\\Programs",
                self.programdata
            ),
            format!("{}\\Programs", self.appdata_local), // per-user 安装（VS Code/Discord 等）
            self.program_files.clone(),
            self.program_files_x86.clone(),
        ];
        out.retain(|p| Path::new(p).is_dir());
        out
    }

    /// 孤儿反向搜索路径：AppData 双根 + 文档 + ProgramData
    fn reverse_paths(&self) -> Vec<String> {
        vec![
            self.appdata_roaming.clone(),
            self.appdata_local.clone(),
            format!("{}\\Documents", self.home),
            self.programdata.clone(),
        ]
    }

    /// 供应商目录表（%APPDATA% 一级子目录，排除系统目录 —— 对齐 macOS 排除 com.apple 精神）
    fn app_support_subdirs(&self) -> Vec<String> {
        let Ok(entries) = std::fs::read_dir(&self.appdata_roaming) else {
            return Vec::new();
        };
        let mut out = Vec::new();
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().into_owned();
            if entry.path().is_dir() && !name.to_ascii_lowercase().contains("microsoft") {
                out.push(name);
            }
        }
        out
    }
}

impl AppMetadata for WindowsAdapter {
    /// 无 codesign 概念；应用元数据来自注册表卸载项（AppDiscovery 里读）
    fn entitlements(&self, _app_path: &Path) -> Option<Vec<String>> {
        None
    }

    fn team_identifier(&self, _app_path: &Path) -> Option<String> {
        None
    }
}

impl SpotlightIndex for WindowsAdapter {
    /// Windows Search 索引接入留后续；暂无等价补充查询
    fn spotlight_supplemental_paths(
        &self,
        _app_name: &str,
        _bundle_id: &str,
        _sensitivity: Sensitivity,
    ) -> Vec<PathBuf> {
        Vec::new()
    }
}

impl PluginPaths for WindowsAdapter {
    /// Windows 无 macOS 式插件目录体系，返回空
    fn plugin_categories(&self) -> Vec<PluginCategory> {
        Vec::new()
    }
}

impl Trash for WindowsAdapter {
    /// Windows 回收站无目录模型；此值为占位（M4 起删除走 move_to_trash 的 SHFileOperationW）
    fn trash_dir(&self) -> PathBuf {
        PathBuf::from(self.temp_dir.clone()).join("apios-trash")
    }
}

impl ProcessControl for WindowsAdapter {
    /// taskkill 实现（M4 落地）；当前返回 0
    fn kill_running_app(&self, _app: &AppInfo) -> u32 {
        0
    }
}

impl DevEnvPaths for WindowsAdapter {
    /// Windows 开发环境缓存表（M5 落地）；当前返回空
    fn dev_envs(&self) -> Vec<DevEnv> {
        Vec::new()
    }
}

impl PackageManagers for WindowsAdapter {
    /// winget（M5 落地）；当前返回空
    fn package_managers(&self) -> Vec<Box<dyn super::PackageManager>> {
        Vec::new()
    }
}

impl AppDiscovery for WindowsAdapter {
    /// 注册表卸载项（主数据源）+ 开始菜单 .lnk（补充便携应用）
    ///
    /// AppInfo 语义：bundle_identifier 恒空 → identifiers 的 use_bundle_identifier
    /// 门控自动关闭 bundle-id 匹配族，app_name/path needle 继续生效（无空值崩溃）。
    /// path 优先级：DisplayIcon 文件（去 ",0" 后缀）> InstallLocation 目录；
    /// 两者都不存在（应用已卸载，仅剩残留）→ 不进入已安装列表 —— 残留由
    /// orphan 反向搜索判定为孤儿，这正是清理目标。
    fn discover_installed_apps(&self) -> Vec<AppInfo> {
        let mut seen: std::collections::HashSet<String> = std::collections::HashSet::new();
        let mut out: Vec<AppInfo> = Vec::new();

        // 1) 注册表卸载项
        for e in super::win_registry::all_uninstall_entries() {
            let icon = e
                .display_icon
                .as_deref()
                .and_then(|s| s.split(',').next()) // "C:\...\foo.exe,0" → 文件路径
                .map(PathBuf::from)
                .filter(|p| p.is_file());
            let path = icon.or_else(|| {
                e.install_location
                    .as_ref()
                    .map(PathBuf::from)
                    .filter(|p| p.is_dir())
            });
            let Some(path) = path else { continue };
            let key = path.to_string_lossy().to_lowercase();
            if seen.insert(key) {
                out.push(minimal_app_info(path, e.display_name));
            }
        }

        // 2) 开始菜单 .lnk（注册表覆盖不到的便携应用；.lnk 本身是真实文件可匹配）
        for root in [self.start_menu_user(), self.start_menu_common()] {
            for lnk in walk_lnk(Path::new(&root)) {
                let name = lnk
                    .file_stem()
                    .map(|s| s.to_string_lossy().into_owned())
                    .unwrap_or_default();
                if name.is_empty() {
                    continue;
                }
                let key = lnk.to_string_lossy().to_lowercase();
                if seen.insert(key) {
                    out.push(minimal_app_info(lnk, name));
                }
            }
        }
        out
    }
}

impl WindowsAdapter {
    fn start_menu_user(&self) -> String {
        format!(
            "{}\\Microsoft\\Windows\\Start Menu\\Programs",
            self.appdata_roaming
        )
    }

    fn start_menu_common(&self) -> String {
        format!(
            "{}\\Microsoft\\Windows\\Start Menu\\Programs",
            self.programdata
        )
    }
}

/// 最小 AppInfo（Windows 无 bundle id / codesign 概念，字段置空）
fn minimal_app_info(path: PathBuf, app_name: String) -> AppInfo {
    AppInfo {
        path,
        bundle_identifier: String::new(),
        app_name,
        entitlements: Vec::new(),
        team_identifier: None,
        web_app: false,
        steam: false,
        wrapped: false,
    }
}

/// 递归找 *.lnk（WalkDir；目录不存在 → 空）
fn walk_lnk(dir: &Path) -> Vec<PathBuf> {
    use walkdir::WalkDir;
    let mut out = Vec::new();
    for entry in WalkDir::new(dir)
        .max_depth(6)
        .into_iter()
        .filter_map(|e| e.ok())
    {
        let is_lnk = entry
            .path()
            .extension()
            .map(|e| e.to_string_lossy().to_ascii_lowercase() == "lnk")
            .unwrap_or(false);
        if entry.file_type().is_file() && is_lnk {
            out.push(entry.into_path());
        }
    }
    out
}
