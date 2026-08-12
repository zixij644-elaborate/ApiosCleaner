# ApiosCleaner 项目进度与路线图

> 最后更新：2026-08-12 ｜ 配套文档：Pearcleaner-Rust重写-分析与计划.md（**仓库外**，位于本工作区根目录；技术细节与对照验证）

## 项目定位（一句话）

**全平台应用清理器**：以 Rust 为核心引擎（平台无关），不同操作系统分发各自的适配层构建（macOS + Windows 就绪 v0.2.0，Linux 规划中）。项目源于 Pearcleaner（macOS 清理工具）的重写，正在独立演进。

## 总进度一览

| 阶段 | 内容 | 状态 |
|---|---|---|
| PoC 核心引擎 | 扫描 / 匹配 / 孤儿 / 回收站（38/38 测试） | ✅ 完成 |
| CLI | 命令参数式（list / uninstall / orphan / clean-orphan），交互确认 + `-y` 非交互 | ✅ 完成（2026-08-12 重设计） |
| 平台适配层 | trait 接口 + 条件编译（macOS 实现 + 其他平台基础版） | ✅ 完成 |
| 许可与发布 | Apache 2.0 + Commons Clause；GitHub 仓库已推送 | ✅ 完成 |
| macOS 功能补齐 | mdfind ✅ / killApp ✅ / 开发环境清理 ✅ / 包管理器 ✅ / Lipo ✅ / 插件 / Package / Daemon / 删除历史 / 文件搜索 / TCC / UI | ⬜ 按功能全集对照表逐项推进 |
| 全平台扩展 | Windows 适配器 ✅（M1-M7，v0.2.0）+ CI 三平台矩阵 ✅；Linux 适配器 + Windows 本机走查验收 | ⬜ 进行中（Linux 待做） |
| 工程化 | CI 门禁 ✅ / 发布流程（签名、Homebrew、SLSA） | ⬜ 规划中（发布流程） |
| 重构路线 | 净室式重构 / 数据表外部化 / 评估换 MIT | ⬜ 远期 |

## 开发准则（2026-08-12 用户明确）

**不逐字复刻源代码**：允许逻辑优化、bug 修复、利用 Rust 语言特性。**只要功能相近、性能不落后于原版**。对照验证保留（作为功能相近的证明），内部实现自由优化；涉及已验证对齐的 CLI 输出/行为时保持兼容。

## 多平台协同（2026-08-12 用户决策）

项目形态决定多平台开发测试。协同原则：**平台分工确定"在哪测"，仓库是 agent 状态的唯一跨机器载体**（Claude Code 会话与 memory 均机器本地，跨机器靠仓库内事实重建上下文 —— 本路线图 + 提交历史 + CLAUDE.md（待建））。

| 平台 | 角色 | 验证内容 |
|---|---|---|
| macOS（主力机） | 日常开发、全量 110 测试 | lipo 等 macOS 专属 |
| Windows 本机 | 真实环境验证（CI 测不到的部分） | 真实注册表/回收站/winget、cmd + PowerShell 双 shell 中文输出 |
| Kali（Parallels Desktop，arm64） | Linux 适配器开发与实测 | XDG 路径行为、`.desktop` 解析（将来）、dev-clean 真实缓存布局 |
| GitHub Actions CI | 机器无关门禁 | macOS 质量 + Linux/Windows cross-check + Windows 原生测试 |

agent 协同模式（**混合**）：日常开发与 Linux 验证都在 macOS 会话（Kali 经 ssh 驱动：VM 内 git clone + 本地 cargo 构建，不用共享文件夹编译）；Windows 本机开原生会话处理 Windows 专属问题，或人肉走查、问题回报 macOS 会话修。跨机并行写代码会撞 git —— **约定同一时间只有一个 agent 写代码**。
待办：repo 级 CLAUDE.md（架构原则 / 提交规则——无 AI 署名 / 验证门禁 / 平台分工 / 里程碑状态），让 Windows 与 Kali 的 agent 启动即加载。

## 路线图详情（每个功能简单解释）

### 阶段一：PoC 核心（✅ 已完成）

| 功能 | 做什么 | 为什么需要 |
|---|---|---|
| 匹配引擎 matcher | 按应用名/包名/团队标识等启发式规则判断"哪些文件属于这个应用"（三级灵敏度：严格/增强/深度） | 清理器的核心判断逻辑 |
| 相关文件搜索 search | 按 macOS 路径表（约 110 条硬编码路径）逐目录扫描应用相关文件 | 找到应用本体之外的残留（缓存、偏好设置、容器等） |
| 孤儿搜索 orphan | 反向思路：扫描"应用数据目录"，剔除被现存应用引用的，剩下的就是孤儿 | 清理已卸载应用留下的垃圾 |
| 回收站 trash | 删除时先进回收站归档目录（可撤销），不做不可逆删除 | 安全底线，误删可恢复 |
| CLI | 命令参数式（2026-08-12 重新设计）：`apios list <app>` / `apios uninstall <app>` / `apios orphan` / `apios clean-orphan`；`<app>` 支持完整路径、应用名（自动查找）、`.`（当前目录）；删除类命令列出影响范围后交互确认（y/N 默认拒绝），`-y` 跳过确认供脚本与 GUI 对接 | 无 GUI 也能用，是对照验证的载体；参数式 + `-y` 预留 GUI/自动化对接通道（二进制名 `apios`） |
| 平台适配层 platform | 三个 trait（目录布局 / 应用元数据 / 回收站）+ `cfg(target_os)` 编译期分发；macOS 实现用 codesign 等原生机制，其他平台有 XDG 基础版 | 全平台目标的骨架：核心引擎零 OS API，每平台各一个实现文件 |
| 对照验证 | Rust CLI vs 原版 CLI 输出 diff | 证明移植正确（Pearcleaner 8/8、Edge 16/16 一致） |
| 许可 + 发布 | Apache 2.0 + Commons Clause（与上游一致），GitHub 私有仓库 `Zniece/ApiosCleaner` | 合规（Rust 移植=衍生作品，不可用 MIT；Commons Clause 禁商用） |

### 阶段二：macOS 功能补齐（⬜ 下一步：见功能全集对照表）

| 功能 | 做什么 | 简单解释 |
|---|---|---|
| ① mdfind Spotlight 补充 | 用 Spotlight 索引做一次全盘补充查询（5s 超时） | 路径表逐目录扫描有盲区（路径表外的深层残留），Spotlight 索引能直接搜到。**✅ 已完成**（2026-08-12）：新增 `SpotlightIndex` 适配层接口，macOS 用 `mdfind -onlyin ~` + NSPredicate 谓词（strict 精确 / enhanced 包含 / deep 待 GUI 时实现），strict 后过滤 + 500 上限 + 5s 超时；输出差距全部消除 |
| ② killApp | 卸载前先终止运行中的应用（killall） | 运行中的应用文件可能被占用或删除不完整。**✅ 已完成**（2026-08-12）：新增 `ProcessControl` 适配层接口，macOS 按 CFBundleExecutable（进程名）pgrep 计数 → killall SIGTERM 优雅终止 → 等 200ms 再删除；确认之后、删除之前执行，实测 "Terminated 1 running process(es)" |
| ③ 开发环境清理 | 清理 Xcode 等开发工具产生的缓存（原版 DevEnvironmentCleaner） | 常见垃圾来源，用户高频需求。**✅ 已完成**（2026-08-12，含平台化+收紧）：`apios dev-clean` —— 无参数列出开发环境占用（rayon 并行，本机 ~1s）；`apios dev-clean <环境名>` 大小写不敏感匹配（支持 `all`），列出各目录大小 → 确认 → 目录**内容**进回收站（保留目录本身，原版 deleteFolderContents 语义）。路径表移入 `DevEnvPaths` 适配层 trait（核心只留纯逻辑）：**macOS 24 个环境（收紧表）+ Linux 18 个环境（XDG 子集，测试保证无 `~/Library` 路径）**。收紧原则：只列**可再生缓存**，移除工具本体（`~/.cargo` 根、`~/.nvm`、conda 发行版）、配置（Application Support 根）、用户数据（Xcode Archives、模拟器设备）；移除 Conda（macOS）与 Ruby Gems；Nix 仅 `~/.cache/nix/`（`/nix/store` 系统级包存储，CLI 一键清空不可接受）；嵌套路径去重（父目录条目覆盖子目录，避免重复统计/删除，本机实测 Cargo 192.1MB 无重复） |
| ④ 包管理器（pkg） | 卸载包本体及其依赖（Homebrew 为首个实现） | 大量 macOS 软件经 Homebrew 安装。**✅ 已完成**（2026-08-12，架构决策：用户指出 Homebrew 属"包管理器"范畴，区别于 dev-clean 的缓存清理，且同一平台可有多个包管理器） |
| ⑤ 插件清理 | 清理扩展（Plug-ins）：音频/偏好面板/QuickLook/屏保/浏览器插件等 12 类目录 + 勾选删除 | 应用卸载后这类残留最容易被忽视。**✅ 已完成**（2026-08-12）：`apios plugins` —— 无参数列出全部插件（18 分类，含每类合计与总计）；`apios plugins <类别>` 大小写不敏感过滤；`apios plugins --clean [类别]` 确认后移入回收站。分类路径表移入 `PluginPaths` 适配层 trait（macOS 18 类全表复用原版 Locations.plugins.subcategories，其他平台空）；`should_include` 过滤规则为纯逻辑（后缀/目录语义，大小写不敏感）；扫描列目录一层（原版 contentsOfDirectory 语义），目录大小 rayon 并行统计。按重写准则修原版 2 处：目录条目大小实时统计（原版 GUI 懒加载）+ 统一隐藏文件过滤 |
| ⑥ UI 壳 | 图形界面（候选：SwiftUI FFI 或 Tauri） | 面向普通用户的产品形态 |
| ⑦ Sentinel/PKG/Helper/Updater 决策 | 评估原版外围功能（进程监控 / PKG 安装器 / 特权 helper / 自动更新）做不做 | 决策点，非必须 |
| ⑧ Lipo 瘦身 | fat 二进制瘦身（扫描全部应用显示可省空间，勾选执行） | **✅ CLI 完成**（2026-08-12）：`apios lipo [app]` 只读扫描 / `apios lipo thin <app> [--sign]` 交互确认瘦身。核心模块 `lipo.rs` 纯 std 跨平台（21 单测），按重写准则修 6 处原版缺陷：编译宏硬编码 → 运行时 cfg!(target_arch)、忽略 cpusubtype → 同 cputype 取最高（arm64e 优先）、无 fat64 → 两种格式都解析、全文件读入 → 只 seek 读切片、直接覆盖写回 → 临时文件 + 原子 rename、无边界校验 → nfat 上限/越界/截断统一校验。实测：fixture 瘦身后 md5 == `lipo -thin arm64`（ca4156b1...）、`--sign` 后 codesign -dv 有签名、非交互 abort、真 app 扫描 Parallels 317 二进制可省 560.9MB + Edge 9 个 28.6MB。GUI 接入待 UI 壳 |
| ⑨ PackageView（pkgutil） | 已安装 .pkg 包列表 + BOM 文件浏览 + 安装日期（pkgutil 命令） | PKG 安装器的包管理页（注意与 Homebrew 区分：不同包形态） |
| ⑩ DaemonView（launchctl） | 守护进程管理：agent/daemon/service 列表，loaded/unloaded/running 三态 + 启停 | launchctl 管理页（登录项功能的完整形态） |
| ⑪ 删除历史 | 删除历史记录 + 撤销（进回收站可恢复，历史 UI 追踪） | 原版 DeleteHistoryView + UndoManager |
| ⑫ 文件搜索 | 全盘文件搜索（关键词/过滤/重命名/批量删除） | 原版 FileSearchView（FileSearchLogic） |
| ⑬ TCC 权限查看器 | 查看/删除各应用的 TCC 权限记录 | 原版 TCCPermissionViewer |

### 阶段三：全平台扩展（🟡 进行中：Windows 适配器已落地 v0.2.0）

| 功能 | 做什么 | 简单解释 |
|---|---|---|
| Linux 适配器 | desktop 文件元数据、XDG 回收站规范、应用目录布局细化（flatpak/snap 等） | 在"能编译"的基础上做成真正可用的 Linux 版 |
| Windows 适配器 | 注册表卸载项、开始菜单、回收站 API | ✅ **已完成（v0.2.0，2026-08-12）**：应用发现（注册表卸载项 + 开始菜单 .lnk）、系统回收站删除（SHFileOperationW FFI）、taskkill 终止进程、dev-clean Windows 表（13 环境）、winget 包管理、UTF-8 控制台；**待 Windows 本机实测走查** |
| CI 三平台矩阵 | macOS/Linux/Windows 构建 + 测试 | ✅ **已完成（2026-08-12）**：quality（macos fmt/clippy/test）+ cross-check（linux/windows target 类型检查）+ windows 原生测试 job；push 时上传 Windows release 二进制 artifact |
| **旗舰：WSL 磁盘瘦身**（Windows） | vhdx 虚拟磁盘只增不减（删文件不释放空间）→ 扫描估算可回收空间 + 一键 `wsl --shrink`（或磁盘压缩 API） | 与 lipo 同构的"平台独有结构 × 真实痛点"：WSL 是 Windows 生态独有，其他平台无对等物，且多数用户不知道 vhdx 需要手动压缩。**Windows 首发（v0.2.0）后启动** |
| **旗舰：生态包清理**（Linux） | Flatpak 未用 runtime / Snap 旧版本 / 孤儿包（apt autoremove 等）清理，复用 `pkg` trait 扩展 | Linux 适配层的旗舰功能：真实堆积场景，成本低（现有架构直接扩展） |

Windows 移植里程碑（2026-08-12）：

| 里程碑 | 内容 | 提交 |
|---|---|---|
| M1 安全加固 | `normalize_absolute` 保留盘符 Prefix（修 `C:\Windows`→`/Windows` 绕过 critical 表高危）+ Windows critical 表（SystemRoot/ProgramFiles/ProgramData/盘符根格式检测）+ 归档名净化 + HOME→adapter | `0b2f013` |
| M2 骨架 | `WindowsAdapter`（SystemPaths/AppMetadata/PluginPaths/PackageManagers 占位）+ mod.rs 接线 | `2cc8ddd` |
| M3 应用发现 | `AppDiscovery` trait + win_registry.rs（Reg*W FFI + 纯解析 + HKCU 集成测试）+ CLI 改造（orphan/list/uninstall 走发现结果） | `6b45c1b` |
| M4 删除链路 | POSIX 归档提炼为共享 `move_to_trash_dir`（macOS 行为逐字节不变）+ win_trash.rs（SHFileOperationW FO_DELETE+FOF_ALLOWUNDO→回收站）+ tasklist/taskkill | `4d55eef` |
| M5 dev-clean + winget | Windows dev_envs 表（13 环境）+ winget.rs（list/uninstall 纯文本解析）+ SetConsoleOutputCP(65001) | `c149291` |
| M6 产物 + 文档 | CI Windows artifact + README/ARCHITECTURE/CHANGELOG 0.2.0 + 本路线图 | 本次提交 |

### 阶段四：工程化（⬜ 规划中，视发布形态）

| 功能 | 做什么 | 简单解释 |
|---|---|---|
| CI 门禁 | cargo fmt / clippy / test 自动检查 | 防回归的最低成本防线。**✅ 已完成**（2026-08-12）：GitHub Actions 两个 job——quality（macos：fmt --check / clippy -D warnings / test）+ cross-check（ubuntu：workspace 在 x86_64-unknown-linux-gnu 上必须能编译，防 macOS 代码漏进核心构建） |
| 发布流程 | release 构建、签名、Homebrew formula、SLSA provenance | 发布形态（分发渠道、签名方式）确定后再定标准，避免返工 |

### 阶段五：重构路线（⬜ 远期，你声明的方向）

| 事项 | 做什么 | 简单解释 |
|---|---|---|
| 净室式重构 | 重构阶段不再参考原版 Swift 代码 | 拉大与上游的实质差异，为换许可证留空间 |
| 数据表外部化 | 约 110 条硬编码路径表改为配置数据 | 路径表随系统版本演化的频率高，外部化更好维护，也便于各平台共享/覆盖。已登记：matcher.rs 的 Steam 规则含 macOS 形态路径字面量（`/Library/Application Support/Steam/...`，Windows/Linux 上恒不命中，2026-08-12 M7 审查确认），届时一并外部化 |
| 评估换 MIT | 重构完成后重新评估换 MIT 协议 | 重构会逐渐改变"衍生作品"的程度，届时重新评估法律状态 |

## 功能全集对照表（2026-08-12 盘点：产品功能基线 = 原版 GUI 页全集）

> 背景：Lipo 遗漏暴露了"路线图偏 CLI 视角"的疏漏。**产品第一步 = 实现或重构原版 GUI 页的所有功能**。核心引擎层（Locations/Conditions/AppInfoFetch/AppPathsFetch/ReversePathsFetch/原版 CLI）已重写 ✅。

| 原版页面 | 功能 | 核心逻辑 | 平台 | 我们的状态 |
|---|---|---|---|---|
| AppsView | 已装应用列表（网格/列表、搜索、排序） | AppInfoFetch | 全平台 | 🔲 GUI 阶段 |
| FilesView | 应用详情：相关文件**按类别分组**、逐项勾选删除、撤销 | AppPathsFetch | 全平台 | ⚠️ 部分（uninstall 全量进回收站，无逐项/类别/撤销） |
| ZombieView | 孤儿清理 | ReversePathsFetch | 全平台 | ✅ `orphan` / `clean-orphan` |
| DevelopmentView | 开发环境清理 | dev_envs_table | 全平台 | ✅ `dev-clean` |
| HomebrewView | 卸载 + 搜索安装 / tap 管理 / 日志 / 自动更新 | Homebrew* | macOS 专属* | ⚠️ 部分（`pkg` 卸载+autoremove ✅；搜索安装、tap、日志、自动更新 🔲） |
| PluginsView | 插件分类扫描 + 勾选删除 | PluginCategory | macOS 专属* | ✅（`apios plugins` / `--clean`；阶段二⑤） |
| LipoView | fat 二进制瘦身 | Lipo.swift | macOS 专属 | ✅ `lipo` / `lipo thin`（GUI 勾选交互待 UI 壳） |
| PackageView | 已安装 .pkg 包列表 + BOM 文件浏览 + 安装日期 | pkgutil | macOS 专属 | 🔲 阶段二⑨ |
| DaemonView | launchctl 守护进程管理（agent/daemon/service 三态 + 启停） | launchctl | macOS 专属 | 🔲 阶段二⑩ |
| DeleteHistoryView | 删除历史 + 撤销 | UndoManager | 全平台 | 🔲 阶段二⑪ |
| FileSearchView | 全盘文件搜索（关键词/过滤/重命名/批量删除） | FileSearchLogic | 全平台 | 🔲 阶段二⑫ |
| AppsUpdaterView | Sparkle / App Store / Homebrew 更新器 | AppsUpdater | 全平台* | 🔲 阶段二⑦ 决策点 |
| Settings（6 页） | General/Interface/Folders/Helper/Update/About | — | 全平台 | 🔲 GUI 阶段 |
| Components | TCC 权限查看器、全局控制台、权限申请 | TCC | macOS 专属 | 🔲 阶段二⑬ |
| MainWindow / DeepLink | 窗口壳、`pearcleaner://` 协议 | AppState | 全平台 | 🔲 GUI 阶段 |

> \* 带 `*` 的"macOS 专属"= 机制是 macOS 的，但架构上走平台适配层 trait（`PluginPaths` / `PackageManager` 已有先例），**同一功能位可为其他平台接自家实现**（Linux: systemd 服务、apt/snap/flatpak 包；Windows: 服务/卸载项）——只有 Lipo（fat Mach-O 格式）、pkgutil（PKG 收据）、launchctl、TCC 是格式/系统级专属，其他平台无等价物，二进制里直接 cfg 门控不包含（同 lipo 模式）。
> 因此：**⑨⑩⑬ 在非 macOS 平台不做**；⑪⑫（删除历史、文件搜索）是全平台通用功能，对 Linux/Windows 也是差异化价值点。

**待决策项**（阶段二⑦）：特权 helper（root 操作：系统级文件/ Lipo 瘦身 / PKG 安装）、进程监控 Sentinel、自动更新（Sparkle）。这些影响发布形态（签名/公证），GUI 阶段一并定。

## 已知问题与待办（代码审查 2026-08-12）

### 已修复（commit `023899e`）

| 项 | 说明 |
|---|---|
| 孤儿容器分支死代码 | 容器判断误用 pearFormat 后路径（斜杠被剥除），`/Containers/` 恒不匹配 → 改回原始路径判断 |
| 孤儿条件表每路径重建 | `is_excluded_by_conditions` 对每个扫描路径重建整张条件表（含磁盘 exists 检查）→ 缓存到 `new()` |
| 孤儿扫描并行化 | `get_sorted_apps` 改 rayon 并行（每 app 解析独立） |
| team_identifier codesign 调用 | 仅 Deep 敏感度使用（GUI 功能），孤儿/CLI 不需要 → 省略（GUI 阶段启用 `get_team_identifier`） |
| UUID 正则重复编译 | orphan/app_info 两处 LazyLock 静态化 |
| trash 归档名消毒 | 应用名含 `/` `:` 时替换为 `_`，防破坏归档目录结构 |

效果：孤儿扫描 1.95s → **1.45s**（本机）；38/38 测试、clippy 0、Edge list 17/17 无回归。

### 待办观察项（按优先级）

| 优先级 | 项 | 说明 |
|---|---|---|
| 中 | 孤儿扫描 walk 深度仅 1 | `~/Applications` 深层 `.app`（如 Utilities 子目录）漏检 → 孤儿关联可能误判。孤儿里程碑时补完整遍历 |
| 低 | `should_skip_item` 每路径 `to_string_lossy` | 微开销，收益小 |
| 低 | `Locations::new` 每次跑 bash（darwin_ct） | 每命令一次，可接受 |
| 低 | 跨设备删除失败 | 外置盘 → `fs::rename` EXDEV；将来换 `/bin/mv` 链或平台回收站实现 |
| 备忘 | 参考 CLI 自污染 | 原版 CLI 每次运行写自身偏好 → 对照必须同刻进行（分析报告 §7.1） |
| 备忘 | 孤儿 CLI 对照死锁 | 原版确定性死锁（上游 bug，分析报告 §7.7），已代码级验证替代 |
| 备忘 | mdfind deep 谓词 | 多元数据组合谓词属 GUI 功能，实现时补（macos.rs TODO） |
| 备忘 | Linux 适配 TODO | desktop 文件元数据、XDG trash 细化（fallback.rs TODO） |
| 备忘 | team_identifier 待 GUI 启用 | Deep 敏感度匹配需要时调用 `get_team_identifier` |

## 关键数据（证明"能跑"）

| 指标 | 值 |
|---|---|
| 单元测试 | macOS 106/106 + Linux 64/64 + Windows 82/82（lib）+ 4/4（bin），三平台 CI 全绿 |
| 对照验证（Pearcleaner.app） | **9/9 完全一致**，耗时 0.43s vs 原版 ~10 分钟 |
| 对照验证（Microsoft Edge.app） | **17/17 完全一致**，耗时 0.4s |
| 孤儿输出（本机实测） | 24 项，2 秒（参考版因上游 bug 无法对照，已代码级验证） |
| 跨平台类型检查 | x86_64-unknown-linux-gnu + x86_64-pc-windows-gnu 双 target 通过，0 警告（macOS 专属代码随 cfg 不进入其他平台构建） |
| Lipo 跨平台实测 | 纯字节解析提取 arm64 slice，md5 与 `lipo -thin` 完全一致 |

## 下一步（优先级排序，2026-08-12 功能全集盘点后重排）

1. **Windows 本机实测走查（首发验收）** —— 用户在 Windows 本机下载 CI artifact，全命令走查：`list`（路径/DisplayName/.lnk 三种形态，含中文名）、`uninstall`（回收站出现归档）、`orphan` / `clean-orphan`、`dev-clean`（Cargo/Npm 等）、`pkg winget list/uninstall`；cmd 与 PowerShell 双 shell 中文输出验证
2. **⑫ 文件搜索 / ⑪ 删除历史** —— 全平台通用核心功能（Windows 首发后与 macOS 同步推进）；⑪ Windows 侧恢复需先决策（SHFileOperationW 不返回回收站内路径）
3. **PackageView（pkgutil）** / **DaemonView（launchctl）** —— macOS 专属，各有独立核心逻辑，可 CLI 化（与其他平台无等价物，二进制 cfg 门控）
4. **旗舰功能**：WSL 磁盘瘦身（Windows v0.2.0 后）、生态包清理（Linux 适配器后）
5. 之后：TCC / UI 壳（GUI 阶段统一接入）

> 2026-08-12 已完成记录：
> - **Windows 移植（v0.2.0，M1-M6 六里程碑全数落地，110 测试全绿 + 三平台 CI 通过）**：用户战略转向"尽早实现 Windows 版本 + 核心功能跨平台同步推进"，确认 GitHub Actions CI / 最小可用首发（list/uninstall/orphan/clean-orphan + 回收站 + dev-clean + winget）/ 手写 FFI（零第三方依赖）。关键决策：**应用发现走新 trait `AppDiscovery`**（注册表 HKLM+HKCU 卸载项 DisplayIcon>InstallLocation + 开始菜单 .lnk；macOS/Fallback 委托 scan.rs walk 零行为变化；bundle id 置空 → identifiers 门控自动关闭 bundle-id 匹配族，name needle 继续生效）；**删除走 `Trash::move_to_trash` 动作级方法**（POSIX 归档提炼为共享 `move_to_trash_dir`，macOS 逐字节不变；Windows 覆写 SHFileOperationW FO_DELETE+FOF_ALLOWUNDO → 系统回收站，可恢复）；**Windows uninstall 不跑 UninstallString**（只移相关文件进回收站，可逆可预期；注册表项删除/Appx 排 v1 外）；**winget 全包归 Formula**（无 formula/cask 概念，detect_kind 天然兼容）；**dev-clean Windows 表收紧原则同 macOS**（13 环境只列可再生缓存）；**控制台 SetConsoleOutputCP(65001)** 防旧代码页中文乱码。安全高危修复：`normalize_absolute` 丢弃 `Component::Prefix` 会把 `C:\Windows` 归一化成 `/Windows` 绕过 POSIX critical 表 → 保留 Prefix + Windows 环境变量驱动 critical 表 + 盘符根格式检测（`X:\` 一律拦截，不硬编码 C:）。Windows 本机实测待验收（下一步第 1 项）。
> - **CLI 重设计**：按常用 CLI 惯例改为命令参数式，`<app>` 三种形式（路径 / 应用名 / `.`），uninstall 默认删除全部相关文件，删除前交互确认（y/N 默认拒绝），`-y` 跳过确认供脚本与 GUI 对接；二进制与 crate 更名为 `apios`。对照原版输出仍一致（9/9、17/17）。
> - **CI 门禁**：GitHub Actions（quality + cross-check 两 job），push 即跑，后续提交均有绿勾验证。
> - **killApp**：uninstall 确认后、删除前终止运行实例（实测 1 进程 → SIGTERM → 200ms 后删除成功）。
> - **开发环境清理**：`apios dev-clean` 26 个环境（本机实测 VS Code 3.6GB、Gradle 1.9GB、Cargo 202.7MB）。随后**平台化 + 收紧**（用户决策：路径表移入 `DevEnvPaths` 适配层 trait，只清理可再生缓存）：macOS 收紧为 24 个环境、新增 Linux 18 个环境子集，移除工具本体/配置/用户数据（Cargo 仅列 `~/.cargo/git/` + `registry/`，实测 192.1MB 无重复统计）。
> - 决策（2026-08-12）：CLI 不做交互式 TUI——交互体验归 GUI 路线（阶段二⑥），CLI 保持参数式 + `-y`；未来若确需选择器再引入 inquire/dialoguer，无需预留。
> - **包管理器范畴（pkg）**：用户架构判断——Homebrew 平台相关且属"包管理器"范畴（与 dev-clean 缓存清理区分），同一平台可有多个包管理器 → 新增 `PackageManager` trait（list/dependents/uninstall/autoremove）+ `PackageManagers` 注册入口（多 PM 选择器），首个实现 macOS Homebrew（platform/homebrew.rs，cfg 门控；fallback 空实现保 Linux 编译）。CLI：`apios pkg <pm> list` / `uninstall <name> [--zap]` / `autoremove`。决策三连：**dependents 警告 + `--ignore-dependencies`**（依赖豁免旗标，非 `--force`；卸载后提示 autoremove 清孤儿依赖，dry-run 先展示）、**cask `--zap` 默认不 zap、显式 `--zap` + 额外确认**（删用户配置不可恢复）、**永不用 `--force`**（多版本/pinned 给提示不自动强删）。dev-clean macOS 表 +"Homebrew"缓存项（~/Library/Caches/Homebrew + Logs，原版 runCleanup 缓存部分；autoremove 归 pkg）。实测：openssl@3 依赖方警告（libngtcp2/node/python@3.14）、真实卸载 android-cli（cask 全流程）→ 同版本重装恢复、未知 PM/未安装/非交互 abort 路径。纯文本解析（brew 输出无列式，管道逐行），不引入 serde。
> - **Lipo 瘦身**：`apios lipo [app]`（只读扫描） / `apios lipo thin <app> [--sign]`（交互确认，子命令式决策）。核心 `apios-core/src/lipo.rs` 纯 std 跨平台（Linux cross-check 通过）。**按重写准则修 6 处原版缺陷**（不照搬）：`#if arch` 编译宏 → 运行时 `cfg!(target_arch)`；只匹配 cputype → 同 cputype 取最高 subtype（arm64e 优先 arm64）；无 fat64 → 32/64 位都解析；全文件读入 → 只 seek 读目标切片；直接覆盖写回 → 临时文件 + 原子 rename（保留权限位，中断不损坏）；无边界校验 → nfat 上限 32 / 切片越界 / 表截断统一校验。决策：默认不重签（对齐原版，签名失效警告提示）、`--sign` 时 ad-hoc 重签（codesign -s -，失败警告不中断）。实测全链路：clang 合成 universal fixture → thin 后 `file` 单架构 arm64 + **md5 == `lipo -thin arm64`**（ca4156b1a487a45eb6f839ec04f44eb6）；`--sign` 后 codesign -dv 签名存在；`< /dev/null` 非交互 Aborted + exit 0；瘦身后重扫描无 fat；真 app 扫描 Parallels Desktop 317 二进制可省 560.9MB、Microsoft Edge 9 个 28.6MB；长路径截断（64 字符上限）。
> - **全库审查修复**（IDE 红色标记诊断 → 三 agent 审查 + 逐项修复；新增 9 测试，105/105）：
>   - **3 个高危误删链路**（优先级最高，先修）：① `search.rs` Wrapper 子串匹配整个路径 → 祖先目录名含 "Wrapper"（如 /Users/wrapperx/…）会误上跳两级、把整个目录收进删除列表 → 改为仅紧邻父目录含 "Wrapper"（真实 wrapped 结构 `…/Wrapper/Foo.app`）才上跳；② `trash.rs` validate_path 按原始字符串精确匹配 → `..`/`//`/尾部斜杠可绕过 critical 表，且 /Users、/Users/Shared、{home}/Applications 不在表中 → 词法归一化（折叠 `..`/重复分隔符/去尾斜杠，相对路径直接拦截）+ critical 表补齐；③ `scan.rs` 跳过符号链接 .app → brew-cask 应用不在已装集合 → 其数据被孤儿扫描误判为残留并删除 → 纳入符号链接 .app（断链/自环安全）+ canonical 去重。
>   - **正确性**：include_force 子串匹配 bundle id（VS Code Insiders ↔ stable 互相误命中条件表）→ 精确匹配；`identifiers.rs` path_component_name 未 pearFormat → 与 matcher 侧 normalized 名称永不相等（隐藏死代码）→ 构造时格式化；finalize 去重只与前一元素比较 + 相等路径不去重 → 与全部已保留元素比较（等价原版 `Set()` 语义且更严格）；homebrew PATH 空段生成相对 "./brew" → 过滤仅绝对路径；dependents 无法区分"无依赖方"与 brew 真实失败（cask 场景 brew exit 1 但有 stdout）→ 空 stdout + 非零退出且 stderr 无 "No available formula" 才 Err；`model.rs` Sensitivity::parse 注释与行为不一致 → 注释修正（兜底 Deep 与 Default 一致）。
>   - **平台/CLI**：`kill_running_app` 重写为 bundle 路径前缀的 ps 遍历（原 killall 按进程名会误杀 Electron 全家 + pgrep 15 字符截断）；darwin_ct 去 bash -c 包装（直接 /usr/bin/getconf）；mdfind 超时从脱离线程 + recv_timeout（超时后线程与子进程泄漏）→ spawn + try_wait 轮询 + 超时 kill；apps_paths 空条目（cache_dir 获取失败）过滤；`arg_is_path` 移除 `Path::exists()` 劫持（cwd 同名文件会绕过应用名查找）+ `~` 展开 + 带 .app 裸名不存在时回退名称查找；删除类命令空列表 → "Nothing to delete" + exit 0（原 exit 1 误报失败；clean-orphan 空列表提前返回）；卸载后 autoremove 失败降级 warning（原整体 exit 1）；lipo freed 下溢 `len - keep.size` → saturating_sub。
>   - **性能**：orphan `container_name_by_uuid` 每路径 read_dir + plist 解析（O(N²) 文件 I/O）→ new() 预建 UUID→bundle id 映射；O(条目×应用×条件) → 扁平 needle 列表 + 已安装条件集合预计算；lipo 扫描整文件 read_to_end → 只读 8 字节头 + 切片表（越界校验用文件长度元数据）；目录大小统计跟随符号链接（指向祖先 → 递归永不终止）→ lstat 语义。
>   - **lipo 追加**：符号链接目录（指向树内祖先）→ 跳过（无限递归防护）；thin_file 加 sync_all（fsync 后 rename，崩溃不暴露半截文件）；**x86_64h 能力门控** —— 无 AVX2 的 x86_64 CPU 上 x86_64h 切片不可运行（原版/朴素 select 会首选 → 老 Intel Mac 瘦身后直接崩）→ `select_runnable_slice` 运行时检测（arm64e 不门控：所有 Apple Silicon 支持指针认证）。
>   - 验证：fmt + clippy `-D warnings` 零警告 + 105/105 测试 + Linux cross-check 通过 + 实测回归（list Safari 输出不变、裸 .app 名回退、`~` 展开、fixture 卸载到回收站 exit 0、lipo Safari 扫描正常、orphan 输出 27 项无系统目录、1.3s）。
> - **平台旗舰功能战略（2026-08-12）**：lipo 价值公式 = **平台独有结构 × 真实痛点**。背景：macOS 26 Tahoe 是最后支持 Intel 的版本，macOS 27 起纯 Apple Silicon，Rosetta 2 在 macOS 28（2027）移除大部分功能 —— lipo 的瘦身对象（universal 二进制中的死重切片）仍将在 2027-2029 达到峰值（Rosetta 退役后 x86_64 切片零风险可删），之后随生态转向纯 arm64 逐步衰减（约 5 年生命周期）；且 lipo 本身是 macOS 专属，Windows/Linux 无对等物。**决策：每个平台一个旗舰功能** —— Windows = **WSL 磁盘瘦身**（vhdx 只增不减、`wsl --shrink` 鲜为人知，生态独有无对等物，与 lipo 同构）；Linux = **生态包清理**（Flatpak 未用 runtime / Snap 旧版本 / 孤儿包，复用 `pkg` trait，成本低）。已评估不做：32 位切片瘦身（i386/ppc/armv7）—— fat 解析已支持、技术可行，但 2026 年存量稀少、切片小、收益与签名失效风险不成比例。实现排期：随阶段三适配器落地。
> - **M7 架构清扫（2026-08-12，531204e）**：审查闭环——critical 表移入 `SystemPaths::critical_paths()`（三平台各持其表，核心 trash.rs 的 cfg(windows)/env 直读清零；Linux 表从 macOS 形换成真实 Linux 系统根，净收紧）；Steam 规则加平台归属注释（macOS 形态字面量，Windows/Linux 恒不命中，登记阶段五数据表外部化）；win_registry 写 API 全部 cfg(test) 门控 + Windows target check 归零警告。验证：110 测试全绿 + clippy 0 警告 + 双 target 0 警告。
> - **多平台协同流程（2026-08-12）**：用户决策——**混合模式**（macOS 主会话日常开发 + Kali 经 ssh 驱动 Linux 验证；Windows 本机开原生会话处理 Windows 专属问题或人肉走查）；路线图移入仓库（docs/，仓库内为唯一工作版本，外部原文件改指引入库版占位，不再双写）；待办 repo 级 CLAUDE.md（Windows/Kali agent 启动即加载架构原则、无 AI 署名提交规则、验证门禁、平台分工）。
> - **Linux 验证链路打通（2026-08-12，M8-M9）**：GitHub main 长期落后本地（M1-M8 未推送，快照证据：scan/trash 直读 `HOME` 的旧代码）→ 全部推送后，Kali VM（PD prlctl exec + 目录内工具链，`HOME=/` 退化环境）与 CI 首跑连续暴露测试平台泄漏：dev_env 测试去 macOS 假设、`default_app_folders` 改 XDG 分流（M8）；trash 三个测试改 critical 表逐项断言、`{home}/Applications` 断言加 `HOME=/` 守卫（M9）。Linux 全量 64+4 全绿 + CLI 冒烟通过（dev-clean 列表、orphan 扫出真实残留）。VM 影响全程锁在 `~/桌面/apios`（1.2G：工具链+缓存+源码），用户确认保留。
> - **CI 闭环（2026-08-12）**：推送后 CI 暴露并修复 2 类 workflow/链接问题——`dtolnay/rust-toolchain` 的 `targets` 输入是**字符串非数组**（workflow 校验层直接拒，三个 run 0s 失败；actionlint 定位）；windows FFI 缺 `#[link]` 声明（Reg*W → advapi32、SHFileOperationW → shell32；cross-check 只 check 不链接，M1 以来从未暴露）。
> - **Windows 原生首跑 6 项修复（2026-08-12，M10）**：windows job 首次真实链接+运行，修复 2 个**生产 bug** + 4 个测试问题——**`widen()` 补 NUL 结尾**（Win32 字符串 API 读越界字节，键名/值名随机写歪；`all_uninstall_entries` 生产路径同受）、**`recycle_batch` 先滤不存在的路径**（事后存在性检查无法区分"批量中已移走"与"本就不存在"，幽灵路径被计入 moved）；winget 排序测试输入改真实列宽（解析器按 ≥2 空格分列，该测试此前从未在任何平台运行）、search 两测试改 `Path::ends_with` 组件匹配（`/` 字面量 vs Windows `\`）、validate_path_windows 放行断言笔误（多余 `!`）。验证：Windows 原生 82+4 全绿 + artifact 产出。
> - **PR #1 合并（2026-08-12）**：外部贡献 `docs: add Chinese README translation`（新增 README.zh-CN.md，169 行）已合并，PR 与合并后 push 两个 CI run 全绿。
