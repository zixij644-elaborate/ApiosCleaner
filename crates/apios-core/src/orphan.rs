//! 孤儿文件反向搜索 —— 忠实移植原版 `ReversePathsSearcher`（CLI 路径）
//! (old/Pearcleaner/Logic/ReversePathsFetch.swift:175-313)

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::LazyLock;

use crate::conditions;
use crate::format::pear_format;
use crate::locations::Locations;
use crate::model::{AppInfo, Condition};
use crate::platform::SystemPaths;

/// UUID 容器目录名（ReversePathsFetch.swift:280-285 的 containerNameByUUID 正则）
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

/// UUID（无连字符，32 位 hex）判断（ReversePathsFetch.swift:280-285）
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

    /// 是否与已安装应用相关（ReversePathsFetch.swift:227-255）
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

    /// 条件排除（ReversePathsFetch.swift:257-278）
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

    /// 单项处理（processItem，ReversePathsFetch.swift:207-225）
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
            if WINDOWS_SYSTEM_DIRS.contains(&scanned_item_name.to_ascii_lowercase().as_str()) {
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

    /// 单位置处理（processLocation，ReversePathsFetch.swift:194-205）
    fn process_location(&mut self, location: &str) {
        let Ok(entries) = std::fs::read_dir(location) else {
            return;
        };
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().to_string();
            self.process_item(&name, &entry.path());
        }
    }

    /// CLI 主入口（reversePathsSearchCLI，ReversePathsFetch.swift:175-178）
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
