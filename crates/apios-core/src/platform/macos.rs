//! macOS 适配器 —— 原 locations.rs / app_info.rs 中的 macOS 专属逻辑
//!
//! - 系统路径：~/Library 布局（原 Locations.swift）+ darwin 缓存目录（getconf）
//! - 元数据：codesign 提取 entitlements / TeamIdentifier
//! - 回收站：~/.Trash

use std::path::{Path, PathBuf};
use std::process::Command;

use super::{AppMetadata, SystemPaths, Trash};

/// macOS 平台适配器
pub struct MacOsAdapter {
    home: String,
    cache_dir: String,
    temp_dir: String,
}

impl MacOsAdapter {
    pub fn new() -> Self {
        let home = std::env::var("HOME").unwrap_or_else(|_| "/Users/".to_string());
        let (cache_dir, temp_dir) = darwin_ct();
        MacOsAdapter {
            home,
            cache_dir,
            temp_dir,
        }
    }
}

/// getconf DARWIN_USER_CACHE_DIR / DARWIN_USER_TEMP_DIR
fn darwin_ct() -> (String, String) {
    let output = Command::new("/bin/bash")
        .args(["-c", "echo $(getconf DARWIN_USER_CACHE_DIR) $(getconf DARWIN_USER_TEMP_DIR)"])
        .output()
        .ok()
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .map(|s| s.trim().to_string());

    if let Some(output) = output {
        let parts: Vec<&str> = output.split(' ').collect();
        if parts.len() >= 2 {
            return (parts[0].trim().to_string(), parts[1].trim().to_string());
        }
    }
    (String::new(), String::new())
}

/// 排除表 + 正则 `\bcom\.apple\b` —— 对应 listAppSupportDirectories 的 exclusions
fn list_app_support_directories(home: &str) -> Vec<String> {
    let app_support = format!("{home}/Library/Application Support");
    let exclusions: std::collections::HashSet<&str> = [
        "MobileSync", ".DS_Store", "Xcode", "SyncServices", "networkserviceproxy", "DiskImages",
        "CallHistoryTransactions", "App Store", "CloudDocs", "icdd", "iCloud", "Instruments",
        "AddressBook", "FaceTime", "AskPermission", "CallHistoryDB",
    ]
    .into_iter()
    .collect();
    let com_apple_regex = regex::Regex::new(r"\bcom\.apple\b").unwrap();

    let mut result = Vec::new();
    if let Ok(entries) = std::fs::read_dir(&app_support) {
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().to_string();
            if !entry.path().is_dir() {
                continue;
            }
            let exclude_by_regex = com_apple_regex.is_match(&name);
            if exclusions.contains(name.as_str()) || exclude_by_regex {
                continue;
            }
            result.push(name);
        }
    }
    result
}

impl SystemPaths for MacOsAdapter {
    fn home(&self) -> String {
        self.home.clone()
    }

    fn user_cache_dir(&self) -> String {
        self.cache_dir.clone()
    }

    fn user_temp_dir(&self) -> String {
        self.temp_dir.clone()
    }

    /// apps.paths（Locations.swift:58-122）
    fn apps_paths(&self) -> Vec<String> {
        let home = &self.home;
        let mut apps_paths = vec![
            format!("{home}"),
            format!("{home}/.config"),
            format!("{home}/Documents"),
            format!("{home}/Desktop"), // for steam game shortcuts
            format!("{home}/Applications"),
            format!("{home}/Library"),
            format!("{home}/Library/Application Scripts"),
            format!("{home}/Library/Application Support"),
            format!("{home}/Library/Application Support/CrashReporter"),
            format!("{home}/Library/Application Support/Steam/steamapps"),
            format!("{home}/Library/Application Support/Steam/steamapps/common"),
            format!(
                "{home}/Library/Application Support/com.apple.sharedfilelist/com.apple.LSSharedFileList.ApplicationRecentDocuments"
            ),
            format!("{home}/Library/Containers"),
            format!("{home}/Library/Caches"),
            format!("{home}/Library/Caches/com.apple.helpd/Generated"),
            format!("{home}/Library/Caches/com.crashlytics"),
            format!("{home}/Library/Caches/com.google.SoftwareUpdate"),
            format!("{home}/Library/Caches/com.google.Keystone"),
            format!("{home}/Library/Caches/org.sparkle-project.Sparkle"),
            format!("{home}/Library/Caches/com.segment.analytics"),
            format!("{home}/Library/Caches/SentryCrash"),
            format!("{home}/Library/Caches/Rollbar"),
            format!("{home}/Library/Caches/Amplitude"),
            format!("{home}/Library/Caches/Realm"),
            format!("{home}/Library/Caches/Parse"),
            format!("{home}/Library/Group Containers"),
            format!("{home}/Library/HTTPStorages"),
            format!("{home}/Library/Internet Plug-Ins"),
            format!("{home}/Library/LaunchAgents"),
            format!("{home}/Library/Logs"),
            format!("{home}/Library/Logs/DiagnosticReports"),
            format!("{home}/Library/Preferences"),
            format!("{home}/Library/PreferencePanes"),
            format!("{home}/Library/Preferences/ByHost"),
            format!("{home}/Library/Saved Application State"),
            format!("{home}/Library/Services"),
            format!("{home}/Library/WebKit"),
            "/Applications".to_string(),
            "/Users/Shared".to_string(),
            "/Users/Library".to_string(),
            "/Users/Shared/Library/Application Support".to_string(),
            "/Library".to_string(),
            "/Library/Application Support".to_string(),
            "/Library/Application Support/CrashReporter".to_string(),
            "/Library/Caches".to_string(),
            "/Library/Extensions".to_string(),
            "/Library/Internet Plug-Ins".to_string(),
            "/Library/LaunchAgents".to_string(),
            "/Library/LaunchDaemons".to_string(),
            "/Library/Logs".to_string(),
            "/Library/Logs/DiagnosticReports".to_string(),
            "/Library/Preferences".to_string(),
            "/Library/PrivilegedHelperTools".to_string(),
            "/private/var/db/receipts".to_string(),
            "/private/tmp".to_string(),
            "/usr/local/bin".to_string(),
            "/usr/local/etc".to_string(),
            "/usr/local/opt".to_string(),
            "/usr/local/sbin".to_string(),
            "/usr/local/share".to_string(),
            "/usr/local/var".to_string(),
            self.cache_dir.clone(),
            self.temp_dir.clone(),
        ];

        // Append Application Support 子目录（深度搜索）
        for folder in list_app_support_directories(home) {
            apps_paths.push(format!("{home}/Library/Application Support/{folder}"));
        }

        apps_paths
    }

    /// reverse.paths（Locations.swift:133-156）
    fn reverse_paths(&self) -> Vec<String> {
        let home = &self.home;
        vec![
            format!("{home}/Library/Application Scripts"),
            format!("{home}/Library/Application Support"),
            format!("{home}/Library/Application Support/Caches"),
            format!(
                "{home}/Library/Application Support/com.apple.sharedfilelist/com.apple.LSSharedFileList.ApplicationRecentDocuments"
            ),
            format!("{home}/Library/Containers"),
            format!("{home}/Library/Caches"),
            format!("{home}/Library/HTTPStorages"),
            format!("{home}/Library/Internet Plug-Ins"),
            format!("{home}/Library/LaunchAgents"),
            format!("{home}/Library/Logs"),
            format!("{home}/Library/Preferences"),
            format!("{home}/Library/PreferencePanes"),
            format!("{home}/Library/Preferences/ByHost"),
            format!("{home}/Library/Saved Application State"),
            format!("{home}/Library/WebKit"),
            "/Users/Shared/Library/Application Support".to_string(),
            "/Library/Application Support".to_string(),
            "/Library/Application Support/CrashReporter".to_string(),
            "/Library/Internet Plug-Ins".to_string(),
            "/Library/LaunchAgents".to_string(),
            "/Library/LaunchDaemons".to_string(),
            "/Library/PrivilegedHelperTools".to_string(),
        ]
    }

    fn app_support_subdirs(&self) -> Vec<String> {
        list_app_support_directories(&self.home)
    }
}

impl AppMetadata for MacOsAdapter {
    /// getEntitlements 移植（AppInfoFetch.swift:691-786）：
    /// application-groups + iCloud 容器标识 + MacOS 二进制名（≥5 字符，排除表）+ 嵌套 .app
    fn entitlements(&self, app_path: &Path) -> Option<Vec<String>> {
        let mut results: Vec<String> = Vec::new();

        // 通过 codesign 提取 entitlements plist（替代 SecCodeCopySigningInformation）。
        // 默认输出 OpenStep 旧式格式（Rust plist crate 不支持），--xml 强制 XML
        let ent = Command::new("codesign")
            .args(["-d", "--entitlements", "-", "--xml", &app_path.to_string_lossy()])
            .output()
            .ok()
            .and_then(|o| {
                // codesign 把 plist 输出到 stdout
                if !o.stdout.is_empty() {
                    plist::from_bytes::<plist::Dictionary>(&o.stdout).ok()
                } else {
                    None
                }
            });
        if let Some(ent) = ent {
            if let Some(groups) = ent
                .get("com.apple.security.application-groups")
                .and_then(|v| v.as_array())
            {
                for g in groups.iter().filter_map(|v| v.as_string()) {
                    results.push(g.to_string());
                }
            }
            if let Some(icloud) = ent
                .get("com.apple.developer.icloud-container-identifiers")
                .and_then(|v| v.as_array())
            {
                for c in icloud.iter().filter_map(|v| v.as_string()) {
                    results.push(c.to_string());
                }
            }
        }

        // 扫描 Contents/MacOS 二进制名（原版：≥5 字符、排除 crashhandler/crash handler/electron、非隐藏）
        let excluded = ["crashhandler", "crash handler", "electron"];
        let macos = app_path.join("Contents/MacOS");
        if macos.is_dir() {
            if let Ok(entries) = std::fs::read_dir(&macos) {
                for e in entries.flatten() {
                    let name = e.file_name().to_string_lossy().to_string();
                    if name.starts_with('.') {
                        continue;
                    }
                    let lower = name.to_lowercase();
                    if name.chars().count() >= 5
                        && !excluded.contains(&lower.as_str())
                        && !results.contains(&name)
                    {
                        results.push(name);
                    }
                }
            }
        }

        // 扫描 Contents/*/ 下嵌套 .app 及其中 MacOS 二进制
        let contents = app_path.join("Contents");
        if contents.is_dir() {
            if let Ok(subdirs) = std::fs::read_dir(&contents) {
                for subdir in subdirs.flatten() {
                    if !subdir.path().is_dir() {
                        continue;
                    }
                    if let Ok(bundles) = std::fs::read_dir(subdir.path()) {
                        for bundle in bundles.flatten() {
                            if bundle.path().extension().is_some_and(|x| x == "app") {
                                let bundle_name = bundle
                                    .path()
                                    .file_stem()
                                    .map(|n| n.to_string_lossy().to_string())
                                    .unwrap_or_default();
                                let bundle_lower = bundle_name.to_lowercase();
                                if bundle_name.chars().count() >= 5
                                    && !excluded.contains(&bundle_lower.as_str())
                                    && !results.contains(&bundle_name)
                                {
                                    results.push(bundle_name.clone());
                                }
                                // 嵌套 bundle 的 MacOS 二进制
                                let nested_macos = bundle.path().join("Contents/MacOS");
                                if let Ok(binaries) = std::fs::read_dir(&nested_macos) {
                                    for binary in binaries.flatten() {
                                        let name =
                                            binary.file_name().to_string_lossy().to_string();
                                        if name.starts_with('.') || name.chars().count() < 5 {
                                            continue;
                                        }
                                        let lower = name.to_lowercase();
                                        if !results.contains(&name)
                                            && !excluded.contains(&lower.as_str())
                                        {
                                            results.push(name);
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }

        if results.is_empty() {
            None
        } else {
            Some(results)
        }
    }

    /// getTeamIdentifier 移植：codesign -d -vvv 的 stderr 中 TeamIdentifier=<ID>
    fn team_identifier(&self, app_path: &Path) -> Option<String> {
        let out = Command::new("codesign")
            .args(["-d", "-vvv", &app_path.to_string_lossy()])
            .output()
            .ok()?;
        let text = String::from_utf8_lossy(&out.stderr).to_string();
        for line in text.lines() {
            if let Some(rest) = line.strip_prefix("TeamIdentifier=") {
                let team = rest.trim();
                if !team.is_empty() {
                    return Some(team.to_string());
                }
            }
        }
        None
    }
}

impl Trash for MacOsAdapter {
    fn trash_dir(&self) -> PathBuf {
        PathBuf::from(format!("{}/.Trash", self.home))
    }
}
