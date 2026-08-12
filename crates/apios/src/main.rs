//! apios：ApiosCleaner 命令行工具（命令参数式）
//!
//! 用法：
//!   apios list <app>         列出应用相关文件（只读，不删除）
//!   apios uninstall <app>    卸载：应用本体 + 全部相关文件 → 回收站（交互确认）
//!   apios orphan             列出孤儿文件（只读，不删除）
//!   apios clean-orphan       删除全部孤儿文件（交互确认）
//!   apios dev-clean [env]    列出开发环境缓存；带 <env> 则清理（交互确认）
//!   apios pkg <pm> <action>  包管理器：卸载包本体 + 依赖处理（brew 为当前实现）
//!   apios plugins [类别]     列出插件（18 类：音频/偏好面板/QuickLook 等）
//!   apios plugins --clean    清理插件（交互确认；可指定类别，如 --clean audio）
//!   apios lipo [app]         扫描通用（fat）二进制，显示可省空间（只读；macOS 专属）
//!   apios lipo thin <app>    瘦身为当前架构（交互确认；--sign 可选 ad-hoc 重签；macOS 专属）
//!
//! <app> 参数支持三种形式：
//!   完整路径      apios uninstall /Applications/Foo.app
//!   应用名        apios uninstall Foo        （在默认应用目录自动查找 Foo.app）
//!   当前目录      apios uninstall .
//!
//! 删除类命令都会先列出影响范围并请求确认（y/N，默认拒绝）。
//! 脚本与 GUI 对接用 `-y` 跳过确认；`--help` 查看全部用法。

use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::process::exit;

#[cfg(not(target_os = "windows"))]
use apios_core::app_info::get_app_info;
use apios_core::dev_env::{dedup_nested, dir_size, env_sizes, expand_globs, expand_home, find_env};
use apios_core::locations::Locations;
use apios_core::model::{AppInfo, Sensitivity};
use apios_core::orphan::ReversePathsSearcher;
use apios_core::pkg::{detect_kind, PkgKind};
#[cfg(target_os = "macos")]
use apios_core::platform::lipo::{self, cpu_name, current_cputype, select_runnable_slice, FatFile};
use apios_core::platform::{
    AppDiscovery, PackageManager, PackageManagers, PluginPaths, ProcessControl, SystemPaths,
};
use apios_core::plugin::{group_by_category, scan_plugins, PluginCategory};
use apios_core::scan::default_app_folders;
#[cfg(target_os = "windows")]
use apios_core::scan::find_app_by_path;
#[cfg(target_os = "macos")]
use apios_core::scan::get_sorted_apps;
use apios_core::search::AppPathFinder;
use apios_core::trash::{delete_files, is_writable};
use clap::{Parser, Subcommand};

/// 回收站文案（Windows 无 Trash 归档目录概念，系统回收站叫 Recycle Bin）
fn trash_label() -> &'static str {
    #[cfg(target_os = "windows")]
    {
        "Recycle Bin"
    }
    #[cfg(not(target_os = "windows"))]
    {
        "Trash"
    }
}

/// 删除完成消息（Windows 回收站内路径不可知，不打印归档目录）
fn deleted_message(count: usize, bundle_folder: &std::path::Path) -> String {
    #[cfg(target_os = "windows")]
    {
        let _ = bundle_folder;
        format!("Deleted {count} files to {}", trash_label())
    }
    #[cfg(not(target_os = "windows"))]
    {
        format!(
            "Deleted {count} files to {} ({})",
            trash_label(),
            bundle_folder.display()
        )
    }
}

#[derive(Parser)]
#[command(
    name = "apios",
    bin_name = "apios", // Windows 上固定显示名（argv[0] 是 apios.exe）
    version,
    about = "ApiosCleaner — a fast cross-platform app cleaner",
    long_about = "ApiosCleaner finds every file an app left behind and cleans it up: \
full uninstalls, orphan files from already-uninstalled apps, dev-environment \
caches, plugin directories, package-manager packages, and (on macOS) dead \
architectures in universal binaries.\n\n\
Deletions are safe by design:\n  \
• every destructive command prints what it will do and asks for confirmation \
(y/N, default no; -y skips it for scripting)\n  \
• files are moved to the Trash (macOS/Linux) or the Recycle Bin (Windows), \
never permanently deleted\n  \
• critical system paths are protected against normalization tricks\n\n\
Platform notes:\n  \
• macOS — full feature set, incl. Homebrew and lipo\n  \
• Windows — registry + Start Menu discovery, Recycle Bin via the system API, \
dev caches, winget\n  \
• Linux — compiles with default XDG behavior",
    after_long_help = "EXAMPLES:\n  \
apios list Firefox                    List everything Firefox leaves behind\n  \
apios uninstall Firefox               Move Firefox + all its files to the Trash\n  \
apios orphan                          Show orphans from uninstalled apps\n  \
apios clean-orphan                    Delete them (after confirmation)\n  \
apios dev-clean cargo                 Show/clean the Cargo cache\n  \
apios pkg brew uninstall git          Uninstall a Homebrew package\n  \
apios plugins --clean audio           Delete audio plugins\n  \
apios lipo thin Firefox               Thin Firefox's universal binaries (macOS)\n\n\
See also: apios <command> --help for command-specific details."
)]
struct Cli {
    /// Skip confirmation prompts (for scripting and GUI/automation integration)
    #[arg(short, long, global = true)]
    yes: bool,
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// List all related files of an app (read-only)
    #[command(
        long_about = "List every file an app owns outside its bundle: caches, preferences, saved \
state, Application Support, launch agents, … (read-only; nothing is modified).\n\n\
<app> accepts a full path, an app name (looked up in the default application folders), \
or \".\" for the current directory. On Windows it accepts a registry DisplayName (e.g. \
\"7-Zip\"), an installer path, or a .lnk path — there is no bundle_identifier, so \
matching falls back to display-name / path keywords.",
        after_long_help = "EXAMPLES:\n  \
apios list /Applications/Firefox.app\n  \
apios list Firefox\n  \
apios list .\n\n\
On Windows:\n  \
apios list \"7-Zip\""
    )]
    List {
        /// Full path to the app, an app name (looked up in the default application
        /// folders), or "." for the current directory.
        /// Windows: registry DisplayName, installer path, or .lnk path.
        app: String,
    },
    /// Uninstall an app: the bundle and ALL related files, moved to Trash
    #[command(
        long_about = "Move an app and ALL its related files to the Trash: the .app bundle, \
caches, preferences, saved state, Application Support, launch agents, …\n\n\
Asks for confirmation (y/N, default no) unless -y is given. Files are moved to a \
timestamped archive folder in the Trash (macOS/Linux) or the Recycle Bin (Windows) — \
nothing is ever permanently deleted.",
        after_long_help = "EXAMPLES:\n  \
apios uninstall Firefox\n  \
apios uninstall -y Firefox          # no confirmation (scripting)"
    )]
    Uninstall {
        /// Full path to the app, an app name (looked up in the default application
        /// folders), or "." for the current directory.
        /// Windows: registry DisplayName, installer path, or .lnk path.
        app: String,
    },
    /// List orphaned files left behind by uninstalled apps (read-only)
    #[command(
        long_about = "Show files left behind by apps that are no longer installed \
(read-only): caches, preferences, and support files whose owning app has gone. \
Detection uses a prebuilt UUID → bundle-id map plus name heuristics.\n\n\
Live apps are never listed — if an app is found again later, its files stop being \
orphans."
    )]
    Orphan,
    /// Delete all orphaned files (asks for confirmation)
    #[command(
        long_about = "Delete all orphaned files (after confirmation). Same safety model as \
uninstall: files move to the Trash/Recycle Bin, never permanent; critical system paths \
are protected."
    )]
    CleanOrphan,
    /// List dev environment caches (read-only); with <env>, clean it
    #[command(
        long_about = "Inspect and clean dev-environment caches.\n\n\
With no <env>: list every known dev environment with its cache location and size \
(read-only).\n\n\
With <env>: clean that environment's cache after confirmation — or \"all\" for every \
known environment. The caches (Cargo, npm, pip, Gradle, Xcode, …) are regenerable by \
the tools themselves; nothing personal is deleted.",
        after_long_help = "EXAMPLES:\n  \
apios dev-clean                   # sizes of all dev caches\n  \
apios dev-clean cargo             # clean the Cargo cache\n  \
apios dev-clean all               # clean every known dev cache"
    )]
    DevClean {
        /// Environment name (case-insensitive), or "all" for everything
        #[arg(
            long_help = "Environment name (case-insensitive), e.g. cargo, npm, pip, uv, \
gradle, maven, go, deno, yarn, xcode, vscode, jetbrains, android, composer — or \"all\" \
for everything."
        )]
        env: Option<String>,
    },
    /// Manage packages installed via a package manager (e.g. Homebrew)
    #[command(
        long_about = "Manage packages installed through a package manager.\n\n\
On macOS this is Homebrew (formulae and casks); on Windows, winget. The <pm> selector \
picks the manager: \"brew\" on macOS, \"winget\" on Windows.",
        after_long_help = "EXAMPLES:\n  \
apios pkg brew list\n  \
apios pkg brew uninstall git\n  \
apios pkg brew uninstall --zap firefox\n  \
apios pkg brew autoremove\n\n\
On Windows:\n  \
apios pkg winget list\n  \
apios pkg winget uninstall \"7-Zip\""
    )]
    Pkg {
        /// Package manager selector, e.g. "brew"
        #[arg(
            long_help = "Package manager selector: \"brew\" on macOS, \"winget\" on \
Windows."
        )]
        pm: String,
        #[command(subcommand)]
        action: PkgAction,
    },
    /// Scan and clean plugin directories (audio, preference panes, quick look, ...)
    #[command(
        long_about = "Scan plugin directories (read-only by default): audio components, \
preference panes, QuickLook generators, screen savers, Mail bundles, … — 18 categories \
on macOS.\n\n\
With <category>, show only that category (case-insensitive). With --clean, delete \
plugins instead: everything listed is moved to the Trash after confirmation.",
        after_long_help = "EXAMPLES:\n  \
apios plugins                     # all categories\n  \
apios plugins audio               # audio components only\n  \
apios plugins --clean             # delete all listed plugins\n  \
apios plugins --clean audio       # delete audio plugins only"
    )]
    Plugins {
        /// Category name to show (case-insensitive); omit for all
        #[arg(long_help = "Category name to show (case-insensitive); omit for all.")]
        category: Option<String>,
        /// Delete the listed plugins instead of just listing (asks for confirmation).
        /// Optional category name; omit for all
        #[arg(
            long,
            num_args = 0..=1,
            default_missing_value = "all",
            long_help = "Delete the listed plugins instead of just listing them (asks for \
confirmation). With an optional category name, only that category is cleaned; without \
one, all categories are cleaned."
        )]
        clean: Option<String>,
    },
    /// Scan apps for universal (fat) binaries; thin them to the current architecture
    /// (macOS only: universal binaries are a Darwin format)
    #[cfg(target_os = "macos")]
    #[command(
        long_about = "Scan apps for universal (fat) binaries and report how much space could \
be freed (read-only).\n\n\
Apple Silicon Macs run universal binaries (arm64 + x86_64), so most apps carry a dead \
second architecture; thinning it away often frees roughly half the binary's size. \
Output is byte-identical to Apple's lipo; the best slice is kept (arm64e preferred, \
x86_64h gated on AVX2).\n\n\
macOS only — the command is not compiled on other platforms.",
        after_long_help = "EXAMPLES:\n  \
apios lipo                         # scan all apps\n  \
apios lipo Firefox                 # scan one app\n  \
apios lipo thin Firefox            # thin one app (irreversible)\n  \
apios lipo thin --sign Firefox     # thin and re-sign ad-hoc"
    )]
    Lipo {
        /// App path or name to scan; omit to scan all apps in the default folders
        #[arg(
            long_help = "App path or name to scan; omit to scan all apps in the default \
folders."
        )]
        app: Option<String>,
        #[command(subcommand)]
        action: Option<LipoAction>,
    },
}

#[derive(Subcommand)]
enum PkgAction {
    /// List installed packages (formulae and casks)
    #[command(
        long_about = "List packages installed through the selected manager (read-only). \
On macOS, formulae and casks are listed separately."
    )]
    List,
    /// Uninstall one package (formula or cask; type auto-detected)
    #[command(
        long_about = "Uninstall one package. On macOS the type (formula or cask) is \
auto-detected, and packages that depend on it are shown before the confirmation \
prompt.\n\n\
--zap (casks only) additionally removes user config and preferences — irreversible, and \
asks for an extra confirmation (skipped with -y).",
        after_long_help = "EXAMPLES:\n  \
apios pkg brew uninstall git\n  \
apios pkg brew uninstall --zap firefox\n  \
apios pkg winget uninstall \"7-Zip\""
    )]
    Uninstall {
        /// Package name as installed, e.g. "git" or "firefox"
        #[arg(
            long_help = "Package name as installed, e.g. \"git\" or \"firefox\"; on Windows, \
the winget package name or ID (case-insensitive)."
        )]
        name: String,
        /// Casks only: also remove user config and preferences (irreversible;
        /// asks for extra confirmation; skipped with -y)
        #[arg(
            long,
            long_help = "Casks only: also remove user config and preferences. Irreversible; \
asks for an extra confirmation (skipped with -y)."
        )]
        zap: bool,
    },
    /// Remove orphaned dependencies (dry-run is shown first; asks for confirmation)
    #[command(
        long_about = "Remove packages nothing depends on anymore (macOS only). A dry-run \
is shown first, then confirmation."
    )]
    Autoremove,
}

#[cfg(target_os = "macos")]
#[derive(Subcommand)]
enum LipoAction {
    /// Thin universal binaries in an app to the current architecture (irreversible)
    #[command(
        long_about = "Thin an app's universal binaries to the current architecture \
(irreversible; asks for confirmation).\n\n\
Code signatures are invalidated by the change; pass --sign to re-sign the thinned \
binaries ad-hoc (codesign -s -).",
        after_long_help = "EXAMPLES:\n  \
apios lipo thin Firefox\n  \
apios lipo thin --sign Firefox     # re-sign ad-hoc after thinning"
    )]
    Thin {
        /// App path or name, or "." for the current directory
        app: String,
        /// Also re-sign thinned binaries ad-hoc (codesign -s -) to fix broken signatures
        #[arg(
            long,
            long_help = "Re-sign the thinned binaries ad-hoc (codesign -s -) so they keep \
working; without it, code signatures are invalidated by thinning."
        )]
        sign: bool,
    },
}

/// 旧版 Windows 控制台默认代码页（GBK 936 / 437）无法输出 UTF-8 中文 →
/// 启动时切换到 65001（UTF-8）。kernel32 手写 FFI，零依赖。
#[cfg(target_os = "windows")]
fn set_console_utf8() {
    extern "system" {
        fn SetConsoleOutputCP(cp: u32) -> i32;
    }
    unsafe {
        SetConsoleOutputCP(65001);
    }
}

fn main() {
    #[cfg(target_os = "windows")]
    set_console_utf8();
    let cli = Cli::parse();
    match cli.command {
        Command::List { ref app } => cmd_list(&cli, app),
        Command::Uninstall { ref app } => cmd_uninstall(&cli, app),
        Command::Orphan => cmd_orphan(&cli),
        Command::CleanOrphan => cmd_clean_orphan(&cli),
        Command::DevClean { ref env } => cmd_dev_clean(&cli, env.as_deref()),
        Command::Pkg { ref pm, ref action } => cmd_pkg(&cli, pm, action),
        Command::Plugins {
            ref category,
            ref clean,
        } => cmd_plugins(&cli, category.as_deref(), clean.as_deref()),
        #[cfg(target_os = "macos")]
        Command::Lipo {
            ref app,
            ref action,
        } => cmd_lipo(&cli, app.as_deref(), action.as_ref()),
    }
}

// ---------- <app> 参数解析 ----------

/// 参数是路径形式（含目录分隔或带 .app 后缀）？
/// 注意：不含 `Path::exists()` —— 裸名（如 "Firefox"）若碰巧在 cwd 有同名文件会被劫持
/// 成路径，绕过应用名查找。裸名一律走 `find_app_by_name`。
/// Windows：反斜杠分隔与盘符（`C:\...`，限定 `X:` 形态避免误伤含 ':' 的 POSIX 名）。
fn arg_is_path(arg: &str) -> bool {
    let b = arg.as_bytes();
    // 盘符形态：`C:` 或 `C:\...`（首字符字母 + 冒号 + 反斜杠/结尾）
    let drive = b.len() >= 2
        && b[0].is_ascii_alphabetic()
        && b[1] == b':'
        && (b.len() == 2 || b[2] == b'\\');
    arg.contains('/') || arg.contains('\\') || drive || arg.to_ascii_lowercase().ends_with(".app")
}

/// 按名称查找应用 → 路径。
/// macOS/Linux：在默认应用目录中查找 <name>.app（先精确匹配，再大小写不敏感）；
/// Windows：无 .app 概念 —— 从发现结果（注册表 DisplayName / 开始菜单名）匹配。
fn find_app_by_name(name: &str, folders: &[String]) -> Option<PathBuf> {
    #[cfg(windows)]
    {
        let _ = folders;
        let apps = apios_core::platform::adapter().discover_installed_apps();
        let lower = name.to_lowercase();
        // 精确匹配优先；否则前缀匹配（注册表 DisplayName 常带版本号，
        // 如 "7-Zip 26.01 (x64)" → 输入 "7-Zip" 命中）。多命中排序：
        // 注册表主条目（path 非 .lnk）优先于开始菜单 .lnk（Help/卸载等杂项），
        // 同源按名称最短取最接近的
        apps.iter()
            .find(|a| a.app_name.to_lowercase() == lower)
            .or_else(|| {
                let mut hits: Vec<&AppInfo> = apps
                    .iter()
                    .filter(|a| a.app_name.to_lowercase().starts_with(&lower))
                    .collect();
                if hits.is_empty() {
                    return None;
                }
                hits.sort_by_key(|a| {
                    (a.path.to_string_lossy().ends_with(".lnk"), a.app_name.len())
                });
                hits.into_iter().next()
            })
            .map(|a| a.path.clone())
    }
    #[cfg(not(windows))]
    {
        let exact = format!("{name}.app");
        for folder in folders {
            let candidate = Path::new(folder).join(&exact);
            if candidate.exists() {
                return Some(candidate);
            }
        }
        // 大小写不敏感兜底（用户输入 "edge" 命中 "Microsoft Edge.app"）
        let lower = exact.to_lowercase();
        for folder in folders {
            if let Ok(entries) = std::fs::read_dir(folder) {
                for entry in entries.flatten() {
                    let file_name = entry.file_name().to_string_lossy().to_lowercase();
                    if file_name == lower {
                        return Some(entry.path());
                    }
                }
            }
        }
        None
    }
}

/// 解析 <app> 参数 → 应用 bundle 路径；失败时打印用法并退出 1
fn resolve_app_or_exit(arg: &str) -> PathBuf {
    let home = apios_core::platform::adapter().home();
    let folders = default_app_folders(&home);

    let resolved = if arg == "." {
        // 当前目录：直接使用；若当前目录是个 .app 之外的东西，后面 get_app_info 会报错
        std::env::current_dir().ok()
    } else if arg_is_path(arg) {
        let p = PathBuf::from(expand_home(arg, &home));
        // 带 .app 后缀的裸名（无 /）：先当路径；不存在时回退到应用名查找
        // （`apios list SomeApp.app` 但 SomeApp.app 不在默认目录外 → 仍按名找到）
        if p.exists() || arg.contains('/') {
            Some(p)
        } else {
            find_app_by_name(arg, &folders)
        }
    } else {
        find_app_by_name(arg, &folders)
    };

    match resolved {
        Some(p) => p,
        None => {
            // Windows 的发现源是注册表卸载项 + 开始菜单（AppDiscovery），
            // 不是目录列表 —— 列 XDG/目录会让用户以为在错误的地方查找
            #[cfg(windows)]
            eprintln!(
                "apios: cannot find \"{arg}\" as a path or an installed app \
                 (searched the registry uninstall entries and the Start Menu)"
            );
            #[cfg(not(windows))]
            eprintln!(
                "apios: cannot find \"{arg}\" as a path or an installed app (looked in {})",
                folders
                    .iter()
                    .map(|f| f.as_str())
                    .collect::<Vec<_>>()
                    .join(", ")
            );
            exit(1);
        }
    }
}

/// 获取 AppInfo，失败时打印错误并退出 1
///
/// Windows：无 Info.plist —— 从发现结果（注册表卸载项/开始菜单）按路径匹配；
/// 未注册的应用（便携版）按目录名构造最小 AppInfo（bundle 空，降级匹配生效）。
fn get_app_info_or_exit(path: &Path) -> AppInfo {
    #[cfg(windows)]
    {
        let apps = apios_core::platform::adapter().discover_installed_apps();
        if let Some(app) = find_app_by_path(path, &apps) {
            return app;
        }
        let app_name = path
            .file_stem()
            .map(|s| s.to_string_lossy().to_string())
            .unwrap_or_else(|| "Unknown".to_string());
        AppInfo {
            path: path.to_path_buf(),
            bundle_identifier: String::new(),
            app_name,
            entitlements: Vec::new(),
            team_identifier: None,
            web_app: false,
            steam: false,
            wrapped: false,
        }
    }
    #[cfg(not(windows))]
    {
        match get_app_info(path) {
            Some(app) => app,
            None => {
                eprintln!(
                    "apios: unable to fetch app info at path: {}",
                    path.display()
                );
                exit(1);
            }
        }
    }
}

// ---------- 交互确认 ----------

/// 请求 y/N 确认（默认拒绝）。`-y` 直接放行（GUI/脚本对接）。
/// 非交互输入（stdin 关闭）时默认拒绝。
fn confirm(cli: &Cli, prompt: &str) -> bool {
    if cli.yes {
        return true;
    }
    print!("{prompt} [y/N] ");
    let _ = io::stdout().flush();
    let mut line = String::new();
    if io::stdin().read_line(&mut line).is_err() {
        return false;
    }
    let answer = line.trim().to_ascii_lowercase();
    answer == "y" || answer == "yes"
}

// ---------- 受保护文件 ----------

/// 列出不可写（需 sudo）的文件；有则提示并以退出码 1 结束
fn check_protected(files: &[PathBuf], hint: &str) {
    let protected: Vec<&PathBuf> = files.iter().filter(|p| !is_writable(p)).collect();
    if protected.is_empty() {
        return;
    }
    eprintln!("apios: protected files detected — please run with sudo:");
    eprintln!("  sudo {hint}");
    eprintln!("Protected files:");
    for file in &protected {
        eprintln!("  {}", file.display());
    }
    exit(1);
}

/// 打印被 validate_path 安全校验拦截的路径（不应出现；出现即上游 bug，要可见）
fn report_blocked(blocked: &[PathBuf]) {
    if blocked.is_empty() {
        return;
    }
    eprintln!("Skipped {} protected path(s):", blocked.len());
    for p in blocked {
        eprintln!("  {}", p.display());
    }
}

// ---------- 命令实现 ----------

fn find_app_paths(app: &AppInfo) -> Vec<PathBuf> {
    let locations = Locations::new();
    let mut finder = AppPathFinder::new(app, &locations, Sensitivity::Strict);
    finder.find_paths_cli()
}

fn cmd_list(cli: &Cli, arg: &str) {
    let _ = cli;
    let path = resolve_app_or_exit(arg);
    let app = get_app_info_or_exit(&path);
    let found = find_app_paths(&app);

    for p in &found {
        println!("{}", p.display());
    }
    println!("\nFound {} application files.\n", found.len());
}

fn cmd_uninstall(cli: &Cli, arg: &str) {
    let path = resolve_app_or_exit(arg);
    let app = get_app_info_or_exit(&path);
    let found = find_app_paths(&app);

    check_protected(&found, &format!("apios uninstall {}", arg));

    println!("{} ({})", app.app_name, app.bundle_identifier);
    println!(
        "{} related files will be moved to {}:",
        found.len(),
        trash_label()
    );
    for p in &found {
        println!("  {}", p.display());
    }
    if !confirm(cli, &format!("Delete {} files? ", found.len())) {
        println!("Aborted — nothing was deleted.");
        return;
    }

    // 确认后、删除前终止运行中的实例（killApp），避免文件占用导致删除失败
    let killed = apios_core::platform::adapter().kill_running_app(&app);
    if killed > 0 {
        println!("Terminated {killed} running process(es).");
    }

    let result = delete_files(&found, Some(&app.app_name));
    if result.success {
        println!(
            "\n{}",
            deleted_message(result.moved.len(), &result.bundle_folder)
        );
        if !result.failed.is_empty() {
            eprintln!(
                "Failed to delete {} files (in use or protected).",
                result.failed.len()
            );
        }
        exit(0);
    } else if result.moved.is_empty() && result.failed.is_empty() {
        // 列表为空或全部被安全校验拦截 → 无事可删，不是错误
        report_blocked(&result.blocked);
        println!("Nothing to delete.");
        exit(0);
    } else {
        eprintln!("\napios: failed to delete files (in use or protected).");
        exit(1);
    }
}

/// 目录顶层条目（忽略 read_dir 失败与单个条目错误）
fn dir_contents(dir: &Path) -> Vec<PathBuf> {
    std::fs::read_dir(dir)
        .into_iter()
        .flatten() // Result<ReadDir> → ReadDir（目录不可读则空）
        .flatten() // Result<DirEntry> → DirEntry（条目错误则跳过）
        .map(|e| e.path())
        .collect()
}

fn cmd_dev_clean(cli: &Cli, env: Option<&str>) {
    let home = apios_core::platform::adapter().home();

    // 无参数：列出所有环境的占用（只读）
    let Some(env_name) = env else {
        let sizes = env_sizes();
        let max_name = sizes
            .iter()
            .map(|(e, _)| e.name.chars().count())
            .max()
            .unwrap_or(0);
        for (env, paths) in &sizes {
            let total: u64 = paths.iter().map(|(_, s)| s).sum();
            println!("{:width$}  {}", env.name, fmt_size(total), width = max_name);
        }
        println!(
            "\n{} environments. Run `apios dev-clean <name>` to clean one (or `all`).",
            sizes.len()
        );
        return;
    };

    let Some(env) = find_env(env_name) else {
        eprintln!("apios: unknown environment \"{env_name}\". Run `apios dev-clean` to list all.");
        exit(1);
    };

    // 目标目录（~ 展开 + 通配展开 + 存在性过滤 + 嵌套去重）
    let mut dirs: Vec<PathBuf> = env
        .paths
        .iter()
        .flat_map(|p| expand_globs(Path::new(&expand_home(p, &home))))
        .filter(|p| p.is_dir())
        .collect();
    dedup_nested(&mut dirs);
    // 清理语义 = 删目录内容（原版 deleteFolderContents），保留顶层目录
    let contents: Vec<PathBuf> = dirs.iter().flat_map(|d| dir_contents(d)).collect();
    if contents.is_empty() {
        println!("{}: nothing to clean.", env.name);
        return;
    }

    check_protected(&contents, &format!("apios dev-clean {env_name}"));

    println!(
        "{} — {} files in {} folder(s):",
        env.name,
        contents.len(),
        dirs.len()
    );
    for d in &dirs {
        println!("  {}  ({})", d.display(), fmt_size(dir_size(d)));
    }
    if !confirm(cli, &format!("Delete contents of {} folders? ", dirs.len())) {
        println!("Aborted — nothing was deleted.");
        return;
    }

    let result = delete_files(&contents, Some(&format!("Development - {}", env.name)));
    if result.success {
        println!(
            "\n{}",
            deleted_message(result.moved.len(), &result.bundle_folder)
        );
        if !result.failed.is_empty() {
            eprintln!(
                "Failed to delete {} files (in use or protected).",
                result.failed.len()
            );
        }
        exit(0);
    } else if result.moved.is_empty() && result.failed.is_empty() {
        // 全部被安全校验拦截 → 无事可删，不是错误
        report_blocked(&result.blocked);
        println!("Nothing to delete.");
        exit(0);
    } else {
        eprintln!("\napios: failed to delete files ({}).", env.name);
        exit(1);
    }
}

// ---------- 包管理器（pkg） ----------

fn cmd_pkg(cli: &Cli, pm: &str, action: &PkgAction) {
    let adapter = apios_core::platform::adapter();
    let Some(pm_obj) = adapter.package_manager(pm) else {
        let managers = adapter.package_managers();
        let names: Vec<&str> = managers.iter().map(|p| p.name()).collect();
        if names.is_empty() {
            eprintln!("apios: no package manager support on this platform.");
        } else {
            eprintln!(
                "apios: unknown package manager \"{pm}\". Available: {}",
                names.join(", ")
            );
        }
        exit(1);
    };
    match action {
        PkgAction::List => cmd_pkg_list(pm_obj.as_ref()),
        PkgAction::Uninstall { name, zap } => cmd_pkg_uninstall(cli, pm_obj.as_ref(), name, *zap),
        PkgAction::Autoremove => cmd_pkg_autoremove(cli, pm_obj.as_ref()),
    }
}

fn kind_plural(kind: PkgKind) -> &'static str {
    match kind {
        PkgKind::Formula => "Formulae",
        PkgKind::Cask => "Casks",
    }
}

fn cmd_pkg_list(pm: &dyn PackageManager) {
    for kind in [PkgKind::Formula, PkgKind::Cask] {
        let pkgs = match pm.list_installed(kind) {
            Ok(p) => p,
            Err(e) => {
                eprintln!("apios: {}: {e}", pm.name());
                exit(1);
            }
        };
        println!("{} ({}):", kind_plural(kind), pkgs.len());
        if pkgs.is_empty() {
            println!("  (none)");
            println!();
            continue;
        }
        let width = pkgs
            .iter()
            .map(|p| p.name.chars().count())
            .max()
            .unwrap_or(0);
        for p in &pkgs {
            println!("  {:width$}  {}", p.name, p.version, width = width);
        }
        println!();
    }
}

fn cmd_pkg_uninstall(cli: &Cli, pm: &dyn PackageManager, name: &str, zap: bool) {
    // 1. 种类判定（两表本地查询）
    let (formulae, casks) = match (
        pm.list_installed(PkgKind::Formula),
        pm.list_installed(PkgKind::Cask),
    ) {
        (Ok(f), Ok(c)) => (
            f.iter().map(|p| p.name.clone()).collect::<Vec<_>>(),
            c.iter().map(|p| p.name.clone()).collect::<Vec<_>>(),
        ),
        (Err(e), _) | (_, Err(e)) => {
            eprintln!("apios: {}: {e}", pm.name());
            exit(1);
        }
    };
    let Some(kind) = detect_kind(name, &formulae, &casks) else {
        eprintln!(
            "apios: {}: \"{name}\" is not installed (run `apios pkg {} list`).",
            pm.name(),
            pm.name()
        );
        exit(1);
    };

    // 2. --zap 仅对 cask 生效
    let mut effective_zap = zap;
    if zap && kind == PkgKind::Formula {
        eprintln!("apios: warning: --zap only applies to casks; ignoring it.");
        effective_zap = false;
    }

    // 3. 被依赖方警告（卸载前）
    let dependents = match pm.dependents(name, kind) {
        Ok(d) => d,
        Err(e) => {
            eprintln!("apios: {}: {e}", pm.name());
            exit(1);
        }
    };
    if !dependents.is_empty() {
        println!(
            "Warning: {} installed package(s) depend on \"{name}\":",
            dependents.len()
        );
        for d in &dependents {
            println!("  {d}");
        }
        println!("Uninstall will use --ignore-dependencies.");
    }

    // 4. 主确认
    if !confirm(cli, &format!("Uninstall {} \"{name}\"? ", kind.as_str())) {
        println!("Aborted — nothing was deleted.");
        return;
    }

    // 5. zap 额外确认（主确认之后才询问）
    if effective_zap {
        println!(
            "WARNING: --zap also removes user config and preferences for \"{name}\" (irreversible)."
        );
        if !confirm(cli, "Proceed with --zap? ") {
            println!("Skipping --zap; uninstalling without it.");
            effective_zap = false;
        }
    }

    // 6. 卸载（有被依赖方时带 --ignore-dependencies）
    if let Err(e) = pm.uninstall(name, kind, effective_zap, !dependents.is_empty()) {
        eprintln!("apios: {}: {e}", pm.name());
        exit(1);
    }
    println!("Uninstalled {name}.");

    // 7. 卸载后孤儿依赖提示（brew autoremove -n 预演）。
    // 卸载已成功 —— 后续步骤失败一律降级为 warning，不把命令整体报成失败。
    match pm.autoremove_dry_run() {
        Ok(orphans) if !orphans.is_empty() => {
            println!("\n{} orphaned package(s) detected:", orphans.len());
            for o in &orphans {
                println!("  {o}");
            }
            if confirm(cli, &format!("Autoremove {} package(s)? ", orphans.len())) {
                match pm.autoremove() {
                    Ok(()) => println!("Autoremoved {} package(s).", orphans.len()),
                    Err(e) => {
                        eprintln!("apios: warning: autoremove failed: {e}");
                        println!("Hint: run `apios pkg {} autoremove` to retry.", pm.name());
                    }
                }
            } else {
                println!(
                    "Hint: run `apios pkg {} autoremove` to remove them.",
                    pm.name()
                );
            }
        }
        Ok(_) => {}
        Err(e) => {
            eprintln!("apios: warning: autoremove dry-run failed: {e}");
        }
    }
}

fn cmd_pkg_autoremove(cli: &Cli, pm: &dyn PackageManager) {
    let orphans = match pm.autoremove_dry_run() {
        Ok(o) => o,
        Err(e) => {
            eprintln!("apios: {}: {e}", pm.name());
            exit(1);
        }
    };
    if orphans.is_empty() {
        println!("Nothing to autoremove.");
        return;
    }
    println!("Autoremove {} package(s):", orphans.len());
    for o in &orphans {
        println!("  {o}");
    }
    if !confirm(cli, &format!("Autoremove {} package(s)? ", orphans.len())) {
        println!("Aborted — nothing was deleted.");
        return;
    }
    if let Err(e) = pm.autoremove() {
        eprintln!("apios: {}: {e}", pm.name());
        exit(1);
    }
    println!("Autoremoved {} package(s).", orphans.len());
}

// ---------- 插件（PluginsView 移植） ----------

/// 分类选择：null/"all" → 全部分类；否则大小写不敏感匹配单个分类
fn resolve_plugin_categories(
    categories: &[PluginCategory],
    arg: Option<&str>,
) -> Vec<PluginCategory> {
    let Some(arg) = arg else {
        return categories.to_vec();
    };
    if arg.eq_ignore_ascii_case("all") {
        return categories.to_vec();
    }
    match categories.iter().find(|c| c.name.eq_ignore_ascii_case(arg)) {
        Some(c) => vec![c.clone()],
        None => {
            eprintln!("apios: unknown plugin category \"{arg}\"");
            let names: Vec<&str> = categories.iter().map(|c| c.name.as_str()).collect();
            eprintln!("available: {}", names.join(", "));
            exit(1);
        }
    }
}

/// 路径截断显示（长路径中间省略，对齐 lipo 惯例）
fn truncated(path: &Path, max: usize) -> String {
    let s = path.display().to_string();
    if s.chars().count() <= max {
        return s;
    }
    let keep = max / 2 - 1;
    let head: String = s.chars().take(keep).collect();
    let tail: String = s.chars().skip(s.chars().count() - keep).collect();
    format!("{head}…{tail}")
}

fn cmd_plugins(cli: &Cli, category: Option<&str>, clean: Option<&str>) {
    let categories = apios_core::platform::adapter().plugin_categories();
    // --clean 有值 → 删除模式（目标 = clean 值）；否则列出（目标 = category 值）
    let target = resolve_plugin_categories(&categories, clean.or(category));
    let grouped = group_by_category(scan_plugins(&target));
    let count: usize = grouped.iter().map(|(_, list)| list.len()).sum();
    let total: u64 = grouped
        .iter()
        .flat_map(|(_, list)| list)
        .map(|p| p.size)
        .sum();

    if clean.is_some() {
        if count == 0 {
            println!("Nothing to delete.");
            return;
        }
        for (cat, list) in &grouped {
            let cat_total: u64 = list.iter().map(|p| p.size).sum();
            println!("{cat} ({}, {})", list.len(), fmt_size(cat_total));
            for p in list {
                println!("  {}  {}", truncated(&p.path, 64), fmt_size(p.size));
            }
        }
        if !confirm(
            cli,
            &format!(
                "\nDelete {count} plugin(s) — {}? (moved to {})",
                fmt_size(total),
                trash_label()
            ),
        ) {
            println!("Aborted — nothing was deleted.");
            return;
        }
        let paths: Vec<PathBuf> = grouped
            .iter()
            .flat_map(|(_, list)| list)
            .map(|p| p.path.clone())
            .collect();
        let result = delete_files(&paths, Some(&format!("Plugins ({count} items)")));
        if result.success {
            println!(
                "\n{}",
                deleted_message(result.moved.len(), &result.bundle_folder)
            );
        }
        if !result.failed.is_empty() {
            eprintln!("apios: {} file(s) could not be moved", result.failed.len());
            exit(1);
        }
        return;
    }

    // 只读列出
    if count == 0 {
        println!("No plugins found.");
        return;
    }
    for (cat, list) in &grouped {
        let cat_total: u64 = list.iter().map(|p| p.size).sum();
        println!("{cat} ({}, {})", list.len(), fmt_size(cat_total));
        for p in list {
            println!("  {}  {}", truncated(&p.path, 64), fmt_size(p.size));
        }
    }
    println!(
        "\n{} plugin(s) across {} categories — {}.",
        count,
        grouped.len(),
        fmt_size(total)
    );
    println!(
        "Run `apios plugins --clean [category]` to delete them (moved to {}).",
        trash_label()
    );
}

// ---------- Lipo（fat 瘦身） ----------

#[cfg(target_os = "macos")]
fn cmd_lipo(cli: &Cli, app: Option<&str>, action: Option<&LipoAction>) {
    match action {
        Some(LipoAction::Thin { app, sign }) => cmd_lipo_thin(cli, app, *sign),
        None => cmd_lipo_scan(cli, app),
    }
}

/// 目标切片（当前架构 + CPU 能力过滤：x86_64h 仅在有 AVX2 时可选）；
/// 当前架构不在文件内 → None（跳过该文件）
#[cfg(target_os = "macos")]
fn keep_slice(fat: &FatFile) -> Option<&apios_core::platform::lipo::FatSlice> {
    select_runnable_slice(&fat.slices, current_cputype())
}

/// 切片描述："arm64 (52.3 MB) · x86_64 (48.1 MB)"
#[cfg(target_os = "macos")]
fn describe_slices(fat: &FatFile) -> String {
    fat.slices
        .iter()
        .map(|s| {
            format!(
                "{} ({})",
                cpu_name(s.cputype, s.cpusubtype),
                fmt_size(s.size)
            )
        })
        .collect::<Vec<_>>()
        .join(" · ")
}

/// 相对路径显示，超长截断（避免 Qt 插件式长路径把列宽撑爆）
#[cfg(target_os = "macos")]
fn truncated_rel(path: &Path, bundle: &Path, max: usize) -> String {
    let rel = path
        .strip_prefix(bundle)
        .unwrap_or(path)
        .display()
        .to_string();
    if rel.chars().count() <= max {
        return rel;
    }
    format!(
        "{}…",
        rel.chars().take(max.saturating_sub(1)).collect::<String>()
    )
}

/// 打印单个 app 的 fat 二进制明细；返回（可省总量, fat 二进制数）
#[cfg(target_os = "macos")]
fn print_app_binaries(bundle: &Path, bins: &[(std::path::PathBuf, FatFile)]) -> (u64, usize) {
    const MAX_PATH: usize = 64;
    let width = bins
        .iter()
        .map(|(p, _)| truncated_rel(p, bundle, MAX_PATH).chars().count())
        .max()
        .unwrap_or(0);
    let mut freed = 0u64;
    for (path, fat) in bins {
        let rel = truncated_rel(path, bundle, MAX_PATH);
        println!("  {rel:width$}  {}", describe_slices(fat), width = width);
        match keep_slice(fat) {
            Some(keep) => {
                let len = std::fs::metadata(path).map(|m| m.len()).unwrap_or(0);
                let free = len.saturating_sub(keep.size);
                freed += free;
                println!(
                    "      → keep {} ({}), free {}",
                    cpu_name(keep.cputype, keep.cpusubtype),
                    fmt_size(keep.size),
                    fmt_size(free)
                );
            }
            None => println!("      (no slice for current architecture)"),
        }
    }
    (freed, bins.len())
}

/// 扫描：`apios lipo`（全部应用）或 `apios lipo <app>`（只读）
#[cfg(target_os = "macos")]
fn cmd_lipo_scan(cli: &Cli, app: Option<&str>) {
    let _ = cli;
    if let Some(arg) = app {
        let bundle = resolve_app_or_exit(arg);
        let app_name = bundle
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("app")
            .to_string();
        let bins = lipo::scan_dir_fat_binaries(&bundle);
        println!("{app_name}:");
        let (freed, count) = print_app_binaries(&bundle, &bins);
        println!();
        if count == 0 {
            println!("No universal binaries in {app_name}.");
        } else {
            println!("{count} fat binary(ies) — can free {}", fmt_size(freed));
        }
        return;
    }

    // 全部默认应用目录
    let home = apios_core::platform::adapter().home();
    let apps = get_sorted_apps(&default_app_folders(&home));
    let mut total_freed = 0u64;
    let mut app_count = 0usize;
    for app in &apps {
        let bins = lipo::scan_dir_fat_binaries(&app.path);
        if bins.is_empty() {
            continue;
        }
        let (freed, count) = print_app_binaries(&app.path, &bins);
        let name = app.app_name.as_str();
        println!(
            "{name}: {count} fat binary(ies) — can free {}",
            fmt_size(freed)
        );
        println!();
        total_freed += freed;
        app_count += 1;
    }
    if app_count == 0 {
        println!("No universal binaries found in any app.");
    } else {
        println!(
            "{app_count} app(s) with universal binaries — total can free {}",
            fmt_size(total_freed)
        );
    }
}

/// 瘦身：`apios lipo thin <app> [--sign]`（破坏性，交互确认）
#[cfg(target_os = "macos")]
fn cmd_lipo_thin(cli: &Cli, arg: &str, sign: bool) {
    let bundle = resolve_app_or_exit(arg);
    let app_name = bundle
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("app")
        .to_string();
    let bins = lipo::scan_dir_fat_binaries(&bundle);

    // 计划：可瘦身的文件（目标切片存在）
    let plan: Vec<(
        &std::path::PathBuf,
        &FatFile,
        &apios_core::platform::lipo::FatSlice,
        u64,
    )> = bins
        .iter()
        .filter_map(|(path, fat)| {
            let keep = keep_slice(fat)?;
            let len = std::fs::metadata(path).map(|m| m.len()).unwrap_or(0);
            Some((path, fat, keep, len))
        })
        .collect();
    if plan.is_empty() {
        println!("Nothing to thin in {app_name}.");
        return;
    }

    let total_free: u64 = plan.iter().map(|(_, _, s, len)| len - s.size).sum();
    let target_name = cpu_name(current_cputype(), 0);
    println!(
        "{} universal binary(ies) will be thinned to {}:",
        plan.len(),
        target_name
    );
    const MAX_PATH: usize = 64;
    let width = plan
        .iter()
        .map(|(p, _, _, _)| truncated_rel(p, &bundle, MAX_PATH).chars().count())
        .max()
        .unwrap_or(0);
    for (path, fat, keep, len) in &plan {
        let rel = truncated_rel(path, &bundle, MAX_PATH);
        println!(
            "  {rel:width$}  {} → {} (free {})",
            describe_slices(fat),
            fmt_size(keep.size),
            fmt_size(len.saturating_sub(keep.size)),
            width = width
        );
    }
    println!("\nFreeing {} total.", fmt_size(total_free));
    if sign {
        println!("WARNING: irreversible — binaries are overwritten in place; they will be re-signed ad-hoc.");
    } else {
        println!("WARNING: irreversible — binaries are overwritten in place and code signatures will be invalidated.");
    }
    if !confirm(cli, &format!("Thin {} binary(ies)? ", plan.len())) {
        println!("Aborted — nothing was deleted.");
        return;
    }

    let mut thinned = 0usize;
    let mut freed = 0u64;
    let mut failed = 0usize;
    for (path, _, keep, len) in &plan {
        match lipo::thin_file(path, keep) {
            Ok(_) => {
                thinned += 1;
                freed += len.saturating_sub(keep.size);
            }
            Err(e) => {
                failed += 1;
                eprintln!("apios: failed to thin {}: {e}", path.display());
            }
        }
    }
    // 刷新 bundle 目录 mtime（Finder 立即更新占用大小；失败静默——非关键）
    let _ = lipo::touch_dir(&bundle);
    if failed > 0 {
        eprintln!(
            "apios: failed to thin {failed} of {} binary(ies).",
            plan.len()
        );
        exit(1);
    }
    println!("Thinned {thinned} binary(ies) — freed {}.", fmt_size(freed));

    if sign {
        let mut signed = 0usize;
        let mut sign_failed = 0usize;
        for (path, _, _, _) in &plan {
            match lipo::re_sign(path) {
                Ok(()) => signed += 1,
                Err(e) => {
                    sign_failed += 1;
                    eprintln!("apios: warning: re-sign failed for {}: {e}", path.display());
                }
            }
        }
        println!("Re-signed {signed} binary(ies) (ad-hoc).");
        if sign_failed > 0 {
            eprintln!(
                "apios: warning: {sign_failed} binary(ies) left unsigned — the app may not launch."
            );
        }
    } else {
        println!(
            "Code signatures were invalidated — re-run with `apios lipo thin --sign` to re-sign ad-hoc."
        );
    }
}

fn fmt_size(bytes: u64) -> String {
    apios_core::dev_env::fmt_size(bytes)
}

fn find_orphans() -> Vec<PathBuf> {
    let locations = Locations::new();
    // 平台化发现：macOS/Linux 内部即旧 walk（行为等价），Windows 走注册表+开始菜单
    let apps = apios_core::platform::adapter().discover_installed_apps();
    let mut searcher = ReversePathsSearcher::new(locations, apps);
    searcher.reverse_paths_search_cli()
}

fn cmd_orphan(cli: &Cli) {
    let _ = cli;
    let found = find_orphans();

    for p in &found {
        println!("{}", p.display());
    }
    println!("\nFound {} orphaned files.\n", found.len());
}

fn cmd_clean_orphan(cli: &Cli) {
    let found = find_orphans();

    if found.is_empty() {
        println!("No orphaned files found.");
        return;
    }

    check_protected(&found, "apios clean-orphan");

    println!(
        "{} orphaned files will be moved to {}:",
        found.len(),
        trash_label()
    );
    if !confirm(cli, &format!("Delete {} files? ", found.len())) {
        println!("Aborted — nothing was deleted.");
        return;
    }

    let result = delete_files(&found, Some("Orphaned"));
    if result.success {
        println!(
            "\n{}",
            deleted_message(result.moved.len(), &result.bundle_folder)
        );
        if !result.failed.is_empty() {
            eprintln!(
                "Failed to delete {} files (in use or protected).",
                result.failed.len()
            );
        }
        exit(0);
    } else if result.moved.is_empty() && result.failed.is_empty() {
        // 列表为空或全部被安全校验拦截 → 无事可删，不是错误
        report_blocked(&result.blocked);
        println!("Nothing to delete.");
        exit(0);
    } else {
        eprintln!("\napios: failed to delete orphaned files.");
        exit(1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_arg_is_path() {
        assert!(arg_is_path("/Applications/Foo.app"));
        assert!(arg_is_path("Foo.app"));
        assert!(arg_is_path("~/Downloads/Foo.app"));
        assert!(!arg_is_path("Foo"));
        assert!(!arg_is_path("Microsoft Edge"));
        // Windows 形态（跨平台断言：POSIX 上同样判定为路径，无副作用）
        assert!(arg_is_path(r"C:\Users\foo\Foo.exe"));
        assert!(arg_is_path(r".\Foo.exe"));
        assert!(arg_is_path("C:\\Program Files\\Foo"));
        assert!(!arg_is_path("C:colon-name")); // 非盘符形态（首字符后不是 :）
    }

    #[test]
    fn test_find_app_by_name_not_found() {
        // 不可能存在的应用名 → None
        let folders = default_app_folders(&apios_core::platform::adapter().home());
        assert!(find_app_by_name("nonexistent-app-zzz-xyz", &folders).is_none());
    }

    #[test]
    fn test_resolve_dot_is_current_dir() {
        // "." 的解析入口就是 current_dir（不存在时返回 None）
        let cwd = std::env::current_dir().unwrap();
        assert!(cwd.exists());
    }

    #[test]
    fn test_dir_contents_lists_top_level() {
        let tmp = tempfile::TempDir::new().unwrap();
        std::fs::write(tmp.path().join("a"), b"x").unwrap();
        std::fs::create_dir(tmp.path().join("sub")).unwrap();
        std::fs::write(tmp.path().join("sub/b"), b"x").unwrap();
        let contents = dir_contents(tmp.path());
        assert_eq!(contents.len(), 2); // 仅顶层条目，不递归
    }
}
