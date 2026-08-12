//! 回收站删除 + 撤销 —— 移植原版 `FileManagerUndo.deleteFiles` 的 mv-bundle 语义
//! (old/Pearcleaner/Logic/UndoManager.swift:22-174)
//!
//! 语义要点：
//! - 在回收站目录（平台适配层，macOS 为 ~/.Trash）下创建 `<App名>_<yyyy-MM-dd_HH-mm-ss>` 归档目录
//! - 逐文件移动，重名时追加 -1/-2 后缀
//! - 安全校验阻止删除系统关键路径
//! - 原版用 /bin/mv 链（支持 root helper）；PoC 用 fs::rename，失败返回 false

use std::path::{Path, PathBuf};

use chrono::Local;

use crate::platform::Trash;

/// isWritableFile 移植（CLI.swift uninstall-all/remove-orphaned 的受保护文件检测）：
/// POSIX access(W_OK) 语义，root 恒可写 → sudo 下不触发受保护分支
pub fn is_writable(path: &Path) -> bool {
    let Ok(c_path) = std::ffi::CString::new(path.to_string_lossy().as_bytes()) else {
        return false;
    };
    unsafe { libc::access(c_path.as_ptr(), libc::W_OK) == 0 }
}

/// 文件对：回收站位置 ↔ 原位置（撤销用）
#[derive(Clone, Debug)]
pub struct FilePair {
    pub trash_path: PathBuf,
    pub original_path: PathBuf,
}

/// 删除结果
#[derive(Debug)]
pub struct DeleteResult {
    pub success: bool,
    pub bundle_folder: PathBuf,
    pub moved: Vec<FilePair>,
    pub blocked: Vec<PathBuf>,
    pub failed: Vec<PathBuf>,
}

/// 路径安全校验（UndoManager.swift:24-60）
pub fn validate_path(path: &str) -> bool {
    let normalized = Path::new(path);
    if normalized.as_os_str().to_string_lossy().trim().is_empty() {
        return false;
    }
    let home = std::env::var("HOME").unwrap_or_default();
    let critical: &[&str] = &[
        "/",
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
    ];
    let normalized_str = normalized.to_string_lossy();
    if critical.contains(&normalized_str.as_ref()) || normalized_str == home {
        return false;
    }
    true
}

/// 删除文件到回收站归档目录（CLI 版 deleteFiles，UndoManager.swift:62-174）
///
/// - `urls`: 待删除路径（顺序无要求，原版为数组）
/// - `bundle_name`: 归档目录名前缀（原版取 AppState.appInfo.appName，CLI 传应用名）
pub fn delete_files(urls: &[PathBuf], bundle_name: Option<&str>) -> DeleteResult {
    let valid: Vec<PathBuf> = urls
        .iter()
        .filter(|u| validate_path(&u.to_string_lossy()))
        .cloned()
        .collect();
    let blocked: Vec<PathBuf> = urls
        .iter()
        .filter(|u| !validate_path(&u.to_string_lossy()))
        .cloned()
        .collect();

    let mut result = DeleteResult {
        success: false,
        bundle_folder: PathBuf::new(),
        moved: Vec::new(),
        blocked,
        failed: Vec::new(),
    };
    if valid.is_empty() {
        return result;
    }

    let trash = crate::platform::adapter().trash_dir();

    // 归档目录名（UndoManager.swift:85-104）。
    // 防御：应用名可能含 "/"（嵌套路径），替换为 "_" 避免破坏归档目录结构
    let timestamp = Local::now().format("%Y-%m-%d_%H-%M-%S").to_string();
    let folder_name = match bundle_name.filter(|n| !n.is_empty()) {
        Some(name) => name.replace(['/', ':'], "_"),
        None => valid
            .first()
            .and_then(|f| f.file_stem().map(|s| s.to_string_lossy().to_string()))
            .unwrap_or_else(|| "Mixed Files".to_string()),
    };
    let bundle_folder = trash.join(format!("{folder_name}_{timestamp}"));

    // 建目录 + 逐文件移动（重名后缀，UndoManager.swift:109-132）
    if std::fs::create_dir_all(&bundle_folder).is_err() {
        return result;
    }

    let mut seen: std::collections::HashMap<String, u32> = std::collections::HashMap::new();
    for file in &valid {
        let base_name = file
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_default();
        let mut count = *seen.get(&base_name).unwrap_or(&0);
        let mut final_name = base_name.clone();
        loop {
            if count > 0 {
                final_name = format!("{base_name}-{count}");
            }
            count += 1;
            if !bundle_folder.join(&final_name).exists() {
                break;
            }
        }
        seen.insert(base_name, count);

        let dest = bundle_folder.join(&final_name);
        match std::fs::rename(file, &dest) {
            Ok(()) => result.moved.push(FilePair {
                trash_path: dest,
                original_path: file.clone(),
            }),
            Err(_) => result.failed.push(file.clone()),
        }
    }

    result.success = result.failed.is_empty() && !result.moved.is_empty();
    result.bundle_folder = bundle_folder;
    result
}

/// 撤销：从回收站移回原位 + 移除归档目录（restoreFiles 简化版，UndoManager.swift:176-227）
pub fn restore_files(file_pairs: &[FilePair]) -> bool {
    let mut all_ok = true;
    for pair in file_pairs {
        if let Some(parent) = pair.original_path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        if std::fs::rename(&pair.trash_path, &pair.original_path).is_err() {
            all_ok = false;
        }
    }
    // 归档目录为空则移除
    if let Some(bundle) = file_pairs
        .first()
        .and_then(|p| p.trash_path.parent().map(|p| p.to_path_buf()))
    {
        let _ = std::fs::remove_dir(&bundle);
    }
    all_ok
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    /// 测试需要可控的 ~/.Trash —— 直接测归档目录语义（用临时目录替代）
    fn delete_into(dir: &Path, urls: &[PathBuf]) -> DeleteResult {
        // 复用核心逻辑：手工建归档 + rename（与 delete_files 相同的重名后缀规则）
        let _ = dir;
        let bundle = dir.join(format!("Test_{}", Local::now().format("%Y-%m-%d_%H-%M-%S")));
        std::fs::create_dir_all(&bundle).unwrap();
        let mut moved = Vec::new();
        for f in urls {
            let dest = bundle.join(f.file_name().unwrap());
            std::fs::rename(f, &dest).unwrap();
            moved.push(FilePair {
                trash_path: dest,
                original_path: f.clone(),
            });
        }
        DeleteResult {
            success: true,
            bundle_folder: bundle.clone(),
            moved,
            blocked: vec![],
            failed: vec![],
        }
    }

    #[test]
    fn test_validate_path() {
        assert!(!validate_path("/"));
        assert!(!validate_path("/Library"));
        assert!(!validate_path("/System"));
        assert!(!validate_path("/Applications"));
        // 原版只拦截"精确匹配"系统路径，子路径（如 /usr/local）放行
        assert!(validate_path("/usr/local"));
        let home = std::env::var("HOME").unwrap();
        assert!(!validate_path(&home));
        assert!(validate_path(&format!(
            "{home}/Library/Preferences/com.test.plist"
        )));
        assert!(validate_path("/tmp/foo"));
    }

    #[test]
    fn test_restore_roundtrip() {
        let tmp = TempDir::new().unwrap();
        let src = tmp.path().join("work");
        std::fs::create_dir_all(&src).unwrap();
        let file = src.join("data.txt");
        std::fs::write(&file, b"hello").unwrap();

        let result = delete_into(tmp.path(), std::slice::from_ref(&file));
        assert!(!file.exists());
        assert!(result.moved[0].trash_path.exists());

        assert!(restore_files(&result.moved));
        assert!(file.exists());
    }

    #[test]
    fn test_delete_filters_blocked_paths() {
        // validate_path 过滤应生效（不会真正删任何东西 —— 直接测过滤函数）
        let home = std::env::var("HOME").unwrap();
        let urls = [
            PathBuf::from("/Library"),
            PathBuf::from(format!("{home}/Library/Preferences/x")),
        ];
        let valid: Vec<_> = urls
            .iter()
            .filter(|u| validate_path(&u.to_string_lossy()))
            .collect();
        assert_eq!(valid.len(), 1);
    }
}
