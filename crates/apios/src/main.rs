//! apios：ApiosCleaner 命令行工具（命令参数式）
//!
//! 用法：
//!   apios list <app>         列出应用相关文件（只读，不删除）
//!   apios uninstall <app>    卸载：应用本体 + 全部相关文件 → 回收站（交互确认）
//!   apios orphan             列出孤儿文件（只读，不删除）
//!   apios clean-orphan       删除全部孤儿文件（交互确认）
//!   apios dev-clean [env]    列出开发环境缓存；带 <env> 则清理（交互确认）
//!   apios pkg <pm> <action>  包管理器：卸载包本体 + 依赖处理（brew 为当前实现）
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
use apios_core::pkg::{detect_kind, PkgKind};
use apios_core::platform::{PackageManager, PackageManagers, ProcessControl};
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
    /// Manage packages installed via a package manager (e.g. Homebrew)
    Pkg {
        /// Package manager selector, e.g. "brew"
        pm: String,
        #[command(subcommand)]
        action: PkgAction,
    },
}

#[derive(Subcommand)]
enum PkgAction {
    /// List installed packages (formulae and casks)
    List,
    /// Uninstall one package (formula or cask; type auto-detected)
    Uninstall {
        /// Package name as installed, e.g. "git" or "firefox"
        name: String,
        /// Casks only: also remove user config and preferences (irreversible;
        /// asks for extra confirmation; skipped with -y)
        #[arg(long)]
        zap: bool,
    },
    /// Remove orphaned dependencies (dry-run is shown first; asks for confirmation)
    Autoremove,
}

fn main() {
    let cli = Cli::parse();
    match cli.command {
        Command::List { ref app } => cmd_list(&cli, app),
        Command::Uninstall { ref app } => cmd_uninstall(&cli, app),
        Command::Orphan => cmd_orphan(&cli),
        Command::CleanOrphan => cmd_clean_orphan(&cli),
        Command::DevClean { ref env } => cmd_dev_clean(&cli, env.as_deref()),
        Command::Pkg { ref pm, ref action } => cmd_pkg(&cli, pm, action),
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

    // 7. 卸载后孤儿依赖提示（brew autoremove -n 预演）
    match pm.autoremove_dry_run() {
        Ok(orphans) if !orphans.is_empty() => {
            println!("\n{} orphaned package(s) detected:", orphans.len());
            for o in &orphans {
                println!("  {o}");
            }
            if confirm(cli, &format!("Autoremove {} package(s)? ", orphans.len())) {
                if let Err(e) = pm.autoremove() {
                    eprintln!("apios: {}: {e}", pm.name());
                    exit(1);
                }
                println!("Autoremoved {} package(s).", orphans.len());
            } else {
                println!(
                    "Hint: run `apios pkg {} autoremove` to remove them.",
                    pm.name()
                );
            }
        }
        Ok(_) => {}
        Err(e) => {
            eprintln!("apios: {}: {e}", pm.name());
            exit(1);
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
