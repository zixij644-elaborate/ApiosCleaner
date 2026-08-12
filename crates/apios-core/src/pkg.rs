//! 包管理器（pkg）核心纯逻辑 —— 平台无关
//!
//! 与 dev-clean（清可再生**缓存**）不同，pkg 范畴处理**包本体卸载**：
//! 卸载包、被依赖方警告、孤儿依赖清理（autoremove）。
//!
//! 本模块只放类型与纯解析函数（`brew list --versions` / `uses` / `autoremove`
//! 的输出解析、公式/cask 种类判定），命令执行在平台适配层
//! （trait `PackageManager`：macOS Homebrew，其他平台暂无）。
//!
//! 解析全部基于纯文本（每行一个条目），不引入 serde/JSON —— 项目无 serde 依赖。

/// 包种类（brew 的 formula / cask）
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PkgKind {
    Formula,
    Cask,
}

impl PkgKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            PkgKind::Formula => "formula",
            PkgKind::Cask => "cask",
        }
    }
}

/// 单个已安装包
#[derive(Clone, Debug)]
pub struct PkgInfo {
    pub name: String,
    pub version: String,
    pub kind: PkgKind,
}

/// 解析 `brew list --versions` 输出（每行 `name version...`，多版本空格分隔）
pub fn parse_brew_list_versions(output: &str, kind: PkgKind) -> Vec<PkgInfo> {
    let mut pkgs: Vec<PkgInfo> = output
        .lines()
        .filter_map(|line| {
            let line = line.trim();
            if line.is_empty() {
                return None;
            }
            let (name, rest) = match line.split_once(char::is_whitespace) {
                Some((n, r)) => (n, r.trim()),
                None => (line, ""),
            };
            if name.is_empty() {
                return None;
            }
            Some(PkgInfo {
                name: name.to_string(),
                version: rest.to_string(),
                kind,
            })
        })
        .collect();
    pkgs.sort_by(|a, b| a.name.cmp(&b.name));
    pkgs
}

/// 解析 `brew uses --installed` 输出（管道输出每行一个被依赖方）
pub fn parse_brew_uses(output: &str) -> Vec<String> {
    output
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .map(str::to_string)
        .collect()
}

/// 解析 `brew autoremove -n` 输出：跳过 `Would autoremove N unneeded formula:` 头行，
/// 收其余非空行（每行一个包名）
pub fn parse_brew_autoremove(output: &str) -> Vec<String> {
    output
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .filter(|l| !l.starts_with("Would autoremove") && !l.starts_with("Autoremoving"))
        .map(str::to_string)
        .collect()
}

/// 按已安装列表判定包种类：formula 表命中 → Formula，否则 cask 命中 → Cask。
/// 两边都命中（brew 禁止，理论上不可能）→ Formula（brew 自身解析优先级，
/// named_args.rb 先按 formula 解析）。
pub fn detect_kind(name: &str, formulae: &[String], casks: &[String]) -> Option<PkgKind> {
    if formulae.iter().any(|f| f == name) {
        Some(PkgKind::Formula)
    } else if casks.iter().any(|c| c == name) {
        Some(PkgKind::Cask)
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_list_versions_basic() {
        let out = "ada-url 4.0.0\nbrotli 1.2.0\nc-ares 1.34.8\n";
        let pkgs = parse_brew_list_versions(out, PkgKind::Formula);
        assert_eq!(pkgs.len(), 3);
        assert_eq!(pkgs[0].name, "ada-url");
        assert_eq!(pkgs[0].version, "4.0.0");
        assert_eq!(pkgs[0].kind, PkgKind::Formula);
    }

    #[test]
    fn test_parse_list_versions_multiple_versions() {
        let out = "python@3.14 3.14.0 3.14.1\n";
        let pkgs = parse_brew_list_versions(out, PkgKind::Formula);
        assert_eq!(pkgs.len(), 1);
        assert_eq!(pkgs[0].version, "3.14.0 3.14.1");
    }

    #[test]
    fn test_parse_list_versions_cask_latest() {
        let out = "android-cli latest\nfirefox 140.0\n";
        let pkgs = parse_brew_list_versions(out, PkgKind::Cask);
        assert_eq!(pkgs.len(), 2);
        assert_eq!(pkgs[0].version, "latest");
        assert!(pkgs.iter().all(|p| p.kind == PkgKind::Cask));
    }

    #[test]
    fn test_parse_list_versions_sorts_and_skips_blank() {
        let out = "zsh 5.9\nalpha 1.0\n\n  \nb 2.0\n";
        let pkgs = parse_brew_list_versions(out, PkgKind::Formula);
        let names: Vec<&str> = pkgs.iter().map(|p| p.name.as_str()).collect();
        assert_eq!(names, vec!["alpha", "b", "zsh"]);
    }

    #[test]
    fn test_parse_list_versions_empty() {
        assert!(parse_brew_list_versions("", PkgKind::Formula).is_empty());
        assert!(parse_brew_list_versions("   \n\n", PkgKind::Cask).is_empty());
    }

    #[test]
    fn test_parse_uses() {
        let out = "libngtcp2\nnode\npython@3.14\n";
        assert_eq!(
            parse_brew_uses(out),
            vec!["libngtcp2", "node", "python@3.14"]
        );
        assert!(parse_brew_uses("").is_empty());
    }

    #[test]
    fn test_parse_uses_trailing_spaces() {
        let out = "libngtcp2  \nnode \n";
        assert_eq!(parse_brew_uses(out), vec!["libngtcp2", "node"]);
    }

    #[test]
    fn test_parse_autoremove_skips_header() {
        let out = "Would autoremove 2 unneeded formula:\na\nb\n";
        assert_eq!(parse_brew_autoremove(out), vec!["a", "b"]);
    }

    #[test]
    fn test_parse_autoremove_singular_header() {
        let out = "Would autoremove 1 unneeded formula:\nx\n";
        assert_eq!(parse_brew_autoremove(out), vec!["x"]);
    }

    #[test]
    fn test_parse_autoremove_no_header() {
        let out = "a\nb\n";
        assert_eq!(parse_brew_autoremove(out), vec!["a", "b"]);
    }

    #[test]
    fn test_parse_autoremove_empty() {
        assert!(parse_brew_autoremove("").is_empty());
    }

    #[test]
    fn test_detect_kind() {
        let formulae = vec!["git".to_string(), "openssl@3".to_string()];
        let casks = vec!["firefox".to_string()];
        assert_eq!(
            detect_kind("git", &formulae, &casks),
            Some(PkgKind::Formula)
        );
        assert_eq!(
            detect_kind("firefox", &formulae, &casks),
            Some(PkgKind::Cask)
        );
        assert_eq!(detect_kind("nope", &formulae, &casks), None);
    }

    #[test]
    fn test_detect_kind_prefers_formula() {
        // 两边都命中 → Formula（brew 自身优先级）
        let formulae = vec!["dual".to_string()];
        let casks = vec!["dual".to_string()];
        assert_eq!(
            detect_kind("dual", &formulae, &casks),
            Some(PkgKind::Formula)
        );
    }
}
