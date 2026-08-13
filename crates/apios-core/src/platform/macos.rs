//! macOS 适配器 —— 原 locations.rs / app_info.rs 中的 macOS 专属逻辑
//!
//! - 系统路径：~/Library 布局+ darwin 缓存目录（getconf）
//! - 元数据：codesign 提取 entitlements / TeamIdentifier
//! - 回收站：~/.Trash

use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

use super::{
    AppDiscovery, AppMetadata, DevEnvPaths, PluginPaths, ProcessControl, SpotlightIndex,
    SystemPaths, Trash,
};
use crate::dev_env::DevEnv;
use crate::format::pear_format;
use crate::model::{AppInfo, Sensitivity};
use crate::plugin::PluginCategory;

/// macOS 平台适配器
pub struct MacOsAdapter {
    home: String,
    cache_dir: String,
    temp_dir: String,
}

impl MacOsAdapter {
    pub fn new() -> Self {
        let home = crate::platform::normalize_home(
            &std::env::var("HOME").unwrap_or_else(|_| "/Users/".to_string()),
        );
        let (cache_dir, temp_dir) = darwin_ct();
        MacOsAdapter {
            home,
            cache_dir,
            temp_dir,
        }
    }
}

/// getconf DARWIN_USER_CACHE_DIR / DARWIN_USER_TEMP_DIR
/// darwin 用户缓存/临时目录（getconf DARWIN_USER_*）。
/// 直接调 /usr/bin/getconf（不经 shell 包装，避免转义与拼接问题）。
fn darwin_ct() -> (String, String) {
    (
        getconf("DARWIN_USER_CACHE_DIR"),
        getconf("DARWIN_USER_TEMP_DIR"),
    )
}

fn getconf(key: &str) -> String {
    Command::new("/usr/bin/getconf")
        .arg(key)
        .output()
        .ok()
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .map(|s| s.trim().to_string())
        .unwrap_or_default()
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

    /// apps.paths
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

        // 过滤空条目（cache_dir/temp_dir 获取失败时为空字符串 → read_dir("") 白跑）
        apps_paths.retain(|p| !p.is_empty());
        apps_paths
    }

    /// reverse.paths
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

    fn critical_paths(&self) -> Vec<String> {
        [
            "/Applications",
            "/Library",
            "/System",
            "/usr",
            "/bin",
            "/sbin",
            "/etc",
            "/var",
            "/private",
            "/opt",
            "/Users",
            "/Users/Shared",
        ]
        .into_iter()
        .map(String::from)
        .collect()
    }
}

impl AppMetadata for MacOsAdapter {
    /// entitlements 提取：
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

    /// 团队标识符提取：codesign -d -vvv 的 stderr 中 TeamIdentifier=<ID>
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

/// 解析 `ps -axo pid=,args=` 输出，返回 argv[0] 以 bundle_prefix 开头的进程 PID。
/// 纯函数（fixture 单测）：bundle 前缀匹配覆盖主进程与 Helper 子进程
/// （argv[0] 都是 bundle 内路径），排除同名无关应用（多个 Electron 系应用互不干扰）。
fn parse_ps_bundle_pids(output: &str, bundle_prefix: &str) -> Vec<u32> {
    let mut pids = Vec::new();
    for line in output.lines() {
        let mut fields = line.trim_start().splitn(2, char::is_whitespace);
        let (Some(pid), Some(args)) = (fields.next(), fields.next()) else {
            continue;
        };
        let Ok(pid) = pid.parse::<u32>() else {
            continue;
        };
        // args 首 token 是 argv[0]（可执行路径）；bundle 前缀匹配
        if args.starts_with(bundle_prefix) {
            pids.push(pid);
        }
    }
    pids
}

/// 运行中的 bundle 进程列表（ps 遍历；ps 失败/退出码非零 → 空）
fn running_bundle_pids(bundle_prefix: &str) -> Vec<u32> {
    let Ok(out) = Command::new("ps").args(["-axo", "pid=,args="]).output() else {
        return Vec::new();
    };
    if !out.status.success() {
        return Vec::new();
    }
    parse_ps_bundle_pids(&String::from_utf8_lossy(&out.stdout), bundle_prefix)
}

impl ProcessControl for MacOsAdapter {
    /// 终止运行中的应用（NSRunningApplication terminate 语义的 CLI 实现）。
    /// 按 **bundle 路径前缀**限定进程（ps argv[0] 匹配），逐个 kill -TERM 优雅终止；
    /// 返回实际终止数（kill 后复查存活，复查失败按 0 计）。
    ///
    /// 相对朴素实现修了两处缺陷：
    /// - `killall <可执行名>` 会连带终止所有同名进程（VS Code/Slack/Discord 的
    ///   可执行名都是 "Electron"）——bundle 前缀只命中本 app 的进程；
    /// - `pgrep -x` 受 macOS 进程名 15 字符截断限制（长可执行名永远匹配不到）——
    ///   argv[0] 完整路径匹配不受此限。
    fn kill_running_app(&self, app: &AppInfo) -> u32 {
        let prefix = format!("{}/", app.path.to_string_lossy());
        let running = running_bundle_pids(&prefix);
        if running.is_empty() {
            return 0;
        }
        for pid in &running {
            let _ = Command::new("kill")
                .args(["-TERM", &pid.to_string()])
                .status();
        }
        // 给进程退出留时间，降低随后的文件移动失败概率
        std::thread::sleep(Duration::from_millis(200));
        let still = running_bundle_pids(&prefix);
        let survived = still.iter().filter(|p| running.contains(p)).count() as u32;
        (running.len() as u32).saturating_sub(survived)
    }
}

/// 开发环境路径表（收紧版，macOS 布局）。
/// 收紧原则（2026-08-12）：只列**可再生缓存**——DerivedData、各 cache 目录、
/// registry 缓存等；移除工具本体（~/.cargo、~/.nvm、conda 发行版、pyenv、gem）、
/// 配置（Application Support 根、User、.config 根）、用户数据（Xcode Archives、
/// CoreSimulator 设备、Android AVD）。
/// 移除的环境：Conda（无安全缓存条目）、Ruby Gems（无独立缓存目录）。
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

/// 插件分类路径表（18 类）。
/// `~` 在 `plugin_categories` 中展开为 home；扫描只列一层目录（原版语义）。
fn plugin_categories_table(home: &str) -> Vec<PluginCategory> {
    let p = |s: &str| s.to_string();
    vec![
        PluginCategory {
            name: "Audio".into(),
            paths: vec![
                p("~/Library/Audio/Plug-Ins/Components"),
                p("~/Library/Audio/Plug-Ins/HAL"),
                p("~/Library/Audio/Plug-Ins/MAS"),
                p("~/Library/Audio/Plug-Ins/VST"),
                p("~/Library/Audio/Plug-Ins/VST3"),
                p("~/Library/Audio/Plug-Ins/CLAP"),
                p("/Library/Audio/Plug-Ins/HAL"),
                p("/Library/Audio/Plug-Ins/VST"),
                p("/Library/Audio/Plug-Ins/VST3"),
                p("/Library/Audio/Plug-Ins/CLAP"),
                p("/Library/Audio/Plug-Ins/Components"),
                p("/Library/Application Support/Avid/Audio/Plug-Ins"),
                p("/Library/Application Support/Digidesign/Plug-Ins"),
            ],
        },
        PluginCategory {
            name: "PreferencePanes".into(),
            paths: vec![
                p("/Library/PreferencePanes"),
                p("~/Library/PreferencePanes"),
            ],
        },
        PluginCategory {
            name: "QuickLook".into(),
            paths: vec![p("/Library/QuickLook"), p("~/Library/QuickLook")],
        },
        PluginCategory {
            name: "Screen Savers".into(),
            paths: vec![p("/Library/Screen Savers"), p("~/Library/Screen Savers")],
        },
        PluginCategory {
            name: "Internet Plug-Ins".into(),
            paths: vec![
                p("/Library/Internet Plug-Ins"),
                p("~/Library/Internet Plug-Ins"),
            ],
        },
        PluginCategory {
            name: "Core Image".into(),
            paths: vec![p("/Library/CoreImage"), p("~/Library/CoreImage")],
        },
        PluginCategory {
            name: "ColorPickers".into(),
            paths: vec![p("/Library/ColorPickers"), p("~/Library/ColorPickers")],
        },
        PluginCategory {
            name: "Fonts".into(),
            paths: vec![p("~/Library/Fonts")],
        },
        PluginCategory {
            name: "Dictionaries".into(),
            paths: vec![p("/Library/Dictionaries"), p("~/Library/Dictionaries")],
        },
        PluginCategory {
            name: "Automator".into(),
            paths: vec![p("/Library/Automator"), p("~/Library/Automator")],
        },
        PluginCategory {
            name: "Safari Extensions".into(),
            paths: vec![
                p("/Library/Safari/Extensions"),
                p("~/Library/Safari/Extensions"),
            ],
        },
        PluginCategory {
            name: "Motion Templates".into(),
            paths: vec![
                p("~/Movies/Motion Templates"),
                p("/Library/Application Support/Final Cut Pro System Support/Plug-ins"),
            ],
        },
        PluginCategory {
            name: "Spotlight".into(),
            paths: vec![p("/Library/Spotlight"), p("~/Library/Spotlight")],
        },
        PluginCategory {
            name: "Services".into(),
            paths: vec![p("/Library/Services"), p("~/Library/Services")],
        },
        PluginCategory {
            name: "Address Book".into(),
            paths: vec![p("~/Library/Address Book Plug-Ins")],
        },
        PluginCategory {
            name: "Contextual Menu".into(),
            paths: vec![
                p("/Library/Contextual Menu Items"),
                p("~/Library/Contextual Menu Items"),
            ],
        },
        PluginCategory {
            name: "Input Methods".into(),
            paths: vec![p("/Library/Input Methods"), p("~/Library/Input Methods")],
        },
        PluginCategory {
            name: "Widgets".into(),
            paths: vec![p("/Library/Widgets"), p("~/Library/Widgets")],
        },
    ]
    .into_iter()
    .map(|mut c| {
        c.paths = c
            .paths
            .iter()
            .map(|path| crate::dev_env::expand_home(path, home))
            .collect();
        c
    })
    .collect()
}

impl PluginPaths for MacOsAdapter {
    fn plugin_categories(&self) -> Vec<PluginCategory> {
        plugin_categories_table(&self.home)
    }
}

impl AppDiscovery for MacOsAdapter {
    /// 委托 scan.rs 的 .app 目录 walk（行为与旧 get_sorted_apps 调用点完全一致）
    fn discover_installed_apps(&self) -> Vec<AppInfo> {
        crate::scan::get_sorted_apps(&crate::scan::default_app_folders(&self.home))
    }
}

/// NSPredicate 字符串转义：单引号 → ''（SQL 风格，Spotlight 查询语法）
fn escape_predicate_value(value: &str) -> String {
    value.replace('\'', "''")
}

/// Strict/Enhanced 谓词。
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
        Sensitivity::Deep => None, // TODO: 多词 AND 谓词等
    }
}

/// Strict 后过滤：
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
    /// Spotlight 补充查询（mdfind）：-onlyin 用户主目录 + 谓词，5s 超时，500 条上限
    fn spotlight_supplemental_paths(
        &self,
        app_name: &str,
        bundle_id: &str,
        sensitivity: Sensitivity,
    ) -> Vec<PathBuf> {
        let Some(predicate) = build_predicate(app_name, bundle_id, sensitivity) else {
            return Vec::new();
        };

        // mdfind 毫秒级完成；索引重建时可能挂起 → 原版 5s 超时语义。
        // 实现：spawn 子进程 + try_wait 轮询（不用脱离线程 —— 原版线程 + recv_timeout
        // 超时后线程与 mdfind 进程继续泄漏）；超时 kill + wait 回收。
        let mut child = match Command::new("mdfind")
            .args(["-onlyin", &self.home, &predicate])
            .stdout(std::process::Stdio::piped())
            .spawn()
        {
            Ok(c) => c,
            Err(_) => return Vec::new(),
        };
        let start = std::time::Instant::now();
        let mut status: Option<std::process::ExitStatus> = None;
        while status.is_none() && start.elapsed() < Duration::from_secs(5) {
            match child.try_wait() {
                Ok(Some(s)) => status = Some(s),
                Ok(None) => std::thread::sleep(Duration::from_millis(50)),
                Err(_) => return Vec::new(),
            }
        }
        let Some(status) = status else {
            let _ = child.kill(); // 超时：终止进程，防泄漏
            let _ = child.wait();
            return Vec::new();
        };
        if !status.success() {
            return Vec::new();
        }

        let mut stdout = String::new();
        let read_ok = match child.stdout.take() {
            Some(mut o) => std::io::Read::read_to_string(&mut o, &mut stdout).is_ok(),
            None => false,
        };
        if !read_ok {
            return Vec::new();
        }
        let paths: Vec<PathBuf> = stdout
            .lines()
            .filter(|l| !l.trim().is_empty())
            .map(PathBuf::from)
            .collect();

        // 上限
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
    fn test_parse_ps_bundle_pids_matches_only_bundle() {
        let out = "    1 /sbin/launchd\n\
            123 /Applications/Foo.app/Contents/MacOS/Foo --flag arg\n\
            456 /Applications/Foo.app/Contents/Frameworks/Electron Helper.app/Contents/MacOS/Electron Helper\n\
            789 /Applications/Bar.app/Contents/MacOS/Bar\n\
            111 /Applications/FooBar.app/Contents/MacOS/FooBar\n";
        // 前缀匹配：bundle 内主进程 + Helper 命中；同名无关应用（Bar）与
        // 前缀近似的其他 bundle（FooBar）排除
        let pids = parse_ps_bundle_pids(out, "/Applications/Foo.app/");
        assert_eq!(pids, vec![123, 456]);
    }

    #[test]
    fn test_parse_ps_bundle_pids_malformed_lines_skipped() {
        let out = "not-a-pid /Applications/Foo.app/x\n\n  abc def\n1234\n";
        assert!(parse_ps_bundle_pids(out, "/Applications/Foo.app/").is_empty());
    }

    #[test]
    fn test_kill_running_app_none_running() {
        // 不存在的 bundle → 0（不误杀任何进程）
        let app = AppInfo {
            path: PathBuf::from("/Applications/nonexistent-app-zzz.app"),
            bundle_identifier: "x".to_string(),
            app_name: "x".to_string(),
            entitlements: vec![],
            team_identifier: None,
            web_app: false,
            steam: false,
            wrapped: false,
        };
        assert_eq!(MacOsAdapter::new().kill_running_app(&app), 0);
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
