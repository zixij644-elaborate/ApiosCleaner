//! Homebrew 包管理器实现（macOS 专属，cfg 门控，Linux/Windows 构建不编译本文件）
//!
//! 范畴：`pkg` 命令（包本体卸载 + 依赖处理）。brew 缓存清理归 dev-clean
//! （macos.rs dev_envs_table 的 "Homebrew" 条目），不在此处。
//!
//! 命令语义（brew 6.x 实测验证）：
//! - `brew list --versions --formula|--cask`：每行 `name version`（管道输出纯文本）
//! - `brew uses --installed <name>`：列出已安装的被依赖方。`uses` 不解析 cask
//!   参数类型（仅过滤输出），cask 走慢路径：stderr 出 `No available formula`
//!   警告，**有依赖方时 brew 仍打印 stdout 但 exit 1**（uses.rb:92 odie）
//!   → dependents() 必须无视退出码解析 stdout
//! - `brew uninstall --formula|--cask [--zap] [--ignore-dependencies] <name>`
//!   （依赖豁免旗标是 --ignore-dependencies，不是 --force；--zap 仅 cask 合法）
//! - `brew autoremove -n`：dry-run，`Would autoremove N unneeded formula:` 头行 +
//!   每行一个包名
//! - pinned 公式会拒绝卸载（`is pinned`），多版本同装会拒绝（`multiple installed
//!   versions`）→ 错误文本追加提示，不自动强删（用户决策：永不用 --force）

use std::path::{Path, PathBuf};

use super::macos::MacOsAdapter;
use super::{PackageManager, PackageManagers};
use crate::cmd_util;
use crate::pkg::{self, PkgInfo, PkgKind};

/// 探测候选位置：Apple Silicon /opt/homebrew、Intel /usr/local、PATH
/// （PATH 空段（`:x::y:`）会生成相对路径 "./brew" —— 过滤为仅绝对路径）
fn brew_candidates(path: &str) -> Vec<PathBuf> {
    let mut candidates = vec![
        PathBuf::from("/opt/homebrew/bin/brew"),
        PathBuf::from("/usr/local/bin/brew"),
    ];
    for dir in std::env::split_paths(path) {
        if dir.is_absolute() {
            candidates.push(dir.join("brew"));
        }
    }
    candidates
}

pub struct Homebrew {
    brew_path: Option<PathBuf>,
}

impl Homebrew {
    pub fn new() -> Self {
        let path = std::env::var_os("PATH").unwrap_or_default();
        let path = path.to_string_lossy().to_string();
        let brew_path = brew_candidates(&path).into_iter().find(|c| c.is_file());
        Homebrew { brew_path }
    }

    fn require_brew(&self) -> Result<&Path, String> {
        self.brew_path.as_deref().ok_or_else(|| {
            "Homebrew is not installed (checked /opt/homebrew/bin, /usr/local/bin, PATH)"
                .to_string()
        })
    }

    /// 严格执行：失败（非零退出）→ Err（stderr 尾部 + brew 专属提示）；
    /// 成功 → Ok(CommandOutput)
    fn run(&self, args: &[&str]) -> Result<cmd_util::CommandOutput, String> {
        let brew = self.require_brew()?;
        let out = cmd_util::run_capture(brew, args, &[("HOMEBREW_NO_AUTO_UPDATE", "1")])?;
        if out.status.success() {
            Ok(out)
        } else {
            Err(brew_error(brew, &out))
        }
    }

    /// 无视退出码执行（cask 的 uses 依赖方场景：brew exit 1 但 stdout 有结果）
    fn run_ignore_status(&self, args: &[&str]) -> Result<cmd_util::CommandOutput, String> {
        let brew = self.require_brew()?;
        cmd_util::run_capture(brew, args, &[("HOMEBREW_NO_AUTO_UPDATE", "1")])
    }
}

impl Default for Homebrew {
    fn default() -> Self {
        Self::new()
    }
}

/// stderr 尾部（~5 行）+ 已识别的特殊失败提示（pinned / 多版本）
fn brew_error(brew: &Path, out: &cmd_util::CommandOutput) -> String {
    let mut msg = cmd_util::stderr_tail(out, brew);
    if out.stderr.contains("is pinned") {
        msg.push_str("\nHint: run `brew unpin` first, then retry (or uninstall via `brew uninstall --force`).");
    } else if out.stderr.contains("multiple installed versions") {
        msg.push_str("\nHint: run `brew uninstall --force` to remove all installed versions.");
    }
    msg
}

/// `brew uninstall --formula|--cask [--zap] [--ignore-dependencies] <name>`
fn uninstall_args(name: &str, kind: PkgKind, zap: bool, ignore_deps: bool) -> Vec<String> {
    let mut args = vec!["uninstall".to_string()];
    args.push(
        match kind {
            PkgKind::Formula => "--formula",
            PkgKind::Cask => "--cask",
            PkgKind::Package => unreachable!("brew has no Package kind"),
        }
        .to_string(),
    );
    if zap && kind == PkgKind::Cask {
        args.push("--zap".to_string());
    }
    if ignore_deps {
        args.push("--ignore-dependencies".to_string());
    }
    args.push(name.to_string());
    args
}

fn uses_args(name: &str) -> Vec<String> {
    vec![
        "uses".to_string(),
        "--installed".to_string(),
        name.to_string(),
    ]
}

fn list_args(kind: PkgKind) -> Vec<String> {
    vec![
        "list".to_string(),
        "--versions".to_string(),
        match kind {
            PkgKind::Formula => "--formula",
            PkgKind::Cask => "--cask",
            PkgKind::Package => unreachable!("brew has no Package kind"),
        }
        .to_string(),
    ]
}

impl PackageManager for Homebrew {
    fn name(&self) -> &str {
        "brew"
    }

    fn list_installed(&self, kind: PkgKind) -> Result<Vec<PkgInfo>, String> {
        let args = list_args(kind);
        let output = self.run(
            args.iter()
                .map(String::as_str)
                .collect::<Vec<_>>()
                .as_slice(),
        )?;
        Ok(pkg::parse_brew_list_versions(&output.stdout, kind))
    }

    fn dependents(&self, name: &str, kind: PkgKind) -> Result<Vec<String>, String> {
        let args = uses_args(name);
        // 无视退出码：cask 参数在**有**依赖方时 brew exit 1（uses.rb:92 odie），
        // 但 stdout 已完整输出。区别对待"无依赖方"与"brew 调用失败"：
        // - exit 0 → 正常（formula 无依赖方时 stdout 为空）
        // - stdout 非空 → 无论退出码，直接解析（cask 有依赖方场景）
        // - 空 stdout + 非零退出：stderr 是 cask 慢路径预期警告（"No available
        //   formula"）→ 视为无依赖方；否则是真实失败 → Err
        let _ = kind;
        let brew = self.require_brew()?;
        let out = self.run_ignore_status(
            args.iter()
                .map(String::as_str)
                .collect::<Vec<_>>()
                .as_slice(),
        )?;
        let dependents = pkg::parse_brew_uses(&out.stdout);
        if out.status.success() || !dependents.is_empty() {
            return Ok(dependents);
        }
        if out.stderr.contains("No available formula") {
            return Ok(dependents);
        }
        Err(brew_error(brew, &out))
    }

    fn uninstall(
        &self,
        name: &str,
        kind: PkgKind,
        zap: bool,
        ignore_deps: bool,
    ) -> Result<(), String> {
        let args = uninstall_args(name, kind, zap, ignore_deps);
        self.run(
            args.iter()
                .map(String::as_str)
                .collect::<Vec<_>>()
                .as_slice(),
        )?;
        Ok(())
    }

    fn autoremove_dry_run(&self) -> Result<Vec<String>, String> {
        let output = self.run(&["autoremove", "-n"])?;
        Ok(pkg::parse_brew_autoremove(&output.stdout))
    }

    fn autoremove(&self) -> Result<(), String> {
        self.run(&["autoremove"])?;
        Ok(())
    }
}

impl PackageManagers for MacOsAdapter {
    fn package_managers(&self) -> Vec<Box<dyn PackageManager>> {
        // PM 始终注册（即使 brew 二进制缺失），CLI 才能区分
        // "未知包管理器" 与 "brew 未安装" 两种错误
        vec![Box::new(Homebrew::new())]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_brew_candidates_ordering() {
        let candidates = brew_candidates("/usr/bin:/bin");
        assert_eq!(
            candidates,
            vec![
                PathBuf::from("/opt/homebrew/bin/brew"),
                PathBuf::from("/usr/local/bin/brew"),
                PathBuf::from("/usr/bin/brew"),
                PathBuf::from("/bin/brew"),
            ]
        );
    }

    #[test]
    fn test_brew_candidates_filters_empty_segments() {
        // PATH 空段 → 不得生成相对路径 "./brew"
        let candidates = brew_candidates(":/usr/bin::/bin:");
        assert_eq!(
            candidates,
            vec![
                PathBuf::from("/opt/homebrew/bin/brew"),
                PathBuf::from("/usr/local/bin/brew"),
                PathBuf::from("/usr/bin/brew"),
                PathBuf::from("/bin/brew"),
            ]
        );
    }

    #[test]
    fn test_uninstall_args_formula_plain() {
        let args = uninstall_args("git", PkgKind::Formula, false, false);
        assert_eq!(args, vec!["uninstall", "--formula", "git"]);
    }

    #[test]
    fn test_uninstall_args_cask_with_zap() {
        let args = uninstall_args("firefox", PkgKind::Cask, true, false);
        assert_eq!(args, vec!["uninstall", "--cask", "--zap", "firefox"]);
    }

    #[test]
    fn test_uninstall_args_ignore_deps() {
        let args = uninstall_args("openssl@3", PkgKind::Formula, false, true);
        assert_eq!(
            args,
            vec![
                "uninstall",
                "--formula",
                "--ignore-dependencies",
                "openssl@3"
            ]
        );
    }

    #[test]
    fn test_uninstall_args_zap_ignored_for_formula() {
        // 公式不允许 --zap（brew 报错），参数构造即剔除
        let args = uninstall_args("git", PkgKind::Formula, true, false);
        assert_eq!(args, vec!["uninstall", "--formula", "git"]);
    }

    #[test]
    fn test_uninstall_args_no_force_flag() {
        // 用户决策：永不用 --force（多版本/pinned 场景给提示不自动强删）
        for (kind, zap, ignore) in [
            (PkgKind::Formula, false, false),
            (PkgKind::Cask, true, true),
            (PkgKind::Formula, true, true),
        ] {
            let args = uninstall_args("x", kind, zap, ignore);
            assert!(
                !args.iter().any(|a| a == "--force"),
                "uninstall args must never contain --force: {args:?}"
            );
        }
    }

    #[test]
    fn test_uses_args_shape() {
        assert_eq!(
            uses_args("openssl@3"),
            vec!["uses", "--installed", "openssl@3"]
        );
    }

    #[test]
    fn test_list_args_shape() {
        assert_eq!(
            list_args(PkgKind::Formula),
            vec!["list", "--versions", "--formula"]
        );
        assert_eq!(
            list_args(PkgKind::Cask),
            vec!["list", "--versions", "--cask"]
        );
    }
}
