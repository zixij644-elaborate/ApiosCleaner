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

/// 包种类。brew 区分 formula / cask（源码包/预编译 GUI 应用）；其他包管理器
/// 无此概念（winget 历史原因全归 Formula；apt/snap 等用通用 `Package`）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PkgKind {
    Formula,
    Cask,
    /// 通用二进制包（apt/snap 等无 formula/cask 区分的管理器）
    Package,
}

impl PkgKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            PkgKind::Formula => "formula",
            PkgKind::Cask => "cask",
            PkgKind::Package => "package",
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

/// 解析 `apt list --installed` 输出。行格式（apt 2.x）：
/// `<name>/<archive>,now <version> <arch> [installed,...]`
/// （如 `adduser/stable,now 3.137 all [installed]`）；跳过 `Listing...` 头行
/// 与不含 `/` 的行（apt 某些本地包行无 archive 段时不产出条目）。
pub fn parse_apt_list(output: &str) -> Vec<PkgInfo> {
    let mut pkgs: Vec<PkgInfo> = output
        .lines()
        .filter_map(|line| {
            let line = line.trim();
            let (name, rest) = line.split_once('/')?;
            let mut fields = rest.split_whitespace();
            // archive 段（"stable,now"）之后第一个字段是版本
            let _archive = fields.next()?;
            let version = fields.next()?;
            if name.is_empty() {
                return None;
            }
            Some(PkgInfo {
                name: name.to_string(),
                version: version.to_string(),
                kind: PkgKind::Package,
            })
        })
        .collect();
    pkgs.sort_by(|a, b| a.name.cmp(&b.name));
    pkgs
}

/// 解析 `apt-get autoremove --dry-run` 输出的 "will be REMOVED" 段。
/// 段结构：标记行后是**缩进**的包名行（可多词一行），随后非缩进的统计行
/// （"N upgraded, ... to remove..."）结束段。
pub fn parse_apt_autoremove(output: &str) -> Vec<String> {
    let mut pkgs: Vec<String> = Vec::new();
    let mut in_section = false;
    for line in output.lines() {
        if line.contains("will be REMOVED") {
            in_section = true;
            continue;
        }
        if in_section {
            if line.starts_with(char::is_whitespace) {
                pkgs.extend(line.split_whitespace().map(str::to_string));
            } else if !line.trim().is_empty() {
                // 非缩进非空行 = 段结束（统计行）
                break;
            }
        }
    }
    pkgs.sort_unstable();
    pkgs.dedup();
    pkgs
}

/// 解析 `apt-cache rdepends --installed <name>` 输出：`Reverse Depends:` 段内
/// 的缩进行（每行一个已安装的被依赖方）。
pub fn parse_apt_rdepends(output: &str) -> Vec<String> {
    let mut deps: Vec<String> = Vec::new();
    let mut in_section = false;
    for line in output.lines() {
        if line.contains("Reverse Depends") {
            in_section = true;
            continue;
        }
        if in_section {
            let t = line.trim();
            if t.is_empty() {
                continue;
            }
            // or 型替代依赖输出 `  |pkgname` —— 剥 `|` 前缀，否则得到
            // "|libxml2-utils" 假依赖名（2026-08-15 审查 P1-21）
            let t = t.trim_start_matches('|').trim();
            if !t.is_empty() {
                deps.push(t.to_string());
            }
        }
    }
    deps.sort_unstable();
    deps.dedup();
    deps
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

    // ---- apt 解析 ----

    #[test]
    fn test_parse_apt_list_basic() {
        let out = "Listing... Done\nadduser/stable,now 3.137 all [installed]\napt/stable,now 2.7.14 amd64 [installed,automatic]\n";
        let pkgs = parse_apt_list(out);
        assert_eq!(pkgs.len(), 2);
        assert_eq!(pkgs[0].name, "adduser");
        assert_eq!(pkgs[0].version, "3.137");
        assert_eq!(pkgs[0].kind, PkgKind::Package);
        assert_eq!(pkgs[1].name, "apt");
        assert_eq!(pkgs[1].version, "2.7.14");
    }

    #[test]
    fn test_parse_apt_list_skips_header_and_junk() {
        // 无 Listing 头；缺 / 的行（"WARNING: apt does not have a stable CLI interface"）跳过
        let out = "WARNING: apt does not have a stable CLI interface. Use with caution in scripts.\nzsh/stable,now 5.9-1 amd64 [installed]\n";
        let pkgs = parse_apt_list(out);
        assert_eq!(pkgs.len(), 1);
        assert_eq!(pkgs[0].name, "zsh");
    }

    #[test]
    fn test_parse_apt_list_empty() {
        assert!(parse_apt_list("").is_empty());
        assert!(parse_apt_list("Listing... Done\n").is_empty());
    }

    #[test]
    fn test_parse_apt_list_sorts() {
        let out = "zsh/stable,now 5.9-1 amd64 [installed]\nalpha/stable,now 1.0 all [installed]\n";
        let pkgs = parse_apt_list(out);
        let names: Vec<&str> = pkgs.iter().map(|p| p.name.as_str()).collect();
        assert_eq!(names, vec!["alpha", "zsh"]);
    }

    #[test]
    fn test_parse_apt_autoremove_basic() {
        let out = "Reading package lists... Done\nBuilding dependency tree... Done\nReading state information... Done\nThe following packages will be REMOVED:\n  libfoo1\n  libbar2\n0 upgraded, 0 newly installed, 2 to remove and 0 not upgraded.\n";
        assert_eq!(parse_apt_autoremove(out), vec!["libbar2", "libfoo1"]);
    }

    #[test]
    fn test_parse_apt_autoremove_multiple_per_line() {
        let out = "The following packages will be REMOVED:\n  libfoo1 libbar2\n0 upgraded, 0 newly installed, 2 to remove.\n";
        assert_eq!(parse_apt_autoremove(out), vec!["libbar2", "libfoo1"]);
    }

    #[test]
    fn test_parse_apt_autoremove_nothing_to_remove() {
        // 无 REMOVED 段（"0 upgraded, 0 newly installed, 0 to remove"）→ 空
        let out = "Reading package lists... Done\n0 upgraded, 0 newly installed, 0 to remove and 0 not upgraded.\n";
        assert!(parse_apt_autoremove(out).is_empty());
    }

    #[test]
    fn test_parse_apt_rdepends_basic() {
        let out = "git\nReverse Depends:\n  libngtcp2\n  node\n";
        assert_eq!(parse_apt_rdepends(out), vec!["libngtcp2", "node"]);
    }

    #[test]
    fn test_parse_apt_rdepends_none() {
        // 无反向依赖：只有头部包名行，无 "Reverse Depends" 段
        assert!(parse_apt_rdepends("git\n").is_empty());
        assert!(parse_apt_rdepends("").is_empty());
    }
}
