//! 孤儿文件反向搜索 —— 忠实移植原版 `ReversePathsSearcher`（CLI 路径）
//! (old/Pearcleaner/Logic/ReversePathsFetch.swift:175-313)

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::LazyLock;

use crate::conditions;
use crate::format::pear_format;
use crate::locations::Locations;
use crate::model::{AppInfo, Condition};

/// UUID 容器目录名（ReversePathsFetch.swift:280-285 的 containerNameByUUID 正则）
static UUID_REGEX: LazyLock<regex::Regex> = LazyLock::new(|| {
    regex::Regex::new(r"^[0-9A-F]{8}-[0-9A-F]{4}-[0-9A-F]{4}-[0-9A-F]{4}-[0-9A-F]{12}$").unwrap()
});

/// 预计算的已安装应用标识（ReversePathsFetch.swift:30-47）
#[derive(Clone)]
struct CachedAppIdentifiers {
    formatted_bundle_id: String,
    formatted_app_name: String,
    formatted_entitlements: Vec<String>,
}

pub struct ReversePathsSearcher {
    locations: Locations,
    collection: Vec<PathBuf>,
    cached_apps: Vec<CachedAppIdentifiers>,
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

/// 容器目录 → bundle ID（Utilities.swift:547-595 的 containerNameByUUID）
fn container_name_by_uuid(path: &Path, home: &str) -> Option<String> {
    let uuid = path.file_name()?.to_string_lossy().to_string();
    if !UUID_REGEX.is_match(&uuid) {
        return None;
    }
    let containers = PathBuf::from(format!("{home}/Library/Containers"));
    if let Ok(entries) = std::fs::read_dir(&containers) {
        for entry in entries.flatten() {
            if entry.file_name().to_string_lossy() == uuid {
                let metadata = entry
                    .path()
                    .join(".com.apple.containermanagerd.metadata.plist");
                if let Ok(data) = std::fs::read(&metadata) {
                    if let Ok(dict) = plist::from_bytes::<plist::Dictionary>(&data) {
                        if let Some(bundle_id) = dict
                            .get("MCMMetadataIdentifier")
                            .and_then(|v| v.as_string())
                        {
                            return Some(bundle_id.to_string());
                        }
                    }
                }
            }
        }
    }
    None
}

/// UUID（无连字符，32 位 hex）判断（ReversePathsFetch.swift:280-285）
fn is_uuid_formatted(file_name: &str) -> bool {
    file_name.len() == 32 && file_name.chars().all(|c| c.is_ascii_hexdigit())
}

impl ReversePathsSearcher {
    pub fn new(locations: Locations, sorted_apps: Vec<AppInfo>) -> ReversePathsSearcher {
        let cached_apps = sorted_apps
            .iter()
            .map(|app| CachedAppIdentifiers {
                formatted_bundle_id: pear_format(&app.bundle_identifier),
                formatted_app_name: pear_format(&app.app_name),
                formatted_entitlements: app
                    .entitlements
                    .iter()
                    .filter_map(|e| {
                        let f = pear_format(e);
                        if f.is_empty() {
                            None
                        } else {
                            Some(f)
                        }
                    })
                    .collect(),
            })
            .collect();
        ReversePathsSearcher {
            locations,
            collection: Vec::new(),
            cached_apps,
            skip_reverse: conditions::skip_reverse(),
            conditions: conditions::conditions(),
        }
    }

    /// 是否与已安装应用相关（ReversePathsFetch.swift:227-255）
    fn is_related_to_installed_app(&self, path: &Path, normalized_path: &str) -> bool {
        let home = std::env::var("HOME").unwrap_or_default();
        // 容器匹配只对 /Containers/ 路径生效（原版用原始路径判断，
        // 不能用 pearFormat 后的路径——斜杠已被剥除）
        let container_name = if path.to_string_lossy().contains("/Containers/") {
            container_name_by_uuid(path, &home).map(|b| pear_format(&b))
        } else {
            None
        };

        for cached in &self.cached_apps {
            if !cached.formatted_bundle_id.is_empty()
                && cached.formatted_bundle_id.chars().count() >= 5
                && normalized_path.contains(&cached.formatted_bundle_id)
            {
                return true;
            }
            if !cached.formatted_app_name.is_empty()
                && cached.formatted_app_name.chars().count() >= 5
                && normalized_path.contains(&cached.formatted_app_name)
            {
                return true;
            }
            for ent in &cached.formatted_entitlements {
                if !ent.is_empty()
                    && ent.chars().count() >= 5
                    && normalized_path.contains(ent.as_str())
                {
                    return true;
                }
            }
            if let Some(cn) = &container_name {
                if !cached.formatted_bundle_id.is_empty()
                    && cached.formatted_bundle_id.chars().count() >= 5
                    && cn.contains(&cached.formatted_bundle_id)
                {
                    return true;
                }
            }
        }
        false
    }

    /// 条件排除（ReversePathsFetch.swift:257-278）
    fn is_excluded_by_conditions(&self, normalized_path: &str) -> bool {
        for condition in &self.conditions {
            // 仅当条件对应的 app 已安装才生效
            let installed = self.cached_apps.iter().any(|c| {
                c.formatted_bundle_id == condition.bundle_id
                    || c.formatted_bundle_id.contains(&condition.bundle_id)
            });
            if !installed {
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
}
