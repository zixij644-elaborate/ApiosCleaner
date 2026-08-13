//! 开发环境缓存清理 —— 核心纯逻辑（平台无关）
//!
//! 路径表按平台分布在适配层（trait `DevEnvPaths`：macOS 完整表 / Linux 子集），
//! 本模块只保留与平台无关的部分：
//! - `~` 展开、`*` 通配段展开、目录大小递归统计、嵌套去重、人类可读大小
//! - `find_env` / `env_sizes` 从当前平台适配器取路径表
//!
//! 清理语义：把目录**内容**移入回收站，保留目录本身。
//! 路径表收紧原则（2026-08-12）：只列**可再生缓存**（DerivedData、各 cache、
//! registry 等），不列工具本体（~/.cargo、~/.nvm、conda 发行版）、配置（Application
//! Support 根、.config 根、User）、用户数据（Xcode Archives、模拟器设备）。

use std::path::{Path, PathBuf};

use rayon::prelude::*;

use crate::platform::{DevEnvPaths, SystemPaths};

/// 单个开发环境
#[derive(Debug, Clone)]
pub struct DevEnv {
    pub name: String,
    pub paths: Vec<String>,
}

/// 环境 + 各存在路径的大小（字节）
pub type EnvSize = (DevEnv, Vec<(PathBuf, u64)>);

/// 展开 `~`（首段为 `~` 或 `~/` 时替换为 home 路径）
pub fn expand_home(pattern: &str, home: &str) -> String {
    if pattern == "~" {
        home.to_string()
    } else if let Some(rest) = pattern.strip_prefix("~/") {
        format!("{home}/{rest}")
    } else {
        pattern.to_string()
    }
}

/// 展开含 `*` 的路径段：匹配父目录下的条目（路径表里的 AndroidStudio*/node/* 等）
pub fn expand_globs(pattern: &Path) -> Vec<PathBuf> {
    let pattern_str = pattern.to_string_lossy();
    if !pattern_str.contains('*') {
        return vec![pattern.to_path_buf()];
    }

    // 逐段展开：对含通配的段，枚举其父目录并过滤匹配
    let mut results = vec![PathBuf::new()];
    for segment in pattern.components() {
        let seg = segment.as_os_str().to_string_lossy().to_string();
        if seg.contains('*') {
            let mut expanded = Vec::new();
            for prefix in &results {
                let Ok(entries) = std::fs::read_dir(prefix) else {
                    continue;
                };
                // 通配段 → 正则（* 匹配任意字符），枚举父目录过滤
                let mut re_str = String::from("^");
                for chunk in seg.split('*') {
                    re_str.push_str(&regex::escape(chunk));
                    re_str.push_str(".*");
                }
                re_str.push('$');
                let Ok(re) = regex::Regex::new(&re_str) else {
                    continue;
                };
                for entry in entries.flatten() {
                    let name = entry.file_name().to_string_lossy().to_string();
                    if re.is_match(&name) {
                        expanded.push(prefix.join(name));
                    }
                }
            }
            results = expanded;
        } else {
            results = results.into_iter().map(|r| r.join(&seg)).collect();
        }
    }
    results
}

/// 目录大小（递归求和，跳过隐藏项）。
/// 用 `file_type()`（lstat 语义）判定目录 —— `metadata()` 跟随符号链接，
/// 指向树内祖先的链接会让递归永不终止。
pub fn dir_size(path: &Path) -> u64 {
    let mut total = 0u64;
    let mut stack = vec![path.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().to_string();
            if name.starts_with('.') {
                continue;
            }
            let p = entry.path();
            match entry.file_type() {
                Ok(t) if t.is_dir() => stack.push(p),
                Ok(t) if t.is_file() => total += entry.metadata().map(|m| m.len()).unwrap_or(0),
                _ => {} // 符号链接/其他：不计（不跟随，防环）
            }
        }
    }
    total
}

/// 嵌套去重：若路径的祖先已在列表中（其大小已包含父目录），移除子路径。
/// 路径表常同时列父目录与子目录（如 `~/.cargo/git/` 与 `~/.cargo/registry/` 无重叠，
/// 但个别条目存在嵌套），合并清理时父目录已覆盖子目录，避免重复计算与重复删除。
pub fn dedup_nested(dirs: &mut Vec<PathBuf>) {
    dirs.sort();
    dirs.dedup();
    let mut i = 0;
    while i < dirs.len() {
        if dirs[..i].iter().any(|o| dirs[i].starts_with(o)) {
            dirs.remove(i);
        } else {
            i += 1;
        }
    }
}

/// 所有环境的大小（rayon 并行：每个环境的路径独立，互不依赖）
pub fn env_sizes() -> Vec<EnvSize> {
    // HOME 统一走适配器（单一事实来源；不直接读环境变量）
    let home = crate::platform::adapter().home();
    crate::platform::adapter()
        .dev_envs()
        .into_par_iter()
        .map(|env| {
            let mut dirs: Vec<PathBuf> = env
                .paths
                .iter()
                .flat_map(|p| {
                    let expanded = expand_home(p, &home);
                    expand_globs(Path::new(&expanded))
                        .into_iter()
                        .filter(|p| p.is_dir())
                        .collect::<Vec<_>>()
                })
                .collect();
            dedup_nested(&mut dirs);
            let sizes = dirs
                .into_iter()
                .map(|dir| {
                    let size = dir_size(&dir);
                    (dir, size)
                })
                .collect();
            (env, sizes)
        })
        .collect()
}

/// 按名称查找环境（大小写不敏感；"all" 返回全部合并）
pub fn find_env(name: &str) -> Option<DevEnv> {
    let lower = name.to_lowercase();
    if lower == "all" {
        let combined = crate::platform::adapter()
            .dev_envs()
            .into_iter()
            .flat_map(|e| e.paths)
            .collect::<Vec<_>>();
        return Some(DevEnv {
            name: "All".into(),
            paths: combined,
        });
    }
    crate::platform::adapter()
        .dev_envs()
        .into_iter()
        .find(|e| e.name.to_lowercase() == lower)
}

/// 人类可读大小（ByteCountFormatter 风格）
pub fn fmt_size(bytes: u64) -> String {
    const UNITS: [&str; 5] = ["B", "KB", "MB", "GB", "TB"];
    let mut value = bytes as f64;
    let mut unit = 0;
    while value >= 1024.0 && unit < UNITS.len() - 1 {
        value /= 1024.0;
        unit += 1;
    }
    if unit == 0 {
        format!("{bytes} B")
    } else {
        format!("{value:.1} {}", UNITS[unit])
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn test_expand_home() {
        assert_eq!(expand_home("~", "/Users/u"), "/Users/u");
        assert_eq!(
            expand_home("~/Library/Caches", "/Users/u"),
            "/Users/u/Library/Caches"
        );
        assert_eq!(expand_home("/usr/local/lib", "/Users/u"), "/usr/local/lib");
        assert_eq!(expand_home("~x/y", "/Users/u"), "~x/y"); // 非 ~ 或 ~/ 开头不展开
    }

    #[test]
    fn test_expand_globs_matches_wildcard_segment() {
        let tmp = TempDir::new().unwrap();
        std::fs::create_dir_all(tmp.path().join("AndroidStudio4.2")).unwrap();
        std::fs::create_dir_all(tmp.path().join("AndroidStudio2024")).unwrap();
        std::fs::create_dir_all(tmp.path().join("Other")).unwrap();

        let pattern = tmp.path().join("AndroidStudio*");
        let matches = expand_globs(&pattern);
        assert_eq!(matches.len(), 2);
        assert!(matches
            .iter()
            .all(|m| m.to_string_lossy().contains("AndroidStudio")));
    }

    #[test]
    fn test_expand_globs_no_wildcard_returns_path() {
        let p = Path::new("/tmp/plain");
        assert_eq!(expand_globs(p), vec![PathBuf::from("/tmp/plain")]);
    }

    #[test]
    fn test_dir_size_skips_hidden() {
        let tmp = TempDir::new().unwrap();
        std::fs::write(tmp.path().join("a.bin"), vec![0u8; 100]).unwrap();
        std::fs::write(tmp.path().join(".hidden.bin"), vec![0u8; 999]).unwrap();
        std::fs::create_dir(tmp.path().join("sub")).unwrap();
        std::fs::write(tmp.path().join("sub/b.bin"), vec![0u8; 50]).unwrap();
        std::fs::write(tmp.path().join("sub/.h.bin"), vec![0u8; 999]).unwrap();

        assert_eq!(dir_size(tmp.path()), 150); // 100 + 50，隐藏项不计
    }

    #[test]
    fn test_dir_size_missing_path_is_zero() {
        assert_eq!(dir_size(Path::new("/nonexistent/zzz")), 0);
    }

    #[test]
    fn test_find_env_case_insensitive() {
        // 平台无关：Cargo 在 macOS/Linux/Windows 三张表都有（Xcode 仅 macOS 表）
        let env = find_env("cargo").expect("cargo");
        assert_eq!(env.name, "Cargo");

        // "all" 等于全部环境的路径合并（与平台表规模无关，逐元素精确比对）
        let all = find_env("all").expect("all");
        let combined: Vec<_> = crate::platform::adapter()
            .dev_envs()
            .into_iter()
            .flat_map(|e| e.paths)
            .collect();
        assert_eq!(all.paths, combined);
        assert!(!all.paths.is_empty()); // 三平台表均非空
    }

    #[test]
    fn test_fmt_size() {
        assert_eq!(fmt_size(0), "0 B");
        assert_eq!(fmt_size(512), "512 B");
        assert_eq!(fmt_size(2048), "2.0 KB");
        assert_eq!(fmt_size(3 * 1024 * 1024), "3.0 MB");
        assert_eq!(fmt_size(2 * 1024 * 1024 * 1024u64), "2.0 GB");
    }

    #[test]
    fn test_dedup_nested_removes_children() {
        let mut dirs = vec![
            PathBuf::from("/a/b"),
            PathBuf::from("/a"),
            PathBuf::from("/a/c"),
            PathBuf::from("/x"),
        ];
        dedup_nested(&mut dirs);
        assert_eq!(dirs, vec![PathBuf::from("/a"), PathBuf::from("/x")]);
    }

    #[test]
    fn test_dedup_nested_no_change() {
        let mut dirs = vec![
            PathBuf::from("/a"),
            PathBuf::from("/b"),
            PathBuf::from("/c"),
        ];
        dedup_nested(&mut dirs);
        assert_eq!(dirs.len(), 3);
    }

    #[test]
    fn test_dedup_nested_empty() {
        let mut dirs: Vec<PathBuf> = vec![];
        dedup_nested(&mut dirs);
        assert!(dirs.is_empty());
    }
}
