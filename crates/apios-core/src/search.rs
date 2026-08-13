//! 相关文件搜索 —— 枚举应用的全部相关文件
//! 覆盖：初始路径处理、容器查找、目录遍历（深度规则 + 供应商目录规则 + 跳过深搜）、
//! 归一化名称匹配、outliers、最终集合整理

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use crate::app_info;
use crate::conditions;
use crate::format::pear_format;
use crate::identifiers::CachedIdentifiers;
use crate::locations::{standard_library_subdirectories, Locations};
use crate::matcher::{should_skip_item, specific_condition};
use crate::model::{AppInfo, Condition, Sensitivity, SkipCondition};
use crate::platform::{SpotlightIndex, SystemPaths};

/// 组件级回收站判断：路径中存在名为 `.Trash` 的路径段。
/// 子串匹配（`contains(".Trash")`）会误伤名为 ".TrashFoo" / "Foo.Trash"
/// 的普通目录；组件判断仅命中真正的回收站段。
fn is_in_trash(path_str: &str) -> bool {
    Path::new(path_str)
        .components()
        .any(|c| c.as_os_str() == ".Trash")
}

pub struct AppPathFinder<'a> {
    pub app: &'a AppInfo,
    pub identifiers: CachedIdentifiers,
    pub locations: &'a Locations,
    pub sensitivity: Sensitivity,
    /// 匹配结果集合（对应原版 collectionSet，Set 语义去重）
    collection: HashSet<PathBuf>,
    /// 容器目录（来自 getAllContainers）
    containers: Vec<PathBuf>,
    conditions: Vec<Condition>,
    skip_conditions: Vec<SkipCondition>,
    standard_subdirs: HashSet<String>,
}

impl<'a> AppPathFinder<'a> {
    pub fn new(
        app: &'a AppInfo,
        locations: &'a Locations,
        sensitivity: Sensitivity,
    ) -> AppPathFinder<'a> {
        let mut finder = AppPathFinder {
            app,
            identifiers: CachedIdentifiers::from_app_info(app),
            locations,
            sensitivity,
            collection: HashSet::new(),
            containers: Vec::new(),
            conditions: conditions::conditions(),
            skip_conditions: conditions::skip_conditions(),
            standard_subdirs: standard_library_subdirectories(),
        };
        finder.containers = finder.get_all_containers();
        finder.initial_url_processing();
        finder
    }

    /// 初始路径处理：插入应用自身（Wrapper 应用上跳两级）
    ///
    /// 原版对整个路径做 `contains("Wrapper")` 子串匹配 —— 任意祖先目录名含 "Wrapper"
    /// （如 /Users/wrapperx/Applications/Foo.app）都会误上跳两级到目录本身，
    /// 整个目录被收进 collection → 删除列表。这里只在**紧邻父目录**名含 "Wrapper"
    /// （真实 wrapped 结构 `…/Wrapper/Foo.app`）时上跳。
    fn initial_url_processing(&mut self) {
        let path_str = self.app.path.to_string_lossy().to_string();
        // .Trash 检测用组件级判断：contains(".Trash") 会误拦
        // 名为 ".TrashFoo" / "Foo.Trash" 的普通目录
        if !is_in_trash(&path_str) {
            let parent_name = self
                .app
                .path
                .parent()
                .and_then(|p| p.file_name())
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_default();
            let modified = if parent_name.contains("Wrapper") {
                self.app
                    .path
                    .parent()
                    .and_then(|p| p.parent())
                    .map(|p| p.to_path_buf())
                    .unwrap_or_else(|| self.app.path.clone())
            } else {
                self.app.path.clone()
            };
            self.collection.insert(modified);
        }
    }

    /// 容器查找 —— UUID 形态的 group 容器扫描。
    /// 设计取舍：按 bundle id 查 group container 的 ObjC API 绝大多数返回 nil，不引入；
    /// 只扫 /Containers 下的 UUID 目录（get_app_containers）。
    fn get_all_containers(&self) -> Vec<PathBuf> {
        let home = crate::platform::adapter().home();
        app_info::get_app_containers(&home, &self.app.bundle_identifier)
    }

    /// 判断位置是否为 Library 根（深度 2 搜索 + skipDeepSearch 生效范围）
    fn is_library_directory(location: &str) -> bool {
        let home = crate::platform::adapter().home();
        location == format!("{home}/Library") || location == "/Library"
    }

    /// 单目录处理
    fn process_location(
        &mut self,
        location: &Path,
        current_depth: usize,
        max_depth: usize,
        is_library_root: bool,
    ) {
        let Ok(entries) = std::fs::read_dir(location) else {
            return;
        };
        let mut local_results: Vec<PathBuf> = Vec::new();
        let mut subdirectories: Vec<PathBuf> = Vec::new();

        for entry in entries.flatten() {
            let path = entry.path();
            let name = entry.file_name().to_string_lossy().to_string();

            // 归一化名称：
            // 目录 → 永不剥扩展名（URL.hasDirectoryPath 语义，含符号链接目标）
            // 文件带扩展名 → 去掉最后一段扩展名再 pearFormat；无扩展名 → 整体 pearFormat
            let normalized_item_name = if !path.is_dir() && path.extension().is_some() {
                let stem = name[..name.rfind('.').unwrap_or(name.len())].to_string();
                pear_format(&stem)
            } else {
                pear_format(&name)
            };

            if should_skip_item(
                &normalized_item_name,
                &path,
                &self.collection,
                &self.skip_conditions,
            ) {
                continue;
            }

            if specific_condition(
                &normalized_item_name,
                &path,
                self.app,
                &self.identifiers,
                self.sensitivity,
                &self.conditions,
            ) {
                // 深度 2 + Library 根搜索：父目录非标准子目录 → 回退加父目录（供应商目录规则）
                let item_to_add = if is_library_root && current_depth == 2 {
                    if let Some(parent) = path.parent() {
                        let parent_name = parent
                            .file_name()
                            .map(|n| n.to_string_lossy().to_string())
                            .unwrap_or_default();
                        if !self.standard_subdirs.contains(&parent_name) {
                            parent.to_path_buf()
                        } else {
                            path.clone()
                        }
                    } else {
                        path.clone()
                    }
                } else {
                    path.clone()
                };
                local_results.push(item_to_add);
            }

            // 目录且未达最大深度 → 收集以待递归
            if path.is_dir() && current_depth < max_depth {
                if is_library_root && current_depth == 0 {
                    let dir_name = path
                        .file_name()
                        .map(|n| n.to_string_lossy().to_string())
                        .unwrap_or_default();
                    if !conditions::skip_deep_search().contains(&dir_name) {
                        subdirectories.push(path);
                    }
                } else {
                    subdirectories.push(path);
                }
            }
        }

        for result in local_results {
            self.collection.insert(result);
        }

        if current_depth < max_depth {
            for subdirectory in subdirectories {
                self.process_location(&subdirectory, current_depth + 1, max_depth, is_library_root);
            }
        }
    }

    /// 收集所有位置（CLI 同步版）
    fn collect_locations_cli(&mut self) {
        for location in &self.locations.apps_paths {
            let is_lib_root = Self::is_library_directory(location);
            let max_depth = if is_lib_root { 2 } else { 1 };
            self.process_location(Path::new(location), 0, max_depth, is_lib_root);
        }
    }

    /// outliers：条件的 includeForce/excludeForce
    fn handle_outliers(&self, include: bool) -> Vec<PathBuf> {
        let mut outliers = Vec::new();
        let bundle_identifier = pear_format(&self.app.bundle_identifier);
        for condition in &self.conditions {
            // 精确匹配（原版 contains 子串 → VS Code Insiders 误命中 stable 的条件表）
            if bundle_identifier == condition.bundle_id {
                if include {
                    outliers.extend(condition.include_force.iter().cloned());
                } else {
                    outliers.extend(condition.exclude_force.iter().cloned());
                }
            }
        }
        outliers
    }

    /// 最终集合整理（CLI 版）
    fn finalize_collection_cli(&self) -> Vec<PathBuf> {
        let outliers = self.handle_outliers(true);
        let outliers_ex = self.handle_outliers(false);

        let mut temp: Vec<PathBuf> = self.collection.iter().cloned().collect();
        temp.extend(self.containers.iter().cloned());
        temp.extend(outliers);

        // Spotlight 补充：只加手动扫描集合外的索引命中
        let spotlight = crate::platform::adapter().spotlight_supplemental_paths(
            &self.app.app_name,
            &self.app.bundle_identifier,
            self.sensitivity,
        );
        temp.extend(
            spotlight
                .into_iter()
                .filter(|p| !self.collection.contains(p)),
        );

        let exclude_paths: HashSet<&Path> = outliers_ex.iter().map(|p| p.as_path()).collect();
        temp.retain(|url| !exclude_paths.contains(url.as_path()));

        // 排序 + 子路径过滤。
        // 原版 CLI 简化只与前一元素比较 → A 是 C 祖先、B 排中间时 C 逃逸；
        // 且相同路径（containers/spotlight/outliers 交叠）不重复去重。
        // 改为与所有已保留元素比较（规模几十到几百，O(n²) 可接受）。
        temp.sort();
        let mut filtered: Vec<PathBuf> = Vec::new();
        for url in &temp {
            let s = url.to_string_lossy();
            let is_sub = filtered.iter().any(|k| {
                let ks = k.to_string_lossy();
                s == ks || s.starts_with(&format!("{ks}/"))
            });
            if !is_sub {
                filtered.push(url.clone());
            }
        }

        // 唯一结果是回收站内文件 → 清空。
        // 组件级 .Trash 判断（见 is_in_trash —— 子串匹配会误伤普通目录名）
        if filtered.len() == 1 && is_in_trash(&filtered[0].to_string_lossy()) {
            filtered.clear();
        }

        filtered
    }

    /// 主入口（对应 findPathsCLI）
    pub fn find_paths_cli(&mut self) -> Vec<PathBuf> {
        if self.app.web_app {
            self.finalize_collection_cli()
        } else {
            self.collect_locations_cli();
            self.finalize_collection_cli()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn make_app(bundle_id: &str, name: &str) -> AppInfo {
        AppInfo {
            path: PathBuf::from(format!("/Applications/{name}.app")),
            bundle_identifier: bundle_id.to_string(),
            app_name: name.to_string(),
            entitlements: vec![],
            team_identifier: None,
            web_app: false,
            steam: false,
            wrapped: false,
        }
    }

    #[test]
    fn test_initial_url_processing_excludes_trash() {
        let app = AppInfo {
            path: PathBuf::from("/Users/u/.Trash/SomeApp.app"),
            ..make_app("com.test.app", "SomeApp")
        };
        let loc = Locations::new();
        let mut finder = AppPathFinder::new(&app, &loc, Sensitivity::Strict);
        finder.initial_url_processing();
        assert!(finder.collection.is_empty(), "回收站内路径不应被加入集合");
    }

    /// 回归：祖先目录名含 "Wrapper"（非紧邻父目录）不得上跳 —— 原版子串匹配会
    /// 把 /Users/wrapperx/Applications 整目录收进 collection
    #[test]
    fn test_initial_url_processing_wrapper_ancestor_no_jump() {
        let app = AppInfo {
            path: PathBuf::from("/Users/wrapperx/Applications/SomeApp.app"),
            ..make_app("com.test.app", "SomeApp")
        };
        let loc = Locations::new();
        let mut finder = AppPathFinder::new(&app, &loc, Sensitivity::Strict);
        finder.initial_url_processing();
        assert_eq!(
            finder.collection.iter().collect::<Vec<_>>(),
            vec![&PathBuf::from("/Users/wrapperx/Applications/SomeApp.app")],
            "普通应用只插入自身"
        );
    }

    /// 真实 wrapped 结构（紧邻父目录为 Wrapper）：上跳两级到 Wrapper 的父目录
    #[test]
    fn test_initial_url_processing_wrapped_app_jumps() {
        let app = AppInfo {
            path: PathBuf::from("/Users/u/Library/Containers/com.x/Data/Wrapper/SomeApp.app"),
            ..make_app("com.test.app", "SomeApp")
        };
        let loc = Locations::new();
        let mut finder = AppPathFinder::new(&app, &loc, Sensitivity::Strict);
        finder.initial_url_processing();
        assert!(finder
            .collection
            .contains(&PathBuf::from("/Users/u/Library/Containers/com.x/Data")));
        assert!(!finder.collection.contains(&PathBuf::from(
            "/Users/u/Library/Containers/com.x/Data/Wrapper"
        )));
    }

    #[test]
    fn test_find_paths_synthetic_tree() {
        // 构造合成目录树：tmp/FakeHome/Library/Application Support/TestApp/ 下放匹配文件
        let tmp = TempDir::new().unwrap();
        let fake_home = tmp.path().join("FakeHome");
        let app_support = fake_home.join("Library/Application Support/TestApp");
        std::fs::create_dir_all(&app_support).unwrap();
        std::fs::create_dir_all(fake_home.join("Library/Preferences")).unwrap();
        std::fs::write(app_support.join("data.plist"), b"x").unwrap();
        std::fs::write(
            fake_home.join("Library/Preferences/com.test.app.plist"),
            b"x",
        )
        .unwrap();
        std::fs::write(fake_home.join("Library/Preferences/Other.app.plist"), b"x").unwrap();

        // 用一个固定的假 home 跑私有方法（直接测 process_location）
        let app = make_app("com.test.app", "TestApp");
        let loc = Locations::new();
        let mut finder = AppPathFinder::new(&app, &loc, Sensitivity::Strict);

        // 遍历合成目录
        finder.process_location(&fake_home, 0, 2, false);
        let paths: Vec<PathBuf> = finder.collection.iter().cloned().collect();
        let joined = paths
            .iter()
            .map(|p| p.to_string_lossy().to_string())
            .collect::<Vec<_>>()
            .join("\n");

        assert!(
            joined.contains("com.test.app.plist"),
            "bundle ID 匹配的 plist 应被找到:\n{joined}"
        );
        assert!(
            joined.contains("TestApp"),
            "应用名匹配的目录应被找到:\n{joined}"
        );
        assert!(
            !joined.contains("Other.app.plist"),
            "无关 plist 不应被找到:\n{joined}"
        );
    }

    #[test]
    fn test_vendor_folder_rule() {
        // 供应商目录规则：Library 根下深度 2 的匹配项，若父目录非标准子目录 → 回退记录父目录
        let tmp = TempDir::new().unwrap();
        let lib = tmp.path().join("Library");
        // 供应商布局：/Library/Objective-See/HelperTools/lulu-data.bin
        // depth1 "Objective-See"、depth2 "HelperTools" 都不含 "lulu"，不会提前命中
        std::fs::create_dir_all(lib.join("Objective-See/HelperTools")).unwrap();
        std::fs::write(lib.join("Objective-See/HelperTools/lulu-data.bin"), b"x").unwrap();

        let app = make_app("com.objective-see.lulu", "LuLu");
        let loc = Locations::new();
        let mut finder = AppPathFinder::new(&app, &loc, Sensitivity::Enhanced);

        // 以 Library 根（is_library_root=true, max_depth=2）遍历
        finder.process_location(&lib, 0, 2, true);
        let paths: Vec<String> = finder
            .collection
            .iter()
            .map(|p| p.to_string_lossy().to_string())
            .collect();

        // 深度2 匹配项是 lulu-data.bin，父目录 HelperTools 非标准 → 应记录 HelperTools
        // （组件匹配而非字符串 ends_with：Windows 分隔符是反斜杠）
        assert!(
            paths
                .iter()
                .any(|p| Path::new(p).ends_with(Path::new("Objective-See/HelperTools"))),
            "应回退到供应商目录: {paths:?}"
        );
        assert!(
            paths.iter().all(|p| !p.ends_with("lulu-data.bin")),
            "不应直接记录深度2 文件本身: {paths:?}"
        );
    }

    #[test]
    fn test_standard_subdir_adds_file_itself() {
        // 标准子目录（Application Support）下深度 2 匹配 → 记录文件本身，不回退
        let tmp = TempDir::new().unwrap();
        let lib = tmp.path().join("Library");
        std::fs::create_dir_all(lib.join("Application Support")).unwrap();
        std::fs::write(lib.join("Application Support/lulu-data.bin"), b"x").unwrap();

        let app = make_app("com.objective-see.lulu", "LuLu");
        let loc = Locations::new();
        let mut finder = AppPathFinder::new(&app, &loc, Sensitivity::Enhanced);

        finder.process_location(&lib, 0, 2, true);
        let paths: Vec<String> = finder
            .collection
            .iter()
            .map(|p| p.to_string_lossy().to_string())
            .collect();

        assert!(
            paths
                .iter()
                .any(|p| Path::new(p).ends_with(Path::new("Application Support/lulu-data.bin"))),
            "标准子目录下应记录文件本身: {paths:?}"
        );
    }
}
