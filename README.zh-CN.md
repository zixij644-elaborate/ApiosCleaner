[English](README.md) | [中文](README.zh-CN.md)

# ApiosCleaner

*ἄπιος (ápios) — 古希腊语「梨」*

一款快速的跨平台应用清理工具，核心采用可移植的 Rust 引擎。它起源于
[Pearcleaner](https://github.com/alienator88/Pearcleaner) 的重写，但并非照搬：
我们修复了原版实际存在的安全与正确性缺陷，重新设计了跨平台架构，并独立演进。

> ⚠️ **当前状态**：v0.2.0 — macOS 和 Windows 适配器已可 CLI 使用；Linux 可编译，
> 默认 XDG 行为；GUI 规划中。

## 为什么要做这个项目

- **通用二进制瘦身（`lipo`）** — Pearcleaner 的标志性功能，其他清理工具几乎都不支持。
  Apple Silicon Mac 运行的是通用二进制（arm64 + x86_64），大多数应用因此带着一份用不上的
  架构副本；`apios lipo` 扫描并瘦身应用，通常能释放近乎一半的二进制体积。与 Apple 的
  `lipo` 工具字节级一致，智能切片选择（arm64e 优先、x86_64h 需 AVX2 门控），原子替换，
  需确认
- **速度**：全量扫描约 0.4~2 秒
- **可移植核心，按 OS 构建**：匹配、扫描和孤儿文件检测均为纯 Rust，不依赖系统 API，
  在非 macOS 目标上类型检查全部通过；平台行为（路径、回收站、Spotlight、包管理器）
  由各平台适配器实现，可独立调整
- **可测试性**：110+ 单元测试覆盖扫描/匹配/孤儿文件/回收站/Lipo/包管理/插件语义，
  外加 Windows 专项测试（注册表枚举、Recycle Bin FFI、winget 解析），在 Windows CI
  上原生运行
- **安全第一**：所有删除操作走回收站（可逆），关键系统路径有规范化攻击防护，每条删除
  命令都需要确认

## 当前状态

| 领域 | 状态 |
|---|---|
| 核心引擎（扫描/匹配/孤儿文件/回收站） | ✅ 已实现 + 单元测试 |
| CLI（`list` / `uninstall` / `orphan` / `clean-orphan` / `dev-clean` / `pkg` / `plugins` / `lipo`） | ✅ macOS 可用，输出已与参考实现校验 |
| 平台适配器 | ✅ macOS：路径/元数据/回收站/Spotlight/Lipo；✅ Windows（v0.2.0）：注册表 + 开始菜单发现、Recycle Bin（系统 API）、taskkill、dev-clean、winget；⬜ Linux：XDG 默认值，计划支持 desktop-file 解析 |
| 验证 | ✅ 测试应用上 9/9 和 17/17 文件集完全一致；✅ Windows 原生测试（注册表/Recycle Bin/winget 解析） |
| UI | ⬜ 计划中 |

## 安装

**预编译二进制**（推荐）——从
[GitHub Releases](https://github.com/Zniece/ApiosCleaner/releases/latest)
下载最新版本，解压后加入 PATH：

```sh
unzip apios-v0.2.0-macos-universal.zip -d ~/bin
# 通用二进制：同时支持 Apple Silicon (arm64) 和 Intel (x86_64)
```

> ⚠️ macOS Gatekeeper：二进制为 ad-hoc 签名；首次从下载的 zip 中运行时，
> 可能需要右键 → **打开**，或执行 `xattr -d com.apple.quarantine ~/bin/apios`。

或者从源码构建（macOS）：

```sh
git clone git@github.com:Zniece/ApiosCleaner.git
cd ApiosCleaner
cargo build --release
# 二进制文件位于 ./target/release/apios
```

也可以用 cargo 直接安装 CLI：

```sh
cargo install --git git@github.com:Zniece/ApiosCleaner.git --locked
```

**Windows**：从最新的
[Release](https://github.com/Zniece/ApiosCleaner/releases/latest)
下载 `apios-windows-x86_64`（zip 包含 `apios.exe`）——或从
[CI 运行](https://github.com/Zniece/ApiosCleaner/actions) → Artifacts
获取最新构建——或在 Windows 本机上执行 `cargo install`。无需管理员权限；
Recycle Bin API 按用户运行。

> ⚠️ 删除类命令会将文件移入 Trash（macOS/Linux）或回收站（Windows），并请求
> 确认，绝不永久删除。请勿以 `sudo` 运行——关键路径保护假设使用非 root 用户。

## 用法

以下示例假设 `apios` 已在 PATH 中（见上文安装说明）。`<app>` 参数接受完整路径、
应用名称（自动在默认应用目录中查找）或 `.` 表示当前目录。删除类命令会请求确认
（`y/N`，默认为否）；传 `-y` 跳过确认（用于脚本或 GUI/自动化集成）。

```sh
# 列出应用的所有关联文件（只读）
apios list /Applications/SomeApp.app
apios list SomeApp

# 卸载应用：应用本体 + 全部关联文件 → 回收站
apios uninstall SomeApp

# 列出已卸载应用留下的孤儿文件（只读）
apios orphan

# 删除所有孤儿文件
apios clean-orphan

# 查看开发环境缓存大小（只读）
apios dev-clean

# 清理某个开发环境（如 Cargo、Gradle、Xcode），或 "all"
apios dev-clean cargo

# 包管理器（macOS 上为 Homebrew）：列出已安装的包
apios pkg brew list

# 卸载单个包（自动检测类型；先提示依赖信息；
# --zap 额外清除 cask 用户配置，需再次确认）
apios pkg brew uninstall git
apios pkg brew uninstall --zap firefox

# 清理孤儿依赖（先展示 dry-run 再确认）
apios pkg brew autoremove

# 列出插件目录（音频组件、偏好面板、QuickLook 生成器、
# 屏幕保护程序……共 18 个类别，只读）
apios plugins

# 查看某个类别（不区分大小写）
apios plugins audio

# 删除插件，移入回收站（需确认；可指定类别限定范围，
# 如 `apios plugins --clean audio`）
apios plugins --clean

# Lipo（仅 macOS）：扫描所有应用的通用二进制，显示可释放空间
# （只读）；也可扫描单个应用
apios lipo
apios lipo Firefox

# 将应用瘦身为当前架构（不可逆；需确认）。
# 代码签名默认会失效；传 --sign 可对瘦身后的二进制进行
# ad-hoc 重签（codesign -s -）
apios lipo thin Firefox
apios lipo thin --sign Firefox
```

### Windows 注意点

`<app>` 参数接受注册表 `DisplayName`（如 `7-Zip`）、安装路径或 `.lnk` 路径
——Windows 上没有 `bundle_identifier`，匹配会回退到显示名/路径关键字。

```sh
# list / uninstall / orphan 与 macOS 用法一致（删除走回收站）

# 包管理器：winget（无 formula/cask 之分）
apios pkg winget list
apios pkg winget uninstall 7-Zip
```

`apios lipo` 仅 macOS 可用；Windows 上该命令不会被编译。`pkg` 和 `plugins`
会报告无管理器/无类别（没有可枚举的内容）。启动时控制台会自动切换到 UTF-8
（代码页 65001），确保中文输出在 cmd/PowerShell 中正确显示。

## 文档

- [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md) — 可移植核心 + 适配器模式、模块图和
  安全模型（[中文版](docs/zh-CN/ARCHITECTURE.zh-CN.md)）
- [CHANGELOG.md](CHANGELOG.md) — 发布历史

## 许可证

初始代码库源自 alienator88 的 [Pearcleaner](https://github.com/alienator88/Pearcleaner)，
基于 **Apache License 2.0 with the Commons Clause License Condition v1.0** 许可。
本项目以**相同许可证**分发，包括禁止销售本软件的限制。完整许可文本（Commons Clause 条款和
Apache License 2.0）请见 [LICENSE](LICENSE)。
