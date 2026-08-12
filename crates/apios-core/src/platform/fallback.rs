//! 非 macOS 基础适配器 —— XDG 目录约定的最小实现
//!
//! 目标：保证非 macOS 平台可编译、有合理默认行为（不谎报能力）。
//! TODO（平台专业适配，每平台一个发行版）：
//! - Linux: desktop 文件元数据（.desktop）、XDG trash 规范（info 文件 + 同名冲突处理）
//! - Windows: 卸载注册表 / 开始菜单 / 回收站 API
//! - 各平台应用目录布局的细化（flatpak / snap / brew 前缀等）

use std::path::{Path, PathBuf};

use super::{
    AppDiscovery, AppMetadata, DevEnvPaths, PackageManager, PackageManagers, PluginPaths,
    ProcessControl, SpotlightIndex, SystemPaths, Trash,
};
use crate::dev_env::DevEnv;
use crate::model::{AppInfo, Sensitivity};
use crate::plugin::PluginCategory;

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
        let cache_dir =
            std::env::var("XDG_CACHE_HOME").unwrap_or_else(|_| format!("{home}/.cache"));
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
    /// XDG trash 目录（XDG 规范允许 $XDG_DATA_HOME/Trash 覆盖，缺省 ~/.local/share/Trash）
    fn trash_dir(&self) -> PathBuf {
        let data_home = std::env::var("XDG_DATA_HOME")
            .unwrap_or_else(|_| format!("{}/.local/share", self.home));
        PathBuf::from(data_home).join("Trash")
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

impl ProcessControl for FallbackAdapter {
    /// 尚未实现：Linux pkill（注意 15 字符进程名截断）、Windows taskkill —— 平台适配 TODO
    fn kill_running_app(&self, _app: &AppInfo) -> u32 {
        0
    }
}

/// 开发环境路径表（Linux XDG 布局子集，收紧原则同 macOS：只列可再生缓存）。
/// TODO（Linux 适配器阶段细化）：Android Studio（~/.android、~/.cache/Google）、
/// VS Code 完整缓存集（~/.config/Code/*）、flatpak/snap 变体、Windows（%LOCALAPPDATA% 等）。
fn dev_envs_table() -> Vec<DevEnv> {
    let p = |s: &str| s.to_string();
    vec![
        DevEnv {
            name: "Cargo".into(),
            paths: vec![p("~/.cargo/git/"), p("~/.cargo/registry/")],
        },
        DevEnv {
            name: "Conda".into(),
            paths: vec![p("~/.conda/pkgs/")], // 包下载缓存（可再生）；环境本体不列
        },
        DevEnv {
            name: "Deno".into(),
            paths: vec![p("~/.cache/deno")],
        },
        DevEnv {
            name: "Go Modules".into(),
            paths: vec![p("~/go/pkg/mod/")],
        },
        DevEnv {
            name: "Gradle".into(),
            paths: vec![p("~/.gradle/caches/"), p("~/.gradle/wrapper/")],
        },
        DevEnv {
            name: "Haskell Stack".into(),
            paths: vec![p("~/.stack/snapshots/")],
        },
        DevEnv {
            name: "Maven".into(),
            paths: vec![p("~/.m2/repository/")],
        },
        DevEnv {
            name: "Nix".into(),
            paths: vec![p("~/.cache/nix/")],
        },
        DevEnv {
            name: "Npm".into(),
            paths: vec![
                p("~/.npm/"),
                p("~/.cache/pnpm/store"),
                p("~/.bun/install/cache"),
            ],
        },
        DevEnv {
            name: "Pip".into(),
            paths: vec![p("~/.cache/pip/")],
        },
        DevEnv {
            name: "Poetry".into(),
            paths: vec![p("~/.cache/pypoetry/")],
        },
        DevEnv {
            name: "Pub".into(),
            paths: vec![p("~/.pub-cache/"), p("~/.cache/flutter_engine/")],
        },
        DevEnv {
            name: "Pyenv".into(),
            paths: vec![p("~/.pyenv/cache/")],
        },
        DevEnv {
            name: "Swift".into(),
            paths: vec![p("~/.swiftpm/")],
        },
        DevEnv {
            name: "Uv".into(),
            paths: vec![p("~/.cache/uv/")],
        },
        DevEnv {
            name: "VS Code".into(),
            paths: vec![
                p("~/.config/Code/Cache"),
                p("~/.config/Code/CachedData"),
                p("~/.config/Code/CachedExtensionVSIXs"),
                p("~/.config/Code/Code Cache"),
            ],
        },
        DevEnv {
            name: "Yarn".into(),
            paths: vec![p("~/.cache/yarn/"), p("~/.yarn-cache/")],
        },
        DevEnv {
            name: "Zed".into(),
            paths: vec![p("~/.cache/zed/"), p("~/.local/share/zed/node/cache/")],
        },
    ]
}

impl DevEnvPaths for FallbackAdapter {
    fn dev_envs(&self) -> Vec<DevEnv> {
        dev_envs_table()
    }
}

impl PluginPaths for FallbackAdapter {
    fn plugin_categories(&self) -> Vec<PluginCategory> {
        vec![] // 其他平台暂无 macOS 式插件目录体系，返回空
    }
}

impl AppDiscovery for FallbackAdapter {
    /// 委托 scan.rs 的 .app 目录 walk（Linux XDG 目录，通常为空；
    /// desktop 文件解析属平台专业适配 TODO）
    fn discover_installed_apps(&self) -> Vec<AppInfo> {
        crate::scan::get_sorted_apps(&crate::scan::default_app_folders(&self.home))
    }
}

/// 尚无包管理器实现（TODO: Linux apt/dnf/pacman、Windows winget/choco —— 平台专业适配）
impl PackageManagers for FallbackAdapter {
    fn package_managers(&self) -> Vec<Box<dyn PackageManager>> {
        Vec::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_dev_envs_cross_platform_paths() {
        // Linux 子集不得含 macOS 布局（~/Library），且不得列工具本体
        let envs = dev_envs_table();
        let all: Vec<&str> = envs
            .iter()
            .flat_map(|e| e.paths.iter().map(|p| p.as_str()))
            .collect();
        assert!(
            all.iter().all(|p| !p.contains("Library/")),
            "Linux 路径表不应含 macOS ~/Library 布局: {:?}",
            all.iter().find(|p| p.contains("Library/"))
        );
        // 完全不出现的路径（前缀禁止）
        for forbidden in ["anaconda3/", "miniconda3/", "~/.gem/"] {
            assert!(
                !all.iter().any(|p| p.starts_with(forbidden)),
                "路径表不应包含路径 {forbidden}"
            );
        }
        // 工具本体根条目不得出现（其下缓存子路径合法）
        for root in ["~/.cargo/", "~/.nvm/", "~/.pyenv/"] {
            assert!(
                !all.iter().any(|p| p == &root),
                "路径表不应包含工具本体根目录 {root}"
            );
        }
        assert!(envs.iter().any(|e| e.name == "Conda")); // Linux 有安全缓存条目（pkgs）
    }
}
