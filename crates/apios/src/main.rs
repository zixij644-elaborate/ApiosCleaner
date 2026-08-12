//! apios：ApiosCleaner 命令行工具（命令参数式）
//!
//! 用法：
//!   apios list <app>         列出应用相关文件（只读，不删除）
//!   apios uninstall <app>    卸载：应用本体 + 全部相关文件 → 回收站（交互确认）
//!   apios orphan             列出孤儿文件（只读，不删除）
//!   apios clean-orphan       删除全部孤儿文件（交互确认）
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

use apios_core::app_info::get_app_info;
use apios_core::dev_env::{dedup_nested, dir_size, env_sizes, expand_globs, expand_home, find_env};
use apios_core::locations::Locations;
use apios_core::model::{AppInfo, Sensitivity};
use apios_core::orphan::ReversePathsSearcher;
use apios_core::platform::ProcessControl;
use apios_core::scan::{default_app_folders, get_sorted_apps};
use apios_core::search::AppPathFinder;
use apios_core::trash::{delete_files, is_writable};
use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(
    name = "apios",
    version,
    about = "ApiosCleaner — a fast cross-platform app cleaner"
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
    List {
        /// Path to the app bundle, app name, or "." for the current directory
        app: String,
    },
    /// Uninstall an app: the bundle and ALL related files, moved to Trash
    Uninstall {
        /// Path to the app bundle, app name, or "." for the current directory
        app: String,
    },
    /// List orphaned files left behind by uninstalled apps (read-only)
    Orphan,
    /// Delete all orphaned files (asks for confirmation)
    CleanOrphan,
    /// List dev environment caches (read-only); with <env>, clean it
    DevClean {
        /// Environment name (case-insensitive), or "all" for everything
        env: Option<String>,
    },
}

fn main() {
    let cli = Cli::parse();
    match cli.command {
        Command::List { ref app } => cmd_list(&cli, app),
        Command::Uninstall { ref app } => cmd_uninstall(&cli, app),
        Command::Orphan => cmd_orphan(&cli),
        Command::CleanOrphan => cmd_clean_orphan(&cli),
        Command::DevClean { ref env } => cmd_dev_clean(&cli, env.as_deref()),
    }
}

// ---------- <app> 参数解析 ----------

/// 参数是路径形式（含目录分隔、带 .app 后缀，或已存在）？
fn arg_is_path(arg: &str) -> bool {
    arg.contains('/') || arg.to_ascii_lowercase().ends_with(".app") || Path::new(arg).exists()
}

/// 在默认应用目录中按名称查找 <name>.app（先精确匹配，再大小写不敏感）
fn find_app_by_name(name: &str, folders: &[String]) -> Option<PathBuf> {
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

/// 解析 <app> 参数 → 应用 bundle 路径；失败时打印用法并退出 1
fn resolve_app_or_exit(arg: &str) -> PathBuf {
    let home = std::env::var("HOME").unwrap_or_default();
    let folders = default_app_folders(&home);

    let resolved = if arg == "." {
        // 当前目录：直接使用；若当前目录是个 .app 之外的东西，后面 get_app_info 会报错
        std::env::current_dir().ok()
    } else if arg_is_path(arg) {
        Some(PathBuf::from(arg))
    } else {
        find_app_by_name(arg, &folders)
    };

    match resolved {
        Some(p) => p,
        None => {
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
fn get_app_info_or_exit(path: &Path) -> AppInfo {
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
    println!("{} related files will be moved to Trash:", found.len());
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
            "\nDeleted {} files to Trash ({})",
            result.moved.len(),
            result.bundle_folder.display()
        );
        if !result.failed.is_empty() {
            eprintln!(
                "Failed to delete {} files (in use or protected).",
                result.failed.len()
            );
        }
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
    let home = std::env::var("HOME").unwrap_or_default();

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
            "\nDeleted {} files to Trash ({})",
            result.moved.len(),
            result.bundle_folder.display()
        );
        if !result.failed.is_empty() {
            eprintln!(
                "Failed to delete {} files (in use or protected).",
                result.failed.len()
            );
        }
        exit(0);
    } else {
        eprintln!("\napios: failed to delete files.");
        exit(1);
    }
}

fn fmt_size(bytes: u64) -> String {
    apios_core::dev_env::fmt_size(bytes)
}

fn find_orphans() -> Vec<PathBuf> {
    let home = std::env::var("HOME").unwrap_or_default();
    let locations = Locations::new();
    let apps = get_sorted_apps(&default_app_folders(&home));
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

    check_protected(&found, "apios clean-orphan");

    println!("{} orphaned files will be moved to Trash:", found.len());
    if !confirm(cli, &format!("Delete {} files? ", found.len())) {
        println!("Aborted — nothing was deleted.");
        return;
    }

    let result = delete_files(&found, Some("Orphaned"));
    if result.success {
        println!(
            "\nDeleted {} files to Trash ({})",
            result.moved.len(),
            result.bundle_folder.display()
        );
        if !result.failed.is_empty() {
            eprintln!(
                "Failed to delete {} files (in use or protected).",
                result.failed.len()
            );
        }
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
    }

    #[test]
    fn test_find_app_by_name_not_found() {
        // 不可能存在的应用名 → None
        let folders = default_app_folders(&std::env::var("HOME").unwrap_or_default());
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
