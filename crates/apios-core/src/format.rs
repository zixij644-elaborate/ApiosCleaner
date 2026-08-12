//! 字符串归一化工具 —— 移植自原版 `String.pearFormat()` 与 `strippingTrailingDigits()`
//! (old/Pearcleaner/Logic/Utilities.swift:702-722, AppPathsFetch.swift:13-24)

use std::sync::LazyLock;

use regex::Regex;

/// 原版 pearFormat()：仅保留 Unicode 字母数字 → 转小写；结果为空时返回原串
/// （避免空字符串导致误匹配）
pub fn pear_format(s: &str) -> String {
    let filtered: String = s.chars().filter(|c| c.is_alphanumeric()).collect();
    let lowered: String = filtered.chars().flat_map(|c| c.to_lowercase()).collect();
    if lowered.is_empty() {
        s.to_string()
    } else {
        lowered
    }
}

static TRAILING_DIGITS: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\s+\d+(\.\d+)*\s*$").unwrap());

/// 去除应用名尾部版本号："Bartender 6" → "Bartender"，"Firefox 120.0" → "Firefox"
pub fn stripping_trailing_digits(s: &str) -> String {
    TRAILING_DIGITS.replace_all(s, "").trim().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_pear_format_basic() {
        assert_eq!(pear_format("Pearcleaner"), "pearcleaner");
        assert_eq!(
            pear_format("com.alienator88.Pearcleaner"),
            "comalienator88pearcleaner"
        );
        assert_eq!(pear_format("Visual Studio Code"), "visualstudiocode");
        assert_eq!(pear_format("Bar Tender"), "bartender");
        assert_eq!(pear_format("ABC 123"), "abc123");
    }

    #[test]
    fn test_pear_format_empty_result_returns_original() {
        // 全是标点符号 → 过滤后为空 → 返回原串（防止空串误匹配）
        assert_eq!(pear_format("..."), "...");
        assert_eq!(pear_format("!@#$"), "!@#$");
    }

    #[test]
    fn test_pear_format_unicode() {
        // 非 ASCII 字母数字保留
        assert_eq!(pear_format("微信"), "微信");
        assert_eq!(pear_format("Café"), "café");
    }

    #[test]
    fn test_stripping_trailing_digits() {
        assert_eq!(stripping_trailing_digits("Bartender 6"), "Bartender");
        assert_eq!(stripping_trailing_digits("Firefox 120.0"), "Firefox");
        assert_eq!(stripping_trailing_digits("VS Code"), "VS Code");
        assert_eq!(stripping_trailing_digits("Xcode 15"), "Xcode");
        assert_eq!(stripping_trailing_digits("WeChat"), "WeChat");
    }
}
