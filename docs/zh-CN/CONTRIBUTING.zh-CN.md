[English](../../CONTRIBUTING.md) | [中文](CONTRIBUTING.zh-CN.md)

<!-- 翻译基准：本次提交（docs: zh-CN — 首次翻译 CONTRIBUTING/SECURITY/CODE_OF_CONDUCT）。英文文档改动后请同步更新译文 -->

# 为 ApiosCleaner 做贡献

感谢你考虑贡献！本项目是一款跨平台应用清理工具：可移植的 Rust 核心 + 按 OS 划分的适配器层。请先阅读 [docs/ARCHITECTURE.md](../ARCHITECTURE.md)（[中文版](ARCHITECTURE.zh-CN.md)）了解分层结构——纯逻辑绝不能依赖系统 API。

## 起步

```sh
git clone git@github.com:Zniece/ApiosCleaner.git
cd ApiosCleaner
cargo build
cargo test --all
```

## 质量门禁

提交前请确认本地门禁通过——CI 同样强制执行：

```sh
cargo fmt --all --check
cargo clippy --all-targets -- -D warnings
cargo test --all
cargo check --workspace --target x86_64-unknown-linux-gnu   # Linux 交叉检查
```

Linux 交叉检查是硬性门禁：核心必须在非 macOS 目标上零改动通过类型检查。如果你的改动需要平台专属行为，把它放进平台适配器层的 trait 中，而不是核心。

## 代码库规则

- **可移植核心**：`crates/apios-core/src/platform/` 之外不得调用系统 API。
- **安全第一**：删除路径必须经过 `trash.rs::validate_path`（词法归一化）和确认流程（`y/N`，默认否）。
- **不要重写，要改进**：代码库源自 Pearcleaner，但不是逐字移植——修复缺陷、化繁为简，而不是照搬。
- **测试**：新的匹配/扫描/解析逻辑需要单元测试，使用固定字节/字符串和 `tempfile` 临时目录树，不依赖真实系统状态。

## 拉取请求流程

1. Fork 本仓库并创建分支（`git checkout -b fix/...`）。
2. 做出改动，保持提交小而专注。
3. 跑完上面全部四道质量门禁。
4. 向 `main` 提交拉取请求。说明改了什么、为什么改、验证了什么。行为变更附上截图或前后输出更佳。
5. CI 运行相同的门禁；红色检查必须在合并前解决。

## 报告缺陷

使用模板创建 issue 报告缺陷。安全问题**不要**开公开 issue——见 [SECURITY.md](../../SECURITY.md)（[中文版](SECURITY.zh-CN.md)）。
