//! pear-core: Pearcleaner 核心引擎（Rust 重写）
//!
//! 从原版 Swift 实现（old/Pearcleaner/Logic/*.swift）忠实移植的核心逻辑：
//! 路径归一化、标识符预计算、启发式匹配、相关文件搜索、孤儿查找、回收站删除。

pub mod app_info;
pub mod conditions;
pub mod dev_env;
pub mod format;
pub mod identifiers;
pub mod lipo;
pub mod locations;
pub mod matcher;
pub mod model;
pub mod orphan;
pub mod pkg;
pub mod platform;
pub mod scan;
pub mod search;
pub mod trash;

pub use model::{AppInfo, Sensitivity};
