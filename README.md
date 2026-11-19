# Earth — 跨平台极速剪贴板与速览工作台

使用纯 Rust（`eframe / egui` + `clipboard-rs` + `rusqlite`）开发的轻量级跨平台剪贴板管理器，支持 **macOS** 与 **Windows**。兼顾极速键盘流与现代卡片式鼠标交互，内置双栏速览工作台、文本内联编辑与备注、离线二维码、快捷内容加工、智能收藏分组（Earth Collections）、敏感信息脱敏与一键版本更新等能力。

> **关于开发方式**：本项目是 **Vibe Coding** 的产物——从架构选型、Rust 跨平台实现、界面视觉打磨到每一处交互细节，全程通过与 AI 协同对话迭代完成。

## 极致轻量与低资源占用

Earth 从架构层避免任何重型浏览器或脚本运行时开销，专注于**常驻后台无感运行、唤起瞬时响应**：

- **无 Electron / Chromium / WebView 负担**：纯 Rust 编译为原生机器码并调用硬件 GPU 渲染，单架构 Release 可执行文件约 `18 MB`，macOS 通用二进制 `.dmg` 安装包约 `18 MB`。
- **低内存与近零空载 CPU**：列表采用**视口虚拟化**（仅渲染可见区域记录），预览文本、时间格式、缩略图纹理与快捷加工检测按记录缓存；Windows 通过系统剪贴板通知监听，macOS 每 100 毫秒检查一次系统剪贴板变化计数。短时间内连续复制可能只保留最后一次；来源应用取自监听时的前台窗口。
- **高效本地持久化**：基于 `SQLite`（WAL 模式）保存文本索引与去重摘要，图片原图与缩略图分离存储，切换查看大图片或长文本依然流畅不卡顿。

## 核心特性

### 1. 极速键盘流与灵活点击交互
- **多类型记录**：完整支持 **文字**（含纯文本与富文本 HTML/RTF）、**图片**（PNG 存储 + 列表缩略图即时预览）以及 **文件 / 文件夹** 复制列表；所有类型记录统一行高（`34 px`），列表整齐利落。
- **全局热键唤醒**：
  - **macOS**：默认 `⇧⌘C` 唤醒，`⌘1`~`⌘9` 瞬时粘贴，`⌥P` 固定，`⌥⌫` 删除，`⇧⌘P` 切换工作台，`Tab / Shift+Tab` 循环切换顶部筛选胶囊。
  - **Windows**：默认 `Shift+Alt+V` 唤醒，`Alt+1`~`Alt+9` 瞬时粘贴，`Alt+P` 固定，`Alt+Delete` 删除，`Ctrl+Shift+P` 切换工作台，`Tab / Shift+Tab` 循环切换顶部筛选胶囊。
  - 全部快捷键均可在「偏好设置 -> 通用」中实时录制修改。
  - 搜索框或文本编辑器获得焦点时，固定、删除、清除历史与工作台快捷键不执行，以保留文字编辑和输入法行为；这些操作也可通过按钮或右键菜单使用。录制会拒绝常见系统保留组合，全局唤醒键注册失败时保留旧值。
- **可切换「单击复制 / 双击复制」模式**：
  - 支持在「偏好设置 -> 通用 -> 鼠标点击记录」中自由切换：
    - **双击复制（单击展开工作台）**（默认）：单击记录立即展开右侧速览工作台，双击执行复制/粘贴。
    - **单击直接复制**：工作台收起时单击直接复制/粘贴（不触发工作台展开）；通过分栏按钮、快捷键或右键打开工作台后，单击其他记录先切换预览，再次点击已选中记录执行复制，彻底避免交互冲突。
- **修饰键差异化粘贴**：
  - `Enter` / 触发复制：按默认行为粘贴或复制。
  - `⌥+Enter`（Windows `Alt+Enter`）：仅复制到剪贴板。
  - `⇧⌘+Enter`（Windows `Ctrl+Shift+Enter`）：复制并以纯文本格式粘贴。
- **实时搜索与高亮**：支持 **精确 / 模糊（nucleo）/ 正则** 三种匹配模式，支持搜内容、文件路径与记录备注名，支持粗体 / 斜体 / 下划线高亮匹配字符。

### 2. 双栏速览工作台与进阶能力
- **类型筛选胶囊与收藏分组（Earth Collections）**：顶部提供 `全部`、`文本`、`图片`、`文件`、`固定` 以及自定义颜色收藏分组（如「常用指令」「代码片段」），固定项与分组内记录不受历史上限自动淘汰影响。
- **双栏「速览工作台」**：
  - **内联文本编辑**：工作台点击 **「编辑」** 即可直接修改文本内容，支持 **「保存修改」** 或 **「保存并粘贴」**，并自动更新字符/行数/词数统计与类型识别。
  - **记录备注与搜索**：支持为任意记录添加/修改/清除备注别名，列表展示备注徽章且可直接按备注搜索。
  - **离线二维码生成与导出**：一键将文本或网页链接生成本地高清二维码，支持 **「复制二维码图片」** 到剪贴板或 **「另存为图片...」**。
  - **图片与文件增强预览**：图片支持切换 **「适应窗口 / 清晰预览（最高 2048px）」** 与另存为，后台解码避免阻塞界面；文件记录自动校验原路径是否存在，并支持一键在 Finder / 资源管理器中定位；十六进制色值支持 HEX / RGB 一键复制。
- **快捷内容加工（Quick Actions）**：内置丰富离线加工工具，结果支持一键复制或直接粘贴：
  - `JSON 格式化 / 压缩`、`Base64 编解码`、`URL 编解码`、`Unix 时间戳 ↔ 本地/UTC 日期转换`
  - 标识符命名转换：`snake_case`、`camelCase`、`UPPER_CASE`
  - 多行处理与实用工具：`多行去重`、`多行排序`、`字符串反转义`、`去除空白`、`SHA-256 摘要`
- **增强隐私保护**：
  - 自动识别常见敏感凭据（Private Key、JWT Token、API Key、Bearer Token 等）并在列表中默认 `••••` 遮罩保护（需点击「显示明文」方可查看与生成二维码）。
  - 支持应用黑/白名单忽略、系统剪贴板类型忽略（如 `ConcealedType` / `TransientType` / `ExcludeClipboardContentFromMonitorProcessing`）、正则表达式忽略，以及「暂停监听」和「仅忽略下一次复制」。
- **备份闭环与自动检查更新**：
  - **JSON 导出与导入**：在「偏好设置 -> 存储」支持导出 JSON 备份，以及导入备份文件并按内容哈希自动去重合并。
  - **自动检查与可信更新**：支持启动时自动检查或设置页手动点击「检查更新」。内置可信发布者身份的构建会校验同一 Release 的 `SHA256SUMS.txt`、安装包签名与发布者身份；macOS 还会验证 Apple 公证。校验通过后可点击 **「重启完成更新」**，macOS 替换时保留原应用备份和安装日志。缺少可信身份、校验文件或受支持安装器时保留 **「前往下载」** 手动更新入口。预发布版本按完整 SemVer 顺序检查更新，稳定版默认只接收稳定版。

## 构建与运行

确保已安装 Rust 工具链（`1.95+` / Edition `2024`，当前验证版本为 `1.100.0`）：

```bash
# 如 rustc/cargo 位于 ~/.cargo/bin，可先确保加入 PATH
export PATH="$HOME/.cargo/bin:$PATH"

# 运行调试版
cargo run

# 运行单元测试
cargo test

# 检查两平台安装器的 SemVer 防降级逻辑（无需 npm 依赖）
node packaging/semver.test.js

# 编译 Release 版本
cargo build --release

# 打包 macOS 开发产物（输出 Earth.app 和带 UNSIGNED 标记的安装包至 dist/）
./scripts/package-macos.sh
```

### 平台说明
- **macOS**：Earth 以无 Dock 图标的 Accessory 菜单栏应用运行（已在 `Info.plist` 设置 `LSUIElement=true`）。执行 `./scripts/package-macos.sh universal` 可生成通用二进制（`arm64` + `x86_64`）的 `dist/Earth.app`、DMG 与 PKG；`native` 参数仅构建本机架构。
  - 默认 `EARTH_RELEASE_CHANNEL=development`，安装包文件名带 `-UNSIGNED`，应用仅做 Ad-hoc 签名，不具备公证，也不作为自动更新资产。正式产物必须显式设置 `EARTH_RELEASE_CHANNEL=official`，提供 Developer ID Application / Installer 身份与公证凭据；签名、发布者校验或公证任一步失败都会中止。PKG 按完整 SemVer 比较已安装应用版本，拒绝稳定版降至同核心版本的预发布版，以及预发布序号倒退。
  - 若启用「选择后自动粘贴」，首次触发需在「系统设置 -> 隐私与安全性 -> 辅助功能」中授予权限；若版本重签名后已勾选仍提示未授权，请在「辅助功能」列表中选中 Earth 点击 `-` 移除后重新添加一次。
- **Windows**：`src/platform/windows.rs` 已基于 `windows-sys` 实现高 DPI 自动缩放定位、系统托盘右键菜单、`SendInput` 自动模拟 `Ctrl+V` 粘贴及开机自启（Windows 可执行文件需在 Windows 本机环境或配置对应交叉编译目标后构建测试）。
  - `scripts/package-windows.ps1` 同样默认生成带 `-UNSIGNED` 的开发安装包。正式模式会为 `Earth.exe` 与 Setup 安装器进行 Authenticode 签名，并核对可信证书 SHA-256；打包时检查 PE 版本资源，安装器通过系统 Windows Script Host 按完整 SemVer 拒绝降级。脚本运行受策略限制、版本损坏或目标目录中的已有应用缺少可靠安装登记时，会在覆盖文件前中止并提示处理。

### 下载未签名的开发包

在仓库的 **Actions → Build & Release (Windows + macOS) → Run workflow** 中选择包含修复的分支，`platform` 选择 `all`，保持 `publish_release` **未勾选**。完成后，在该次运行的 **Artifacts** 下载 `earth-windows-x64-development`（Setup EXE、便携 ZIP）或 `earth-macos-universal-development`（DMG、PKG）。解压后的安装包文件名带 `-UNSIGNED`；此流程无需配置签名证书，也不会创建 GitHub Release。重新运行旧标签的失败任务仍会使用原提交和正式签名模式。

### 正式发布配置

所有版本均从 `Cargo.toml` 通过 `cargo metadata` 读取。发布标签必须严格等于 `v{Cargo 版本}`；CI 的格式检查、Clippy 和测试分别作为独立门禁。推送版本标签或手动选择该版本标签并开启发布时，只有两平台签名验证均成功的正式产物才会进入 Release，并生成完整的 `SHA256SUMS.txt`。手动发布必须选择 `all` 平台；未开启发布的手动运行只生成开发产物。

在 GitHub Actions 仓库配置中设置：

| 平台 | Variables | Secrets |
| --- | --- | --- |
| macOS | `EARTH_UPDATE_MACOS_TEAM_ID`（10 位 Team ID）、`EARTH_MACOS_SIGN_IDENTITY`（Developer ID Application）、`EARTH_MACOS_INSTALLER_IDENTITY`（Developer ID Installer） | `EARTH_MACOS_APP_P12_BASE64`、`EARTH_MACOS_INSTALLER_P12_BASE64`、`EARTH_MACOS_P12_PASSWORD`、`EARTH_APPLE_ID`、`EARTH_APPLE_APP_PASSWORD` |
| Windows | `EARTH_UPDATE_WINDOWS_CERT_SHA256`（发布者证书 DER 的 64 位 SHA-256 十六进制摘要） | `EARTH_WINDOWS_PFX_BASE64`、`EARTH_WINDOWS_PFX_PASSWORD` |

本地正式打包时，macOS 还需设置 `EARTH_NOTARY_PROFILE` 为已创建的 `notarytool` 钥匙串凭据名称；Windows 需设置 `EARTH_WINDOWS_SIGN_THUMBPRINT` 为当前用户证书库中签名证书的 SHA-1 指纹，并安装 Windows SDK 的 `signtool`。可信发布者标识会编译进应用，必须在构建时提供。缺少正式发布凭据会直接失败，不会降级为未签名发布。

## 存储与采集限制

默认每次复制最多保存 2 MiB 文本（含 HTML / RTF）、20 MiB 图片和 4000 万像素。可在「偏好设置 → 存储」调整。超限的图片或正文跳过；富文本超限时保留可用的纯文本。开启来源或格式过滤时，若系统无法提供所需信息，本次复制不写入历史。

历史和备份含剪贴板原文，不加密；请只把备份保存在你信任的位置。新版备份内嵌 PNG 文件，导入旧版备份时会显示缺失图片的数量。

[许可证（MIT）](LICENSE) · [贡献指南](CONTRIBUTING.md) · [安全问题反馈](SECURITY.md)

## 关于作者

- **作者**：西岚
- **微信交流 / 反馈**：扫码添加作者微信

<img src="./assets/wechat_qr.jpg" width="240" alt="西岚微信二维码" />
