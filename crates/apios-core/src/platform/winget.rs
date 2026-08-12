//! winget 包管理器实现（Windows 专属，cfg 门控，其他平台不编译）
//!
//! 范畴：`pkg` 命令（包本体卸载）。winget 缓存清理可归 dev-clean（尚无
//! 安全条目，缓存路径分散在 %LOCALAPPDATA%\Packages\Microsoft.DesktopAppInstaller）。
//!
//! 语义差异（对比 Homebrew）：
//! - winget 无 formula/cask 概念 → 全部归 Formula（Cask 表恒空），
//!   detect_kind 天然兼容；`pkg winget list` 只展示一遍（CLI 遍历双 kind）
//! - 无依赖查询 / 无 autoremove → dependents/autoremove 恒空
//! - 自动化运行必须带 --accept-source-agreements --disable-interactivity，
//!   否则首次运行卡交互/失败（CI/脚本环境）
//! - 卸载走 `winget uninstall --name <n> --silent`：仅删包本体，用户配置
//!   残留走 `apios uninstall`/orphan（对齐 cleaner 语义，不跑卸载器）

use std::path::{Path, PathBuf};
use std::process::Command;

use super::windows::WindowsAdapter;
use super::{PackageManager, PackageManagers};
use crate::pkg::{self, PkgInfo, PkgKind};

/// 自动化环境必须的全局旗标（首次运行接受源协议 + 禁用交互，防卡死）
const AUTOMATION_FLAGS: [&str; 2] = ["--accept-source-agreements", "--disable-interactivity"];

/// PATH 探测候选（';' 分隔；空段过滤防相对路径）
fn winget_candidates(path: &str) -> Vec<PathBuf> {
    let mut candidates = Vec::new();
    for dir in path.split(';') {
        if !dir.is_empty() {
            candidates.push(PathBuf::from(dir).join("winget.exe"));
        }
    }
    candidates
}

pub struct Winget {
    winget_path: Option<PathBuf>,
}

impl Winget {
    pub fn new() -> Self {
        let path = std::env::var_os("PATH").unwrap_or_default();
        let path = path.to_string_lossy().to_string();
        let winget_path = winget_candidates(&path).into_iter().find(|c| c.is_file());
        Winget { winget_path }
    }

    fn require_winget(&self) -> Result<&Path, String> {
        self.winget_path.as_deref().ok_or_else(|| {
            "winget is not installed (checked PATH; install App Installer from the Microsoft Store)"
                .to_string()
        })
    }

    /// 严格执行：失败（非零退出）→ Err（stderr 尾部）
    fn run(&self, args: &[&str]) -> Result<std::process::Output, String> {
        let winget = self.require_winget()?;
        let output = Command::new(winget)
            .args(args)
            .output()
            .map_err(|e| format!("failed to run winget: {e}"))?;
        if output.status.success() {
            Ok(output)
        } else {
            let stderr = String::from_utf8_lossy(&output.stderr);
            let tail: Vec<&str> = stderr.lines().rev().take(5).collect();
            let tail = tail.iter().rev().copied().collect::<Vec<_>>().join("\n");
            let mut msg = format!("winget {} failed:\n{tail}", args.first().unwrap_or(&""));
            // winget 的"未找到"与版本类错误走 stderr 首行带 --help 提示，一并带上
            let head = stderr.lines().take(1).collect::<Vec<_>>().join(" ");
            if !head.is_empty() && head.len() < 160 {
                msg = format!("winget: {head}");
            }
            Err(msg)
        }
    }
}

impl Default for Winget {
    fn default() -> Self {
        Self::new()
    }
}

/// 解析 `winget list --columns Name,Version` 输出（表格式纯文本）：
/// 跳过表头（Name Version）与全虚线分隔行；每行按 ≥2 空格分格 ——
/// 名称格可含单空格（"7-Zip"、"Microsoft Edge"），列间填充 ≥2 空格。
pub fn parse_winget_list(output: &str) -> Vec<PkgInfo> {
    let re = regex::Regex::new(r"\s{2,}").unwrap();
    let mut pkgs = Vec::new();
    for line in output.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with("Name") {
            continue;
        }
        if line.chars().all(|c| c == '-' || c == ' ') {
            continue; // 分隔行
        }
        let cells: Vec<&str> = re.split(line).collect();
        if cells.len() >= 2 {
            pkgs.push(PkgInfo {
                name: cells[0].to_string(),
                version: cells[cells.len() - 1].to_string(),
                kind: PkgKind::Formula,
            });
        }
    }
    pkgs.sort_by(|a, b| a.name.cmp(&b.name));
    pkgs
}

impl PackageManager for Winget {
    fn name(&self) -> &str {
        "winget"
    }

    fn list_installed(&self, kind: PkgKind) -> Result<Vec<PkgInfo>, String> {
        // winget 无 formula/cask 概念：Formula 返回全量；Cask 恒空
        // （CLI 双 kind 遍历，避免重复展示）
        if kind == PkgKind::Cask {
            return Ok(Vec::new());
        }
        let mut args = vec!["list", "--columns", "Name,Version"];
        args.extend(AUTOMATION_FLAGS);
        let output = self.run(&args)?;
        Ok(parse_winget_list(&String::from_utf8_lossy(&output.stdout)))
    }

    fn dependents(&self, _name: &str, _kind: PkgKind) -> Result<Vec<String>, String> {
        Ok(Vec::new()) // winget 无依赖查询
    }

    fn uninstall(
        &self,
        name: &str,
        _kind: PkgKind,
        _zap: bool,
        _ignore_deps: bool,
    ) -> Result<(), String> {
        let mut args = vec!["uninstall", "--name", name, "--silent"];
        args.extend(AUTOMATION_FLAGS);
        self.run(&args)?;
        Ok(())
    }

    fn autoremove_dry_run(&self) -> Result<Vec<String>, String> {
        Ok(Vec::new()) // winget 无孤儿依赖概念
    }

    fn autoremove(&self) -> Result<(), String> {
        Ok(())
    }
}

impl PackageManagers for WindowsAdapter {
    fn package_managers(&self) -> Vec<Box<dyn PackageManager>> {
        // 始终注册（即使 winget 缺失），CLI 才能区分
        // "未知包管理器" 与 "winget 未安装" 两种错误
        vec![Box::new(Winget::new())]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_winget_candidates_splits_semicolon() {
        let candidates = winget_candidates(r"C:\Windows\System32;C:\Program Files\foo");
        assert_eq!(
            candidates,
            vec![
                PathBuf::from(r"C:\Windows\System32\winget.exe"),
                PathBuf::from(r"C:\Program Files\foo\winget.exe"),
            ]
        );
    }

    #[test]
    fn test_winget_candidates_filters_empty_segments() {
        let candidates = winget_candidates(";C:\\Windows\\System32;;");
        assert_eq!(
            candidates,
            vec![PathBuf::from(r"C:\Windows\System32\winget.exe")]
        );
    }

    #[test]
    fn test_parse_list_basic_table() {
        let out = "Name                    Version\n\
            -------------------------------------  -------\n\
            7-Zip                   24.09\n\
            Microsoft Edge          139.0.2849.58\n";
        let pkgs = parse_winget_list(out);
        assert_eq!(pkgs.len(), 2);
        assert_eq!(pkgs[0].name, "7-Zip");
        assert_eq!(pkgs[0].version, "24.09");
        // 名称含单空格 → 仍是同一格
        assert_eq!(pkgs[1].name, "Microsoft Edge");
        assert_eq!(pkgs[1].version, "139.0.2849.58");
        assert!(pkgs.iter().all(|p| p.kind == PkgKind::Formula));
    }

    #[test]
    fn test_parse_list_empty_and_header_only() {
        assert!(parse_winget_list("").is_empty());
        // 无包时 winget 仍打印表头 + 分隔行
        assert!(parse_winget_list("Name    Version\n--------  -------\n").is_empty());
    }

    #[test]
    fn test_parse_list_skips_noise_lines() {
        let out = "Name    Version\n--------  -------\n\
            Some package \"note\" text\n\
            Foo       1.0.0\n";
        // 引号注释行（如 "No installed package found..."）→ 分格不足 2 → 忽略
        let pkgs = parse_winget_list(out);
        assert_eq!(pkgs.len(), 1);
        assert_eq!(pkgs[0].name, "Foo");
    }

    #[test]
    fn test_parse_list_sorts() {
        let out = "Name    Version\n--------  -------\n\
            zz 2.0\naa 1.0\n";
        let pkgs = parse_winget_list(out);
        assert_eq!(pkgs[0].name, "aa");
        assert_eq!(pkgs[1].name, "zz");
    }
}
