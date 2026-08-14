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
[GitHub Releases](https://github.com/zixij644-elaborate/ApiosCleaner/releases/latest)
下载最新版本，解压后加入 PATH：

```sh
# macOS：按芯片选择 zip —— Apple Silicon (arm64) 或 Intel (x86_64)
unzip apios-macos-arm64.zip -d ~/bin        # 或 apios-macos-x86_64.zip
# Linux：apios-linux-x86_64 / apios-linux-aarch64（静态链接）
# Windows：apios-windows-x86_64（zip 内含 apios.exe）
```

> ⚠️ macOS Gatekeeper：二进制为 ad-hoc 签名；首次从下载的 zip 中运行时，
> 可能需要右键 → **打开**，或执行 `xattr -d com.apple.quarantine ~/bin/apios`。

或者从源码构建（macOS）：

```sh
git clone git@github.com:zixij644-elaborate/ApiosCleaner.git
cd ApiosCleaner
cargo build --release
# 二进制文件位于 ./target/release/apios
```

也可以用 cargo 直接安装 CLI：

```sh
cargo install --git git@github.com:zixij644-elaborate/ApiosCleaner.git --locked
```

**Windows**：从最新的
[Release](https://github.com/zixij644-elaborate/ApiosCleaner/releases/latest)
下载 `apios-windows-x86_64`（zip 包含 `apios.exe`）——或从
[CI 运行](https://github.com/zixij644-elaborate/ApiosCleaner/actions) → Artifacts
获取最新构建——或在 Windows 本机上执行 `cargo install`。无需管理员权限；
Recycle Bin API 按用户运行。

> ⚠️ 删除类命令会将文件移入 Trash（macOS/Linux）或回收站（Windows），并请求
> 确认，绝不永久删除。请勿以 `sudo` 运行——关键路径保护假设使用非 root 用户。

## 用法

以下示例假设 `apios` 已在 PATH 中（见上文安装说明）。`<app>` 参数接受完整路径、
应用名称（自动在默认应用目录中查找）或 `.` 表示当前目录。删除类命令会请求确认
（`y/N`，默认为否）；传 `-y` 跳过确认（用于脚本或 GUI/自动化集成）。

```sh
# 列出发现机制能找到的已装应用（只读）
apios apps

# 列出应用的所有关联文件（只读）
apios list /Applications/SomeApp.app
apios list SomeApp

# 卸载应用：应用本体 + 全部关联文件 → 回收站
apios uninstall SomeApp
apios uninstall SomeApp --except ~/Projects/foo   # 保留该路径（未匹配项会警告）

# 列出已卸载应用留下的孤儿文件（只读；编号与 clean-orphan 完全一致，
# 受保护条目标注 [sudo]）
apios orphan

# 删除孤儿文件——候选按编号列出，输入要删的编号（如 "1,3-5"）、
# 'a' 全删或直接回车取消。只移动选中的文件，故意的残留
# （游戏存档目录等）可以保留。受保护条目（root 所有）标注 [sudo]，
# 可跳过，无需为清理单个孤儿整体 sudo。带 NAME 过滤词只处理路径
# 匹配的孤儿（脚本化选择性删除）。-y 跳过选择直接全删（脚本用）。
apios clean-orphan
apios clean-orphan pear            # 只处理路径含 "pear" 的孤儿

# 查看开发环境缓存大小（只读）
apios dev-clean

# 清理某个开发环境（如 Cargo、Gradle、Xcode），或 "all"
apios dev-clean cargo

# 清理系统临时目录（$TMPDIR + /tmp + /var/tmp / %TEMP%）：
# 只删 7 天（默认）前未触碰的条目——X 会话/systemd/com.apple 服务目录、
# socket 和锁文件受保护；目录只要含新鲜子项即视为新鲜
apios clean-tmp
apios clean-tmp --older-than 1    # 只删 1 天前未触碰的条目

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
# list / uninstall / orphan / clean-orphan 与 macOS 用法一致（删除走回收站；
# clean-orphan 的编号选择交互在 Windows 同样生效）

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

[MIT License](LICENSE)。初始代码库源自 alienator88 的 [Pearcleaner](https://github.com/alienator88/Pearcleaner)
（原基于 Apache 2.0 with Commons Clause）；本项目已获原作者授权，重新以 MIT 许可分发。
