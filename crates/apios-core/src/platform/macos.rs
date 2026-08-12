//! macOS 适配器 —— 原 locations.rs / app_info.rs 中的 macOS 专属逻辑
//!
//! - 系统路径：~/Library 布局（原 Locations.swift）+ darwin 缓存目录（getconf）
//! - 元数据：codesign 提取 entitlements / TeamIdentifier
//! - 回收站：~/.Trash

use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

use super::{AppMetadata, DevEnvPaths, ProcessControl, SpotlightIndex, SystemPaths, Trash};
use crate::app_info;
use crate::dev_env::DevEnv;
use crate::format::pear_format;
use crate::model::{AppInfo, Sensitivity};

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
        .args([
            "-c",
            "echo $(getconf DARWIN_USER_CACHE_DIR) $(getconf DARWIN_USER_TEMP_DIR)",
        ])
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
        "MobileSync",
        ".DS_Store",
        "Xcode",
        "SyncServices",
        "networkserviceproxy",
        "DiskImages",
        "CallHistoryTransactions",
        "App Store",
        "CloudDocs",
        "icdd",
        "iCloud",
        "Instruments",
        "AddressBook",
        "FaceTime",
        "AskPermission",
        "CallHistoryDB",
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
            .args([
                "-d",
                "--entitlements",
                "-",
                "--xml",
                &app_path.to_string_lossy(),
            ])
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
                                        let name = binary.file_name().to_string_lossy().to_string();
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

/// 运行中的进程数（pgrep -x 精确匹配进程名；macOS pgrep 无 -c 计数选项 → 按行数统计，
/// 无匹配时退出码 1 即 0）
fn count_processes(name: &str) -> u32 {
    let Ok(out) = Command::new("pgrep").args(["-x", name]).output() else {
        return 0;
    };
    if !out.status.success() {
        return 0;
    }
    String::from_utf8_lossy(&out.stdout)
        .lines()
        .count()
        .try_into()
        .unwrap_or(0)
}

impl ProcessControl for MacOsAdapter {
    /// killApp 移植（原版 GUI 用 NSRunningApplication terminate）：
    /// 按 CFBundleExecutable（进程名）pgrep 计数 → killall SIGTERM 优雅终止
    fn kill_running_app(&self, app: &AppInfo) -> u32 {
        // 进程名取 CFBundleExecutable（app_name 是显示名，可能 ≠ 可执行文件，
        // 如 "Visual Studio Code" 的可执行文件是 "Electron"）
        let executable = app_info::get_executable_name(&app.path)
            .filter(|e| !e.is_empty())
            .unwrap_or_else(|| app.app_name.clone());

        let count = count_processes(&executable);
        if count > 0 {
            let _ = Command::new("killall").args(["-q", &executable]).status();
            // 给进程退出留时间，降低随后的文件移动失败概率
            std::thread::sleep(Duration::from_millis(200));
        }
        count
    }
}

/// 开发环境路径表（原版 PathLibrary 收紧版，macOS 布局）。
/// 收紧原则（2026-08-12）：只列**可再生缓存**——DerivedData、各 cache 目录、
/// registry 缓存等；移除工具本体（~/.cargo、~/.nvm、conda 发行版、pyenv、gem）、
/// 配置（Application Support 根、User、.config 根）、用户数据（Xcode Archives、
/// CoreSimulator 设备、Android AVD）。
/// 与原版相比移除的环境：Conda（无安全缓存条目）、Ruby Gems（无独立缓存目录）。
fn dev_envs_table() -> Vec<DevEnv> {
    let p = |s: &str| s.to_string();
    vec![
        DevEnv {
            name: "Android Studio".into(),
            paths: vec![
                p("~/Library/Logs/AndroidStudio/"),
                p("~/Library/Caches/Google/AndroidStudio*/"),
            ],
        },
        DevEnv {
            name: "Cargo".into(),
            paths: vec![p("~/.cargo/git/"), p("~/.cargo/registry/")],
        },
        DevEnv {
            name: "Carthage".into(),
            paths: vec![
                p("~/Carthage/"),
                p("~/Library/Caches/org.carthage.CarthageKit/"),
            ],
        },
        DevEnv {
            name: "CocoaPods".into(),
            paths: vec![p("~/Library/Caches/CocoaPods/"), p("~/.cocoapods/repos/")],
        },
        DevEnv {
            name: "Composer".into(),
            paths: vec![p("~/.composer/cache/")],
        },
        DevEnv {
            name: "Cursor".into(),
            paths: vec![
                p("~/Library/Application Support/Cursor/Cache"),
                p("~/Library/Application Support/Cursor/GPUCache"),
                p("~/Library/Application Support/Cursor/CachedConfigurations"),
                p("~/Library/Application Support/Cursor/CachedData"),
                p("~/Library/Application Support/Cursor/CachedExtensionVSIXs"),
                p("~/Library/Application Support/Cursor/CachedExtensions"),
                p("~/Library/Application Support/Cursor/CachedProfilesData"),
                p("~/Library/Application Support/Cursor/Code Cache"),
            ],
        },
        DevEnv {
            name: "Deno".into(),
            paths: vec![p("~/Library/Caches/deno")],
        },
        DevEnv {
            name: "Go Modules".into(),
            paths: vec![p("~/go/pkg/mod/")],
        },
        DevEnv {
            name: "Gradle".into(),
            paths: vec![p("~/.gradle/caches/"), p("~/.gradle/wrapper/")],
        },
        // brew 缓存/日志（可再生，原版 runCleanup 缓存部分；卸载包本体归 `pkg brew`）
        DevEnv {
            name: "Homebrew".into(),
            paths: vec![
                p("~/Library/Caches/Homebrew/"),
                p("~/Library/Logs/Homebrew/"),
            ],
        },
        DevEnv {
            name: "Haskell Stack".into(),
            paths: vec![p("~/.stack/snapshots/")],
        },
        DevEnv {
            name: "IntelliJ IDEA".into(),
            paths: vec![
                p("~/Library/Caches/JetBrains/"),
                p("~/Library/Logs/JetBrains/"),
            ],
        },
        DevEnv {
            name: "Maven".into(),
            paths: vec![p("~/.m2/repository/")],
        },
        DevEnv {
            name: "Nix".into(),
            paths: vec![p("~/.cache/nix/")],
        },
        DevEnv {
            name: "Npm".into(),
            paths: vec![
                p("~/.npm/"),
                p("~/Library/pnpm/store"),
                p("~/.bun/install/cache"),
            ],
        },
        DevEnv {
            name: "Pip".into(),
            paths: vec![p("~/Library/Caches/pip/")],
        },
        DevEnv {
            name: "Poetry".into(),
            paths: vec![p("~/Library/Caches/pypoetry/")],
        },
        DevEnv {
            name: "Pub".into(),
            paths: vec![p("~/.pub-cache/"), p("~/Library/Caches/flutter_engine/")],
        },
        DevEnv {
            name: "Pyenv".into(),
            paths: vec![p("~/.pyenv/cache/")],
        },
        DevEnv {
            name: "Swift".into(),
            paths: vec![p("~/.swiftpm/")],
        },
        DevEnv {
            name: "Uv".into(),
            paths: vec![p("~/.cache/uv/")],
        },
        DevEnv {
            name: "VS Code".into(),
            paths: vec![
                p("~/Library/Application Support/Code/Cache"),
                p("~/Library/Application Support/Code/GPUCache"),
                p("~/Library/Application Support/Code/CachedConfigurations"),
                p("~/Library/Application Support/Code/CachedData"),
                p("~/Library/Application Support/Code/CachedExtensionVSIXs"),
                p("~/Library/Application Support/Code/CachedExtensions"),
                p("~/Library/Application Support/Code/CachedProfilesData"),
                p("~/Library/Application Support/Code/Code Cache"),
            ],
        },
        DevEnv {
            name: "Xcode".into(),
            paths: vec![
                p("~/Library/Caches/com.apple.dt.xcodebuild/"),
                p("~/Library/Caches/com.apple.dt.Xcode.sourcecontrol.Git/"),
                p("~/Library/Developer/DeveloperDiskImages/"),
                p("~/Library/Developer/Xcode/DerivedData/"),
                p("~/Library/Developer/Xcode/DocumentationCache/"),
                p("~/Library/Developer/Xcode/iOS DeviceSupport/"),
                p("~/Library/Developer/Xcode/tvOS DeviceSupport/"),
                p("~/Library/Developer/Xcode/watchOS DeviceSupport/"),
                p("~/Library/Developer/Xcode/macOS DeviceSupport/"),
            ],
        },
        DevEnv {
            name: "Yarn".into(),
            paths: vec![p("~/.cache/yarn/"), p("~/.yarn-cache/")],
        },
        DevEnv {
            name: "Zed".into(),
            paths: vec![
                p("~/Library/Caches/Zed/"),
                p("~/Library/Application Support/Zed/node/cache/"),
            ],
        },
    ]
}

impl DevEnvPaths for MacOsAdapter {
    fn dev_envs(&self) -> Vec<DevEnv> {
        dev_envs_table()
    }
}

/// NSPredicate 字符串转义：单引号 → ''（SQL 风格，Spotlight 查询语法）
fn escape_predicate_value(value: &str) -> String {
    value.replace('\'', "''")
}

/// Strict/Enhanced 谓词（AppPathsFetch.swift:500-510）。
/// Deep 的多元数据组合谓词（Comment/Creator/Copyright/TextContent 等）属 GUI 功能，暂不实现。
fn build_predicate(app_name: &str, bundle_id: &str, sensitivity: Sensitivity) -> Option<String> {
    let name = escape_predicate_value(app_name);
    let bundle = escape_predicate_value(bundle_id);
    match sensitivity {
        Sensitivity::Strict => Some(format!(
            "kMDItemDisplayName == '{name}'cd || kMDItemDisplayName == '{bundle}'cd"
        )),
        Sensitivity::Enhanced => Some(format!(
            "kMDItemDisplayName CONTAINS[cd] '{name}' || kMDItemPath CONTAINS[cd] '{name}' \
             || kMDItemDisplayName CONTAINS[cd] '{bundle}' || kMDItemPath CONTAINS[cd] '{bundle}'"
        )),
        Sensitivity::Deep => None, // TODO: 多词 AND 谓词等（AppPathsFetch.swift:516-577）
    }
}

/// Strict 后过滤（AppPathsFetch.swift:601-607）：
/// 末段组件 pearFormat 后必须等于 appName/bundleID 的 pearFormat
fn strict_post_filter(paths: Vec<PathBuf>, app_name: &str, bundle_id: &str) -> Vec<PathBuf> {
    let name_formatted = pear_format(app_name);
    let bundle_formatted = pear_format(bundle_id);
    paths
        .into_iter()
        .filter(|p| {
            let last = p
                .file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_default();
            let formatted = pear_format(&last);
            formatted == name_formatted || formatted == bundle_formatted
        })
        .collect()
}

impl SpotlightIndex for MacOsAdapter {
    /// mdfind 移植（替代 NSMetadataQuery）：-onlyin 用户主目录 + 谓词，5s 超时，500 条上限
    fn spotlight_supplemental_paths(
        &self,
        app_name: &str,
        bundle_id: &str,
        sensitivity: Sensitivity,
    ) -> Vec<PathBuf> {
        let Some(predicate) = build_predicate(app_name, bundle_id, sensitivity) else {
            return Vec::new();
        };

        // mdfind 毫秒级完成；索引重建时可能挂起 → 原版 5s 超时语义（线程 + recv_timeout）
        let (tx, rx) = std::sync::mpsc::channel();
        let home = self.home.clone();
        std::thread::spawn(move || {
            let output = Command::new("mdfind")
                .args(["-onlyin", &home, &predicate])
                .output();
            let _ = tx.send(output);
        });
        // channel 元素即 Result<Output>；recv_timeout 失败（超时）或命令失败 → 空结果
        let output = rx
            .recv_timeout(Duration::from_secs(5))
            .ok()
            .and_then(|r| r.ok());
        let Some(output) = output else {
            return Vec::new();
        };
        if !output.status.success() {
            return Vec::new();
        }

        let paths: Vec<PathBuf> = String::from_utf8_lossy(&output.stdout)
            .lines()
            .filter(|l| !l.trim().is_empty())
            .map(PathBuf::from)
            .collect();

        // 上限（AppPathsFetch.swift:610-612）
        let paths = if paths.len() > 500 {
            paths.into_iter().take(500).collect()
        } else {
            paths
        };

        // Strict 精确匹配后过滤（Enhanced 无后过滤，原版仅 strict 过滤）
        if sensitivity == Sensitivity::Strict {
            strict_post_filter(paths, app_name, bundle_id)
        } else {
            paths
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_escape_predicate_value() {
        assert_eq!(escape_predicate_value("Microsoft Edge"), "Microsoft Edge");
        assert_eq!(escape_predicate_value("It's"), "It''s");
        assert_eq!(escape_predicate_value("a'b'c"), "a''b''c");
    }

    #[test]
    fn test_build_predicate_strict() {
        let p = build_predicate(
            "Microsoft Edge",
            "com.microsoft.edgemac",
            Sensitivity::Strict,
        )
        .unwrap();
        assert_eq!(
            p,
            "kMDItemDisplayName == 'Microsoft Edge'cd || kMDItemDisplayName == 'com.microsoft.edgemac'cd"
        );
    }

    #[test]
    fn test_build_predicate_enhanced() {
        let p = build_predicate("App", "com.app", Sensitivity::Enhanced).unwrap();
        assert!(p.contains("CONTAINS[cd] 'App'"));
        assert!(p.contains("kMDItemPath"));
    }

    #[test]
    fn test_build_predicate_deep_unsupported() {
        assert!(build_predicate("App", "com.app", Sensitivity::Deep).is_none());
    }

    #[test]
    fn test_build_predicate_escapes_quotes() {
        // 用户可控值（Info.plist 应用名）含单引号 → 转义防谓词语法破坏
        let p = build_predicate("It's App", "com.it.sapp", Sensitivity::Strict).unwrap();
        assert_eq!(
            p,
            "kMDItemDisplayName == 'It''s App'cd || kMDItemDisplayName == 'com.it.sapp'cd"
        );
    }

    #[test]
    fn test_dev_envs_tightened() {
        // 收紧验证：不列工具本体 / 系统包存储 / 配置 / 用户数据
        let envs = dev_envs_table();
        assert_eq!(envs.len(), 25); // 原版 26 环境，移除 Conda 与 Ruby Gems；+Homebrew 缓存（2026-08-12）
        let all: Vec<&str> = envs
            .iter()
            .flat_map(|e| e.paths.iter().map(|p| p.as_str()))
            .collect();
        // 完全不出现的路径（前缀禁止）
        for forbidden in [
            "/nix/store/",
            "anaconda3/",
            "miniconda3/",
            "~/go/bin/",
            "~/.gem/",
            "Archives/",
            "CoreSimulator/",
            "~/.android/",
            "~/Library/Application Support/JetBrains/", // IDE 配置
            "~/Library/Application Support/Google/AndroidStudio", // IDE 配置
        ] {
            assert!(
                !all.iter().any(|p| p.starts_with(forbidden)),
                "路径表不应包含路径 {forbidden}"
            );
        }
        // 工具本体根条目不得出现（其下缓存子路径合法：git/registry/cache）
        for root in ["~/.cargo/", "~/.nvm/", "~/.pyenv/", "~/.conda/"] {
            assert!(
                !all.iter().any(|p| p == &root),
                "路径表不应包含工具本体根目录 {root}"
            );
        }
        // 配置根（Application Support 根、User、.config 根）不得出现
        assert!(!all.iter().any(|p| p.ends_with("Support/Cursor/")));
        assert!(!all.iter().any(|p| p.ends_with("Support/Code/")));
        assert!(!all.iter().any(|p| p.ends_with("/User")));
        // 核心目标必须保留
        assert!(all.iter().any(|p| p.contains("DerivedData")));
        assert!(all.iter().any(|p| p.contains("gradle/caches")));
        assert!(all.iter().any(|p| p.contains("registry")));
    }

    #[test]
    fn test_count_processes_none_running() {
        // 不存在的进程名 → 0（不误杀任何进程）
        assert_eq!(count_processes("nonexistent-proc-zzz-xyz"), 0);
    }

    #[test]
    fn test_strict_post_filter() {
        let paths = vec![
            PathBuf::from("/Users/u/Projects/Pearcleaner"), // 末段 == appName pearFormat ✅
            PathBuf::from("/Users/u/Library/Preferences/com.alienator88.Pearcleaner.plist"),
            // ↑ 末段带扩展名，pearFormat 后 != 两者，严格模式应剔除（原版语义）
            PathBuf::from("/Users/u/Library/WebKit/com.alienator88.Pearcleaner"), // 末段 == bundleID ✅
            PathBuf::from("/Users/u/Library/Group Containers/UBF8T346G9.com.microsoft.oneauth"), // 无关
        ];
        let filtered = strict_post_filter(paths, "Pearcleaner", "com.alienator88.Pearcleaner");
        assert_eq!(filtered.len(), 2);
        assert!(filtered[0].ends_with("Projects/Pearcleaner"));
        assert!(filtered[1].ends_with("WebKit/com.alienator88.Pearcleaner"));
    }
}
