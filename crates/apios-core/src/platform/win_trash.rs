//! SHFileOperationW FFI —— Windows 系统回收站删除
//!
//! Windows 回收站无目录模型（没有 macOS ~/.Trash 式的归档目录），删除直接进
//! 系统回收站（FOF_ALLOWUNDO → 用户可在回收站恢复）。手写 FFI 而非第三方
//! crate（回收站功能只需这一个 API，与 win_registry 同技能栈，零依赖）。
//!
//! 分层：`recycle_batch`（FFI 调用 + 失败分类）与纯函数 `build_from_list`
//! （双 NUL 结尾路径列表构造，可单测断言字节）分离。
//!
//! 已知限制：SHFileOperationW 路径上限 ~260 字符（无 \\?\ 长路径支持）；
//! 超长路径的文件会进 failed 列表，由 CLI 提示。

use std::path::PathBuf;

/// SHFileOperation wFunc（SHFILEOPSTRUCT）
const FO_DELETE: u32 = 0x0003;
/// fFlags：静默（无进度 UI）
const FOF_SILENT: u16 = 0x0004;
/// fFlags：不弹确认框（CLI 已有交互确认，双确认是噪音）
const FOF_NOCONFIRMATION: u16 = 0x0010;
/// fFlags：放入回收站（不加此标志 = 永久删除！）
const FOF_ALLOWUNDO: u16 = 0x0040;
/// fFlags：错误不弹 UI（错误经返回值/逐文件分类上报）
const FOF_NOERRORUI: u16 = 0x0400;

/// SHFILEOPSTRUCTW（关键字段；hwnd/name mappings 等不需要的置空）
/// 命名保持 Win32 API 原名（W 后缀），clippy 缩写 lint 放行
#[allow(clippy::upper_case_acronyms)]
#[repr(C)]
struct SHFILEOPSTRUCTW {
    hwnd: *mut u8,
    w_func: u32,
    /// 双 NUL 结尾路径列表（PCZZWSTR）
    p_from: *const u16,
    p_to: *const u16,
    f_flags: u16,
    f_any_operations_aborted: i32,
    h_name_mappings: *mut u8,
    lpsz_progress_title: *const u16,
}

#[link(name = "shell32")]
extern "system" {
    /// 返回 0 = 成功（BOOL 语义）；失败非零
    fn SHFileOperationW(lp_file_op: *mut SHFILEOPSTRUCTW) -> i32;
}

/// 双 NUL 结尾路径列表（PCZZWSTR）：每路径 NUL 结尾，列表末尾再补一个 NUL
fn build_from_list(paths: &[PathBuf]) -> Vec<u16> {
    let mut buf = Vec::new();
    for p in paths {
        buf.extend(p.to_string_lossy().encode_utf16());
        buf.push(0);
    }
    buf.push(0);
    buf
}

/// 一次 SHFileOperationW 调用（全静默，无 UI）
fn shfileop_delete(paths: &[PathBuf]) -> bool {
    let from = build_from_list(paths);
    let mut op = SHFILEOPSTRUCTW {
        hwnd: std::ptr::null_mut(),
        w_func: FO_DELETE,
        p_from: from.as_ptr(),
        p_to: std::ptr::null(),
        f_flags: FOF_SILENT | FOF_NOCONFIRMATION | FOF_ALLOWUNDO | FOF_NOERRORUI,
        f_any_operations_aborted: 0,
        h_name_mappings: std::ptr::null_mut(),
        lpsz_progress_title: std::ptr::null(),
    };
    unsafe { SHFileOperationW(&mut op) == 0 }
}

/// 把 paths 移入回收站，返回成功移入的路径。
///
/// 先批量一次调用（快路径）；失败时 SHFileOperationW 可能部分成功
/// （如个别文件被占用）→ 存在性检查区分已移走项，再逐文件重试分类。
pub fn recycle_batch(paths: &[PathBuf]) -> Vec<PathBuf> {
    if paths.is_empty() {
        return Vec::new();
    }
    // 先滤掉调用时不存在的路径：它们不可能被"移入"回收站，也不该计入 moved
    // （事后存在性检查无法区分"批量中已移走"与"本就不存在"）
    let existing: Vec<PathBuf> = paths.iter().filter(|p| p.exists()).cloned().collect();
    if existing.is_empty() {
        return Vec::new();
    }
    if shfileop_delete(&existing) {
        return existing;
    }
    // 批量失败：已不存在的 = 批量调用中已移走（部分成功）
    let mut moved: Vec<PathBuf> = Vec::new();
    let mut remaining: Vec<PathBuf> = Vec::new();
    for p in &existing {
        if p.exists() {
            remaining.push(p.clone());
        } else {
            moved.push(p.clone());
        }
    }
    for p in remaining {
        if shfileop_delete(std::slice::from_ref(&p)) {
            moved.push(p);
        }
    }
    moved
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    /// 双 NUL 结尾断言：`A\0B\0\0`
    #[test]
    fn test_build_from_list_double_null_terminated() {
        let paths = vec![
            PathBuf::from(r"C:\Users\x\AppData\Roaming\Foo"),
            PathBuf::from(r"C:\Users\x\AppData\Roaming\Foo\bar.txt"),
        ];
        let buf = build_from_list(&paths);
        let s: Vec<u16> = buf.to_vec();
        let joined: String = s
            .iter()
            .map(|&c| {
                if c == 0 {
                    '|'
                } else {
                    char::from_u32(c as u32).unwrap()
                }
            })
            .collect();
        assert_eq!(
            joined,
            r"C:\Users\x\AppData\Roaming\Foo|C:\Users\x\AppData\Roaming\Foo\bar.txt||"
        );
        // 结尾两个 NUL
        assert_eq!(buf[buf.len() - 1], 0);
        assert_eq!(buf[buf.len() - 2], 0);
        assert_ne!(buf[buf.len() - 3], 0);
    }

    #[test]
    fn test_build_from_list_single_path() {
        let buf = build_from_list(&[PathBuf::from(r"C:\x.txt")]);
        assert_eq!(buf.last(), Some(&0));
        assert_eq!(buf[buf.len() - 2], 0);
    }

    #[test]
    fn test_build_from_list_empty() {
        let buf = build_from_list(&[]);
        assert_eq!(buf, vec![0]); // 仅列表终结 NUL
    }

    /// 集成测试：临时文件 → 系统回收站 → 原路径消失（CI windows runner 可跑，
    /// FOF_SILENT 全静默无桌面依赖）。回收站内路径不可知，只断言源消失。
    #[cfg(windows)]
    #[test]
    fn test_recycle_file_disappears() {
        let dir = std::env::temp_dir().join("apios-trash-test");
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let file = dir.join("to-recycle.txt");
        fs::write(&file, b"junk").unwrap();

        let moved = recycle_batch(std::slice::from_ref(&file));
        assert!(!file.exists(), "文件应已移入回收站");
        assert_eq!(moved.len(), 1);

        let _ = fs::remove_dir_all(&dir);
    }

    #[cfg(windows)]
    #[test]
    fn test_recycle_nonexistent_path_fails_silently() {
        // 不存在的路径：批量调用失败 → 逐文件重试失败 → 不算 moved（不 panic）
        let ghost = PathBuf::from(r"C:\__apios_nonexistent_zzz__\ghost.txt");
        let moved = recycle_batch(std::slice::from_ref(&ghost));
        assert!(moved.is_empty());
    }
}
