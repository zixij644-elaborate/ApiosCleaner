//! 已安装应用发现 —— 移植原版 `getSortedApps`（Logic.swift:133-350）的 PoC 子集
//! 用于孤儿搜索的关联判断：遍历应用文件夹，收集 bundle ID / 应用名 / entitlements

use std::path::Path;

use rayon::prelude::*;

use crate::app_info;
use crate::model::AppInfo;

/// 清理器自身 bundle id（原版 Pearcleaner；重写后 GUI 化时替换为 ApiosCleaner 实际 id）
const SELF_BUNDLE_ID: &str = "com.alienator88.Pearcleaner";

/// 默认应用扫描文件夹（原版 FolderSettingsManager 默认值）。
/// macOS：.app bundle 目录三件套；其他平台：XDG 形态（Linux 应用为 .desktop
/// 文件，desktop 解析属适配器 TODO —— 这里目录先取对，避免误导性查找路径）。
pub fn default_app_folders(home: &str) -> Vec<String> {
    #[cfg(target_os = "macos")]
    {
        vec![
            format!("{home}/Applications"),
            "/Applications".to_string(),
            "/Users/Shared/Applications".to_string(),
        ]
    }
    #[cfg(not(target_os = "macos"))]
    {
        vec![
            format!("{home}/Applications"),
            format!("{home}/.local/share/applications"),
            "/usr/local/share/applications".to_string(),
            "/usr/share/applications".to_string(),
            format!("{home}/.local/share/flatpak/exports/share/applications"),
            format!("{home}/.var/app"),
        ]
    }
}

/// 受限应用（原版 isRestricted：Safari / self / /Applications/Utilities）
fn is_restricted(path: &Path, bundle_id: &str) -> bool {
    if bundle_id == "com.apple.Safari" {
        return true;
    }
    // 本应用自身（原版对比 AppState 路径；CLI 尚无 bundle id，先按原名排除，
    // 未来 GUI 化时替换为实际 id —— 防止把清理器自己扫进孤儿列表）
    if bundle_id == SELF_BUNDLE_ID {
        return true;
    }
    path.to_string_lossy()
        .starts_with("/Applications/Utilities/")
}

/// 发现已安装应用（并行 walk；跳过受限应用，纳入符号链接 .app 并去重）。
/// 每个 app 的 Info.plist + codesign 解析相互独立 → rayon 并行显著提速孤儿扫描。
pub fn get_sorted_apps(paths: &[String]) -> Vec<AppInfo> {
    paths
        .par_iter()
        .flat_map(|folder| walk_apps(Path::new(folder)))
        .collect()
}

/// 在发现的应用列表中按路径查找（Windows 用：无 Info.plist，AppInfo 来自
/// 发现结果）。路径大小写不敏感，同时匹配 exe（DisplayIcon）与安装目录
/// （InstallLocation）两种形态 —— 任一为对方的路径前缀即命中。
pub fn find_app_by_path(path: &Path, apps: &[AppInfo]) -> Option<AppInfo> {
    let target = path.to_string_lossy().to_lowercase();
    apps.iter()
        .find(|a| {
            let p = a.path.to_string_lossy().to_lowercase();
            p == target || target.starts_with(&p) || p.starts_with(&target)
        })
        .cloned()
}

/// 在单个文件夹（深度 1）中查找 *.app 目录并解析
fn walk_apps(folder: &Path) -> Vec<AppInfo> {
    let mut result = Vec::new();
    let Ok(entries) = std::fs::read_dir(folder) else {
        return result;
    };
    // 符号链接 .app（brew cask 安装到 /Applications 的链接）必须纳入已装集合，
    // 否则其数据会被孤儿扫描误判为残留并删除。is_dir() 跟随链接（断链/自环 → false）。
    // 同一真实 bundle 可能经多个链接/目录重复出现 → 按 canonical 目标去重。
    let mut seen: std::collections::HashSet<std::path::PathBuf> = std::collections::HashSet::new();
    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_dir() || !path.extension().is_some_and(|e| e == "app") {
            continue;
        }
        let canonical = std::fs::canonicalize(&path).unwrap_or_else(|_| path.clone());
        if !seen.insert(canonical) {
            continue;
        }
        if let Some(app) = app_info::get_app_info(&path) {
            if !is_restricted(&path, &app.bundle_identifier) {
                result.push(app);
            }
        }
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::platform::SystemPaths;

    #[test]
    fn test_default_folders() {
        let home = crate::platform::adapter().home();
        let folders = default_app_folders(&home);
        assert!(!folders.is_empty());
        // 首项恒为 {home}/Applications（macOS 与 XDG 形态一致）
        assert!(folders[0].ends_with("/Applications"));
    }
}
