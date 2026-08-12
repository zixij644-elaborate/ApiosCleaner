//! 每应用条件表 / 跳过表 —— 忠实移植原版 `Conditions.swift`
//! 注意：bundle_id/include/exclude 在构造时 pearFormat；force 路径仅保留磁盘上存在的

use std::path::PathBuf;

use crate::format::pear_format;
use crate::model::{Condition, SkipCondition};

fn cond(
    bundle_id: &str,
    include: &[&str],
    exclude: &[&str],
    include_force: &[&str],
    exclude_force: &[&str],
) -> Condition {
    Condition {
        bundle_id: pear_format(bundle_id),
        include: include.iter().map(|s| pear_format(s)).collect(),
        exclude: exclude.iter().map(|s| pear_format(s)).collect(),
        include_force: include_force
            .iter()
            .filter(|p| PathBuf::from(p).exists())
            .map(PathBuf::from)
            .collect(),
        exclude_force: exclude_force
            .iter()
            .filter(|p| PathBuf::from(p).exists())
            .map(PathBuf::from)
            .collect(),
    }
}

/// 每应用 include/exclude/force 条件（Conditions.swift:45-202）
pub fn conditions() -> Vec<Condition> {
    let home = std::env::var("HOME").unwrap_or_default();
    vec![
        cond(
            "com.apple.dt.xcode",
            &["com.apple.dt", "xcode", "simulator"],
            &[
                "com.robotsandpencils.xcodesapp",
                "com.xcodesorg.xcodesapp",
                "com.oneminutegames.xcodecleaner",
                "io.hyperapp.xcodecleaner",
                "available-xcodes",
                "xcodes",
                "cleaner for xcode",
            ],
            &[format!("{home}/Library/Containers/com.apple.iphonesimulator.ShareExtension").as_str()],
            &[],
        ),
        cond(
            "com.robotsandpencils.xcodesapp",
            &[],
            &["com.apple.dt.xcode", "com.oneminutegames.xcodecleaner", "io.hyperapp.xcodecleaner"],
            &[],
            &[],
        ),
        cond(
            "com.xcodesorg.xcodesapp",
            &[],
            &["com.apple.dt.xcode", "com.oneminutegames.xcodecleaner", "io.hyperapp.xcodecleaner"],
            &[],
            &[],
        ),
        cond(
            "io.hyperapp.xcodecleaner",
            &[],
            &[
                "com.robotsandpencils.xcodesapp",
                "com.oneminutegames.xcodecleaner",
                "com.apple.dt.xcode",
                "xcodes.json",
            ],
            &[],
            &[],
        ),
        cond("us.zoom.xos", &["zoom"], &[], &[], &[]),
        cond("com.brave.browser", &["brave"], &[], &[], &[]),
        cond("com.okta.mobile", &["okta"], &[], &[], &[]),
        cond(
            "com.google.chrome",
            &["google", "chrome"],
            &["iterm", "chromefeaturestate", "monochrome"],
            &[],
            &[],
        ),
        cond(
            "com.microsoft.edgemac",
            &[],
            &["vscode", "rdc", "appcenter", "office", "oneauth"],
            &[],
            &[],
        ),
        cond("com.microsoft.teams2", &[], &["office"], &[], &[]),
        cond("org.mozilla.firefox", &["firefox"], &["thunderbird"], &[], &[]),
        cond("org.mozilla.thunderbird", &[], &["firefox"], &[], &[]),
        cond(
            "org.mozilla.firefox.nightly",
            &["mozilla", "firefox"],
            &["thunderbird"],
            &[],
            &[],
        ),
        cond(
            "com.logi.optionsplus",
            &["logi", "logipluginservice"],
            &["login", "logic"],
            &[],
            &[],
        ),
        cond(
            "com.microsoft.VSCode",
            &["vscode"],
            &["vscodeinsiders", "insiders"],
            &[format!("{home}/Library/Application Support/Code/").as_str()],
            &[],
        ),
        cond(
            "com.microsoft.VSCodeInsiders",
            &["vscodeinsiders", "insiders"],
            &[],
            &[format!("{home}/Library/Application Support/Code - Insiders/").as_str()],
            &[],
        ),
        cond("com.facebook.archon.developerid", &["archon.loginhelper"], &[], &[], &[]),
        cond("eu.exelban.stats", &[], &["video"], &[], &[]),
        cond("me.mhaeuser.BatteryToolkit", &["memhaeuser"], &[], &[], &[]),
        cond(
            "jetbrains",
            &["jcef"],
            &[],
            &[
                format!("{home}/Library/Application Support/JetBrains/").as_str(),
                format!("{home}/Library/Caches/JetBrains/").as_str(),
                format!("{home}/Library/Logs/JetBrains/").as_str(),
            ],
            &[],
        ),
        cond(
            "company.thebrowser.Browser",
            &["firestore"],
            &[],
            &[
                format!("{home}/Library/Application Support/Arc/").as_str(),
                format!("{home}/Library/Caches/Arc/").as_str(),
            ],
            &[],
        ),
        cond("com.1password.1password", &["waveboxapp", "sidekick"], &[], &[], &[]),
        cond("com.now.gg.BlueStacks", &["bst_boost_interprocess"], &[], &[], &[]),
        cond("com.electron.sdm", &["strongdm"], &[], &[], &[]),
        cond("com.github.githubclient", &["comgithubelectron"], &[], &[], &[]),
        cond(
            "com.native-instruments.nativeaccess",
            &["comnative", "nativeinstruments"],
            &[],
            &[],
            &[],
        ),
    ]
}

/// 系统文件/文件夹跳过条件（Conditions.swift:207-213）
pub fn skip_conditions() -> Vec<SkipCondition> {
    let home = std::env::var("HOME").unwrap_or_default();
    vec![SkipCondition {
        skip_prefix: vec![
            "mobiledocuments".to_string(),
            "reminders".to_string(),
            "dsstore".to_string(),
            "comapplepasswordmanager".to_string(),
        ],
        allow_prefixes: vec![
            "comappleconfigurator".to_string(),
            "comappledt".to_string(),
            "comappleiwork".to_string(),
            "comapplesfsymbols".to_string(),
            "comappletestflight".to_string(),
            "comapplesharedfilelist".to_string(),
            "comapplelssharedfilelist".to_string(),
        ],
        skip_paths: vec![
            format!("{home}/.Trash"),
            "/Library/SystemExtensions".to_string(),
            "/System/Volumes/Preboot/Cryptexes/App/System/Library/CoreServices/PasswordManagerBrowserExtensionHelper.app/Contents/MacOS/PasswordManagerBrowserExtensionHelper"
                .to_string(),
            format!(
                "{home}/Library/Application Support/Chromium/NativeMessagingHosts/com.apple.passwordmanager.json"
            ),
            format!(
                "{home}/Library/Application Support/Google/Chrome/NativeMessagingHosts/com.apple.passwordmanager.json"
            ),
        ],
    }]
}

/// Library 深度搜索时排除的系统目录（Conditions.swift:218-256）
pub fn skip_deep_search() -> std::collections::HashSet<String> {
    const LIST: &[&str] = &[
        // Core System
        "Apple", "Audio", "Bluetooth", "ColorSync", "Components", "CoreAnalytics",
        "CoreMediaIO", "DirectoryServices", "Filesystems", "GPUBundles", "Graphics",
        "KernelCollections", "OSAnalytics", "OpenDirectory", "Sandbox", "Security",
        "SystemExtensions", "SystemMigration", "SystemProfiler", "StagedDriverExtensions",
        "StagedExtensions", "StartupItems",
        // User Data & System Services
        "Accessibility", "Accounts", "AppleMediaServices", "Assistant", "Assistants",
        "Autosave Information", "Biome", "Calendars", "CallServices", "CloudStorage",
        "Contacts", "Cookies", "DataAccess", "DataDeliveryServices", "DoNotDisturb",
        "DuetExpertCenter", "Finance", "FinanceBackup", "FrontBoard", "GameKit",
        "GroupContainersAlias", "HomeKit", "IdentityServices", "IntelligencePlatform",
        "Intents", "KeyboardServices", "LanguageModeling", "LockdownMode", "Mail",
        "MediaAnalysis", "Messages", "Metadata", "Mobile Documents", "MobileDevice",
        "News", "Passes", "PersonalizationPortrait", "Photos", "PrivateCloudCompute",
        "Reminders", "ResponseKit", "Safari", "SafariSafeBrowsing", "SafariSandboxBroker",
        "ScreenRecordings", "StatusKit", "Suggestions", "SyncedPreferences", "Translation",
        "UnifiedAssetFramework", "Weather", "homeenergyd", "studentd",
        // Development/System Tools
        "Developer", "Perl", "Ruby", "Java", "Python", "Catacomb", "InstallerSandboxes",
        "Trial", "Updates", "Staging", "ContainerManager", "Daemon Containers",
        // Additional System Directories
        "ColorPickers", "Colors", "Compositions", "Contextual Menu Items", "Documentation",
        "DriverExtensions", "Favorites", "FontCollections", "Fonts", "Image Capture",
        "Input Methods", "Jupyter", "Keyboard", "Keyboard Layouts", "Keychains",
        "Managed Preferences", "PDF Services", "Printers", "QuickLook", "Receipts",
        "Screen Savers", "ScriptingAdditions", "Scripts", "Sharing", "Shortcuts",
        "Sounds", "Speech", "Spelling", "Spotlight", "User Pictures", "User Template",
        "Video", "WebServer", "Workflows",
        // Apple service bundles
        "com.apple.AppleMediaServices", "com.apple.WatchListKit",
        "com.apple.aiml.instrumentation", "com.apple.appleaccountd",
        "com.apple.bluetooth.services.cloud", "com.apple.bluetoothuser",
        "com.apple.familycircled", "com.apple.iTunesCloud",
        "com.apple.internal.ck",
    ];
    LIST.iter().map(|s| s.to_string()).collect()
}

/// 孤儿搜索时跳过的名称列表（Conditions.swift:260）
pub fn skip_reverse() -> std::collections::HashSet<String> {
    const LIST: &[&str] = &[
        "apple", "temporary", "btserver", "proapps", "scripteditor", "ilife", "livefsd",
        "siritoday", "addressbook", "animoji", "appstore", "askpermission", "callhistory",
        "clouddocs", "diskimages", "dock", "facetime", "fileprovider", "instruments",
        "knowledge", "mobilesync", "syncservices", "homeenergyd", "icloud", "icdd",
        "networkserviceproxy", "familycircle", "geoservices", "installation", "passkit",
        "sharedimagecache", "desktop", "mbuseragent", "swiftpm", "baseband", "coresimulator",
        "photoslegacyupgrade", "photosupgrade", "siritts", "ipod", "globalpreferences",
        "apmanalytics", "apmexperiment", "avatarcache", "byhost", "contextstoreagent",
        "mobilemeaccounts", "mobiledocuments", "mobile", "intentbuilderc", "loginwindow",
        "momc", "replayd", "sharedfilelistd", "clang", "audiocomponent",
        "csexattrcryptoservice", "livetranscriptionagent", "sandboxhelper", "statuskitagent",
        "betaenrollmentd", "contentlinkingd", "diagnosticextensionsd", "gamed", "heard",
        "homed", "itunescloudd", "lldb", "mds", "mediaanalysisd", "metrickitd",
        "mobiletimerd", "proactived", "ptpcamerad", "studentd", "talagent", "watchlistd",
        "apptranslocation", "xcrun", "ds_store", "caches", "crashreporter", "trash",
        "pearcleaner", "amsdatamigratortool", "arfilecache", "assistant", "chromium",
        "cloudkit", "webkit", "databases", "diagnostic", "cache", "gamekit", "homebrew",
        "logi", "microsoft", "mozilla", "sync", "google", "sentinel", "hexnode", "sentry",
        "tvappservices", "reminders", "pbs", "notarytool", "differentialprivacy",
        "storeassetd", "webpush", "storedownloadd", "fsck", "crash", "python",
        "discrecording", "photossearch", "pylint", "jamf", "scopedbookmarkagent",
        "anonymous", "identifier", "isolated", "nobackup", "privacypreservingmeasurement",
        "symbols", "stickersd", "privatecloudcomputed", "tipsd", "controlcenter",
        "contactsd", "staticcheck", "index", "segment", "sparkle", "summaryevents",
        "launchdarkly", "identityservicesd", "embeddedbinaryvalidationutility",
        "comalienator88", "aaprofilepicture", "minilauncher", "jna", "automator",
        "locationaccessstored", "spotlight", "cef",
    ];
    LIST.iter().map(|s| s.to_string()).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_conditions_bundle_ids_are_formatted() {
        for c in conditions() {
            assert_eq!(c.bundle_id, pear_format(&c.bundle_id), "bundle_id 未归一化");
            assert!(!c.bundle_id.contains('.'), "bundle_id 含点号");
        }
    }

    #[test]
    fn test_skip_tables_non_empty() {
        assert!(!skip_deep_search().is_empty());
        assert!(skip_deep_search().contains("Safari"));
        assert!(!skip_reverse().is_empty());
        assert!(skip_reverse().contains("microsoft"));
        assert_eq!(skip_conditions().len(), 1);
    }
}
