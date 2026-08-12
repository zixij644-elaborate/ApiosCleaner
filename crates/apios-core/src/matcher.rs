//! 启发式匹配引擎 —— 忠实移植原版 `specificCondition` / `shouldSkipItem`
//! (old/Pearcleaner/Logic/AppPathsFetch.swift:294-434)

use std::collections::HashSet;
use std::path::Path;

use crate::conditions;
use crate::format::pear_format;
use crate::identifiers::CachedIdentifiers;
use crate::model::{AppInfo, Sensitivity, SkipCondition};

/// shouldSkipItem 移植：集合成员 / 路径前缀 / 名称前缀 + 豁免
pub fn should_skip_item(
    normalized_item_name: &str,
    path: &Path,
    collection: &HashSet<std::path::PathBuf>,
    skip_conditions: &[SkipCondition],
) -> bool {
    if collection.contains(path) {
        return true;
    }
    for skip in skip_conditions {
        // 路径前缀排除（原版 hasPrefix）
        for skip_path in &skip.skip_paths {
            if path.to_string_lossy().starts_with(skip_path.as_str()) {
                return true;
            }
        }
        // 名称前缀排除 + 豁免（原版 allowPrefixes）
        if skip
            .skip_prefix
            .iter()
            .any(|p| normalized_item_name.starts_with(p))
        {
            let is_allowed = skip
                .allow_prefixes
                .iter()
                .any(|p| normalized_item_name.starts_with(p));
            if !is_allowed {
                return true;
            }
        }
    }
    false
}

/// specificCondition 移植（AppPathsFetch.swift:323-434）
#[allow(clippy::too_many_arguments)]
pub fn specific_condition(
    normalized_item_name: &str,
    path: &Path,
    app: &AppInfo,
    ids: &CachedIdentifiers,
    sensitivity: Sensitivity,
    conditions: &[crate::model::Condition],
) -> bool {
    let path_str = path.to_string_lossy();
    let path_ext = path
        .extension()
        .map(|e| e.to_string_lossy().to_lowercase())
        .unwrap_or_default();

    // --- Steam 桌面快捷方式（/Desktop/*.app）---
    if path_str.contains("/Desktop/") && path_ext == "app" {
        let desktop_app_name = pear_format(&path.file_stem().unwrap_or_default().to_string_lossy());
        if desktop_app_name == ids.formatted_app_name
            || desktop_app_name == ids.app_name_letters_only
        {
            return true;
        }
    }

    // --- Steam 游戏主目录 ---
    if app.steam && path_str.contains("/Library/Application Support/Steam/steamapps/common/") {
        let folder_name = pear_format(&path.file_name().unwrap_or_default().to_string_lossy());
        if folder_name == ids.formatted_app_name || folder_name == ids.app_name_letters_only {
            return true;
        }
    }

    // --- Steam 清单文件（appmanifest_<id>.acf）---
    if app.steam
        && path_str.contains("/Library/Application Support/Steam/steamapps/")
        && path
            .file_name()
            .is_some_and(|n| n.to_string_lossy().starts_with("appmanifest_"))
        && path_ext == "acf"
    {
        if let Some(game_id_from_file) =
            extract_game_id(&path.file_name().unwrap_or_default().to_string_lossy())
        {
            if let Some(game_id_from_launcher) = get_steam_game_id(&app.path) {
                return game_id_from_file == game_id_from_launcher;
            }
        }
    }

    // --- entitlements 匹配（strict: 精确；enhanced/deep: 包含）---
    for entitlement in &ids.formatted_entitlements {
        let is_match = if sensitivity == Sensitivity::Strict {
            normalized_item_name == entitlement
        } else {
            normalized_item_name.contains(entitlement.as_str())
        };
        if is_match {
            return true;
        }
    }

    // --- 每应用条件（先 exclude 后 include，AppPathsFetch.swift:369-378）---
    if ids.use_bundle_identifier {
        for condition in conditions {
            if ids
                .formatted_bundle_id
                .contains(condition.bundle_id.as_str())
            {
                if condition
                    .exclude
                    .iter()
                    .any(|e| normalized_item_name.contains(e.as_str()))
                {
                    return false;
                }
                if condition
                    .include
                    .iter()
                    .any(|i| normalized_item_name.contains(i.as_str()))
                {
                    return true;
                }
            }
        }
    }

    // --- webApp：仅 bundle ID 包含匹配 ---
    if app.web_app {
        return normalized_item_name.contains(ids.formatted_bundle_id.as_str());
    }

    let full_bundle_match = normalized_item_name.contains(ids.formatted_bundle_id.as_str());
    let strict = sensitivity == Sensitivity::Strict;

    // 空值保护（原版 !isEmpty 守卫）
    let app_name_match = !ids.formatted_app_name.is_empty()
        && if strict {
            normalized_item_name == ids.formatted_app_name
        } else {
            normalized_item_name.contains(ids.formatted_app_name.as_str())
        };
    let path_name_match = !ids.path_component_name.is_empty()
        && if strict {
            normalized_item_name == ids.path_component_name
        } else {
            normalized_item_name.contains(ids.path_component_name.as_str())
        };
    let app_name_letters_match = !ids.app_name_letters_only.is_empty()
        && if strict {
            normalized_item_name == ids.app_name_letters_only
        } else {
            normalized_item_name.contains(ids.app_name_letters_only.as_str())
        };

    // 两段 bundle ID 匹配（Enhanced/Deep 仅）
    let two_component_match = sensitivity != Sensitivity::Strict
        && normalized_item_name.contains(ids.bundle_last_two_components.as_str());

    // company 名匹配（Deep 仅）
    let company_match = sensitivity == Sensitivity::Deep
        && ids
            .formatted_company_name
            .as_ref()
            .is_some_and(|c| !c.is_empty() && normalized_item_name.contains(c.as_str()));

    // team ID 匹配（Deep 仅）
    let team_id_match = sensitivity == Sensitivity::Deep
        && ids
            .formatted_team_identifier
            .as_ref()
            .is_some_and(|t| !t.is_empty() && normalized_item_name.contains(t.as_str()));

    // 基础 bundle ID 匹配（带 helper/agent 后缀的应用）
    let base_bundle_id_match = ids
        .formatted_base_bundle_id
        .as_ref()
        .is_some_and(|b| !b.is_empty() && normalized_item_name.contains(b.as_str()));

    // 去版本号名称匹配（Enhanced/Deep 仅）
    let stripped_app_name_match = sensitivity != Sensitivity::Strict
        && ids
            .formatted_app_name_stripped
            .as_ref()
            .is_some_and(|s| !s.is_empty() && normalized_item_name.contains(s.as_str()));

    (ids.use_bundle_identifier && full_bundle_match)
        || app_name_match
        || path_name_match
        || app_name_letters_match
        || two_component_match
        || company_match
        || team_id_match
        || base_bundle_id_match
        || stripped_app_name_match
}

/// 从 "appmanifest_1289310.acf" 提取 "1289310"（AppPathsFetch.swift:437-445）
fn extract_game_id(filename: &str) -> Option<String> {
    let mut parts = filename.split('_');
    parts.next()?; // "appmanifest"
    let id_with_ext = parts.next()?;
    Some(id_with_ext.split('.').next()?.to_string())
}

/// 从 Steam 启动器 run.sh 提取游戏 ID（steam://run/<id>，AppPathsFetch.swift:448-469）
fn get_steam_game_id(app_path: &Path) -> Option<String> {
    let run_sh = app_path.join("Contents/MacOS/run.sh");
    let content = std::fs::read_to_string(run_sh).ok()?;
    let marker = "steam://run/";
    let idx = content.find(marker)?;
    let after = &content[idx + marker.len()..];
    Some(after.chars().take_while(|c| c.is_ascii_digit()).collect())
}

/// 默认每应用条件缓存（调用方按需获取，避免每次重建）
pub fn default_conditions() -> Vec<crate::model::Condition> {
    conditions::conditions()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::conditions;
    use crate::identifiers::CachedIdentifiers;
    use crate::model::{AppInfo, Condition};
    use crate::platform::SystemPaths;
    use std::path::PathBuf;

    fn make_app(bundle_id: &str, name: &str) -> (AppInfo, CachedIdentifiers) {
        let app = AppInfo {
            path: PathBuf::from(format!("/Applications/{name}.app")),
            bundle_identifier: bundle_id.to_string(),
            app_name: name.to_string(),
            entitlements: vec![],
            team_identifier: None,
            web_app: false,
            steam: false,
            wrapped: false,
        };
        let ids = CachedIdentifiers::from_app_info(&app);
        (app, ids)
    }

    fn conds() -> Vec<Condition> {
        conditions::conditions()
    }

    #[test]
    fn test_bundle_id_match() {
        let (app, ids) = make_app("com.microsoft.VSCode", "Visual Studio Code");
        // ~/Library/Application Support/Code/ → pearFormat "code"？不匹配。
        // 容器类：com.microsoft.VSCode.helper → "commicrosoftvscodehelper" 包含 "commicrosoftvscode"
        assert!(specific_condition(
            "commicrosoftvscodehelper",
            Path::new("/Users/u/Library/Containers/x/Data"),
            &app,
            &ids,
            Sensitivity::Strict,
            &conds()
        ));
    }

    #[test]
    fn test_app_name_exact_strict() {
        let (app, ids) = make_app("com.example.Bartender", "Bartender 6");
        // strict：名称必须精确相等（"bartendertool" != "bartender6"）→ 不匹配
        assert!(!specific_condition(
            "bartendertool",
            Path::new("/tmp/x"),
            &app,
            &ids,
            Sensitivity::Strict,
            &conds()
        ));
        // strict：精确等于格式化名（含版本号）→ 匹配
        assert!(specific_condition(
            "bartender6",
            Path::new("/tmp/x"),
            &app,
            &ids,
            Sensitivity::Strict,
            &conds()
        ));
        // enhanced：stripped 名 "bartender" 包含匹配
        assert!(specific_condition(
            "bartendertool",
            Path::new("/tmp/x"),
            &app,
            &ids,
            Sensitivity::Enhanced,
            &conds()
        ));
    }

    #[test]
    fn test_condition_exclude_wins() {
        let (app, ids) = make_app("com.google.chrome", "Google Chrome");
        // chrome 条件 exclude 含 "iterm" —— 必须先排除
        assert!(!specific_condition(
            "iterm",
            Path::new("/tmp/iterm"),
            &app,
            &ids,
            Sensitivity::Deep,
            &conds()
        ));
        // include "chrome" → 匹配
        assert!(specific_condition(
            "chrome",
            Path::new("/tmp/chrome"),
            &app,
            &ids,
            Sensitivity::Deep,
            &conds()
        ));
    }

    #[test]
    fn test_company_match_deep_only() {
        let (app, ids) = make_app("com.knollsoft.Rectangle", "Rectangle");
        // company "knollsoft" 只在 deep 匹配
        assert!(!specific_condition(
            "knollsoft",
            Path::new("/tmp/knollsoft"),
            &app,
            &ids,
            Sensitivity::Enhanced,
            &conds()
        ));
        assert!(specific_condition(
            "knollsoft",
            Path::new("/tmp/knollsoft"),
            &app,
            &ids,
            Sensitivity::Deep,
            &conds()
        ));
    }

    #[test]
    fn test_steam_manifest() {
        let mut app = AppInfo {
            path: PathBuf::from("/Applications/SomeGame.app"),
            bundle_identifier: "com.steam.game".to_string(),
            app_name: "SomeGame".to_string(),
            entitlements: vec![],
            team_identifier: None,
            web_app: false,
            steam: true,
            wrapped: false,
        };
        let ids = CachedIdentifiers::from_app_info(&app);
        // 没有 run.sh 时不应崩溃
        let result = specific_condition(
            "appmanifest_1289310acf",
            Path::new(
                "/Users/u/Library/Application Support/Steam/steamapps/appmanifest_1289310.acf",
            ),
            &app,
            &ids,
            Sensitivity::Deep,
            &conds(),
        );
        assert!(!result); // run.sh 不存在 → 不匹配
        app.steam = false;
    }

    #[test]
    fn test_extract_game_id() {
        assert_eq!(
            extract_game_id("appmanifest_1289310.acf").as_deref(),
            Some("1289310")
        );
        assert_eq!(
            extract_game_id("appmanifest_123.acf").as_deref(),
            Some("123")
        );
        assert_eq!(extract_game_id("foo"), None);
    }

    #[test]
    fn test_should_skip_trash() {
        let home = crate::platform::adapter().home();
        let collection = HashSet::new();
        let skip = conditions::skip_conditions();
        // ~/.Trash 下的任何路径都跳过
        assert!(should_skip_item(
            "foo",
            Path::new(&format!("{home}/.Trash/foo")),
            &collection,
            &skip
        ));
        // 正常路径不跳过
        assert!(!should_skip_item(
            "foo",
            Path::new("/tmp/foo"),
            &collection,
            &skip
        ));
    }

    #[test]
    fn test_skip_prefix_with_allowance() {
        let collection = HashSet::new();
        // "reminders" 前缀被跳过，但 "comappledt" 豁免名单外 —— 用真实条件验证
        let skip = conditions::skip_conditions();
        assert!(should_skip_item(
            "remindershelper",
            Path::new("/tmp/remindershelper"),
            &collection,
            &skip
        ));
        // allowPrefixes 里没有 "reminders" 的豁免，验证 skip_prefix 含 "mobiledocuments"
        assert!(should_skip_item(
            "mobiledocumentsfoo",
            Path::new("/tmp/x"),
            &collection,
            &skip
        ));
    }
}
