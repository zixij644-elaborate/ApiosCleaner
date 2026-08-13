//! 孤儿文件反向搜索 —— 找出已卸载应用遗留的相关文件
//! macOS：预构建 UUID→bundle-id 映射 + 名称启发式；Windows：exe 路径派生 needle + 系统目录过滤

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::LazyLock;

use crate::conditions;
use crate::format::pear_format;
use crate::locations::Locations;
use crate::model::{AppInfo, Condition};
use crate::platform::SystemPaths;

/// UUID 容器目录名（containerNameByUUID 正则）
static UUID_REGEX: LazyLock<regex::Regex> = LazyLock::new(|| {
    regex::Regex::new(r"^[0-9A-F]{8}-[0-9A-F]{4}-[0-9A-F]{4}-[0-9A-F]{4}-[0-9A-F]{12}$").unwrap()
});

/// 预扫描 {home}/Library/Containers：UUID 目录名 → bundle ID（读一次，替代
/// 原版每个 /Containers/ 路径重复 read_dir + plist 解析的 O(N²)）
fn build_container_uuid_map(home: &str) -> std::collections::HashMap<String, String> {
    let mut map = std::collections::HashMap::new();
    let containers = PathBuf::from(format!("{home}/Library/Containers"));
    let Ok(entries) = std::fs::read_dir(&containers) else {
        return map;
    };
    for entry in entries.flatten() {
        let uuid = entry.file_name().to_string_lossy().to_string();
        if !UUID_REGEX.is_match(&uuid) {
            continue;
        }
        let metadata = entry
            .path()
            .join(".com.apple.containermanagerd.metadata.plist");
        let Ok(data) = std::fs::read(&metadata) else {
            continue;
        };
        let Ok(dict) = plist::from_bytes::<plist::Dictionary>(&data) else {
            continue;
        };
        if let Some(bundle_id) = dict
            .get("MCMMetadataIdentifier")
            .and_then(|v| v.as_string())
        {
            map.insert(uuid, bundle_id.to_string());
        }
    }
    map
}

pub struct ReversePathsSearcher {
    locations: Locations,
    collection: Vec<PathBuf>,
    /// needles 的合并 Alternation 正则（new() 构建一次，匹配 O(paths)）。
    /// 标识来自 ≥5 字符的 bundle id / 应用名 / entitlements（Windows 另含
    /// 非 ASCII 短名），构建时已 pearFormat + escape
    needles_regex: Option<regex::Regex>,
    /// 仅 bundle id（≥5 字符）—— 容器匹配用（语义：cn 包含已安装 bundle id）
    bundle_needles: Vec<String>,
    /// UUID 容器目录名 → bundle ID（new() 预建，替代逐路径 read_dir）
    container_uuids: std::collections::HashMap<String, String>,
    /// 已安装的条件的 bundle id（预计算 —— 原版每条路径 × 条件 × 已装应用三重循环）
    installed_conditions: HashSet<String>,
    skip_reverse: HashSet<String>,
    /// 条件表缓存（new() 时构建一次；cond() 构建含磁盘 exists 检查，不能每路径重建）
    conditions: Vec<Condition>,
}

/// isSupportedFileType（AlinFoundation）：普通文件 / 目录 / 符号链接
fn is_supported_file_type(path: &Path) -> bool {
    std::fs::symlink_metadata(path)
        .map(|m| {
            let t = m.file_type();
            t.is_file() || t.is_dir() || t.is_symlink()
        })
        .unwrap_or(false)
}

/// UUID（无连字符，32 位 hex）判断
fn is_uuid_formatted(file_name: &str) -> bool {
    file_name.len() == 32 && file_name.chars().all(|c| c.is_ascii_hexdigit())
}

/// needle 长度门槛。macOS 有 bundle id（≥5 无歧义）—— 短名防误配
/// （"win"/"sys" 类短词路径命中率过高）；Windows 无 bundle id 兜底，
/// 中文短名应用（微信/QQ/百度网盘，恰是最常见清理对象）若同样被 <5
/// 丢弃则残留永远匹配不到。非 ASCII 名放宽到 ≥2 字符（单字名罕见且
/// 单字符 needle 误配面过大）；纯 ASCII 保持 ≥5。
fn needle_qualifies(s: &str) -> bool {
    let long_enough = s.chars().count() >= 5;
    #[cfg(windows)]
    {
        long_enough || (s.chars().count() >= 2 && !s.is_ascii())
    }
    #[cfg(not(windows))]
    {
        long_enough
    }
}

/// path 派生 needle 门槛：≥3 字符（ASCII/非 ASCII 一致）。全串 needle 的
/// ≥5 门槛是为防 "win"/"sys" 类短词全局误配；path 派生 needle 来自真实
/// 应用可执行文件位置（父目录名/文件名），3 字符已足够特异。多匹配只造成
/// 漏报（安全方向）—— 宁可漏报，也不让已装应用目录进孤儿列表。
#[cfg(windows)]
fn path_needle_qualifies(s: &str) -> bool {
    s.chars().count() >= 3
}

impl ReversePathsSearcher {
    pub fn new(locations: Locations, sorted_apps: Vec<AppInfo>) -> ReversePathsSearcher {
        let home = crate::platform::adapter().home();

        // 标识集合预计算：≥5 字符的 bundle id / 应用名 / entitlements
        // （原版逐路径 × 逐应用 × 逐标识的嵌套循环 → 扁平 needle 列表）
        let mut needles: Vec<String> = Vec::new();
        let mut bundle_needles: Vec<String> = Vec::new();
        for app in &sorted_apps {
            let bundle_id = pear_format(&app.bundle_identifier);
            if needle_qualifies(&bundle_id) {
                needles.push(bundle_id.clone());
                bundle_needles.push(bundle_id);
            }
            let app_name = pear_format(&app.app_name);
            if needle_qualifies(&app_name) {
                needles.push(app_name);
            }
            #[cfg(windows)]
            {
                // Windows 无 bundle id 兜底，且 DisplayName 常含版本号
                // （"7-Zip 24.09 (x64)"）匹配不上供应商目录名（"7-Zip"）——
                // 已装应用的 Program Files/AppData 目录会整段误报为孤儿。
                // 从可执行文件路径派生 needle：祖先目录（最多 3 级，exe 常
                // 嵌在供应商多级目录下：Tencent\Weixin\Weixin.exe、Netease\
                // CloudMusic\cloudmusic.exe）+ 文件 stem（Code.exe→code）。
                // 结构/系统目录名跳过 —— 作为 needle 会成全路径宽匹配
                // （"programfiles" 命中所有 Program Files 路径，fix A 直接
                // 失效；"local"/"appdata" 同理）。.lnk 发现的便携应用路径
                // 是 .lnk 本身：祖先即 Programs/Start Menu 结构目录，无贡献。
                const STRUCTURAL_DIRS: [&str; 21] = [
                    "programs",
                    "programsx86",
                    "startmenu",
                    "applications",
                    "programfiles",
                    "programfilesx86",
                    "commonfiles",
                    "program",
                    "bin",
                    "microsoft",
                    "windows",
                    "system32",
                    "syswow64",
                    "local",
                    "roaming",
                    "appdata",
                    "programdata",
                    "documents",
                    "desktop",
                    "temp",
                    "users",
                ];
                for anc in app.path.ancestors().skip(1).take(3) {
                    let Some(name) = anc.file_name() else {
                        continue;
                    };
                    let f = pear_format(&name.to_string_lossy());
                    if path_needle_qualifies(&f) && !STRUCTURAL_DIRS.contains(&f.as_str()) {
                        needles.push(f);
                    }
                }
                if let Some(stem) = app
                    .path
                    .file_stem()
                    .map(|n| pear_format(&n.to_string_lossy()))
                    .filter(|s| path_needle_qualifies(s))
                {
                    needles.push(stem);
                }
            }
            for ent in app.entitlements.iter().filter_map(|e| {
                let f = pear_format(e);
                if f.is_empty() {
                    None
                } else {
                    Some(f)
                }
            }) {
                if needle_qualifies(&ent) {
                    needles.push(ent);
                }
            }
        }
        needles.sort();
        needles.dedup();
        bundle_needles.sort();
        bundle_needles.dedup();

        // 合并正则（regex::escape 防 needle 中的正则元字符误配）—— 替代
        // 逐 needle contains 的 O(paths × needles)；Alternation 等价于
        // "任一 needle 是路径子串"，语义不变。needle 已 pearFormat
        // （仅字母数字 + 小写，含中文）。
        let needles_regex = if needles.is_empty() {
            None
        } else {
            let pattern = needles
                .iter()
                .map(|n| regex::escape(n))
                .collect::<Vec<_>>()
                .join("|");
            regex::Regex::new(&pattern).ok()
        };

        let conditions = conditions::conditions();
        // 已安装应用匹配到的条件的 bundle id（预计算：原版每条路径重新遍历已装应用）
        let installed_ids: Vec<String> = sorted_apps
            .iter()
            .map(|a| pear_format(&a.bundle_identifier))
            .collect();
        let installed_conditions = conditions
            .iter()
            .filter(|c| {
                installed_ids
                    .iter()
                    .any(|id| id == &c.bundle_id || id.contains(&c.bundle_id))
            })
            .map(|c| c.bundle_id.clone())
            .collect();

        ReversePathsSearcher {
            locations,
            collection: Vec::new(),
            needles_regex,
            bundle_needles,
            container_uuids: build_container_uuid_map(&home),
            installed_conditions,
            skip_reverse: conditions::skip_reverse(),
            conditions,
        }
    }

    /// 是否与已安装应用相关
    fn is_related_to_installed_app(&self, path: &Path, normalized_path: &str) -> bool {
        if self
            .needles_regex
            .as_ref()
            .is_some_and(|re| re.is_match(normalized_path))
        {
            return true;
        }
        // 容器匹配只对 /Containers/ 路径生效（原版用原始路径判断，
        // 不能用 pearFormat 后的路径——斜杠已被剥除）。
        // UUID → bundle ID 走预建映射（不再逐路径 read_dir）
        let raw = path.to_string_lossy();
        if raw.contains("/Containers/") {
            let uuid = path
                .file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_default();
            if let Some(bundle_id) = self.container_uuids.get(&uuid) {
                let cn = pear_format(bundle_id);
                if self.bundle_needles.iter().any(|n| cn.contains(n.as_str())) {
                    return true;
                }
            }
        }
        false
    }

    /// 条件排除
    fn is_excluded_by_conditions(&self, normalized_path: &str) -> bool {
        for condition in &self.conditions {
            // 仅当条件对应的 app 已安装才生效（new() 预计算）
            if !self.installed_conditions.contains(&condition.bundle_id) {
                continue;
            }
            if condition
                .include
                .iter()
                .any(|k| normalized_path.contains(k.as_str()))
            {
                return true;
            }
            for force in &condition.include_force {
                let f = pear_format(&force.to_string_lossy());
                if normalized_path.contains(&f) {
                    return true;
                }
            }
        }
        false
    }

    /// 单项处理（processItem）
    fn process_item(&mut self, scanned_item_name: &str, path: &Path) {
        let normalized_item_path = pear_format(&path.to_string_lossy());

        // 排除列表（fsm.fileFolderPathsZ 用户异常列表，CLI 默认空）+ dsstore 等
        if normalized_item_path.contains("dsstore")
            || normalized_item_path.contains("daemonnameoridentifierhere")
        {
            return;
        }

        // Windows 系统目录过滤（clean-orphan 会真删，系统目录是危险面）：
        // - junction（AppData\Local\Application Data、ProgramData\Documents 等
        //   系统 ReparsePoint）每台系统都有、永不被引用 —— 100% 误报
        // - AppData/ProgramData 一级子项里的系统/结构目录（应用安装根、
        //   系统临时目录、UWP 包目录、系统组件数据）—— 删除破坏系统与已装应用。
        //   名单按名字匹配（大小写不敏感），与父目录无关（名字语义稳定）
        #[cfg(target_os = "windows")]
        {
            use std::os::windows::fs::MetadataExt;
            if std::fs::symlink_metadata(path)
                .map(|m| {
                    // is_symlink 实测已覆盖 junction（检查 REPARSE_POINT 属性位，
                    // 106→96 实证）；属性位双保险 —— 防 std 未来把 is_symlink
                    // 收窄为仅 SYMLINK tag 时 junction 重新泄漏
                    const FILE_ATTRIBUTE_REPARSE_POINT: u32 = 0x0400;
                    m.file_type().is_symlink()
                        || m.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0
                })
                .unwrap_or(false)
            {
                return;
            }
            const WINDOWS_SYSTEM_DIRS: [&str; 15] = [
                "packages",
                "programs",
                "temp",
                "virtualstore",
                "connecteddevicesplatform",
                "peerdistrepub",
                "publishers",
                "speech",
                "comms",
                "upgrade",
                "placeholdertilelogofolder",
                "usoprivate",
                "usoshared",
                "whesvc",
                "ssh",
            ];
            // 系统/结构目录过滤（AppData 系统目录名单 + Program Files(+x86)
            // 根下的 OS 组件目录）：validate_path 只拦 critical 根（AppData/
            // Program Files 本身），子目录全放行 —— clean-orphan 会真删它们，
            // 必须在此过滤。名字语义稳定，与父目录无关，跨位置生效（安全方向）。
            // AppData 名单来自实测（Packages=UWP、Temp、VirtualStore、
            // ConnectedDevicesPlatform 等每台系统必有且永不被应用引用）；
            // Program Files 侧：名称以 "Windows" 开头的（Windows Defender/NT/
            // Media Player/WindowsApps/Windows Kits…）或已知共享/系统目录
            // （Common Files/MSBuild/…）。
            let name_lower = scanned_item_name.to_ascii_lowercase();
            if WINDOWS_SYSTEM_DIRS.contains(&name_lower.as_str())
                || name_lower.starts_with("windows")
                || matches!(
                    name_lower.as_str(),
                    "common files"
                        | "internet explorer"
                        | "msbuild"
                        | "reference assemblies"
                        | "wsl"
                        | "modifiablewindowsapps"
                        | "application verifier"
                )
            {
                return;
            }
        }

        let normalized_item_name = pear_format(scanned_item_name);
        if is_uuid_formatted(&normalized_item_name) {
            return;
        }
        if self
            .skip_reverse
            .iter()
            .any(|s| normalized_item_name.contains(s.as_str()))
        {
            return;
        }
        if !is_supported_file_type(path) {
            return;
        }
        if self.is_related_to_installed_app(path, &normalized_item_path) {
            return;
        }
        if self.is_excluded_by_conditions(&normalized_item_path) {
            return;
        }

        self.collection.push(path.to_path_buf());
    }

    /// 单位置处理（processLocation）
    fn process_location(&mut self, location: &str) {
        let Ok(entries) = std::fs::read_dir(location) else {
            return;
        };
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().to_string();
            self.process_item(&name, &entry.path());
        }
    }

    /// CLI 主入口（reversePathsSearchCLI）
    pub fn reverse_paths_search_cli(&mut self) -> Vec<PathBuf> {
        let locations = self.locations.reverse_paths.clone();
        for location in locations {
            if Path::new(&location).exists() {
                self.process_location(&location);
            }
        }
        self.collection.clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_uuid_format() {
        assert!(is_uuid_formatted("5A2E3F1B0C4D4E5F8A9B0C1D2E3F4A5B"));
        assert!(!is_uuid_formatted("notauuid"));
        assert!(!is_uuid_formatted("5A2E3F1B0C4D4E5F8A9B0C1D2E3F4A5"));
    }

    #[test]
    fn test_related_by_bundle_id() {
        // 5+ 字符 bundle ID 在路径中出现 → 相关
        let app = AppInfo {
            path: PathBuf::from("/Applications/Foo.app"),
            bundle_identifier: "com.example.fooapp".to_string(),
            app_name: "FooApp".to_string(),
            entitlements: vec![],
            team_identifier: None,
            web_app: false,
            steam: false,
            wrapped: false,
        };
        let locations = Locations::new();
        let searcher = ReversePathsSearcher::new(locations, vec![app]);
        let p = Path::new("/Users/u/Library/Application Support/com.example.fooapp");
        assert!(searcher.is_related_to_installed_app(p, &pear_format(&p.to_string_lossy())));

        // 短名称（<5 字符）不应误判
        let app2 = AppInfo {
            path: PathBuf::from("/Applications/X.app"),
            bundle_identifier: "com.x".to_string(),
            app_name: "X".to_string(),
            entitlements: vec![],
            team_identifier: None,
            web_app: false,
            steam: false,
            wrapped: false,
        };
        let locations = Locations::new();
        let searcher = ReversePathsSearcher::new(locations, vec![app2]);
        let p = Path::new("/Users/u/Library/Preferences/com.x");
        assert!(!searcher.is_related_to_installed_app(p, &pear_format(&p.to_string_lossy())));
    }

    #[test]
    fn test_needle_qualifies_threshold() {
        // ASCII 短名（"QQ"）任何平台都不应成为 needle（误配面过大）
        assert!(!needle_qualifies("qq"));
        // ≥5 字符全平台放行
        assert!(needle_qualifies("comexamplefooapp"));
    }

    #[cfg(windows)]
    #[test]
    fn test_needle_qualifies_windows_non_ascii_short_names() {
        // Windows 无 bundle id 兜底：中文短名应用（微信/QQ 等）残留必须可匹配
        assert!(needle_qualifies("微信"));
        assert!(needle_qualifies("百度网盘"));
        // 单字名仍不放行（单字符 needle 误配面过大）
        assert!(!needle_qualifies("钉"));
    }

    #[test]
    fn test_merged_regex_matches_any_needle() {
        // 合并正则等价于逐 needle contains（bundle id / 应用名任一命中即相关）
        let app = AppInfo {
            path: PathBuf::from("/Applications/WeChat.app"),
            bundle_identifier: "com.tencent.wechat".to_string(),
            app_name: "WeChat".to_string(),
            entitlements: vec![],
            team_identifier: None,
            web_app: false,
            steam: false,
            wrapped: false,
        };
        let locations = Locations::new();
        let searcher = ReversePathsSearcher::new(locations, vec![app]);
        let re = searcher.needles_regex.as_ref().expect("应构建合并正则");
        assert!(re.is_match("usersulibrarycontainerscomtencentwechat"));
        assert!(re.is_match("usersulibraryapplicationsupportwechat"));
        assert!(!re.is_match("usersulibraryapplicationsupportother"));
    }

    #[cfg(windows)]
    #[test]
    fn test_path_derived_dir_needle() {
        // 中文 DisplayName（夸克网盘）匹配不上英文目录名（QuarkCloudDrive）：
        // path 派生的父目录 needle 兜底。7-Zip 同理（DisplayName 含版本号）
        let app = AppInfo {
            path: PathBuf::from(r"C:\Program Files\QuarkCloudDrive\QuarkCloudDrive.exe"),
            bundle_identifier: String::new(),
            app_name: "夸克网盘".to_string(),
            entitlements: vec![],
            team_identifier: None,
            web_app: false,
            steam: false,
            wrapped: false,
        };
        let locations = Locations::new();
        let searcher = ReversePathsSearcher::new(locations, vec![app]);
        let re = searcher.needles_regex.as_ref().expect("应构建合并正则");
        assert!(re.is_match("cprogramfilesquarkclouddrive"));
        assert!(re.is_match("cprogramdataquarkclouddrive"));

        let app = AppInfo {
            path: PathBuf::from(r"C:\Program Files\7-Zip\7zFM.exe"),
            bundle_identifier: String::new(),
            app_name: "7-Zip 24.09 (x64)".to_string(),
            entitlements: vec![],
            team_identifier: None,
            web_app: false,
            steam: false,
            wrapped: false,
        };
        let locations = Locations::new();
        let searcher = ReversePathsSearcher::new(locations, vec![app]);
        assert!(searcher
            .needles_regex
            .as_ref()
            .unwrap()
            .is_match("cprogramfiles7zip"));
    }

    #[cfg(windows)]
    #[test]
    fn test_path_derived_stem_needle() {
        // Code.exe → "code"（4 字符，path 门槛 ≥3 放行；全串门槛不放行）
        let app = AppInfo {
            path: PathBuf::from(r"C:\Users\u\AppData\Local\Programs\Microsoft VS Code\Code.exe"),
            bundle_identifier: String::new(),
            app_name: "Visual Studio Code".to_string(),
            entitlements: vec![],
            team_identifier: None,
            web_app: false,
            steam: false,
            wrapped: false,
        };
        let locations = Locations::new();
        let searcher = ReversePathsSearcher::new(locations, vec![app]);
        let re = searcher.needles_regex.as_ref().expect("应构建合并正则");
        assert!(re.is_match("cusersuappdataroamingcode"));
    }

    #[cfg(windows)]
    #[test]
    fn test_nested_vendor_dirs_derive_needles() {
        // 微信：exe 嵌在 Tencent\Weixin 下 —— 只取父目录（weixin）匹配不上
        // Program Files\Tencent。祖先 3 级必须给出 tencent
        let app = AppInfo {
            path: PathBuf::from(r"C:\Program Files\Tencent\Weixin\Weixin.exe"),
            bundle_identifier: String::new(),
            app_name: "微信".to_string(),
            entitlements: vec![],
            team_identifier: None,
            web_app: false,
            steam: false,
            wrapped: false,
        };
        let locations = Locations::new();
        let searcher = ReversePathsSearcher::new(locations, vec![app]);
        let re = searcher.needles_regex.as_ref().expect("应构建合并正则");
        assert!(re.is_match("cprogramfilestencent"));
        assert!(re.is_match("cprogramdatatencent"));
        assert!(
            re.is_match("cusersznieappdataroamingtencentweixin"),
            "weixin stem"
        );
        // 已知限制：微信 4.0 数据目录 xwechat_files 与 Weixin.exe 名字无交集
        // （xwechat ≠ weixin/tencent/微信）—— 不匹配，保留在孤儿列表
        assert!(!re.is_match("cuserszniedocumentsxwechatfiles"));

        // 网易云：Netease\CloudMusic\cloudmusic.exe → netease
        let app = AppInfo {
            path: PathBuf::from(r"C:\Program Files\Netease\CloudMusic\cloudmusic.exe"),
            bundle_identifier: String::new(),
            app_name: "网易云音乐".to_string(),
            entitlements: vec![],
            team_identifier: None,
            web_app: false,
            steam: false,
            wrapped: false,
        };
        let locations = Locations::new();
        let searcher = ReversePathsSearcher::new(locations, vec![app]);
        assert!(searcher
            .needles_regex
            .as_ref()
            .unwrap()
            .is_match("cprogramfilesnetease"));

        // 深层 exe（Program 结构目录穿插）：Thunder Network\Thunder\Program\
        // Thunder.exe → 3 级祖先给出 thunder + thundernetwork（Program 是
        // 结构名跳过；Program Files 是第 4 级不进入）
        let app = AppInfo {
            path: PathBuf::from(r"C:\Program Files\Thunder Network\Thunder\Program\Thunder.exe"),
            bundle_identifier: String::new(),
            app_name: "迅雷".to_string(),
            entitlements: vec![],
            team_identifier: None,
            web_app: false,
            steam: false,
            wrapped: false,
        };
        let locations = Locations::new();
        let searcher = ReversePathsSearcher::new(locations, vec![app]);
        let re = searcher.needles_regex.as_ref().expect("应构建合并正则");
        assert!(re.is_match("cprogramfilesthunder"));
        assert!(re.is_match("cprogramdatathundernetwork"));
    }

    #[cfg(windows)]
    #[test]
    fn test_lnk_programs_dir_not_a_needle() {
        // 开始菜单 .lnk 的父目录 Programs 是结构目录，不得成为 needle
        // （否则全路径宽匹配）。app_name（微信）仍正常生效
        let app = AppInfo {
            path: PathBuf::from(
                r"C:\Users\u\AppData\Roaming\Microsoft\Windows\Start Menu\Programs\微信.lnk",
            ),
            bundle_identifier: String::new(),
            app_name: "微信".to_string(),
            entitlements: vec![],
            team_identifier: None,
            web_app: false,
            steam: false,
            wrapped: false,
        };
        let locations = Locations::new();
        let searcher = ReversePathsSearcher::new(locations, vec![app]);
        let re = searcher.needles_regex.as_ref().expect("应构建合并正则");
        // 中文 app_name needle 只匹配含中文的路径（lnk 文件本身、微信目录）
        assert!(re.is_match("cusersuappdataroamingmicrosoftwindowsstartmenuprograms微信lnk"));
        // 若 "programs" 成了 needle，此 ASCII 路径会命中 —— 必须不命中
        assert!(!re.is_match("cusersuappdatalocalprogramsfoo"));
    }

    #[cfg(windows)]
    #[test]
    fn test_windows_system_dirs_filtered_from_orphans() {
        let locations = Locations::new();
        let mut searcher = ReversePathsSearcher::new(locations, vec![]);
        let base = std::env::temp_dir().join("apios-orphan-sysdir-test");
        let _ = std::fs::remove_dir_all(&base);
        std::fs::create_dir_all(&base).unwrap();
        // 名单内条目：目录名命中即过滤（文件/目录不存在也不影响 —— 过滤先于存在检查）
        for name in [
            "Windows NT",
            "WindowsApps",
            "Common Files",
            "MSBuild",
            "Internet Explorer",
            "WSL",
            "Application Verifier",
            "Reference Assemblies",
            "Windows Kits",
            "Windows App Certification Kit",
            // AppData 系统目录名单（每台系统必有，永不被应用引用）
            "Packages",
            "Programs",
            "Temp",
            "VirtualStore",
            "ConnectedDevicesPlatform",
            "PeerDistRepub",
            "Publishers",
            "speech",
            "Comms",
            "Upgrade",
            "PlaceholderTileLogoFolder",
            "USOPrivate",
            "USOShared",
            "Whesvc",
            "ssh",
        ] {
            searcher.process_item(name, &base.join(name));
        }
        // 普通供应商目录（已存在）：不是系统目录 → 进列表
        let vendor = base.join("QuarkCloudDrive");
        std::fs::create_dir_all(&vendor).unwrap();
        searcher.process_item("QuarkCloudDrive", &vendor);
        assert_eq!(
            searcher.collection.len(),
            1,
            "系统目录必须被过滤，供应商目录保留"
        );
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn test_no_needles_means_no_regex() {
        // 无合格标识（短 ASCII 名 + 空 bundle id）→ needles_regex 为 None，
        // is_related_to_installed_app 直接短路（不构造空 Alternation）
        let app = AppInfo {
            path: PathBuf::from("/Applications/X.app"),
            bundle_identifier: "com.x".to_string(),
            app_name: "X".to_string(),
            entitlements: vec![],
            team_identifier: None,
            web_app: false,
            steam: false,
            wrapped: false,
        };
        let locations = Locations::new();
        let searcher = ReversePathsSearcher::new(locations, vec![app]);
        assert!(searcher.needles_regex.is_none());
        let p = Path::new("/Users/u/Library/Preferences/com.x");
        assert!(!searcher.is_related_to_installed_app(p, &pear_format(&p.to_string_lossy())));
    }
}
