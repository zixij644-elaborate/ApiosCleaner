//! 临时目录清理（clean-tmp）—— 跨平台扫描与安全过滤逻辑
//!
//! 背景（2026-08-14 用户点名）：AI 工具等大量写 `/tmp`（及 `%TEMP%`），系统
//! 不自动清理 —— `/tmp` 是真实垃圾堆积点。
//!
//! 安全策略（参照 BleachBit `system.tmp` 思路，仅思想层）：
//! - **mtime 过滤**：只处理 N 天前（默认 7）的条目 —— 正在使用的文件通常是
//!   新写入的
//! - **白名单（组件级）**：X 会话运行时文件（.X11-unix/.X0-lock 等）、
//!   systemd-private-* 服务目录、socket、*.lock —— 删它们会破坏运行中的
//!   会话/服务
//! - 只扫描各根目录的**顶层条目**（不递归深入 —— 目录本身按 mtime 判定，
//!   删除时整个目录进回收站；递归细粒度清理留后续）
//!
//! 平台路径（`/tmp`、`/var/tmp`、`%TEMP%`）由调用方（CLI）提供，
//! 本模块只做扫描与过滤，可独立单测。

use std::path::PathBuf;
use std::time::{Duration, SystemTime};

#[cfg(unix)]
use std::os::unix::fs::FileTypeExt;

/// X 会话运行时文件（POSIX 图形会话）：删除会破坏正在运行的桌面
fn is_x_session_runtime(name: &str) -> bool {
    matches!(
        name,
        ".X11-unix" | ".X0-lock" | ".XIM-unix" | ".font-unix" | ".ICE-unix" | ".X11-pipe"
    )
}

/// clean-tmp 白名单（组件级）：命中则跳过。
/// - X 会话运行时文件（上）
/// - systemd-private-*：运行中服务的私有目录（Linux）
/// - com.apple.*：macOS 系统服务临时目录（launchd 服务用 $TMPDIR，
///   删除有风险——实测 com.apple.avconferenced 等 278 项被 1 天门槛列出）
/// - socket 文件（由调用方传 file_type 判定）
/// - *.lock：锁文件（运行中程序持有）
pub fn is_tmp_excluded(name: &str, is_socket: bool) -> bool {
    is_x_session_runtime(name)
        || name.starts_with("systemd-private-")
        || name.starts_with("com.apple.")
        || is_socket
        || name.ends_with(".lock")
}

/// 扫描临时根目录的顶层条目，返回满足以下条件的路径：
/// - mtime 早于 `older_than`（该文件/目录至少 N 天未被触碰）
/// - 不命中白名单（X 会话 / systemd-private / socket / *.lock）
///
/// 目录条目本身按目录 mtime 判定（整个目录进回收站，不递归）。
/// 根目录不存在/不可读 → 跳过（不报错）。
pub fn scan_tmp(roots: &[PathBuf], older_than: Duration) -> Vec<PathBuf> {
    let cutoff = SystemTime::now()
        .checked_sub(older_than)
        .unwrap_or(SystemTime::UNIX_EPOCH);
    let mut found = Vec::new();
    for root in roots {
        let Ok(entries) = std::fs::read_dir(root) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            let Ok(meta) = entry.metadata() else {
                continue;
            };
            // socket 判定（entry.file_type 更准——不跟随符号链接）；
            // is_socket 是 unix API（Windows 无 socket 文件概念）
            let is_socket = entry.file_type().is_ok_and(|t| {
                #[cfg(unix)]
                {
                    t.is_socket()
                }
                #[cfg(not(unix))]
                {
                    false
                }
            });
            if is_tmp_excluded(&entry.file_name().to_string_lossy(), is_socket) {
                continue;
            }
            // mtime 过滤：条目必须比 cutoff 更旧
            let Ok(mtime) = meta.modified() else {
                continue;
            };
            if mtime < cutoff {
                found.push(path);
            }
        }
    }
    found
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_tmp_whitelist() {
        // X 会话运行时文件
        assert!(is_tmp_excluded(".X11-unix", false));
        assert!(is_tmp_excluded(".X0-lock", false));
        assert!(is_tmp_excluded(".ICE-unix", false));
        // systemd 私有目录
        assert!(is_tmp_excluded(
            "systemd-private-abc-service.service-x",
            false
        ));
        // macOS 系统服务临时目录（launchd 服务用 $TMPDIR）
        assert!(is_tmp_excluded("com.apple.avconferenced", false));
        assert!(is_tmp_excluded(
            "com.apple.ThreadCommissionerService",
            false
        ));
        // 锁文件与 socket
        assert!(is_tmp_excluded("app.lock", false));
        assert!(is_tmp_excluded("app.sock", true));
        // 普通条目放行
        assert!(!is_tmp_excluded("bleachbit-sync.tar", false));
        assert!(!is_tmp_excluded("config-err-x", false));
        // 非 socket 的 .sock 后缀文件仍放行（名字不是白名单依据）
        assert!(!is_tmp_excluded("notes.sock.md", false));
    }

    #[cfg(unix)]
    #[test]
    fn test_scan_tmp_filters_by_mtime_and_whitelist() {
        use std::os::unix::ffi::OsStrExt;
        let tmp = tempfile::TempDir::new().unwrap();
        let old = tmp.path().join("old-file.txt");
        let fresh = tmp.path().join("fresh-file.txt");
        let lock = tmp.path().join("app.lock");
        std::fs::write(&old, b"x").unwrap();
        std::fs::write(&fresh, b"x").unwrap();
        std::fs::write(&lock, b"x").unwrap();
        // old-file 改 mtime 到 1970 年（30 天前以上；libc::utimes，Windows 无此 API）
        let c = std::ffi::CString::new(old.as_os_str().as_bytes()).unwrap();
        let times = [libc::timeval {
            tv_sec: 0,
            tv_usec: 0,
        }; 2];
        unsafe {
            libc::utimes(c.as_ptr(), times.as_ptr());
        }

        let found = scan_tmp(&[tmp.path().to_path_buf()], Duration::from_secs(7 * 86400));
        // 只有改旧的 old-file 入选；fresh 太新、lock 命中白名单
        assert_eq!(found, vec![old]);
    }

    #[test]
    fn test_scan_tmp_missing_root_skipped() {
        assert!(scan_tmp(
            &[PathBuf::from("/nonexistent/zzz")],
            Duration::from_secs(86400)
        )
        .is_empty());
        assert!(scan_tmp(&[], Duration::from_secs(86400)).is_empty());
    }
}
