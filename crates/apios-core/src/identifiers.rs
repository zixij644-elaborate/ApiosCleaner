//! 标识符预计算 —— 匹配用的归一化标识符集合（bundle id / 应用名的格式化变体）

use crate::format::{pear_format, stripping_trailing_digits};
use crate::model::AppInfo;

/// 预计算的匹配标识符集合
#[derive(Clone, Debug)]
pub struct CachedIdentifiers {
    pub formatted_bundle_id: String,
    pub bundle_last_two_components: String,
    pub formatted_app_name: String,
    pub formatted_app_name_stripped: Option<String>,
    pub app_name_letters_only: String,
    pub path_component_name: String,
    pub use_bundle_identifier: bool,
    pub formatted_company_name: Option<String>,
    pub formatted_entitlements: Vec<String>,
    pub formatted_team_identifier: Option<String>,
    pub formatted_base_bundle_id: Option<String>,
}

/// 常见 helper 后缀
const COMMON_SUFFIXES: &[&str] = &[
    "helper",
    "agent",
    "daemon",
    "service",
    "xpc",
    "launcher",
    "updater",
    "installer",
    "uninstaller",
    "login",
    "extension",
    "plugin",
];

impl CachedIdentifiers {
    pub fn from_app_info(app: &AppInfo) -> CachedIdentifiers {
        let formatted_bundle_id = pear_format(&app.bundle_identifier);

        // bundle 组件：过滤 "-"，小写
        let bundle_components: Vec<String> = app
            .bundle_identifier
            .split('.')
            .filter(|c| *c != "-")
            .map(|c| c.to_lowercase())
            .collect();
        let bundle_last_two_components = bundle_components
            .iter()
            .rev()
            .take(2)
            .collect::<Vec<_>>()
            .iter()
            .rev()
            .map(|s| s.as_str())
            .collect::<String>();

        let formatted_app_name = pear_format(&app.app_name);
        let app_name_letters_only: String = formatted_app_name
            .chars()
            .filter(|c| c.is_alphabetic())
            .collect();
        // 原版：lastPathComponent.replacingOccurrences(of: ".app", with: "")（替换所有出现）。
        // 必须 pearFormat —— matcher 侧 normalized_item_name 已格式化，
        // 不格式化则比较永不相等（原版此处是隐藏死代码 → 路径名匹配始终 false）
        let path_component_name = app
            .path
            .file_name()
            .map(|n| pear_format(&n.to_string_lossy().replace(".app", "")))
            .unwrap_or_default();

        // bundle id 合法性判定（isValidBundleIdentifier 语义）
        let raw_components: Vec<&str> = app.bundle_identifier.split('.').collect();
        let use_bundle_identifier = if raw_components.len() == 1 {
            app.bundle_identifier.chars().count() >= 5
        } else {
            true
        };

        // 3 段 bundle ID 时提取公司名："com.knollsoft.Rectangle" → "knollsoft"
        // 注意：用未过滤的 rawComponents（原版行为，与 bundle_components 不同）
        let formatted_company_name = if raw_components.len() == 3 {
            Some(pear_format(raw_components[1]))
        } else {
            None
        };

        let formatted_entitlements: Vec<String> = app
            .entitlements
            .iter()
            .filter_map(|e| {
                let formatted = pear_format(e);
                if formatted.is_empty() {
                    None
                } else {
                    Some(formatted)
                }
            })
            .collect();

        let formatted_team_identifier = app.team_identifier.as_ref().map(|t| pear_format(t));

        // 基础 bundle ID：去掉 helper/agent 等后缀
        let formatted_base_bundle_id = if raw_components.len() >= 4 {
            let last = raw_components.last().unwrap_or(&"").to_lowercase();
            if COMMON_SUFFIXES.contains(&last.as_str()) {
                let base = raw_components[..raw_components.len() - 1].join(".");
                Some(pear_format(&base))
            } else {
                None
            }
        } else {
            None
        };

        // 去版本号的应用名（Enhanced/Deep 使用）
        let formatted_app_name_stripped = {
            let stripped = pear_format(&stripping_trailing_digits(&app.app_name));
            if stripped != formatted_app_name && !stripped.is_empty() {
                Some(stripped)
            } else {
                None
            }
        };

        CachedIdentifiers {
            formatted_bundle_id,
            bundle_last_two_components,
            formatted_app_name,
            formatted_app_name_stripped,
            app_name_letters_only,
            path_component_name,
            use_bundle_identifier,
            formatted_company_name,
            formatted_entitlements,
            formatted_team_identifier,
            formatted_base_bundle_id,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn app(bundle_id: &str, name: &str) -> AppInfo {
        AppInfo {
            path: PathBuf::from(format!("/Applications/{name}.app")),
            bundle_identifier: bundle_id.to_string(),
            app_name: name.to_string(),
            entitlements: vec![],
            team_identifier: None,
            web_app: false,
            steam: false,
            wrapped: false,
        }
    }

    #[test]
    fn test_standard_app() {
        let ids =
            CachedIdentifiers::from_app_info(&app("com.microsoft.VSCode", "Visual Studio Code"));
        assert_eq!(ids.formatted_bundle_id, "commicrosoftvscode");
        assert_eq!(ids.bundle_last_two_components, "microsoftvscode");
        assert_eq!(ids.formatted_app_name, "visualstudiocode");
        assert_eq!(ids.app_name_letters_only, "visualstudiocode");
        assert_eq!(ids.path_component_name, "visualstudiocode");
        assert!(ids.use_bundle_identifier);
        // 3 段 bundle ID → company = 中间段（原版 rawComponents[1]）
        assert_eq!(ids.formatted_company_name.as_deref(), Some("microsoft"));
        assert_eq!(ids.formatted_base_bundle_id, None);
    }

    #[test]
    fn test_three_component_company() {
        let ids = CachedIdentifiers::from_app_info(&app("com.knollsoft.Rectangle", "Rectangle"));
        assert_eq!(ids.formatted_company_name.as_deref(), Some("knollsoft"));
    }

    #[test]
    fn test_helper_suffix_base_id() {
        let ids = CachedIdentifiers::from_app_info(&app(
            "com.objective-see.blockblock.helper",
            "BlockBlock",
        ));
        // base = "com.objective-see.blockblock" → pearFormat（连字符也被过滤）
        assert_eq!(
            ids.formatted_base_bundle_id.as_deref(),
            Some("comobjectiveseeblockblock")
        );
        // 非后缀结尾 → None
        let ids2 =
            CachedIdentifiers::from_app_info(&app("com.objective-see.blockblock", "BlockBlock"));
        assert_eq!(ids2.formatted_base_bundle_id, None);
    }

    #[test]
    fn test_version_stripped_name() {
        let ids = CachedIdentifiers::from_app_info(&app("com.example.Bartender", "Bartender 6"));
        assert_eq!(
            ids.formatted_app_name_stripped.as_deref(),
            Some("bartender")
        );
        // 无版本号时 stripped 应等于普通名 → None
        let ids2 = CachedIdentifiers::from_app_info(&app("com.example.WeChat", "WeChat"));
        assert_eq!(ids2.formatted_app_name_stripped, None);
    }

    #[test]
    fn test_single_component_bundle_id() {
        let ids = CachedIdentifiers::from_app_info(&app("Foo", "Foo"));
        assert!(!ids.use_bundle_identifier); // 单段且 < 5 字符
        let ids2 = CachedIdentifiers::from_app_info(&app("Short", "Short"));
        assert!(ids2.use_bundle_identifier); // 单段且 == 5 字符（>= 5）
    }

    #[test]
    fn test_dash_filtered_components() {
        // "-" 组件被过滤
        let ids = CachedIdentifiers::from_app_info(&app("com.example.-.Foo", "Example"));
        assert_eq!(ids.bundle_last_two_components, "examplefoo");
    }
}
