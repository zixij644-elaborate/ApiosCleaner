//! 开发环境缓存清理 —— 移植原版 `PathLibrary` 的路径表 + 目录大小统计
//! (old/Pearcleaner/Views/DevelopmentView.swift:1603-1750, 884-910)
//!
//! 语义要点：
//! - 每个"开发环境"是一组路径（~ 展开 + 支持 `*` 通配段），大多是可再生的缓存/包存储
//! - 清理 = 把目录**内容**移入回收站（原版 deleteFolderContents），保留目录本身
//! - 大小统计 = 递归求和（跳过隐藏文件，与原版 skipsHiddenFiles 一致）

use std::path::{Path, PathBuf};

use rayon::prelude::*;

/// 单个开发环境（原版 PathEnv）
#[derive(Debug, Clone)]
pub struct DevEnv {
    pub name: String,
    pub paths: Vec<String>,
}

/// 环境 + 各存在路径的大小（字节）
pub type EnvSize = (DevEnv, Vec<(PathBuf, u64)>);

/// 路径表（PathLibrary.getPaths 移植，26 个环境）。
/// 与原版差异：Nix 移除了 `/nix/store/` —— 系统级包存储，CLI 一键清空不可接受；
/// 保留可再生的 `~/.cache/nix/`。
pub fn dev_environments() -> Vec<DevEnv> {
    vec![
        DevEnv {
            name: "Android Studio".into(),
            paths: [
                "~/.android/",
                "~/Library/Application Support/Google/AndroidStudio*/",
                "~/Library/Logs/AndroidStudio/",
                "~/Library/Caches/Google/AndroidStudio*/",
            ]
            .map(str::to_string)
            .to_vec(),
        },
        DevEnv {
            name: "Cargo".into(),
            paths: ["~/.cargo/", "~/.cargo/git/", "~/.cargo/registry/"]
                .map(str::to_string)
                .to_vec(),
        },
        DevEnv {
            name: "Carthage".into(),
            paths: ["~/Carthage/", "~/Library/Caches/org.carthage.CarthageKit/"]
                .map(str::to_string)
                .to_vec(),
        },
        DevEnv {
            name: "CocoaPods".into(),
            paths: ["~/Library/Caches/CocoaPods/", "~/.cocoapods/repos/"]
                .map(str::to_string)
                .to_vec(),
        },
        DevEnv {
            name: "Composer".into(),
            paths: ["~/.composer/cache/"].map(str::to_string).to_vec(),
        },
        DevEnv {
            name: "Conda".into(),
            paths: ["~/.conda/", "~/anaconda3/", "~/miniconda3/"]
                .map(str::to_string)
                .to_vec(),
        },
        DevEnv {
            name: "Cursor".into(),
            paths: [
                "~/Library/Application Support/Cursor/",
                "~/Library/Application Support/Cursor/Cache",
                "~/Library/Application Support/Cursor/GPUCache",
                "~/Library/Application Support/Cursor/CachedConfigurations",
                "~/Library/Application Support/Cursor/CachedData",
                "~/Library/Application Support/Cursor/CachedExtensionVSIXs",
                "~/Library/Application Support/Cursor/CachedExtensions",
                "~/Library/Application Support/Cursor/CachedProfilesData",
                "~/Library/Application Support/Cursor/Code Cache",
                "~/Library/Application Support/Cursor/User",
                "~/.cursor/",
                "~/.cursor/extensions/",
            ]
            .map(str::to_string)
            .to_vec(),
        },
        DevEnv {
            name: "Deno".into(),
            paths: ["~/Library/Caches/deno"].map(str::to_string).to_vec(),
        },
        DevEnv {
            name: "Go Modules".into(),
            paths: ["~/go/bin/", "~/go/pkg/mod/"].map(str::to_string).to_vec(),
        },
        DevEnv {
            name: "Gradle".into(),
            paths: ["~/.gradle/caches/", "~/.gradle/wrapper/"]
                .map(str::to_string)
                .to_vec(),
        },
        DevEnv {
            name: "Haskell Stack".into(),
            paths: [
                "~/.stack/",
                "~/.stack/global-project/",
                "~/.stack/snapshots/",
            ]
            .map(str::to_string)
            .to_vec(),
        },
        DevEnv {
            name: "IntelliJ IDEA".into(),
            paths: [
                "~/Library/Application Support/JetBrains/",
                "~/Library/Caches/JetBrains/",
                "~/Library/Logs/JetBrains/",
            ]
            .map(str::to_string)
            .to_vec(),
        },
        DevEnv {
            name: "Maven".into(),
            paths: ["~/.m2/"].map(str::to_string).to_vec(),
        },
        DevEnv {
            name: "Nix".into(),
            paths: ["~/.cache/nix/"].map(str::to_string).to_vec(),
        },
        DevEnv {
            name: "Npm".into(),
            paths: [
                "/usr/local/lib/node_modules/",
                "~/.nvm/versions/node/*/",
                "~/.npm/",
                "~/.nvm/",
                "~/Library/pnpm/store",
                "~/.bun/install/cache",
            ]
            .map(str::to_string)
            .to_vec(),
        },
        DevEnv {
            name: "Pip".into(),
            paths: ["~/Library/Caches/pip/"].map(str::to_string).to_vec(),
        },
        DevEnv {
            name: "Poetry".into(),
            paths: [
                "~/Library/Caches/pypoetry/",
                "~/Library/Application Support/pypoetry/",
            ]
            .map(str::to_string)
            .to_vec(),
        },
        DevEnv {
            name: "Pub".into(),
            paths: ["~/.pub-cache/", "~/Library/Caches/flutter_engine/"]
                .map(str::to_string)
                .to_vec(),
        },
        DevEnv {
            name: "Pyenv".into(),
            paths: ["~/.pyenv/", "~/.pyenv/cache/"]
                .map(str::to_string)
                .to_vec(),
        },
        DevEnv {
            name: "Ruby Gems".into(),
            paths: ["~/.gem/", "~/.gem/ruby/*/"].map(str::to_string).to_vec(),
        },
        DevEnv {
            name: "Swift".into(),
            paths: ["~/.swiftpm/"].map(str::to_string).to_vec(),
        },
        DevEnv {
            name: "Uv".into(),
            paths: ["~/.cache/uv/", "~/.config/uv/", "~/.local/share/uv/"]
                .map(str::to_string)
                .to_vec(),
        },
        DevEnv {
            name: "VS Code".into(),
            paths: [
                "~/Library/Application Support/Code/",
                "~/Library/Application Support/Code/Cache",
                "~/Library/Application Support/Code/GPUCache",
                "~/Library/Application Support/Code/CachedConfigurations",
                "~/Library/Application Support/Code/CachedData",
                "~/Library/Application Support/Code/CachedExtensionVSIXs",
                "~/Library/Application Support/Code/CachedExtensions",
                "~/Library/Application Support/Code/CachedProfilesData",
                "~/Library/Application Support/Code/Code Cache",
                "~/Library/Application Support/Code/User",
                "~/.vscode/",
                "~/.vscode/extensions/",
                "~/.vscode/cli/",
            ]
            .map(str::to_string)
            .to_vec(),
        },
        DevEnv {
            name: "Xcode".into(),
            paths: [
                "~/Library/Caches/com.apple.dt.xcodebuild/",
                "~/Library/Caches/com.apple.dt.Xcode.sourcecontrol.Git/",
                "~/Library/Developer/CoreSimulator/Devices/",
                "~/Library/Developer/DeveloperDiskImages/",
                "~/Library/Developer/Xcode/Archives/",
                "~/Library/Developer/Xcode/DerivedData/",
                "~/Library/Developer/Xcode/DocumentationCache/",
                "~/Library/Developer/Xcode/iOS DeviceSupport/",
                "~/Library/Developer/Xcode/tvOS DeviceSupport/",
                "~/Library/Developer/Xcode/watchOS DeviceSupport/",
                "~/Library/Developer/Xcode/macOS DeviceSupport/",
                "~/Library/Developer/Xcode/UserData/",
            ]
            .map(str::to_string)
            .to_vec(),
        },
        DevEnv {
            name: "Yarn".into(),
            paths: ["~/.cache/yarn/", "~/.yarn-cache/", "~/.yarn/global/"]
                .map(str::to_string)
                .to_vec(),
        },
        DevEnv {
            name: "Zed".into(),
            paths: [
                "~/.config/zed/",
                "~/Library/Caches/Zed/",
                "~/Library/Application Support/Zed/",
                "~/Library/Application Support/Zed/node/cache/",
            ]
            .map(str::to_string)
            .to_vec(),
        },
    ]
}

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

/// 展开含 `*` 的路径段：匹配父目录下的条目（原版路径表里的 AndroidStudio*/node/* 等）
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

/// 目录大小（递归求和，跳过隐藏项 —— 原版 skipsHiddenFiles）
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
            match entry.metadata() {
                Ok(m) if m.is_dir() => stack.push(p),
                Ok(m) => total += m.len(),
                Err(_) => {}
            }
        }
    }
    total
}

/// 嵌套去重：若路径的祖先已在列表中（其大小已包含父目录），移除子路径。
/// 路径表常同时列父目录与子目录（如 `~/.cargo/` + `~/.cargo/registry/`），
/// 合并清理时父目录已覆盖子目录，避免重复计算与重复删除。
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
    let home = std::env::var("HOME").unwrap_or_default();
    dev_environments()
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
        let combined = dev_environments()
            .into_iter()
            .flat_map(|e| e.paths)
            .collect::<Vec<_>>();
        return Some(DevEnv {
            name: "All".into(),
            paths: combined,
        });
    }
    dev_environments()
        .into_iter()
        .find(|e| e.name.to_lowercase() == lower)
}

/// 人类可读大小（原版 ByteCountFormatter 风格）
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
        let env = find_env("xcode").expect("xcode");
        assert_eq!(env.name, "Xcode");
        assert!(env.paths.iter().any(|p| p.contains("DerivedData")));

        let all = find_env("all").expect("all");
        assert!(all.paths.len() > 50); // 全部环境路径合并
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

    #[test]
    fn test_nix_store_excluded() {
        // 防御性差异：/nix/store/ 不得出现在路径表中
        let nix = dev_environments()
            .into_iter()
            .find(|e| e.name == "Nix")
            .unwrap();
        assert!(!nix.paths.iter().any(|p| p.contains("/nix/store")));
    }
}
