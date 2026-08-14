//! XDG Trash 规范（freedesktop trash-spec 1.0）的结构与格式 —— 纯逻辑，平台无关
//!
//! 规范要点（trash-spec-1.0.html）：
//! - 删除 = 把文件移入 `<trash>/files/<name>`，同时在 `<trash>/info/<name>.trashinfo`
//!   写入元数据：`[Trash Info]` 段的 `Path=`（原始绝对路径，percent-encoding）与
//!   `DeletionDate=`（本地时间 ISO 8601）
//! - 同名冲突：追加 `.1` / `.2` 序号后缀（files 与 info 同步）
//! - 恢复：读 info 的 Path 解码出原始路径，把文件从 files/ 移回
//!
//! 平台层职责：提供 trash 根目录（Linux `~/.local/share/Trash` + 各挂载点
//! `.Trash-$uid`；macOS `~/.Trash` 扁平结构不经此模块）与移动动作；
//! 本模块只做结构计算与文本格式，可独立单测。

use std::path::{Path, PathBuf};

/// percent-encoding 的保留字符（规范：除这些外的字节都编码）
fn is_unreserved(b: u8) -> bool {
    matches!(b, b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'/' | b'-' | b'_' | b'.' | b'~')
}

/// 原始绝对路径 → trashinfo 的 Path 字段（percent-encoding，UTF-8 字节级）
pub fn encode_path(path: &str) -> String {
    let mut out = String::with_capacity(path.len());
    for &b in path.as_bytes() {
        if is_unreserved(b) {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

/// trashinfo 的 Path 字段 → 原始绝对路径（percent-decode；非法转义原样保留）
pub fn decode_path(encoded: &str) -> String {
    let bytes = encoded.as_bytes();
    let mut out: Vec<u8> = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            if let (Some(h), Some(l)) = (hex_val(bytes[i + 1]), hex_val(bytes[i + 2])) {
                out.push(h * 16 + l);
                i += 3;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn hex_val(b: u8) -> Option<u8> {
    match b {
        b'0'..=b'9' => Some(b - b'0'),
        b'a'..=b'f' => Some(b - b'a' + 10),
        b'A'..=b'F' => Some(b - b'A' + 10),
        _ => None,
    }
}

/// 生成 trashinfo 内容
pub fn generate_trashinfo(original_path: &str, deletion_date: &str) -> String {
    format!(
        "[Trash Info]\nPath={}\nDeletionDate={}\n",
        encode_path(original_path),
        deletion_date
    )
}

/// 解析 trashinfo 内容 → 原始路径 + 删除时间。缺 Path → None（非法条目）。
pub fn parse_trashinfo(content: &str) -> Option<(String, String)> {
    let mut path_enc: Option<String> = None;
    let mut date = String::new();
    let mut in_section = false;
    for line in content.lines() {
        if line == "[Trash Info]" {
            in_section = true;
            continue;
        }
        if !in_section {
            continue;
        }
        if let Some(v) = line.strip_prefix("Path=") {
            path_enc = Some(v.to_string());
        } else if let Some(v) = line.strip_prefix("DeletionDate=") {
            date = v.to_string();
        }
    }
    let path = decode_path(&path_enc?);
    if path.is_empty() {
        return None;
    }
    Some((path, date))
}

/// info 文件名（`<file_name>.trashinfo`）
pub fn info_file_name(file_name: &str) -> String {
    format!("{file_name}.trashinfo")
}

/// 从 info 文件名还原 files/ 下的条目名（剥 `.trashinfo` 后缀）
pub fn file_name_from_info(info_name: &str) -> Option<&str> {
    info_name.strip_suffix(".trashinfo")
}

/// files/ 目录下同名冲突的序号后缀解析：`name` / `name.1` / `name.2` …
/// 返回不冲突的名字（与 info 名同步使用）。
pub fn unique_name(dir: &Path, name: &str) -> PathBuf {
    let candidate = dir.join(name);
    if !candidate.exists() {
        return candidate;
    }
    let mut i = 1u32;
    loop {
        let candidate = dir.join(format!("{name}.{i}"));
        if !candidate.exists() {
            return candidate;
        }
        i += 1;
    }
}

/// files/ 与 info/ **双侧**冲突检查的唯一定名（2026-08-15 审查 P1-10）：
/// 只查 files/ 时，若 info/<name>.trashinfo 已存在而 files 条目缺失（上次
/// 崩溃残留），新写入会覆盖旧 trashinfo，丢失旧条目的恢复元数据。
/// 注意：exists() 检查与后续 rename 之间非原子（TOCTOU）——并发两个实例
/// 移入同名文件时后到者可能覆盖；单进程内安全，文档化勿并发。
pub fn unique_name_pair(files: &Path, info: &Path, name: &str) -> PathBuf {
    let taken = |candidate: &std::path::Path| -> bool {
        candidate.exists()
            || candidate
                .file_name()
                .and_then(|n| n.to_str())
                .map(|n| info.join(info_file_name(n)).exists())
                .unwrap_or(false)
    };
    if !taken(&files.join(name)) {
        return files.join(name);
    }
    let mut i = 1u32;
    loop {
        let candidate = files.join(format!("{name}.{i}"));
        if !taken(&candidate) {
            return candidate;
        }
        i += 1;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_encode_path_basic() {
        assert_eq!(
            encode_path("/home/user/文件.txt"),
            "/home/user/%E6%96%87%E4%BB%B6.txt"
        );
        assert_eq!(encode_path("/usr/bin/app"), "/usr/bin/app");
        assert_eq!(encode_path("/path with space"), "/path%20with%20space");
    }

    #[test]
    fn test_decode_roundtrip() {
        for p in [
            "/home/user/文件.txt",
            "/path with space/&weird?name#.log",
            "/usr/bin/app",
        ] {
            assert_eq!(decode_path(&encode_path(p)), p);
        }
    }

    #[test]
    fn test_decode_invalid_escape_kept() {
        assert_eq!(decode_path("/a%zz/b%2"), "/a%zz/b%2");
        assert_eq!(decode_path("/a%2"), "/a%2"); // 截断转义
    }

    #[test]
    fn test_generate_and_parse_roundtrip() {
        let info = generate_trashinfo("/home/u/文件.txt", "2026-08-13T22:30:00");
        let (path, date) = parse_trashinfo(&info).unwrap();
        assert_eq!(path, "/home/u/文件.txt");
        assert_eq!(date, "2026-08-13T22:30:00");
    }

    #[test]
    fn test_parse_trashinfo_missing_path_is_none() {
        assert!(parse_trashinfo("[Trash Info]\nDeletionDate=2026-01-01T00:00:00\n").is_none());
        assert!(parse_trashinfo("").is_none());
        assert!(parse_trashinfo("[Other]\nPath=/x\n").is_none()); // 段外 Path 忽略
    }

    #[test]
    fn test_info_file_names() {
        assert_eq!(info_file_name("app.log"), "app.log.trashinfo");
        assert_eq!(file_name_from_info("app.log.trashinfo"), Some("app.log"));
        assert_eq!(file_name_from_info("no-suffix"), None);
    }

    #[test]
    fn test_unique_name_appends_suffix() {
        let tmp = tempfile::TempDir::new().unwrap();
        // 目录为空 → 原名
        assert_eq!(unique_name(tmp.path(), "a"), tmp.path().join("a"));
        // 创建 a → 下一个是 a.1
        std::fs::write(tmp.path().join("a"), b"x").unwrap();
        assert_eq!(unique_name(tmp.path(), "a"), tmp.path().join("a.1"));
        // 创建 a.1 → a.2
        std::fs::write(tmp.path().join("a.1"), b"x").unwrap();
        assert_eq!(unique_name(tmp.path(), "a"), tmp.path().join("a.2"));
    }
}
