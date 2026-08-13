//! .desktop 文件解析（Linux 桌面应用发现）—— 纯文本，跨平台
//!
//! freedesktop Desktop Entry 规范（version 1.0）的 [Desktop Entry] 段子集：
//! 应用发现只需 Name/Exec/Icon/Type/NoDisplay 五个键。解析器无文件系统依赖，
//! 可单测；平台层负责枚举 .desktop 文件并把解析结果组装成 AppInfo。
//!
//! 有效性规则（对齐常见实现）：
//! - 仅 `Type=Application` 的桌面项算应用（Link/Directory 是目录入口/快捷方式）
//! - `NoDisplay=true` 的桌面项不出现在应用菜单 → 不产出
//! - Name 与 Exec 缺失 → 无效
//! - 本地化键（`Name[zh_CN]`）按主键（`Name`）处理，不做语言选择

/// 解析出的桌面项
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DesktopEntry {
    pub name: String,
    pub exec: String,
    pub icon: Option<String>,
    pub no_display: bool,
}

/// 解析 [Desktop Entry] 段；无效（非 Application / NoDisplay / 缺 Name 或 Exec）→ None。
/// 段外的内容（[Desktop Action x]、[KDE Desktop Entry] 等）一律忽略。
pub fn parse_desktop(content: &str) -> Option<DesktopEntry> {
    let mut name: Option<String> = None;
    let mut exec: Option<String> = None;
    let mut icon: Option<String> = None;
    let mut no_display = false;
    let mut in_entry = false;
    for line in content.lines() {
        let line = line.trim();
        if line.starts_with('[') {
            in_entry = line == "[Desktop Entry]";
            continue;
        }
        if !in_entry || line.is_empty() || line.starts_with('#') {
            continue;
        }
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        match key.trim() {
            "Type" if value.trim() != "Application" => return None,
            "Name" => name = Some(value.trim().to_string()),
            "Exec" => exec = Some(value.trim().to_string()),
            "Icon" => icon = Some(value.trim().to_string()),
            "NoDisplay" if value.trim() == "true" => no_display = true,
            _ => {}
        }
    }
    if no_display {
        return None;
    }
    Some(DesktopEntry {
        name: name?,
        exec: exec?,
        icon,
        no_display,
    })
}

/// Exec 行的第一个可执行 token（供可执行性判断）。处理两种形态：
/// - 引号包裹的路径：`Exec="/opt/My App/bin/app" --flag` → `/opt/My App/bin/app`
/// - `env` 前缀变量赋值：`Exec=env FOO=bar /usr/bin/app %U` → `/usr/bin/app`
pub fn exec_first_word(exec: &str) -> Option<&str> {
    let (mut first, mut rest) = split_first_token(exec)?;
    if first == "env" {
        loop {
            let (t, r) = split_first_token(rest)?;
            rest = r;
            if !is_env_assignment(t) {
                first = t;
                break;
            }
        }
    }
    Some(first)
}

/// 读取第一个 token 及其剩余部分：`"..."` / `'...'` 包裹时含内部空白
/// （未闭合引号/空引号 → None，无效 Exec）
fn split_first_token(s: &str) -> Option<(&str, &str)> {
    let s = s.trim_start();
    let mut chars = s.char_indices();
    let (_, first_char) = chars.next()?;
    if first_char == '"' || first_char == '\'' {
        let rest = &s[first_char.len_utf8()..];
        let end = rest.find(first_char)?;
        let inner = &rest[..end];
        if inner.is_empty() {
            return None;
        }
        Some((inner, &rest[end + first_char.len_utf8()..]))
    } else {
        let end = s.find(char::is_whitespace).unwrap_or(s.len());
        let (tok, rest) = s.split_at(end);
        if tok.is_empty() {
            return None;
        }
        Some((tok, rest))
    }
}

/// `VAR=value` 形态（键为全大写字母/下划线）—— env 前缀的变量赋值
fn is_env_assignment(tok: &str) -> bool {
    match tok.split_once('=') {
        Some((k, _)) => !k.is_empty() && k.chars().all(|c| c.is_ascii_uppercase() || c == '_'),
        None => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_basic() {
        let content = "[Desktop Entry]\nType=Application\nName=Firefox\nExec=/usr/bin/firefox %U\nIcon=firefox\n";
        let entry = parse_desktop(content).unwrap();
        assert_eq!(entry.name, "Firefox");
        assert_eq!(entry.exec, "/usr/bin/firefox %U");
        assert_eq!(entry.icon.as_deref(), Some("firefox"));
        assert!(!entry.no_display);
    }

    #[test]
    fn test_parse_type_link_rejected() {
        let content = "[Desktop Entry]\nType=Link\nName=Home\nURL=file:///home\n";
        assert!(parse_desktop(content).is_none());
    }

    #[test]
    fn test_parse_no_display_rejected() {
        let content =
            "[Desktop Entry]\nType=Application\nName=Hidden\nExec=/usr/bin/x\nNoDisplay=true\n";
        assert!(parse_desktop(content).is_none());
    }

    #[test]
    fn test_parse_missing_name_or_exec_rejected() {
        assert!(parse_desktop("[Desktop Entry]\nType=Application\nExec=/usr/bin/x\n").is_none());
        assert!(parse_desktop("[Desktop Entry]\nType=Application\nName=X\n").is_none());
        assert!(parse_desktop("").is_none());
        assert!(parse_desktop("[Other Section]\nName=X\nExec=/usr/bin/x\n").is_none());
    }

    #[test]
    fn test_parse_ignores_comments_and_other_sections() {
        let content = "# comment\n[Desktop Entry]\nType=Application\nName=Code\nExec=/usr/bin/code\n[Desktop Action new]\nName=New Window\nExec=/usr/bin/code --new\n";
        let entry = parse_desktop(content).unwrap();
        assert_eq!(entry.name, "Code");
        assert_eq!(entry.exec, "/usr/bin/code");
    }

    #[test]
    fn test_parse_localized_keys_ignored() {
        // Name[zh_CN] 不是主键 Name，不覆盖
        let content = "[Desktop Entry]\nType=Application\nName=Firefox\nName[zh_CN]=火狐\nExec=/usr/bin/firefox\n";
        assert_eq!(parse_desktop(content).unwrap().name, "Firefox");
    }

    #[test]
    fn test_exec_first_word_plain() {
        assert_eq!(
            exec_first_word("/usr/bin/firefox %U"),
            Some("/usr/bin/firefox")
        );
        assert_eq!(exec_first_word("firefox"), Some("firefox"));
        assert_eq!(exec_first_word(""), None);
    }

    #[test]
    fn test_exec_first_word_quoted_path() {
        assert_eq!(
            exec_first_word("\"/opt/My App/bin/app\" --flag"),
            Some("/opt/My App/bin/app")
        );
    }

    #[test]
    fn test_exec_first_word_env_prefix() {
        assert_eq!(
            exec_first_word("env FOO=bar /usr/bin/app %U"),
            Some("/usr/bin/app")
        );
        assert_eq!(
            exec_first_word("env GTK_USE_PORTAL=1 XDG_CURRENT=1 app"),
            Some("app")
        );
    }
}
