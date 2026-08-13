//! apios-core: ApiosCleaner 跨平台核心引擎
//!
//! 平台无关的纯逻辑层：路径归一化、标识符预计算、启发式匹配、相关文件搜索、
//! 孤儿查找、回收站语义、包管理抽象、开发环境缓存。引擎零 OS API 依赖，
//! 平台相关行为全部收敛在 `platform` 模块（trait + `cfg(target_os)` 编译期选型）。
//!
//! 本项目起源于 Pearcleaner（macOS Swift 工具）的 Rust 重写，已独立演进为
//! 跨平台清理器：macOS 全功能、Windows 适配器（真机验证）、Linux 默认 XDG。

pub mod app_info;
pub mod cmd_util;
pub mod conditions;
pub mod desktop;
pub mod dev_env;
pub mod format;
pub mod identifiers;
pub mod locations;
pub mod matcher;
pub mod model;
pub mod orphan;
pub mod pkg;
pub mod platform;
pub mod plugin;
pub mod scan;

pub mod search;
pub mod trash;

pub use model::{AppInfo, Sensitivity};
