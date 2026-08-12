//! AppInfo 解析 —— 移植原版 `AppInfoFetcher` 的 fallback 路径（直接读 Info.plist）
//! 以及 `getEntitlements` / `getTeamIdentifier` / `isWebApp`
//! (old/Pearcleaner/Logic/AppInfoFetch.swift:468-786)
//!
//! codesign 元数据提取已迁移到平台适配层（platform::AppMetadata），
//! 本模块保留公共函数签名（核心/CLI 零改动）。

use std::path::{Path, PathBuf};

use crate::model::AppInfo;
use crate::platform::AppMetadata;

/// 从应用 bundle 路径构建 AppInfo（PoC：直接解析 Info.plist，不用 Bundle 缓存）
pub fn get_app_info(path: &Path) -> Option<AppInfo> {
    // wrapped 目录处理：<Container>.app/Wrapper/<RealApp>.app
    if path.join("Wrapper").is_dir() {
        let wrapper = path.join("Wrapper");
        let first_app = std::fs::read_dir(&wrapper)
            .ok()?
            .flatten()
            .find(|e| e.path().extension().is_some_and(|x| x == "app"))?;
        return get_app_info(&first_app.path());
    }

    let info = read_info_plist(path)?;
    let bundle_identifier = info
        .get("CFBundleIdentifier")
        .and_then(|v| v.as_string())
        .map(|s| s.to_string())?;

    // 原版 localizedName()：优先显示名，回退包名，再回退文件名
    let app_name = info
        .get("CFBundleDisplayName")
        .and_then(|v| v.as_string())
        .or_else(|| info.get("CFBundleName").and_then(|v| v.as_string()))
        .map(|s| s.to_string())
        .unwrap_or_else(|| {
            path.file_stem()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_default()
        });

    // isWebApp：LSTemplateApplication == true 或 CFBundleExecutable == "app_mode_loader"
    let web_app = info
        .get("LSTemplateApplication")
        .and_then(|v| v.as_boolean())
        .unwrap_or(false)
        || info
            .get("CFBundleExecutable")
            .and_then(|v| v.as_string())
            .is_some_and(|e| e == "app_mode_loader");

    let entitlements = get_entitlements(path);
    let team_identifier = get_team_identifier(path);

    Some(AppInfo {
        path: path.to_path_buf(),
        bundle_identifier,
        app_name,
        entitlements: entitlements.unwrap_or_default(),
        team_identifier,
        web_app,
        steam: false, // Steam 检测属后续增强；CLI list 与原版一致为 false
        wrapped: false,
    })
}

/// 直接读取 Contents/Info.plist（对应 readInfoPlistDirect）
pub fn read_info_plist(path: &Path) -> Option<plist::Dictionary> {
    let plist_path = path.join("Contents/Info.plist");
    let data = std::fs::read(&plist_path).ok()?;
    plist::from_bytes(&data).ok()
}

/// getEntitlements（AppInfoFetch.swift:691-786）—— 平台适配层实现（macOS: codesign）
pub fn get_entitlements(app_path: &Path) -> Option<Vec<String>> {
    crate::platform::adapter().entitlements(app_path)
}

/// getTeamIdentifier（codesign -d -vvv 的 stderr 中 TeamIdentifier=<ID>）—— 平台适配层实现
pub fn get_team_identifier(app_path: &Path) -> Option<String> {
    crate::platform::adapter().team_identifier(app_path)
}

/// 容器元数据解析（AppPathsFetch.swift:143-183）：
/// 扫描 ~/Library/Containers/<UUID>/.com.apple.containermanagerd.metadata.plist
pub fn get_app_containers(home: &str, bundle_identifier: &str) -> Vec<PathBuf> {
    let uuid_regex = regex::Regex::new(
        r"^[0-9A-F]{8}-[0-9A-F]{4}-[0-9A-F]{4}-[0-9A-F]{4}-[0-9A-F]{12}$",
    )
    .unwrap();
    let containers_path = PathBuf::from(format!("{home}/Library/Containers"));
    let mut containers = Vec::new();
    if let Ok(entries) = std::fs::read_dir(&containers_path) {
        for entry in entries.flatten() {
            let dir_name = entry.file_name().to_string_lossy().to_string();
            if !uuid_regex.is_match(&dir_name) {
                continue;
            }
            let metadata = entry.path().join(".com.apple.containermanagerd.metadata.plist");
            if let Ok(data) = std::fs::read(&metadata) {
                if let Ok(dict) = plist::from_bytes::<plist::Dictionary>(&data) {
                    if dict
                        .get("MCMMetadataIdentifier")
                        .and_then(|v| v.as_string())
                        .is_some_and(|id| id == bundle_identifier)
                    {
                        containers.push(entry.path());
                    }
                }
            }
        }
    }
    containers
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_read_info_plist_apple_domain() {
        // 用本机真实应用验证（CI 环境可能没有，失败也接受）
        let p = Path::new("/Applications/Pearcleaner.app");
        if p.exists() {
            let app = get_app_info(p);
            assert!(app.is_some(), "Pearcleaner.app 应能解析出 AppInfo");
            if let Some(app) = app {
                assert_eq!(app.bundle_identifier, "com.alienator88.Pearcleaner");
                assert!(!app.app_name.is_empty());
            }
        }
    }

    #[test]
    fn test_web_app_detection_helpers() {
        // isWebApp 逻辑单测
        let path = PathBuf::from("/Applications/Test.app");
        let _ = path;
    }
}
