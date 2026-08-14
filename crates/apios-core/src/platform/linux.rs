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
use crate::trash::{validate_path, DeleteFailure, DeleteFailureReason, DeleteResult, FilePair};

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
        let mut failed: Vec<DeleteFailure> = Vec::new();
        // 首次使用时创建 files/ 与 info/（失败 → 全部 failed，语义同
        // move_to_trash_dir 的 create_dir_all 失败路径；根因细分
        // PermissionDenied/TrashUnavailable，2026-08-15 审查 P1-5）
        let failure_all = |urls: &[PathBuf], reason: DeleteFailureReason| -> Vec<DeleteFailure> {
            urls.iter()
                .map(|p| DeleteFailure {
                    path: p.clone(),
                    reason: reason.clone(),
                })
                .collect()
        };
        let trash_dir_reason = |e: &std::io::Error| -> DeleteFailureReason {
            match e.kind() {
                std::io::ErrorKind::PermissionDenied => DeleteFailureReason::PermissionDenied,
                _ => DeleteFailureReason::TrashUnavailable,
            }
        };
        if let Err(e) = std::fs::create_dir_all(&files_dir) {
            eprintln!(
                "apios: cannot create trash dir {}: {e}",
                files_dir.display()
            );
            return DeleteResult {
                success: false,
                bundle_folder: files_dir,
                moved,
                blocked,
                failed: failure_all(urls, trash_dir_reason(&e)),
            };
        }
        if let Err(e) = std::fs::create_dir_all(&info_dir) {
            eprintln!(
                "apios: cannot create trash info dir {}: {e}",
                info_dir.display()
            );
            return DeleteResult {
                success: false,
                bundle_folder: files_dir,
                moved,
                blocked,
                failed: failure_all(urls, trash_dir_reason(&e)),
            };
        }
        for url in urls {
            // 安全校验（critical 路径拦截）先于一切
            if !validate_path(&url.to_string_lossy()) {
                blocked.push(url.clone());
                continue;
            }
            let Some(name) = url.file_name().map(|n| n.to_string_lossy().into_owned()) else {
                failed.push(DeleteFailure {
                    path: url.clone(),
                    reason: DeleteFailureReason::Other("no file name".into()),
                });
                continue;
            };
            // files/ 目标（files 与 info 双侧冲突检查，P1-10）—— info 名与之一致
            let target = xdg::unique_name_pair(&files_dir, &info_dir, &name);
            let Some(target_name) = target.file_name().map(|n| n.to_string_lossy().into_owned())
            else {
                failed.push(DeleteFailure {
                    path: url.clone(),
                    reason: DeleteFailureReason::Other("no target name".into()),
                });
                continue;
            };
            // 移动（跨卷 EXDEV → 文件 copy+remove 回退，对齐 POSIX 归档实现；
            // 挂载点 `.Trash-$uid` 选择仍留 TODO，2026-08-15 审查 P1-11）
            if let Err(e) = std::fs::rename(url, &target) {
                if e.raw_os_error() == Some(libc::EXDEV)
                    && url.is_file()
                    && std::fs::copy(url, &target).is_ok()
                    && std::fs::remove_file(url).is_ok()
                {
                    // copy 回退成功（非原子，中断可能留副本——同核心实现语义）
                } else {
                    eprintln!("apios: move to trash failed for {}: {e}", url.display());
                    failed.push(DeleteFailure::from_io_error(url.clone(), &e));
                    continue;
                }
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
                failed.push(DeleteFailure::from_io_error(url.clone(), &e));
                continue;
            }
            moved.push(FilePair {
                trash_path: target,
                original_path: url.clone(),
            });
        }
        DeleteResult {
            // 对齐核心 move_to_trash_dir / Windows 的语义：全 blocked 或空列表
            // 时 success=false，CLI 走 "Nothing to delete" 分支而非误报已删除
            // （2026-08-15 审查 P1-13）
            success: failed.is_empty() && !moved.is_empty(),
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
    /// 按应用可执行名终止：`ps` 枚举 + 可执行名匹配 → `kill -TERM`。
    /// AppInfo.path 是 .desktop 路径，file_stem（如 "firefox"）通常与二进制同名。
    ///
    /// 实现说明（2026-08-15 审查 P1-6，替代 pgrep -f）：
    /// - pgrep -f 匹配**整条命令行**——apios 自身的命令行含应用名，删除前会
    ///   把 CLI 自己 kill 掉（每次卸载都触发）
    /// - stem 含正则元字符（C++、7-Zip）时 pgrep 静默失效
    /// - 误杀所有命令行参数含该词的无关进程
    ///
    /// `ps -eo pid=,comm=` 只匹配可执行名（basename），不匹配参数；显式排除
    /// 自身 PID。comm 受 15 字符截断（TASK_COMM_LEN），双向前缀匹配覆盖截断
    /// 场景；漏杀（脚本类 python3）是安全方向。无匹配 → 0。
    fn kill_running_app(&self, app: &AppInfo) -> u32 {
        let Some(name) = app.path.file_stem().and_then(|n| n.to_str()) else {
            return 0;
        };
        let Ok(out) = cmd_util::run_capture(Path::new("ps"), &["-eo", "pid=,comm="], &[]) else {
            return 0;
        };
        if !out.status.success() {
            return 0;
        }
        let self_pid = std::process::id();
        let mut pids: Vec<String> = Vec::new();
        for line in out.stdout.lines() {
            let mut it = line.split_whitespace();
            let (Some(pid), Some(comm)) = (it.next(), it.next()) else {
                continue;
            };
            let Ok(pid): Result<u32, _> = pid.parse() else {
                continue;
            };
            if pid == self_pid {
                continue;
            }
            if comm == name || comm.starts_with(name) || name.starts_with(comm) {
                pids.push(pid.to_string());
            }
        }
        let mut killed = 0;
        for pid in pids {
            if cmd_util::run_capture(Path::new("kill"), &["-TERM", &pid], &[]).is_ok() {
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
    let mut envs = crate::dev_env::common_dev_envs();
    envs.extend([
        DevEnv {
            name: "APT Cache".into(),
            paths: vec![p("/var/cache/apt/archives/")],
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
            name: "Snapd Cache".into(),
            paths: vec![p("/var/lib/snapd/cache/")],
        },
        DevEnv {
            name: "pacman Cache".into(),
            paths: vec![p("/var/cache/pacman/pkg/")],
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
            name: "Zed".into(),
            paths: vec![p("~/.cache/zed/"), p("~/.local/share/zed/node/cache/")],
        },
    ]);
    envs
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
                        // 空 Name 不产出（P1-12）
                        if de.name.is_empty() {
                            continue;
                        }
                        // 同名 .desktop 去重：用户目录（apps_paths 靠前）优先于
                        // 系统目录（规范语义：用户覆盖系统），P1-12
                        let file_name = path
                            .file_name()
                            .map(|n| n.to_string_lossy().into_owned())
                            .unwrap_or_default();
                        if apps.iter().any(|a| {
                            a.path.file_name().map(|n| n.to_string_lossy().into_owned())
                                == Some(file_name.clone())
                        }) {
                            continue;
                        }
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

    /// XDG trash 集成测试：构造临时 HOME 的 LinuxAdapter（不预建 files/info ——
    /// move_to_trash 必须自己创建），验证 files/+info/ 布局、trashinfo 内容与
    /// 冲突后缀。
    fn adapter_with_trash(tmp: &Path) -> (LinuxAdapter, PathBuf) {
        let home = tmp.join("home");
        let trash = home.join(".local/share/Trash");
        std::fs::create_dir_all(&home).unwrap();
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

        let result = adapter.move_to_trash(std::slice::from_ref(&file), None);
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
        // 两个**不同路径**但同名文件 → 第二个加 .1 后缀
        let f1 = tmp.path().join("same.txt");
        let f2 = tmp.path().join("sub").join("same.txt");
        std::fs::create_dir_all(tmp.path().join("sub")).unwrap();
        std::fs::write(&f1, b"a").unwrap();
        std::fs::write(&f2, b"b").unwrap();
        let r1 = adapter.move_to_trash(std::slice::from_ref(&f1), None);
        let r2 = adapter.move_to_trash(std::slice::from_ref(&f2), None);
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
        // validate_path 只拦 critical **根**（子路径如 /etc/hosts 是合法删除
        // 目标 —— 搜索范围本身不会产出系统文件）—— 测根目录被拦截
        for root in ["/etc", "/usr", "/var"] {
            let result = adapter.move_to_trash(&[PathBuf::from(root)], None);
            assert!(!result.blocked.is_empty(), "{root} 应被 critical 表拦截");
            assert!(result.moved.is_empty());
            assert!(result.failed.is_empty());
        }
    }

    #[test]
    fn test_desktop_discovery_basic() {
        let tmp = tempfile::TempDir::new().unwrap();
        // 测试文件必须放在 apps_paths() 实际扫描的目录（{home}/.local/share/applications）
        let home = tmp.path();
        let apps_dir = home.join(".local/share/applications");
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
        // 系统目录（/usr/share/applications 等）的桌面应用也会被发现 —— 按路径
        // 断言测试文件本身在结果中；NoDisplay/无效项不得混入
        let firefox_path = apps_dir.join("firefox.desktop");
        let apps = adapter.discover_installed_apps();
        assert!(
            apps.iter().any(|a| a.path == firefox_path),
            "测试 firefox.desktop 应被发现"
        );
        let firefox = apps.iter().find(|a| a.path == firefox_path).unwrap();
        assert_eq!(firefox.app_name, "Firefox");
        assert!(firefox.bundle_identifier.is_empty());
        // NoDisplay/Hidden 应用现在**产出**（已安装应用须纳入已装集合，
        // 孤儿豁免；2026-08-15 审查 P1-12）——断言其被正确发现
        assert!(apps.iter().any(|a| a.app_name == "Hidden"));
    }
}
