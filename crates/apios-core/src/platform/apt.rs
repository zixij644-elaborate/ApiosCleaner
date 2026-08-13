//! apt 包管理器实现（Linux 专属，cfg 门控）
//!
//! 范畴：`pkg` 命令（包本体卸载 + 依赖处理 + 孤儿依赖 autoremove）。
//! apt 缓存清理归 dev-clean（linux.rs dev_envs_table 的 "APT Cache" 条目），
//! 不在此处 —— 与 macOS brew 的划分一致（缓存归 dev-clean，本体归 pkg）。
//!
//! 命令语义（Debian/Ubuntu/Kali 实测格式）：
//! - `apt list --installed`：`<name>/<archive>,now <version> <arch> [installed,...]`
//! - `apt-cache rdepends --installed <name>`：`Reverse Depends:` 段内缩进行
//! - `apt-get remove -y <name>`：卸载（保留配置文件；purge 属显式选项，不在此命令）
//! - `apt-get autoremove --dry-run`：输出 "will be REMOVED" 段（缩进包名行）
//! - `apt-get autoremove -y`：移除孤儿依赖
//!
//! 权限：apt-get 写操作需要 root —— 非 root 失败时错误消息提示
//! `sudo apios pkg apt ...`（不隐式提权，与 check_protected 的 sudo 提示一致）。
//! 只读命令（list / rdepends / dry-run）普通用户可执行。

use std::path::Path;

use super::{PackageManager, PackageManagers};
use crate::cmd_util;
use crate::pkg::{self, PkgInfo, PkgKind};

/// apt 系列可执行文件（PATH 解析；各发行版均在 /usr/bin）
const APT_GET: &str = "apt-get";
const APT_CACHE: &str = "apt-cache";
const APT: &str = "apt";

pub struct Apt;

impl Apt {
    fn run(&self, program: &str, args: &[&str]) -> Result<cmd_util::CommandOutput, String> {
        let out = cmd_util::run_capture(Path::new(program), args, &[])?;
        if out.status.success() {
            Ok(out)
        } else {
            Err(apt_error(&out, program))
        }
    }
}

/// 错误消息：stderr 尾部 + 权限场景的 sudo 提示
fn apt_error(out: &cmd_util::CommandOutput, program: &str) -> String {
    let mut msg = cmd_util::stderr_tail(out, Path::new(program));
    let stderr = out.stderr.to_ascii_lowercase();
    if stderr.contains("permission denied") || stderr.contains("lock file") {
        msg.push_str("\nHint: apt needs root — run with sudo: sudo apios pkg apt ...");
    }
    msg
}

impl PackageManager for Apt {
    fn name(&self) -> &str {
        "apt"
    }

    /// 本管理器只有通用二进制包（无 formula/cask 之分）
    fn supported_kinds(&self) -> Vec<PkgKind> {
        vec![PkgKind::Package]
    }

    fn list_installed(&self, kind: PkgKind) -> Result<Vec<PkgInfo>, String> {
        if kind != PkgKind::Package {
            return Ok(Vec::new());
        }
        let out = self.run(APT, &["list", "--installed"])?;
        Ok(pkg::parse_apt_list(&out.stdout))
    }

    fn dependents(&self, name: &str, _kind: PkgKind) -> Result<Vec<String>, String> {
        let out = self.run(APT_CACHE, &["rdepends", "--installed", name])?;
        Ok(pkg::parse_apt_rdepends(&out.stdout))
    }

    /// `apt-get remove -y`：保留配置文件（purge 是显式选项，需另行设计，
    /// 与 brew `--zap` 的"不自动删配置"决策一致）。
    /// zap / ignore_deps 无对应物，忽略。
    fn uninstall(
        &self,
        name: &str,
        _kind: PkgKind,
        _zap: bool,
        _ignore_deps: bool,
    ) -> Result<(), String> {
        self.run(APT_GET, &["remove", "-y", name])?;
        Ok(())
    }

    fn autoremove_dry_run(&self) -> Result<Vec<String>, String> {
        let out = self.run(APT_GET, &["autoremove", "--dry-run"])?;
        Ok(pkg::parse_apt_autoremove(&out.stdout))
    }

    fn autoremove(&self) -> Result<(), String> {
        self.run(APT_GET, &["autoremove", "-y"])?;
        Ok(())
    }
}

impl PackageManagers for crate::platform::linux::LinuxAdapter {
    fn package_managers(&self) -> Vec<Box<dyn PackageManager>> {
        // 与 macOS 的决策一致：PM 始终注册（即使 apt 缺失），CLI 才能区分
        // "未知包管理器" 与 "apt 不可用" 两种错误（错误发生在命令执行时）
        vec![Box::new(Apt)]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_apt_error_permission_hint() {
        let out = cmd_util::CommandOutput {
            status: std::process::ExitStatus::default(),
            stdout: String::new(),
            stderr: "E: Could not open lock file /var/lib/dpkg/lock-frontend - open (13: Permission denied)\n".to_string(),
        };
        let msg = apt_error(&out, APT_GET);
        assert!(msg.contains("sudo apios"), "msg: {msg}");
    }

    #[test]
    fn test_apt_error_other_error_no_sudo_hint() {
        let out = cmd_util::CommandOutput {
            status: std::process::ExitStatus::default(),
            stdout: String::new(),
            stderr: "E: Unable to locate package nonexistent-zzz\n".to_string(),
        };
        let msg = apt_error(&out, APT_GET);
        assert!(!msg.contains("sudo"), "msg: {msg}");
    }
}
