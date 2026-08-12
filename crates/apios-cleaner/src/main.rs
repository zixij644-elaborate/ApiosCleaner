//! apios-cleaner：ApiosCleaner 命令行工具（对照原版 CLI.swift 输出格式）

use std::path::PathBuf;
use std::process::exit;

use clap::{Parser, Subcommand};
use apios_core::app_info::get_app_info;
use apios_core::locations::Locations;
use apios_core::model::Sensitivity;
use apios_core::orphan::ReversePathsSearcher;
use apios_core::scan::{default_app_folders, get_sorted_apps};
use apios_core::search::AppPathFinder;
use apios_core::trash::delete_files;

#[derive(Parser)]
#[command(
    name = "apios-cleaner",
    version,
    about = "Command-line interface for the ApiosCleaner app (Rust rewrite of Pearcleaner)"
)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// List application files available for uninstall at the specified path
    List {
        /// Path to the application
        path: String,
    },
    /// List orphaned files available for removal
    ListOrphaned,
    /// Uninstall only the application bundle at the specified path
    Uninstall {
        /// Path to the application
        path: String,
    },
    /// Uninstall application bundle and ALL related files at the specified path
    UninstallAll {
        /// Path to the application
        path: String,
    },
    /// Remove ALL orphaned files
    RemoveOrphaned,
}

fn main() {
    let cli = Cli::parse();
    match cli.command {
        Command::List { path } => cmd_list(&path),
        Command::ListOrphaned => cmd_list_orphaned(),
        Command::Uninstall { path } => cmd_uninstall(&path),
        Command::UninstallAll { path } => cmd_uninstall_all(&path),
        Command::RemoveOrphaned => cmd_remove_orphaned(),
    }
}

/// 获取 AppInfo，失败时按原版输出错误并退出 1
fn get_app_info_or_exit(path: &str) -> apios_core::model::AppInfo {
    match get_app_info(&PathBuf::from(path)) {
        Some(app) => app,
        None => {
            println!("Error: Invalid path or unable to fetch app info at path: {path}\n");
            exit(1);
        }
    }
}

fn cmd_list(path: &str) {
    let app = get_app_info_or_exit(path);
    let locations = Locations::new();
    // 原版 CLI 无灵敏度参数，用 @AppStorage 默认（strict）
    let mut finder = AppPathFinder::new(&app, &locations, Sensitivity::Strict);
    let found = finder.find_paths_cli();

    for p in &found {
        println!("{}", p.display());
    }
    println!("\nFound {} application files.\n", found.len());
}

fn cmd_list_orphaned() {
    let home = std::env::var("HOME").unwrap_or_default();
    let locations = Locations::new();
    let apps = get_sorted_apps(&default_app_folders(&home));
    let mut searcher = ReversePathsSearcher::new(locations, apps);
    let found = searcher.reverse_paths_search_cli();

    for p in &found {
        println!("{}", p.display());
    }
    println!("\nFound {} orphaned files.\n", found.len());
}

fn cmd_uninstall(path: &str) {
    let app = get_app_info_or_exit(path);

    // 原版先 killApp（PoC 跳过，提示用户手动退出）
    let success = delete_files(&[app.path.clone()], Some(&app.app_name)).success;

    if success {
        println!("Application deleted successfully.\n");
        exit(0);
    } else {
        println!("Failed to delete application.\n");
        exit(1);
    }
}

fn cmd_uninstall_all(path: &str) {
    // 原版 uninstall-all 的错误消息不带尾随 \n（CLI.swift:162）
    let app = match get_app_info(&PathBuf::from(path)) {
        Some(app) => app,
        None => {
            println!("Error: Invalid path or unable to fetch app info at path: {path}");
            exit(1);
        }
    };
    let locations = Locations::new();
    let mut finder = AppPathFinder::new(&app, &locations, Sensitivity::Strict);
    let found = finder.find_paths_cli();

    // 受保护文件检测（CLI.swift:172-184）：不可写且无 helper → 提示 sudo 并列出
    let protected: Vec<PathBuf> = found
        .iter()
        .filter(|p| !apios_core::trash::is_writable(p))
        .cloned()
        .collect();
    if !protected.is_empty() {
        println!("Protected files detected. Please run this command with sudo:\n");
        println!("sudo apios-cleaner uninstall-all {path}");
        println!("\nProtected files:\n");
        for file in &protected {
            println!("{}", file.display());
        }
        exit(1);
    }

    let result = delete_files(&found, Some(&app.app_name));

    if result.success {
        println!("The application and related files have been deleted successfully.\n");
        exit(0);
    } else {
        println!("Failed to delete some files, they might be protected or in use.\n");
        exit(1);
    }
}

fn cmd_remove_orphaned() {
    let home = std::env::var("HOME").unwrap_or_default();
    let locations = Locations::new();
    let apps = get_sorted_apps(&default_app_folders(&home));
    let mut searcher = ReversePathsSearcher::new(locations, apps);
    let found = searcher.reverse_paths_search_cli();

    // 受保护文件检测（CLI.swift:216-224）
    let protected: Vec<PathBuf> = found
        .iter()
        .filter(|p| !apios_core::trash::is_writable(p))
        .cloned()
        .collect();
    if !protected.is_empty() {
        println!("Protected files detected. Please run this command with sudo:\n");
        println!("sudo apios-cleaner remove-orphaned");
        println!("\nProtected files:\n");
        for file in &protected {
            println!("{}", file.display());
        }
        exit(1);
    }

    let result = delete_files(&found, Some("Orphaned"));

    if result.success {
        println!("Orphaned files have been deleted successfully.\n");
        exit(0);
    } else {
        println!("Failed to delete some orphaned files.\n");
        exit(1);
    }
}
