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

use crate::platform::{SystemPaths, Trash};

/// isWritableFile 移植（CLI.swift uninstall-all/remove-orphaned 的受保护文件检测）：
/// POSIX rename 语义 —— 需要的是**父目录**可写，而非条目自身（条目只读不影响 mv）。
/// 原版 FileManager.isWritableFile 测条目本身 → 只读文件被误报为受保护。
/// root 恒可写 → sudo 下不触发受保护分支。
///
/// Windows：恒 true（注册表卸载的应用通常可写；只读属性不阻 SHFileOperation 移动；
/// 真正的阻断是文件占用 sharing violation，走 failed 列表 + taskkill 提示）。
pub fn is_writable(path: &Path) -> bool {
    #[cfg(not(windows))]
    {
        let parent = path.parent().unwrap_or_else(|| Path::new("/"));
        let Ok(c_path) = std::ffi::CString::new(parent.to_string_lossy().as_bytes()) else {
            return false;
        };
        unsafe { libc::access(c_path.as_ptr(), libc::W_OK) == 0 }
    }
    #[cfg(windows)]
    {
        let _ = path;
        true
    }
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

/// 词法归一化绝对路径：折叠 `.`/`..`、合并重复分隔符、去掉尾部斜杠。
/// `..` 越界收缩到根（`/Users/../Library` → `/Library`）。
/// 相对路径（不以 / 开头）返回 None —— 删除列表必须是绝对路径，相对路径说明上游有 bug。
///
/// Windows：保留盘符 Prefix（`C:\Windows` → `C:\Windows`，此前 Prefix 被丢弃会归一化
/// 成 `/Windows` 而绕过 critical 表 —— 安全高危）。POSIX 路径无 Prefix 组件，
/// 分支与旧逻辑逐字节一致。
fn normalize_absolute(path: &str) -> Option<String> {
    use std::path::Component;
    let mut parts: Vec<String> = Vec::new();
    let mut absolute = false;
    let mut prefix: Option<String> = None;
    for comp in Path::new(path).components() {
        match comp {
            Component::Prefix(p) => prefix = Some(p.as_os_str().to_string_lossy().into_owned()),
            Component::RootDir => {
                absolute = true;
                // 仅 POSIX 根清空（旧逻辑）；Windows 盘符后紧跟的 RootDir 不清空
                if prefix.is_none() {
                    parts.clear();
                }
            }
            Component::CurDir => {}
            Component::ParentDir => {
                // 空栈 pop 无害（`C:\..\x` → `C:\x`，语义正确）
                parts.pop();
            }
            Component::Normal(n) => parts.push(n.to_string_lossy().into_owned()),
        }
    }
    if !absolute {
        return None;
    }
    if let Some(p) = prefix {
        Some(format!("{p}\\{}", parts.join("\\")))
    } else if parts.is_empty() {
        Some("/".to_string())
    } else {
        Some(format!("/{}", parts.join("/")))
    }
}

/// 路径安全校验（UndoManager.swift:24-60）。
/// 原版按原始字符串精确匹配 → `..`/`//`/尾部斜杠可绕过 critical 表；
/// 这里先做词法归一化再匹配，再叠加 home 根与（POSIX 专属）{home}/Applications。
/// critical 表来自适配层（SystemPaths::critical_paths，平台路径数据归平台），
/// 子路径（/usr/local、{home}/Library/...）仍放行 —— 是合法删除目标。
pub fn validate_path(path: &str) -> bool {
    let Some(normalized) = normalize_absolute(path) else {
        return false;
    };
    let adapter = crate::platform::adapter();
    let home = adapter.home();
    // 盘符根格式检测（`X:\` 长度为 3，Windows 专属形态；格式检测而非硬编码盘符，
    // 防非 C: 系统）。`/` 是 POSIX 根；Windows 上归一化出 `/` 的根相对路径
    // （如 `\Windows` → `/Windows`）不在此形态内，但不会来自删除列表的路径表。
    let drive_root = normalized.len() == 3
        && normalized.as_bytes()[1] == b':'
        && normalized.as_bytes()[2] == b'\\';
    if drive_root
        || normalized == "/"
        || adapter.critical_paths().contains(&normalized)
        || normalized == home
    {
        return false;
    }
    // 仅 POSIX：{home}/Applications 是受保护区（home 下唯一整体保护的应用目录；
    // Windows 用户目录无此结构，不拦）
    #[cfg(not(windows))]
    {
        let home_apps = format!("{home}/Applications");
        normalized != home_apps
    }
    #[cfg(windows)]
    {
        true
    }
}

/// POSIX 归档式回收站移动（共享核心函数，Trash::move_to_trash 的默认实现）。
/// 在 `trash_dir` 下创建 `<名>_<时间戳>` 归档目录并逐文件移入
/// （重名 -N 后缀 / 跨卷 copy 回退）。Windows 回收站无目录模型，
/// 由 WindowsAdapter 覆写走 SHFileOperationW，本函数仅 POSIX 语义。
///
/// - `urls`: 待删除路径（顺序无要求，原版为数组）
/// - `bundle_name`: 归档目录名前缀（原版取 AppState.appInfo.appName，CLI 传应用名）
pub fn move_to_trash_dir(
    urls: &[PathBuf],
    bundle_name: Option<&str>,
    trash_dir: PathBuf,
) -> DeleteResult {
    let mut result = DeleteResult {
        success: false,
        bundle_folder: PathBuf::new(),
        moved: Vec::new(),
        blocked: Vec::new(),
        failed: Vec::new(),
    };
    if urls.is_empty() {
        return result;
    }

    // 归档目录名（UndoManager.swift:85-104）。
    // 防御：应用名可能含 "/"（嵌套路径），替换为 "_" 避免破坏归档目录结构
    let timestamp = Local::now().format("%Y-%m-%d_%H-%M-%S").to_string();
    let folder_name = match bundle_name.filter(|n| !n.is_empty()) {
        Some(name) => name.replace(['/', ':', '\\', '<', '>', '"', '|', '?', '*'], "_"),
        None => urls
            .first()
            .and_then(|f| f.file_stem().map(|s| s.to_string_lossy().to_string()))
            .unwrap_or_else(|| "Mixed Files".to_string()),
    };
    let bundle_folder = trash_dir.join(format!("{folder_name}_{timestamp}"));

    // 建目录 + 逐文件移动（重名后缀，UndoManager.swift:109-132）
    if std::fs::create_dir_all(&bundle_folder).is_err() {
        return result;
    }

    let mut seen: std::collections::HashMap<String, u32> = std::collections::HashMap::new();
    for file in urls {
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
            // 跨卷（EXDEV）：rename 无法跨文件系统，回退为 copy + remove。
            // 非原子（中断可能留下副本）—— 但回收站通常与源同卷，仅跨卷文件走此路径。
            Err(e) if e.raw_os_error() == Some(libc::EXDEV) => {
                if file.is_file()
                    && std::fs::copy(file, &dest).is_ok()
                    && std::fs::remove_file(file).is_ok()
                {
                    result.moved.push(FilePair {
                        trash_path: dest,
                        original_path: file.clone(),
                    });
                } else {
                    result.failed.push(file.clone());
                }
            }
            Err(_) => result.failed.push(file.clone()),
        }
    }

    result.success = result.failed.is_empty() && !result.moved.is_empty();
    result.bundle_folder = bundle_folder;
    result
}

/// 删除文件到回收站（CLI 版 deleteFiles，UndoManager.swift:62-174）。
///
/// 安全校验分区（平台无关）→ 委托平台适配器的 Trash::move_to_trash
/// （macOS/Linux 默认走 move_to_trash_dir 归档；Windows 走系统回收站 API）。
pub fn delete_files(urls: &[PathBuf], bundle_name: Option<&str>) -> DeleteResult {
    // 单次遍历分区（原版对每个 URL 校验两次）
    let (valid, blocked): (Vec<PathBuf>, Vec<PathBuf>) = {
        let (v, b): (Vec<&PathBuf>, Vec<&PathBuf>) = urls
            .iter()
            .partition(|u| validate_path(&u.to_string_lossy()));
        (
            v.into_iter().cloned().collect(),
            b.into_iter().cloned().collect(),
        )
    };

    let mut result = crate::platform::adapter().move_to_trash(&valid, bundle_name);
    result.blocked = blocked;
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
        // critical 表来自适配层：当前平台的每一条根都必须拦截
        for c in crate::platform::adapter().critical_paths() {
            assert!(!validate_path(&c), "critical 根必须拦截: {c}");
        }
        // 子路径（如 /usr/local）放行 —— 是合法删除目标
        assert!(validate_path("/usr/local"));
        let home = crate::platform::adapter().home();
        assert!(!validate_path(&home));
        // {home}/Applications 仅 POSIX 拦截（Windows 无此结构，impl 已 cfg 门控）。
        // home="/"（root 会话 HOME 未设）时构造串为 //Applications，与归一化结果
        // 恒不匹配 —— 退化环境跳过，真实桌面/CI 的 HOME 均正常
        #[cfg(not(windows))]
        if home != "/" {
            assert!(!validate_path(&format!("{home}/Applications")));
        }
        assert!(validate_path(&format!(
            "{home}/Library/Preferences/com.test.plist"
        )));
        assert!(validate_path("/tmp/foo"));
    }

    /// 归一化绕过用例：`..` / `//` / 尾部斜杠 拼出的字符串必须等效于 critical 条目
    #[test]
    fn test_validate_path_bypass_normalization() {
        // critical 根的归一化绕过：`..` 回根 / 尾部斜杠 / 重复分隔符 构造的等效
        // 字符串必须拦截（逐项取自适配层表，三平台各测各的）
        for c in crate::platform::adapter().critical_paths() {
            // "/X/.." 归一化回根（POSIX 根或盘符根，独立于 critical 表）
            assert!(!validate_path(&format!("{c}/..")), "归一化回根: {c}");
            #[cfg(not(windows))]
            {
                assert!(!validate_path(&format!("{c}/")), "尾部斜杠: {c}");
                assert!(
                    !validate_path(&format!("//{}", c.trim_start_matches('/'))),
                    "重复分隔符: {c}"
                );
            }
        }
        // 相对路径 → 拦截（删除列表必须绝对）
        assert!(!validate_path("Library"));
        assert!(!validate_path(".."));
        // 归一化后仍是合法子路径 → 放行
        assert!(validate_path(
            "/Users/u/Library/Application Support/Foo/Bar.txt"
        ));
        assert!(validate_path("/tmp/foo/../bar"));
    }

    /// Windows critical 表：盘符保留 + 盘符根/系统目录拦截（回归门禁：安全高危修复）
    #[cfg(windows)]
    #[test]
    fn test_validate_path_windows() {
        assert!(!validate_path("C:\\Windows"));
        assert!(!validate_path("C:\\Program Files"));
        assert!(!validate_path("C:\\Program Files (x86)"));
        assert!(!validate_path("C:\\ProgramData"));
        assert!(!validate_path("C:\\")); // 盘符根格式检测
        assert!(!validate_path("D:\\")); // 非 C: 盘根同样拦截
        assert!(!validate_path("C:\\Windows\\..\\Windows")); // 归一化后仍拦截
        assert!(validate_path("C:\\Program Files\\Foo\\x.txt")); // 合法子路径放行
        assert!(validate_path("C:\\Windows\\..\\System32")); // → C:\System32 子路径合法
        let home = crate::platform::adapter().home();
        assert!(!validate_path(&home)); // USERPROFILE 根拦截
        assert!(validate_path(&format!("{home}\\AppData\\Roaming\\x"))); // 子路径放行
    }

    /// move_to_trash_dir（POSIX 归档核心函数）直接单测：
    /// 归档目录命名/重名 -N 后缀/空列表短路
    #[test]
    fn test_move_to_trash_dir_archive_semantics() {
        let tmp = TempDir::new().unwrap();
        let t1 = tmp.path().join("t1");
        let t2 = tmp.path().join("t2");
        std::fs::create_dir_all(&t1).unwrap();
        std::fs::create_dir_all(&t2).unwrap();
        let f1 = t1.join("same.txt");
        let f2 = t2.join("same.txt");
        std::fs::write(&f1, b"1").unwrap();
        std::fs::write(&f2, b"2").unwrap();

        let trash = tmp.path().join("trash");
        let result = move_to_trash_dir(&[f1.clone(), f2.clone()], Some("App"), trash.clone());
        assert!(result.success);
        assert_eq!(result.moved.len(), 2);
        assert!(result.failed.is_empty());
        assert!(!f1.exists() && !f2.exists());
        assert!(result.bundle_folder.starts_with(&trash));
        // 重名 → 归档内 second 变为 same.txt-1
        let names: Vec<String> = result
            .moved
            .iter()
            .map(|p| {
                p.trash_path
                    .file_name()
                    .unwrap()
                    .to_string_lossy()
                    .into_owned()
            })
            .collect();
        assert!(names.contains(&"same.txt".to_string()));
        assert!(names.contains(&"same.txt-1".to_string()));

        // 归档目录名前缀净化："/" → "_"
        let r2 = move_to_trash_dir(&[f1], Some("Test/App"), trash.clone());
        let folder = r2
            .bundle_folder
            .file_name()
            .unwrap()
            .to_string_lossy()
            .into_owned();
        assert!(folder.starts_with("Test_App_"), "归档名应净化: {folder}");

        // 空列表 → 短路（success=false，不建目录）
        let empty = move_to_trash_dir(&[], Some("App"), trash.clone());
        assert!(!empty.success);
        assert!(empty.moved.is_empty() && empty.failed.is_empty());
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
        let home = crate::platform::adapter().home();
        let critical = crate::platform::adapter().critical_paths();
        let urls = [
            PathBuf::from(&critical[0]), // critical 根必须被滤掉
            PathBuf::from(format!("{home}/Library/Preferences/x")), // 正常子路径放行
        ];
        let valid: Vec<_> = urls
            .iter()
            .filter(|u| validate_path(&u.to_string_lossy()))
            .collect();
        assert_eq!(valid.len(), 1);
    }
}
