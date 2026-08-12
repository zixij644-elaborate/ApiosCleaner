//! 核心数据模型 —— 对应原版 AppInfo、SearchSensitivityLevel、Condition 等

use std::path::PathBuf;

/// 搜索敏感度 —— 对应原版 `SearchSensitivityLevel`（严格/增强/深度）
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum Sensitivity {
    Strict,
    Enhanced,
    #[default]
    Deep,
}

impl Sensitivity {
    pub fn parse(s: &str) -> Sensitivity {
        match s.to_lowercase().as_str() {
            "strict" => Sensitivity::Strict,
            "enhanced" => Sensitivity::Enhanced,
            // 未知值兜底 Deep（原版 @AppStorage 持久化值只有这三种；本枚举 Default 即 Deep，
            // 兜底与 Default 一致。调用方（CLI 固定 Strict）不受影响）
            _ => Sensitivity::Deep,
        }
    }
}

/// 应用信息 —— PoC 只保留 CLI/扫描所需字段（对应原版 AppInfo 的子集）
#[derive(Clone, Debug)]
pub struct AppInfo {
    /// 应用 bundle 路径，如 /Applications/Foo.app
    pub path: PathBuf,
    pub bundle_identifier: String,
    pub app_name: String,
    /// 来自 codesign 的 entitlements 键名列表
    pub entitlements: Vec<String>,
    pub team_identifier: Option<String>,
    pub web_app: bool,
    pub steam: bool,
    pub wrapped: bool,
}

/// 单条匹配结果（对应原版 findPathsCLI 返回的 URL 集合）
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct MatchedFile {
    pub path: PathBuf,
}

impl From<PathBuf> for MatchedFile {
    fn from(path: PathBuf) -> Self {
        MatchedFile { path }
    }
}

/// 每应用条件 —— 对应原版 `Condition`（old/Pearcleaner/Logic/Conditions.swift:10-34）
/// 注意：bundle_id / include / exclude 在构造时已 pearFormat；force 路径仅保留存在的
#[derive(Clone, Debug)]
pub struct Condition {
    pub bundle_id: String,
    pub include: Vec<String>,
    pub exclude: Vec<String>,
    pub include_force: Vec<PathBuf>,
    pub exclude_force: Vec<PathBuf>,
}

/// 跳过条件 —— 对应原版 `SkipCondition`（Conditions.swift:36-40）
#[derive(Clone, Debug)]
pub struct SkipCondition {
    pub skip_prefix: Vec<String>,
    pub allow_prefixes: Vec<String>,
    pub skip_paths: Vec<String>,
}
