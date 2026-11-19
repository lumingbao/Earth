<p align="center">
  <picture>
    <source media="(prefers-color-scheme: dark)" srcset="assets/earth-wordmark-dark.svg">
    <img src="assets/earth-wordmark.svg" alt="Earth" height="56">
  </picture>
</p>

<p align="center">
  纯 Rust 打造的极速跨平台剪贴板与速览工作台。
</p>

<p align="center">
  <a href="README.md">English</a> · 简体中文
</p>

<p align="center">
  <a href="https://github.com/lumingbao/Earth/releases/latest"><b>下载</b></a>
  &nbsp;·&nbsp;
  <a href="#核心特性">核心特性</a>
  &nbsp;·&nbsp;
  <a href="#快捷键">快捷键</a>
  &nbsp;·&nbsp;
  <a href="#构建与发布">构建与发布</a>
  &nbsp;·&nbsp;
  <a href="#交流与反馈">交流与反馈</a>
</p>

Earth 把极速键盘流剪贴板历史与双栏速览工作台带到一个以纯 Rust（`eframe / egui` +
`clipboard-rs` + `rusqlite`）为核心的轻量级原生桌面应用里。

支持文本、富文本、图片、文件、内联编辑与备注、离线二维码、快捷内容加工、智能收藏分组、敏感信息脱敏与中英文多语言界面。提供 **macOS** 与 **Windows** 版本——无 Electron、无 Chromium、无 WebView 负担。

## 极致轻量

| 指标 | Earth |
| --- | --- |
| 运行时 | 纯 Rust 原生机器码 + GPU 硬件渲染（无 Electron / WebView） |
| 体积 | 单架构 Release 可执行文件约 **18 MB** / macOS 通用二进制 `.dmg` 约 **18 MB** |
| 列表渲染 | 视口虚拟化（统一 `34 px` 行高），预览文本、时间与缩略图纹理按项缓存 |
| 剪贴板监听 | Windows 系统通知事件；macOS 每 100 ms 检查剪贴板变化计数 |
| 本地持久化 | SQLite（WAL 模式）内容哈希去重，原图与缩略图分离存储 |

## 核心特性

- **多类型记录与实时搜索**：完整支持纯文本与富文本（HTML / RTF）、图片（PNG + 列表缩略图）及文件 / 文件夹。顶部提供 `全部`、`文本`、`图片`、`文件`、`固定` 与自定义颜色 **Earth Collections** 收藏分组（固定项与分组记录不受历史上限淘汰）。支持 **精确 / 模糊（nucleo）/ 正则** 搜索内容、路径与备注，并高亮匹配字符。
- **双栏速览工作台**：
  - **内联编辑与备注**：工作台直接修改文本并 **保存修改** 或 **保存并粘贴**；支持为任意记录添加可搜索的备注别名。
  - **离线二维码**：一键生成本地高清二维码，支持 **复制二维码图片** 或 **另存为图片...**。
  - **图片、文件与色值**：支持 **适应窗口 / 清晰预览（最高 2048 px）** 与后台解码；文件记录自动校验路径并一键在 Finder / 资源管理器定位；色值记录支持 HEX / RGB 一键复制。
- **快捷内容加工（Quick Actions）**：内置离线工具并支持一键复制或粘贴——`JSON 格式化 / 压缩`、`Base64`、`URL 编解码`、`Unix 时间戳 ↔ 本地/UTC`、`snake_case` / `camelCase` / `UPPER_CASE`、`多行去重 / 排序`、`字符串反转义`、`去除空白` 与 `SHA-256`。
- **灵活的点击与粘贴交互**：可在 **偏好设置 › 通用** 自由切换 **双击复制（单击展开工作台）** 与 **单击直接复制** 两种模式。
- **隐私保护与敏感脱敏**：自动识别 Private Key、JWT、API Key、Bearer Token 等凭据并在列表默认 `••••` 遮罩（需点击「显示明文」查看）。支持应用黑/白名单、剪贴板格式忽略（如 `ConcealedType` / `TransientType` / `ExcludeClipboardContentFromMonitorProcessing`）、正则忽略、**暂停监听** 与 **仅忽略下一次复制**。
- **多语言界面（i18n）**：内置 **English** 与 **简体中文**，默认 **跟随系统**，可在 **偏好设置 › 通用 › 界面语言** 随时切换，主界面与菜单栏/系统托盘即时生效。
- **备份闭环与可信更新**：支持导出 / 导入内嵌 PNG 的 JSON 备份并自动去重。正式签名构建会校验 `SHA256SUMS.txt`、代码签名、发布者身份与 macOS Apple 公证，通过后可点击 **重启完成更新**；未签名构建保留 **前往下载** 手动更新入口。

## 快捷键

所有快捷键均可在 **偏好设置 › 通用** 中实时录制修改（搜索框或内联编辑器聚焦时自动避让，保留正常文字编辑与输入法行为）。

| 操作 | macOS | Windows |
| --- | --- | --- |
| 唤醒 / 隐藏 Earth | `⇧⌘C` | `Shift+Alt+V` |
| 瞬时粘贴前 1–9 条 | `⌘1`–`⌘9` | `Alt+1`–`Alt+9` |
| 仅复制到剪贴板 | `⌥+Enter` | `Alt+Enter` |
| 复制并以纯文本粘贴 | `⇧⌘+Enter` | `Ctrl+Shift+Enter` |
| 展开 / 收起速览工作台 | `⇧⌘P` | `Ctrl+Shift+P` |
| 固定 / 取消固定记录 | `⌥P` | `Alt+P` |
| 删除记录 | `⌥⌫` | `Alt+Delete` |
| 循环切换顶部筛选胶囊 | `Tab` / `Shift+Tab` | `Tab` / `Shift+Tab` |

## 构建与发布

需要 Rust 工具链（`1.95+` / Edition `2024`，当前验证版本 `1.100.0`）：

```bash
cargo run                      # 运行调试版
cargo test                     # 运行单元测试
node packaging/semver.test.js  # 验证两平台安装器的 SemVer 防降级逻辑
cargo build --release          # 编译 Release 版本
./scripts/package-macos.sh     # 打包 macOS 开发产物（Earth.app、DMG、PKG -> dist/）
```

<details>
<summary>平台说明、存储限制与正式签名发布配置</summary>

### 平台说明

- **macOS**：以无 Dock 图标的 Accessory 菜单栏应用运行（`LSUIElement=true`）。执行 `./scripts/package-macos.sh universal` 生成通用二进制（`arm64` + `x86_64`）产物，`native` 仅构建本机架构。启用「选择后自动粘贴」需在 **系统设置 › 隐私与安全性 › 辅助功能** 授予权限（版本重签名后若仍提示未授权，请先点击 `-` 移除再重新添加）。
- **Windows**：基于 `windows-sys` 实现高 DPI 定位、系统托盘菜单、`SendInput`（`Ctrl+V`）自动粘贴及登录自启，使用 `scripts/package-windows.ps1` 打包。macOS PKG 与 Windows Setup 均按完整 SemVer 拒绝降级安装。

### 存储与采集限制

默认单条上限为 2 MiB 文本（含 HTML / RTF）、20 MiB 图片与 4000 万像素（可在 **偏好设置 › 存储** 调整）。超限图片或正文会跳过，富文本超限时回退保存纯文本。历史数据库与 JSON 备份未加密，请仅保存在可信位置。

### CI 构建产物与签名配置

- **下载未签名开发包**：在 **Actions › Build & Release (Windows + macOS) › Run workflow** 选择分支并保持 `publish_release` 未勾选，即可在 **Artifacts** 下载 `earth-windows-x64-development` 或 `earth-macos-universal-development`。
- **Tag 正式发布**：推送严格匹配 `v{Cargo.toml 版本}` 的标签。未配置签名凭据时，对应平台自动回退为未签名构建并照常发布安装包与 `SHA256SUMS.txt`；完整配置凭据时按 `official` 模式签名、Apple 公证并验证可信发布者（仅配置部分凭据时立即失败中止以防意外降级）。

| 平台 | Repository Variables | Repository Secrets |
| --- | --- | --- |
| macOS | `EARTH_UPDATE_MACOS_TEAM_ID`、`EARTH_MACOS_SIGN_IDENTITY`、`EARTH_MACOS_INSTALLER_IDENTITY` | `EARTH_MACOS_APP_P12_BASE64`、`EARTH_MACOS_INSTALLER_P12_BASE64`、`EARTH_MACOS_P12_PASSWORD`、`EARTH_APPLE_ID`、`EARTH_APPLE_APP_PASSWORD` |
| Windows | `EARTH_UPDATE_WINDOWS_CERT_SHA256` | `EARTH_WINDOWS_PFX_BASE64`、`EARTH_WINDOWS_PFX_PASSWORD` |

本地正式打包还需设置 `EARTH_NOTARY_PROFILE`（macOS `notarytool` 钥匙串凭据）或 `EARTH_WINDOWS_SIGN_THUMBPRINT` + Windows SDK `signtool`（Windows）。

</details>

## 交流与反馈

- [GitHub Issues](https://github.com/lumingbao/Earth/issues)：问题反馈、功能建议与讨论。
- 微信交流（作者：西岚），扫码添加：

  <img src="assets/wechat_qr.jpg" alt="西岚微信二维码" width="200">

- 参与贡献与安全反馈：参见 [CONTRIBUTING.md](CONTRIBUTING.md) 与 [SECURITY.md](SECURITY.md)。

## 许可证

[MIT](LICENSE)。

<sub>本项目是 Vibe Coding 的产物——从架构选型、Rust 跨平台实现、界面视觉打磨到交互细节，全程与 AI 协同迭代完成。</sub>
