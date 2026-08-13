//! Linux 适配器 —— XDG 目录约定 + XDG trash 规范 + .desktop 应用发现 + apt 包管理
//!
//! 非 macOS/Windows 平台的正式适配器（取代原 fallback 定位）。
//! 借鉴 BleachBit 的 Linux 清理思路（仅思想层）：XDG trash 的多位置布局、
//! 包管理器外部命令 + 文本解析模式。
//!
//! 现状：
//! - SystemPaths：XDG Base Directory 约定
//! - Trash：XDG trash 规范（files/ + info/ + trashinfo）；跨挂载点 `.Trash-$uid`
//!   留 TODO（真机验证阶段补）
//! - AppDiscovery：.desktop 文件解析（desktop.rs）
//! - ProcessControl：pgrep -f + kill -TERM
//! - DevEnvPaths：18 通用环境 + 包管理器缓存（APT/DNF/pacman/Snapd，root 项走
//!   check_protected 的 sudo 提示）
//! - PackageManagers：apt（apt.rs）

use std::path::{Path, PathBuf};

use chrono::Local;

use super::{
    AppDiscovery, AppMetadata, DevEnvPaths, PluginPaths, ProcessControl, SpotlightIndex,
    SystemPaths, Trash,
};
use crate::cmd_util;
use crate::desktop;
use crate::dev_env::DevEnv;
use crate::model::{AppInfo, Sensitivity};
use crate::plugin::PluginCategory;
use crate::trash::xdg;
use crate::trash::{validate_path, DeleteResult, FilePair};

pub struct LinuxAdapter {
    home: String,
    cache_dir: String,
    temp_dir: String,
}

impl LinuxAdapter {
    pub fn new() -> Self {
        let home = crate::platform::normalize_home(&std::env::var("HOME").unwrap_or_default());
        // XDG Base Directory 规范
        let cache_dir =
            std::env::var("XDG_CACHE_HOME").unwrap_or_else(|_| format!("{home}/.cache"));
        let temp_dir = std::env::temp_dir().to_string_lossy().to_string();
        LinuxAdapter {
            home,
            cache_dir,
            temp_dir,
        }
    }
}

impl SystemPaths for LinuxAdapter {
    fn home(&self) -> String {
        self.home.clone()
    }

    fn user_cache_dir(&self) -> String {
        self.cache_dir.clone()
    }

    fn user_temp_dir(&self) -> String {
        self.temp_dir.clone()
    }

    /// 常见 Linux 应用入口目录（.desktop 文件位置；flatpak/snap 前缀已含）
    fn apps_paths(&self) -> Vec<String> {
        let home = &self.home;
        vec![
            format!("{home}/.config"),
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

    /// Linux 系统保护区（对齐 macOS 精神；~/.local/share 等用户目录根不整体保护，
    /// 只有 {home}/Applications 由 validate_path 的 POSIX 分支额外拦截）
    fn critical_paths(&self) -> Vec<String> {
        [
            "/bin", "/boot", "/dev", "/etc", "/lib", "/opt", "/proc", "/root", "/run", "/sbin",
            "/snap", "/srv", "/sys", "/usr", "/var",
        ]
        .into_iter()
        .map(String::from)
        .collect()
    }
}

impl AppMetadata for LinuxAdapter {
    /// 无 codesign 概念（.desktop 的 Exec/Icon 由 AppDiscovery 解析）
    fn entitlements(&self, _app_path: &Path) -> Option<Vec<String>> {
        None
    }

    fn team_identifier(&self, _app_path: &Path) -> Option<String> {
        None
    }
}

impl Trash for LinuxAdapter {
    /// XDG 主 trash 目录（$XDG_DATA_HOME/Trash，缺省 ~/.local/share/Trash）。
    /// 跨挂载点（各挂载点 .Trash-$uid）留 TODO：文件与主 trash 跨卷时 rename
    /// 会 EXDEV 失败进 failed 列表，真机验证阶段补。
    fn trash_dir(&self) -> PathBuf {
        let data_home = std::env::var("XDG_DATA_HOME")
            .unwrap_or_else(|_| format!("{}/.local/share", self.home));
        PathBuf::from(data_home).join("Trash")
    }

    /// XDG trash 规范删除：逐文件 rename 到 `files/<name>` + 写 `info/<name>.trashinfo`
    /// （percent-encoded 原始路径 + 本地时间 DeletionDate）。同名冲突按规范追加
    /// `.1`/`.2` 序号后缀（files 与 info 同步）。
    /// `bundle_name` 无对应概念（XDG 无归档目录），忽略。
    /// info 写失败 → 回滚文件到原位（否则成为无法恢复的孤儿条目）。
    fn move_to_trash(&self, urls: &[PathBuf], bundle_name: Option<&str>) -> DeleteResult {
        let _ = bundle_name;
        let trash = self.trash_dir();
        let files_dir = trash.join("files");
        let info_dir = trash.join("info");
        let mut moved: Vec<FilePair> = Vec::new();
        let mut blocked: Vec<PathBuf> = Vec::new();
        let mut failed: Vec<PathBuf> = Vec::new();
        for url in urls {
            // 安全校验（critical 路径拦截）先于一切
            if !validate_path(&url.to_string_lossy()) {
                blocked.push(url.clone());
                continue;
            }
            let Some(name) = url.file_name().map(|n| n.to_string_lossy().into_owned()) else {
                failed.push(url.clone());
                continue;
            };
            // files/ 目标（同名冲突加序号）—— info 名与之一致
            let target = xdg::unique_name(&files_dir, &name);
            let Some(target_name) = target.file_name().map(|n| n.to_string_lossy().into_owned())
            else {
                failed.push(url.clone());
                continue;
            };
            // 移动（跨卷 EXDEV 在此失败，挂载点 TODO 处理）
            if let Err(e) = std::fs::rename(url, &target) {
                eprintln!("apios: move to trash failed for {}: {e}", url.display());
                failed.push(url.clone());
                continue;
            }
            // 写 trashinfo（失败 → 回滚，避免无法恢复的孤儿条目）
            let info_path = info_dir.join(xdg::info_file_name(&target_name));
            let content = xdg::generate_trashinfo(
                &url.to_string_lossy(),
                &Local::now().format("%Y-%m-%dT%H:%M:%S").to_string(),
            );
            if let Err(e) = std::fs::write(&info_path, content) {
                let _ = std::fs::rename(&target, url); // 回滚
                eprintln!(
                    "apios: failed to write trashinfo for {}: {e}",
                    url.display()
                );
                failed.push(url.clone());
                continue;
            }
            moved.push(FilePair {
                trash_path: target,
                original_path: url.clone(),
            });
        }
        DeleteResult {
            success: failed.is_empty(),
            bundle_folder: files_dir,
            moved,
            blocked,
            failed,
        }
    }
}

impl SpotlightIndex for LinuxAdapter {
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

impl ProcessControl for LinuxAdapter {
    /// 按应用可执行名终止：`pgrep -f <可执行名>` → `kill -TERM`。
    /// -f 全命令行匹配（绕过进程名 15 字符截断）；AppInfo.path 是 .desktop 路径，
    /// file_stem（如 "firefox"）通常与二进制同名。无匹配（pgrep exit 1）→ 0。
    fn kill_running_app(&self, app: &AppInfo) -> u32 {
        let Some(name) = app.path.file_stem().and_then(|n| n.to_str()) else {
            return 0;
        };
        let Ok(out) = cmd_util::run_capture(Path::new("pgrep"), &["-f", name], &[]) else {
            return 0;
        };
        if !out.status.success() {
            return 0;
        }
        let pids: Vec<&str> = out
            .stdout
            .lines()
            .map(str::trim)
            .filter(|l| !l.is_empty())
            .collect();
        let mut killed = 0;
        for pid in pids {
            if cmd_util::run_capture(Path::new("kill"), &["-TERM", pid], &[]).is_ok() {
                killed += 1;
            }
        }
        killed
    }
}

/// 开发环境路径表（Linux XDG 布局；收紧原则同 macOS：只列可再生缓存）。
/// 18 通用环境 + 包管理器缓存（APT/DNF/pacman/Snapd —— 均 /var 下 root 目录，
/// 由 dev-clean 的 check_protected 触发 sudo 提示；与 macOS brew 缓存归 dev-clean
/// 的划分一致：缓存归 dev-clean，包本体卸载归 `pkg apt`）。
fn dev_envs_table() -> Vec<DevEnv> {
    let p = |s: &str| s.to_string();
    vec![
        DevEnv {
            name: "APT Cache".into(),
            paths: vec![p("/var/cache/apt/archives/")],
        },
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
            name: "DNF Cache".into(),
            paths: vec![p("/var/cache/dnf/")],
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
            name: "Snapd Cache".into(),
            paths: vec![p("/var/lib/snapd/cache/")],
        },
        DevEnv {
            name: "pacman Cache".into(),
            paths: vec![p("/var/cache/pacman/pkg/")],
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

impl DevEnvPaths for LinuxAdapter {
    fn dev_envs(&self) -> Vec<DevEnv> {
        dev_envs_table()
    }
}

impl PluginPaths for LinuxAdapter {
    fn plugin_categories(&self) -> Vec<PluginCategory> {
        vec![] // 其他平台暂无 macOS 式插件目录体系，返回空
    }
}

impl AppDiscovery for LinuxAdapter {
    /// 扫描 apps_paths 下的 .desktop 文件（desktop.rs 解析）→ AppInfo。
    /// bundle_identifier 空 → identifiers 门控自动关闭 bundle-id 匹配族，
    /// name needle 继续生效（与 Windows 未注册应用的降级路径一致）。
    fn discover_installed_apps(&self) -> Vec<AppInfo> {
        let mut apps: Vec<AppInfo> = Vec::new();
        for dir in self.apps_paths() {
            let Ok(entries) = std::fs::read_dir(dir) else {
                continue;
            };
            for entry in entries.flatten() {
                let path = entry.path();
                if path.extension().is_some_and(|e| e == "desktop") {
                    let Ok(content) = std::fs::read_to_string(&path) else {
                        continue;
                    };
                    if let Some(de) = desktop::parse_desktop(&content) {
                        apps.push(AppInfo {
                            path,
                            bundle_identifier: String::new(),
                            app_name: de.name,
                            entitlements: Vec::new(),
                            team_identifier: None,
                            web_app: false,
                            steam: false,
                            wrapped: false,
                        });
                    }
                }
            }
        }
        apps.sort_by(|a, b| a.app_name.cmp(&b.app_name));
        apps
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_dev_envs_cross_platform_paths() {
        // Linux 表不得含 macOS 布局（~/Library），且不得列工具本体
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
                                                         // 新增的包管理器缓存项（借鉴 BleachBit 的 Linux 清理清单）
        for name in ["APT Cache", "DNF Cache", "pacman Cache", "Snapd Cache"] {
            assert!(envs.iter().any(|e| e.name == name), "缺 {name}");
        }
    }

    /// XDG trash 集成测试：构造临时 HOME 的 LinuxAdapter，验证 files/+info/ 布局、
    /// trashinfo 内容与冲突后缀。
    fn adapter_with_trash(tmp: &Path) -> (LinuxAdapter, PathBuf) {
        let home = tmp.join("home");
        let trash = home.join(".local/share/Trash");
        std::fs::create_dir_all(trash.join("files")).unwrap();
        std::fs::create_dir_all(trash.join("info")).unwrap();
        let adapter = LinuxAdapter {
            home: home.to_string_lossy().into_owned(),
            cache_dir: String::new(),
            temp_dir: String::new(),
        };
        (adapter, trash)
    }

    #[test]
    fn test_xdg_trash_move_writes_files_and_info() {
        let tmp = tempfile::TempDir::new().unwrap();
        let (adapter, trash) = adapter_with_trash(tmp.path());
        let file = tmp.path().join("victim.txt");
        std::fs::write(&file, b"data").unwrap();

        let result = adapter.move_to_trash(&[file.clone()], None);
        assert!(result.success);
        assert!(result.blocked.is_empty());
        assert!(result.failed.is_empty());
        assert_eq!(result.moved.len(), 1);

        // files/ 有文件，原路径消失
        let target = &result.moved[0].trash_path;
        assert!(target.starts_with(trash.join("files")));
        assert!(target.exists());
        assert!(!file.exists());
        // info/ 有对应 trashinfo，Path 字段指向原路径
        let info_name = target
            .file_name()
            .and_then(|n| n.to_str())
            .map(xdg::info_file_name)
            .unwrap();
        let info_content = std::fs::read_to_string(trash.join("info").join(&info_name)).unwrap();
        let (orig, _date) = xdg::parse_trashinfo(&info_content).unwrap();
        assert_eq!(orig, file.to_string_lossy());
    }

    #[test]
    fn test_xdg_trash_conflict_appends_suffix() {
        let tmp = tempfile::TempDir::new().unwrap();
        let (adapter, trash) = adapter_with_trash(tmp.path());
        let f1 = tmp.path().join("same.txt");
        let f2 = tmp.path().join("same.txt");
        // 两个不同路径但同名文件 → 第二个加 .1 后缀
        std::fs::write(&f1, b"a").unwrap();
        std::fs::write(&f2, b"b").unwrap();
        let r1 = adapter.move_to_trash(&[f1.clone()], None);
        let r2 = adapter.move_to_trash(&[f2.clone()], None);
        assert!(r1.success && r2.success);
        let names: Vec<String> = r2
            .moved
            .iter()
            .map(|p| {
                p.trash_path
                    .file_name()
                    .unwrap()
                    .to_string_lossy()
                    .into_owned()
            })
            .collect();
        assert_eq!(names, vec!["same.txt.1"]);
        // 两个 trashinfo 都存在
        assert!(trash.join("info/same.txt.trashinfo").exists());
        assert!(trash.join("info/same.txt.1.trashinfo").exists());
    }

    #[test]
    fn test_xdg_trash_blocked_path_rejected() {
        let tmp = tempfile::TempDir::new().unwrap();
        let (adapter, _trash) = adapter_with_trash(tmp.path());
        // critical 路径（/etc/…）被 validate_path 拦截
        let result = adapter.move_to_trash(&[PathBuf::from("/etc/hosts")], None);
        assert!(!result.blocked.is_empty());
        assert!(result.moved.is_empty());
        assert!(result.failed.is_empty());
    }

    #[test]
    fn test_desktop_discovery_basic() {
        let tmp = tempfile::TempDir::new().unwrap();
        let apps_dir = tmp.path().join("applications");
        std::fs::create_dir_all(&apps_dir).unwrap();
        std::fs::write(
            apps_dir.join("firefox.desktop"),
            "[Desktop Entry]\nType=Application\nName=Firefox\nExec=/usr/bin/firefox %U\n",
        )
        .unwrap();
        // NoDisplay 与无效项不产出
        std::fs::write(
            apps_dir.join("hidden.desktop"),
            "[Desktop Entry]\nType=Application\nName=Hidden\nExec=/usr/bin/x\nNoDisplay=true\n",
        )
        .unwrap();
        std::fs::write(
            apps_dir.join("link.desktop"),
            "[Desktop Entry]\nType=Link\nName=Home\nURL=file:///home\n",
        )
        .unwrap();
        // 非 desktop 扩展名跳过
        std::fs::write(apps_dir.join("readme.txt"), "not a desktop file").unwrap();

        let adapter = LinuxAdapter {
            home: tmp.path().to_string_lossy().into_owned(),
            cache_dir: String::new(),
            temp_dir: String::new(),
        };
        let apps = adapter.discover_installed_apps();
        assert_eq!(apps.len(), 1);
        assert_eq!(apps[0].app_name, "Firefox");
        assert_eq!(
            apps[0].path.file_name().unwrap().to_string_lossy(),
            "firefox.desktop"
        );
        assert!(apps[0].bundle_identifier.is_empty());
    }
}
