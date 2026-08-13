//! 插件扫描 —— 18 个插件分类（Audio / PreferencePanes / QuickLook / …），每分类
//! 一组目录（用户级 + 系统级）
//! - 分类路径表由平台适配层提供（`PluginPaths` trait：macOS 全表，其他平台为空）
//! - 过滤规则（should_include）为纯逻辑，按分类的后缀/目录语义判定
//! - 扫描只列目录一层，不递归
//! - 目录条目大小实时统计（CLI 一次性给出）；隐藏文件过滤用统一规则

use std::path::{Path, PathBuf};

use crate::dev_env::dir_size;

/// 一个插件分类：分类名 + 搜索路径表
#[derive(Clone)]
pub struct PluginCategory {
    pub name: String,
    pub paths: Vec<String>,
}

/// 扫描到的插件条目（插件 = 分类目录下的一个文件或目录）
pub struct Plugin {
    pub name: String,
    pub path: PathBuf,
    pub category: String,
    pub is_directory: bool,
    pub size: u64,
}

/// 过滤规则：该分类下，给定名字/类型的条目是否算插件。
/// 大小写不敏感。
pub fn should_include(name: &str, is_dir: bool, category: &str) -> bool {
    let n = name.to_lowercase();
    match category {
        "Audio" => true, // 音频组件/VST/AU 等，全部收录
        "PreferencePanes" => n.ends_with(".prefpane"),
        "QuickLook" => n.ends_with(".qlgenerator"),
        "Screen Savers" => n.ends_with(".saver"),
        "Internet Plug-Ins" => n.ends_with(".plugin") || n.ends_with(".webplugin"),
        "Core Image" => n.ends_with(".plugin"),
        "ColorPickers" => n.ends_with(".colorpicker"),
        "Fonts" => {
            n.ends_with(".ttf")
                || n.ends_with(".otf")
                || n.ends_with(".dfont")
                || n.ends_with(".ttc")
        }
        "Dictionaries" => n.ends_with(".dictionary"),
        "Automator" => n.ends_with(".action") || n.ends_with(".workflow"),
        "Safari Extensions" => n.ends_with(".safariextz") || n.ends_with(".appex"),
        "Motion Templates" => is_dir || n.contains("template") || n.ends_with(".motn"),
        "Spotlight" => n.ends_with(".mdimporter"),
        "Services" => n.ends_with(".service"),
        "Address Book" => is_dir || n.ends_with(".plugin"),
        "Contextual Menu" => is_dir || n.ends_with(".plugin") || n.ends_with(".bundle"),
        "Input Methods" => is_dir || n.ends_with(".app") || n.ends_with(".bundle"),
        "Widgets" => n.ends_with(".wdgt") || n.ends_with(".appex"),
        _ => true, // 未知分类：全部收录（原版 default）
    }
}

/// 扫描全部分类：每路径列目录一层（不递归，原版语义），隐藏条目跳过，
/// 命中 `should_include` 的条目统计大小（目录用 dir_size，文件用 len）。
/// 大小统计并行（rayon，目录多为大目录；顺序无关，归并即可）。
pub fn scan_plugins(categories: &[PluginCategory]) -> Vec<Plugin> {
    let mut raw: Vec<(String, PathBuf, bool, String)> = Vec::new();
    for cat in categories {
        for path in &cat.paths {
            let dir = Path::new(path);
            let Ok(entries) = std::fs::read_dir(dir) else {
                continue; // 路径不存在/不可读：跳过（原版 fileExists 守卫）
            };
            for entry in entries.flatten() {
                let name = entry.file_name().to_string_lossy().into_owned();
                if name.starts_with('.') {
                    continue; // 隐藏文件（原版 skipsHiddenFiles）
                }
                let is_dir = entry.path().is_dir(); // 跟随符号链接（原版 URL.isDirectory）
                if should_include(&name, is_dir, &cat.name) {
                    raw.push((name, entry.path(), is_dir, cat.name.clone()));
                }
            }
        }
    }

    use rayon::prelude::*;
    raw.into_par_iter()
        .map(|(name, path, is_dir, category)| {
            let size = if is_dir {
                dir_size(&path)
            } else {
                std::fs::metadata(&path).map(|m| m.len()).unwrap_or(0)
            };
            Plugin {
                name,
                path,
                category,
                is_directory: is_dir,
                size,
            }
        })
        .collect()
}

/// 按分类分组（保持分类表顺序，空分类剔除）
pub fn group_by_category(plugins: Vec<Plugin>) -> Vec<(String, Vec<Plugin>)> {
    let mut out: Vec<(String, Vec<Plugin>)> = Vec::new();
    for p in plugins {
        match out.iter_mut().find(|(n, _)| *n == p.category) {
            Some((_, list)) => list.push(p),
            None => out.push((p.category.clone(), vec![p])),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_should_include_extensions() {
        assert!(should_include("ZoomAudioDevice.driver", false, "Audio"));
        assert!(should_include("libfoo.vst", true, "Audio"));
        assert!(should_include("Apple.prefPane", false, "PreferencePanes"));
        assert!(!should_include("Readme.txt", false, "PreferencePanes"));
        assert!(should_include("Sierra.qlgenerator", false, "QuickLook"));
        assert!(should_include("Fliqlo.saver", false, "Screen Savers"));
        assert!(should_include(
            "Flash Player.plugin",
            false,
            "Internet Plug-Ins"
        ));
        assert!(should_include("foo.webplugin", false, "Internet Plug-Ins"));
        assert!(should_include("GaussianBlur.plugin", false, "Core Image"));
        assert!(!should_include("GaussianBlur.so", false, "Core Image"));
        assert!(should_include("custom.colorPicker", false, "ColorPickers"));
        assert!(should_include("Mona.ttf", false, "Fonts"));
        assert!(should_include("Mona.otf", false, "Fonts"));
        assert!(should_include("Dict.dictionary", false, "Dictionaries"));
        assert!(should_include("Convert.action", false, "Automator"));
        assert!(should_include("run.workflow", false, "Automator"));
        assert!(should_include("ext.safariextz", false, "Safari Extensions"));
        assert!(should_include("ext.appex", false, "Safari Extensions"));
        assert!(should_include("My Template", true, "Motion Templates"));
        assert!(should_include("x.motn", false, "Motion Templates"));
        assert!(should_include("imp.mdimporter", false, "Spotlight"));
        assert!(should_include("svc.service", false, "Services"));
        assert!(should_include("AddNote", true, "Address Book"));
        assert!(should_include("Foo.bundle", false, "Contextual Menu"));
        assert!(should_include("SogouInput", true, "Input Methods"));
        assert!(should_include("gadget.wdgt", false, "Widgets"));
    }

    #[test]
    fn test_should_include_case_insensitive() {
        assert!(should_include("FLIQLO.SAVER", false, "Screen Savers"));
        assert!(should_include("Mona.TTF", false, "Fonts"));
    }

    #[test]
    fn test_scan_plugins_filters_by_extension_and_hidden() {
        let tmp = tempfile::TempDir::new().unwrap();
        let dir = tmp.path().join("plugins");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("Foo.saver"), "x").unwrap();
        std::fs::write(dir.join("Readme.txt"), "x").unwrap();
        std::fs::write(dir.join(".hidden.saver"), "x").unwrap();
        std::fs::create_dir_all(dir.join("Bundle.app")).unwrap();

        let cats = vec![PluginCategory {
            name: "Screen Savers".into(),
            paths: vec![dir.to_string_lossy().into_owned()],
        }];
        let found = scan_plugins(&cats);
        let names: Vec<&str> = found.iter().map(|p| p.name.as_str()).collect();
        assert!(names.contains(&"Foo.saver"));
        assert!(!names.contains(&"Readme.txt"));
        assert!(!names.contains(&".hidden.saver"));
        assert!(!names.contains(&"Bundle.app")); // .app 不属于 Screen Savers
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].size, 1);
    }

    #[test]
    fn test_scan_missing_dir_skipped() {
        let cats = vec![PluginCategory {
            name: "Audio".into(),
            paths: vec!["/nonexistent/plugins".into()],
        }];
        assert!(scan_plugins(&cats).is_empty());
    }
}
