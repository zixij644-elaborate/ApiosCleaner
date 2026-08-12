//! 注册表卸载项枚举 —— Windows 应用发现的数据源（Reg*W FFI 薄壳 + 纯解析）
//!
//! Windows 没有 macOS 式 `.app` bundle + Info.plist；已安装应用记录在
//! `HKLM/HKCU\Software\Microsoft\Windows\CurrentVersion\Uninstall\*` 卸载项。
//! 本模块只枚举卸载项（DisplayName/InstallLocation/DisplayIcon 等），AppInfo
//! 构造与匹配逻辑在 windows.rs（AppDiscovery impl）。
//!
//! 分层：`enum_uninstall`（FFI 调用）与 `entry_from_values`（纯解析，零注册表）
//! 分离，解析逻辑可独立单测。

/// HKEY 句柄常量（HANDLE 值直接作 isize 使用）
const HKEY_LOCAL_MACHINE: isize = -2147483646;
const HKEY_CURRENT_USER: isize = -2147483647;
/// KEY_READ = STANDARD_RIGHTS_READ | KEY_QUERY_VALUE | KEY_ENUMERATE_SUB_KEYS | KEY_NOTIFY
const KEY_READ: u32 = 0x0002_0019;
const ERROR_NO_MORE_ITEMS: i32 = 259;
const REG_SZ: u32 = 1;
const REG_EXPAND_SZ: u32 = 2;

const UNINSTALL_SUBKEY: &str = "Software\\Microsoft\\Windows\\CurrentVersion\\Uninstall";

#[link(name = "advapi32")]
extern "system" {
    // 写入 API 仅测试用（HKCU 临时键集成测试），生产只读
    #[cfg(test)]
    fn RegCreateKeyExW(
        key: isize,
        sub_key: *const u16,
        _reserved: u32,
        _class: *mut u16,
        _options: u32,
        access: u32,
        _security: *mut u8,
        out: *mut isize,
        _disposition: *mut u32,
    ) -> i32;
    #[cfg(test)]
    fn RegSetValueExW(
        key: isize,
        name: *const u16,
        _reserved: u32,
        ty: u32,
        data: *const u8,
        len: u32,
    ) -> i32;
    #[cfg(test)]
    fn RegDeleteKeyW(key: isize, sub_key: *const u16) -> i32;
    fn RegOpenKeyExW(
        key: isize,
        sub_key: *const u16,
        options: u32,
        access: u32,
        out: *mut isize,
    ) -> i32;
    fn RegEnumKeyExW(
        key: isize,
        index: u32,
        name: *mut u16,
        name_len: *mut u32,
        _reserved: *mut u32,
        _class: *mut u16,
        _class_len: *mut u32,
        _last_write: *mut i64,
    ) -> i32;
    fn RegQueryValueExW(
        key: isize,
        name: *const u16,
        _reserved: *mut u32,
        ty: *mut u32,
        data: *mut u8,
        len: *mut u32,
    ) -> i32;
    fn RegCloseKey(key: isize) -> i32;
    fn ExpandEnvironmentStringsW(src: *const u16, dst: *mut u16, size: u32) -> u32;
}

/// UTF-16 转换助手
/// 宽字符串 —— Win32 API 要求 NUL 结尾（缺了会读越界字节，值名/键名随机写歪）
fn widen(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

/// 读到第一个 NUL 为止
fn narrow(buf: &[u16]) -> String {
    let end = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
    String::from_utf16_lossy(&buf[..end])
}

/// 一个卸载项（缺失 DisplayName 的条目无意义，不进入结果）
/// publisher / uninstall_string 为数据模型保留字段（CLI 后续展示用）
#[derive(Debug)]
#[allow(dead_code)]
pub struct UninstallEntry {
    pub display_name: String,
    pub install_location: Option<String>,
    pub display_icon: Option<String>,
    pub publisher: Option<String>,
    pub uninstall_string: Option<String>,
}

/// `%VAR%` 环境变量展开（REG_EXPAND_SZ 值；变量未定义时回退原字符串）
fn expand_env(s: &str) -> String {
    let src = widen(s);
    // 第一次调用返回所需长度（含结尾 NUL）
    let needed = unsafe { ExpandEnvironmentStringsW(src.as_ptr(), std::ptr::null_mut(), 0) };
    if needed == 0 {
        return s.to_string();
    }
    let mut buf = vec![0u16; needed as usize];
    unsafe {
        ExpandEnvironmentStringsW(src.as_ptr(), buf.as_mut_ptr(), needed);
    }
    narrow(&buf)
}

/// 纯解析：值映射（名字 → (类型, 原始字节)）→ 卸载项。
/// `%VAR%` 展开只对 REG_EXPAND_SZ 生效；REG_SZ 原样。
pub fn entry_from_values(
    values: &std::collections::HashMap<String, (u32, Vec<u8>)>,
) -> Option<UninstallEntry> {
    let get = |name: &str| -> Option<String> {
        let (ty, raw) = values.get(name)?;
        // REG_SZ / REG_EXPAND_SZ 都是 UTF-16LE 字节序列（结尾 NUL）
        let u16buf: Vec<u16> = raw
            .chunks_exact(2)
            .map(|c| u16::from_le_bytes([c[0], c[1]]))
            .collect();
        let s = narrow(&u16buf);
        match ty {
            &REG_EXPAND_SZ => Some(expand_env(&s)),
            &REG_SZ => Some(s), // 原样（不展开）
            _ => Some(s),
        }
    };
    let display_name = get("DisplayName").filter(|n| !n.is_empty())?;
    Some(UninstallEntry {
        display_name,
        install_location: get("InstallLocation"),
        display_icon: get("DisplayIcon"),
        publisher: get("Publisher"),
        uninstall_string: get("UninstallString"),
    })
}

/// 读取键下的一个值（先查大小再读数据；读失败返回 None）
fn read_value(key: isize, name: &str) -> Option<(u32, Vec<u8>)> {
    let name_wide = widen(name);
    let mut ty: u32 = 0;
    let mut len: u32 = 0;
    let rc = unsafe {
        RegQueryValueExW(
            key,
            name_wide.as_ptr(),
            std::ptr::null_mut(),
            &mut ty,
            std::ptr::null_mut(),
            &mut len,
        )
    };
    if rc != 0 {
        return None;
    }
    let mut data = vec![0u8; len as usize];
    let rc = unsafe {
        RegQueryValueExW(
            key,
            name_wide.as_ptr(),
            std::ptr::null_mut(),
            &mut ty,
            data.as_mut_ptr(),
            &mut len,
        )
    };
    if rc != 0 {
        return None;
    }
    data.truncate(len as usize);
    Some((ty, data))
}

/// 枚举一个 hive 下的全部卸载项（hive: HKEY_LOCAL_MACHINE / HKEY_CURRENT_USER）
pub fn enum_uninstall(hive: isize) -> Vec<UninstallEntry> {
    let mut root: isize = 0;
    let subkey = widen(UNINSTALL_SUBKEY);
    let rc = unsafe { RegOpenKeyExW(hive, subkey.as_ptr(), 0, KEY_READ, &mut root) };
    if rc != 0 {
        return Vec::new(); // 权限失败 → 空（不 panic）
    }

    let mut out = Vec::new();
    let mut index: u32 = 0;
    loop {
        let mut name_buf = vec![0u16; 512];
        let mut name_len: u32 = 512;
        let rc = unsafe {
            RegEnumKeyExW(
                root,
                index,
                name_buf.as_mut_ptr(),
                &mut name_len,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                std::ptr::null_mut(),
            )
        };
        if rc == ERROR_NO_MORE_ITEMS {
            break;
        }
        if rc != 0 {
            break;
        }
        index += 1;

        let key_name = narrow(&name_buf);
        // 打开子键读值（部分卸载项仅系统可读，失败跳过）
        let mut sub: isize = 0;
        let key_name_wide = widen(&key_name);
        if unsafe { RegOpenKeyExW(root, key_name_wide.as_ptr(), 0, KEY_READ, &mut sub) } != 0 {
            continue;
        }
        let mut values = std::collections::HashMap::new();
        for v in [
            "DisplayName",
            "InstallLocation",
            "DisplayIcon",
            "Publisher",
            "UninstallString",
        ] {
            if let Some((ty, data)) = read_value(sub, v) {
                values.insert(v.to_string(), (ty, data));
            }
        }
        unsafe { RegCloseKey(sub) };

        if let Some(entry) = entry_from_values(&values) {
            out.push(entry);
        }
    }
    unsafe { RegCloseKey(root) };
    out
}

/// 两个 hive 的合并枚举（HKLM 后 HKCU；重复显示名由调用方处理）
pub fn all_uninstall_entries() -> Vec<UninstallEntry> {
    let mut out = enum_uninstall(HKEY_LOCAL_MACHINE);
    out.extend(enum_uninstall(HKEY_CURRENT_USER));
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// REG_SZ 值构造（UTF-16LE + NUL 结尾）
    fn sz(s: &str) -> (u32, Vec<u8>) {
        let mut raw: Vec<u8> = s.encode_utf16().flat_map(|c| c.to_le_bytes()).collect();
        raw.extend([0, 0]); // 结尾 NUL
        (REG_SZ, raw)
    }

    fn expanded(s: &str) -> (u32, Vec<u8>) {
        let mut raw: Vec<u8> = s.encode_utf16().flat_map(|c| c.to_le_bytes()).collect();
        raw.extend([0, 0]);
        (REG_EXPAND_SZ, raw)
    }

    #[test]
    fn test_entry_from_values_basic() {
        let mut values = std::collections::HashMap::new();
        values.insert("DisplayName".into(), sz("Visual Studio Code"));
        values.insert(
            "InstallLocation".into(),
            sz(r"C:\Users\znie\AppData\Local\Programs\Microsoft VS Code"),
        );
        values.insert("Publisher".into(), sz("Microsoft Corporation"));
        let entry = entry_from_values(&values).unwrap();
        assert_eq!(entry.display_name, "Visual Studio Code");
        assert_eq!(
            entry.install_location.as_deref(),
            Some(r"C:\Users\znie\AppData\Local\Programs\Microsoft VS Code")
        );
        assert_eq!(entry.publisher.as_deref(), Some("Microsoft Corporation"));
        assert!(entry.display_icon.is_none());
    }

    #[test]
    fn test_entry_missing_display_name_skipped() {
        let mut values = std::collections::HashMap::new();
        values.insert("InstallLocation".into(), sz(r"C:\x"));
        assert!(entry_from_values(&values).is_none());

        values.insert("DisplayName".into(), sz(""));
        assert!(entry_from_values(&values).is_none());
    }

    #[test]
    fn test_entry_expand_sz_expands_env() {
        let mut values = std::collections::HashMap::new();
        values.insert("DisplayName".into(), sz("Foo"));
        // %LOCALAPPDATA% 在测试环境必然定义（Windows）；展开后不得再含 %VAR%
        values.insert(
            "InstallLocation".into(),
            expanded(r"%LOCALAPPDATA%\Programs\Foo"),
        );
        let entry = entry_from_values(&values).unwrap();
        let loc = entry.install_location.unwrap();
        assert!(!loc.contains("%LOCALAPPDATA%"));
        assert!(loc.ends_with(r"\Programs\Foo"));
    }

    #[test]
    fn test_entry_reg_sz_not_expanded() {
        let mut values = std::collections::HashMap::new();
        values.insert("DisplayName".into(), sz("Foo"));
        values.insert("InstallLocation".into(), sz(r"%UNKNOWN_VAR%\x"));
        let entry = entry_from_values(&values).unwrap();
        assert_eq!(entry.install_location.as_deref(), Some(r"%UNKNOWN_VAR%\x"));
    }

    /// 集成测试：在 HKCU 卸载项下建临时键 → 枚举 → 断言 → 删除。
    /// HKCU 对当前用户可写；若环境拒绝建键则直接跳过（不失败）。
    #[cfg(windows)]
    #[test]
    fn test_enum_uninstall_hkcu_integration() {
        const KEY_WRITE: u32 = 0x0002_0006;
        const TEST_SUBKEY: &str =
            "Software\\Microsoft\\Windows\\CurrentVersion\\Uninstall\\ApiosCleanerTests";

        let mut key: isize = 0;
        let rc = unsafe {
            RegCreateKeyExW(
                HKEY_CURRENT_USER,
                widen(TEST_SUBKEY).as_ptr(),
                0,
                std::ptr::null_mut(),
                0,
                KEY_WRITE,
                std::ptr::null_mut(),
                &mut key,
                std::ptr::null_mut(),
            )
        };
        if rc != 0 {
            return; // 环境不允许写 HKCU → 跳过
        }
        let name_wide = widen("DisplayName");
        let value = sz("ApiosCleanerTestApp");
        let _ = unsafe {
            RegSetValueExW(
                key,
                name_wide.as_ptr(),
                0,
                REG_SZ,
                value.1.as_ptr(),
                value.1.len() as u32,
            )
        };
        unsafe { RegCloseKey(key) };

        let entries = enum_uninstall(HKEY_CURRENT_USER);
        assert!(
            entries
                .iter()
                .any(|e| e.display_name == "ApiosCleanerTestApp"),
            "枚举结果应包含测试键"
        );

        let _ = unsafe { RegDeleteKeyW(HKEY_CURRENT_USER, widen(TEST_SUBKEY).as_ptr()) };
    }
}
