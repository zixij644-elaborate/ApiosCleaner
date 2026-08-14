//! 外部命令执行工具 —— 跨平台通用
//!
//! 包管理器/系统命令的调用模式各平台重复（spawn + 捕获输出 + 退出码/错误处理 +
//! 文本解析），提炼为统一工具：
//! - `run_capture`：执行并捕获 stdout/stderr（退出码由调用方决定）
//! - `run_checked`：严格模式（非零退出 → Err，附 stderr 尾部）
//! - `stderr_tail`：错误消息的 stderr 尾部截取
//! - `parse_freed_bytes`：从命令输出解析释放空间（"freed_space_regex" 模式：
//!   外部命令打印清理结果，正则提取数字 + 单位）
//!
//! 纯 std + regex，无 OS API 依赖；平台层（brew/apt/winget）只提供
//! 各自的参数构造与专属错误提示。

use std::path::Path;
use std::process::Command;

/// 命令输出（stdout/stderr 已转 UTF-8 lossy）
#[derive(Debug)]
pub struct CommandOutput {
    pub status: std::process::ExitStatus,
    pub stdout: String,
    pub stderr: String,
}

/// 执行命令并捕获输出。spawn 失败 → Err（程序不存在/权限）；退出码不判定，
/// 由调用方决定严格/宽松语义（如 brew uses 有依赖方时 exit 1 但 stdout 有效）。
pub fn run_capture(
    program: &Path,
    args: &[&str],
    envs: &[(&str, &str)],
) -> Result<CommandOutput, String> {
    let mut cmd = Command::new(program);
    cmd.args(args);
    for (k, v) in envs {
        cmd.env(k, v);
    }
    let output = cmd
        .output()
        .map_err(|e| format!("failed to run {}: {e}", program.display()))?;
    Ok(CommandOutput {
        status: output.status,
        stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
        stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
    })
}

/// 严格模式：非零退出 → Err（stderr 尾部）；成功 → Ok(CommandOutput)
pub fn run_checked(
    program: &Path,
    args: &[&str],
    envs: &[(&str, &str)],
) -> Result<CommandOutput, String> {
    let out = run_capture(program, args, envs)?;
    if out.status.success() {
        Ok(out)
    } else {
        Err(stderr_tail(&out, program))
    }
}

/// stderr 尾部（~5 行）错误消息：`<program> failed:` + 尾部
pub fn stderr_tail(out: &CommandOutput, program: &Path) -> String {
    let tail: Vec<&str> = out.stderr.lines().rev().take(5).collect();
    let tail = tail.iter().rev().copied().collect::<Vec<_>>().join("\n");
    format!("{} failed:\n{tail}", program.display())
}

/// 从命令输出解析释放空间字节数（freed_space_regex 模式）。
/// `regex` 必须含**一个**数字捕获组，可带小数；其后可选单位后缀
/// （K/M/G/T + 可选 i + B，按 1024 进制换算）。取第一个匹配。
/// 无匹配/解析失败 → 0（调用方自行决定是否视为"不可估算"）。
pub fn parse_freed_bytes(output: &str, regex: &str) -> u64 {
    let Ok(re) = regex::Regex::new(regex) else {
        return 0;
    };
    let Some(caps) = re.captures(output) else {
        return 0;
    };
    let Some(num) = caps.get(1) else {
        return 0;
    };
    let Ok(value) = num.as_str().replace(',', "").parse::<f64>() else {
        return 0;
    };
    // 单位后缀：数字之后紧跟 K/M/G/T（可选 i）+ B，如 "2,120 kB" / "1.5 GiB"
    let after = &output[num.end()..];
    let trimmed = after.trim_start();
    let mult: f64 = if let Some(unit) = trimmed.chars().next() {
        match unit.to_ascii_uppercase() {
            'K' => 1024.0,
            'M' => 1024.0 * 1024.0,
            'G' => 1024.0 * 1024.0 * 1024.0,
            'T' => 1024.0 * 1024.0 * 1024.0 * 1024.0,
            _ => 1.0,
        }
    } else {
        1.0
    };
    (value * mult) as u64
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[cfg(unix)]
    #[test]
    fn test_run_capture_echo() {
        let out = run_capture(Path::new("/bin/echo"), &["hello"], &[]).unwrap();
        assert!(out.status.success());
        assert_eq!(out.stdout.trim(), "hello");
    }

    #[test]
    fn test_run_capture_missing_program() {
        let err = run_capture(&PathBuf::from("/nonexistent/apios-zzz"), &[], &[]);
        assert!(err.is_err());
        assert!(err.unwrap_err().contains("failed to run"));
    }

    #[cfg(unix)]
    #[test]
    fn test_run_checked_nonzero_exit() {
        // /bin/sh -c "exit 3"：非零退出 → Err 含 stderr 尾部
        let err = run_checked(Path::new("/bin/sh"), &["-c", "echo boom >&2; exit 3"], &[]);
        assert!(err.is_err());
        assert!(err.unwrap_err().contains("boom"));
    }

    #[test]
    fn test_stderr_tail_limits_lines() {
        let out = CommandOutput {
            status: std::process::ExitStatus::default(),
            stdout: String::new(),
            stderr: "a\nb\nc\nd\ne\nf\ng\n".to_string(),
        };
        let msg = stderr_tail(&out, Path::new("tool"));
        // 只保留尾部 5 行
        assert_eq!(msg, "tool failed:\nc\nd\ne\nf\ng");
    }

    #[test]
    fn test_parse_freed_bytes_plain_number() {
        // 千分位逗号（"2,120"）剥离后解析
        assert_eq!(parse_freed_bytes("2,120 kB", r"(\d+[\d,]*)\s*kB"), 2_170_880);
        assert_eq!(parse_freed_bytes("2 kB", r"(\d+)\s*kB"), 2048);
    }

    #[test]
    fn test_parse_freed_bytes_units() {
        assert_eq!(
            parse_freed_bytes("Freed 1.5 MiB", r"Freed (\d+(?:\.\d+)?)\s*MiB"),
            1_572_864
        );
        assert_eq!(
            parse_freed_bytes("disk space freed: 12 GB", r"freed: (\d+)\s*GB"),
            12_884_901_888
        );
    }

    #[test]
    fn test_parse_freed_bytes_no_match_returns_zero() {
        assert_eq!(parse_freed_bytes("nothing to report", r"(\d+)\s*MiB"), 0);
        assert_eq!(parse_freed_bytes("", r"(\d+)"), 0);
    }

    #[test]
    fn test_parse_freed_bytes_bad_regex_returns_zero() {
        assert_eq!(parse_freed_bytes("x", r"("), 0);
    }
}
