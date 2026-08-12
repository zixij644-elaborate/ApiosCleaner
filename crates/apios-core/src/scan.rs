//! 已安装应用发现 —— 移植原版 `getSortedApps`（Logic.swift:133-350）的 PoC 子集
//! 用于孤儿搜索的关联判断：遍历应用文件夹，收集 bundle ID / 应用名 / entitlements

use std::path::Path;

use rayon::prelude::*;

use crate::app_info;
use crate::model::AppInfo;

/// 默认应用扫描文件夹（原版 FolderSettingsManager 默认值）
pub fn default_app_folders(home: &str) -> Vec<String> {
    vec![
        format!("{home}/Applications"),
        "/Applications".to_string(),
        "/Users/Shared/Applications".to_string(),
    ]
}

/// 受限应用（原版 isRestricted：Safari / self / /Applications/Utilities）
fn is_restricted(path: &Path, bundle_id: &str) -> bool {
    if bundle_id == "com.apple.Safari" {
        return true;
    }
    // 本应用自身（原版对比 AppState 路径）
    if bundle_id == "com.alienator88.Pearcleaner" {
        return true;
    }
    path.to_string_lossy().starts_with("/Applications/Utilities/")
}

/// 发现已安装应用（并行 walk；跳过符号链接与受限应用）。
/// 每个 app 的 Info.plist + codesign 解析相互独立 → rayon 并行显著提速孤儿扫描。
pub fn get_sorted_apps(paths: &[String]) -> Vec<AppInfo> {
    paths
        .par_iter()
        .flat_map(|folder| walk_apps(Path::new(folder)))
        .collect()
}

/// 在单个文件夹（深度 1）中查找 *.app 目录并解析
fn walk_apps(folder: &Path) -> Vec<AppInfo> {
    let mut result = Vec::new();
    let Ok(entries) = std::fs::read_dir(folder) else {
        return result;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let is_symlink = std::fs::symlink_metadata(&path)
            .map(|m| m.file_type().is_symlink())
            .unwrap_or(false);
        if is_symlink {
            continue;
        }
        if path.is_dir() && path.extension().is_some_and(|e| e == "app") {
            if let Some(app) = app_info::get_app_info(&path) {
                if !is_restricted(&path, &app.bundle_identifier) {
                    result.push(app);
                }
            }
        }
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_folders() {
        let home = std::env::var("HOME").unwrap();
        let folders = default_app_folders(&home);
        assert_eq!(folders.len(), 3);
        assert!(folders[1].ends_with("/Applications"));
    }
}
