# Earth — 跨平台智能剪贴板管理工具

使用纯 Rust（`eframe / egui` + `clipboard-rs` + `rusqlite`）开发的轻量化跨平台剪贴板管理器，支持 **macOS** 与 **Windows**。参考 Maccy 的极速键盘流体验，并在此基础上打造了双栏速览工作台、快捷内容加工、智能收藏分组（Earth Collections）与敏感信息脱敏等专属能力。

## 核心特性

### 1. Maccy 级极速键盘流与多模态记录
- **多类型记录**：完整支持 **文字**（含纯文本与富文本 HTML/RTF）、**图片**（PNG 存储 + 缩略图即时渲染）以及 **文件 / 文件夹** 复制列表。
- **全局热键唤醒**：
  - **macOS**：默认 `⇧⌘C` 唤醒，`⌘1`~`⌘9` 瞬时粘贴，`⌥P` 置顶，`⌥⌫` 删除，`⌃空格` 展开预览。
  - **Windows**：默认 `Alt+V` 唤醒，`Alt+1`~`Alt+9` 瞬时粘贴，`Alt+P` 置顶，`Alt+Delete` 删除，`Ctrl+Space` 展开预览。
  - 支持全部快捷键在「偏好设置 -> 通用」中实时录制修改。
- **修饰键差异化粘贴**：
  - `Enter` / 单击：按默认行为粘贴或复制。
  - `⌥+Enter`（Windows `Alt+Enter`）：仅复制到剪贴板。
  - `⇧⌘+Enter`（Windows `Ctrl+Shift+Enter`）：复制并以纯文本格式粘贴。
- **实时搜索与高亮**：支持 **精确 / 模糊（nucleo）/ 正则** 三种匹配模式，支持粗体 / 斜体 / 下划线高亮匹配字符。

### 2. Earth 专属特色功能
- **类型过滤胶囊**：顶部快捷筛选 `全部`、`文本`、`图片`、`文件`、`颜色`、`链接`、`置顶` 以及自定义收藏分组。
- **双栏「速览工作台」**：支持手动或延迟自动展开右侧详情面板，提供大图 1:1 缩放与另存为、文件有效性检查与 Finder/资源管理器定位、色值 HEX/RGB 一键复制。
- **快捷内容加工（Quick Actions）**：对选中文本一键执行 `JSON 格式化/压缩`、`Base64 编解码`、`URL 编解码`、`Unix 时间戳 ↔ 本地/UTC 日期转换`、`去除空白`，并支持直接复制或粘贴加工结果。
- **智能收藏分组（Earth Collections）**：除基础置顶外，支持创建带颜色标签的自定义分组（如「常用指令」「代码片段」），组内记录不受历史上限自动淘汰影响。
- **增强隐私保护**：
  - 自动识别常见敏感凭据（API Key、JWT Token、Bearer Token、Private Key）并在列表中默认 `••••` 遮罩保护。
  - 支持应用黑/白名单忽略、系统剪贴板格式忽略（如 `ConcealedType` / `TransientType` / `ExcludeClipboardContentFromMonitorProcessing`）、正则表达式忽略。
  - 支持「暂停监听」与「仅忽略下一次复制」。

## 构建与运行

确保已安装 Rust 工具链（`1.88+` / Edition `2024`，当前验证版本为 `1.100.0`）：

```bash
# 如 rustc/cargo 位于 ~/.cargo/bin，可先确保加入 PATH
export PATH="$HOME/.cargo/bin:$PATH"

# 运行调试版
cargo run

# 运行单元测试
cargo test

# 编译 Release 版本
cargo build --release

# 打包 macOS 安装程序（输出 Earth.app、.dmg 与 .pkg 至 dist/）
./scripts/package-macos.sh
```

### 平台说明
- **macOS**：Earth 以无 Dock 图标的 Accessory 菜单栏应用运行（已在 `Info.plist` 设置 `LSUIElement=true`）。可执行 `./scripts/package-macos.sh` 生成通用二进制（`arm64` + `x86_64`）的 `dist/Earth.app`、`dist/Earth-0.1.0-macOS-universal.dmg` 及 `dist/Earth-0.1.0-macOS-universal.pkg`。
  - 默认打包产物使用 Ad-hoc 签名；若需跨机器公证分发，可在签名与公证环节接入 Apple Developer ID 证书（`Developer ID Application` / `Developer ID Installer`）。
  - 若启用「选择后自动粘贴」，首次触发需在「系统设置 -> 隐私与安全性 -> 辅助功能」中授予权限。
- **Windows**：`src/platform/windows.rs` 已基于 `windows-sys` 实现高 DPI 自动缩放定位、系统托盘右键菜单、`SendInput` 自动模拟 `Ctrl+V` 粘贴及开机自启（Windows 可执行文件需在 Windows 本机环境或配置对应交叉编译目标后构建测试）。
