//! 搜索路径表 —— 忠实移植原版 `Locations`（old/Pearcleaner/Logic/Locations.swift）
//! 以及 `darwinCT()` / `listAppSupportDirectories()`（Logic.swift:353-421）

use std::collections::HashSet;
use std::path::Path;
use std::process::Command;

use crate::format::pear_format;

/// 标准 macOS Library 子目录 —— 深度2匹配时判断是否回退到父目录（供应商目录规则）
pub const STANDARD_LIBRARY_SUBDIRECTORIES: &[&str] = &[
    "Application Scripts",
    "Application Support",
    "Caches",
    "Containers",
    "Group Containers",
    "HTTPStorages",
    "Internet Plug-Ins",
    "LaunchAgents",
    "LaunchDaemons",
    "Logs",
    "Preferences",
    "PreferencePanes",
    "PrivilegedHelperTools",
    "Saved Application State",
    "Services",
    "WebKit",
    "Extensions",
    "Frameworks",
];

pub fn standard_library_subdirectories() -> HashSet<String> {
    STANDARD_LIBRARY_SUBDIRECTORIES
        .iter()
        .map(|s| s.to_string())
        .collect()
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
    let exclusions: HashSet<&str> = [
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

/// 搜索位置集合（apps.paths + reverse.paths，PoC 阶段插件路径暂不导出）
pub struct Locations {
    pub home: String,
    pub cache_dir: String,
    pub temp_dir: String,
    pub apps_paths: Vec<String>,
    pub reverse_paths: Vec<String>,
}

impl Locations {
    pub fn new() -> Locations {
        let home = std::env::var("HOME").unwrap_or_else(|_| "/Users/".to_string());
        let (cache_dir, temp_dir) = darwin_ct();

        // ---- apps.paths（Locations.swift:58-122）----
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
            cache_dir.clone(),
            temp_dir.clone(),
        ];

        // Append Application Support 子目录（深度搜索）
        for folder in list_app_support_directories(&home) {
            apps_paths.push(format!("{home}/Library/Application Support/{folder}"));
        }

        // ---- reverse.paths（Locations.swift:133-156）----
        let reverse_paths = vec![
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
        ];

        Locations {
            home,
            cache_dir,
            temp_dir,
            apps_paths,
            reverse_paths,
        }
    }
}

/// 路径存在性判断（原版 Condition.init 中 FileManager.fileExists）
pub fn existing(path: &str) -> bool {
    Path::new(path).exists()
}

/// 为测试提供构造入口
#[allow(dead_code)]
fn _assert_tables_unique() {
    // 与标准目录表做交叉检查时使用
    let _ = pear_format("placeholder");
}
