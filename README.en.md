# Earth — Ultra-Fast Cross-Platform Clipboard Manager & Quick-Preview Workbench

[中文](README.md) | English

A lightweight, cross-platform clipboard manager built in pure Rust (`eframe / egui` + `clipboard-rs` + `rusqlite`) for **macOS** and **Windows**. Designed for both blazing-fast keyboard workflows and modern card-based mouse interactions, Earth includes a split-pane Quick-Preview Workbench, inline text editing & notes, offline QR code generation, Quick Actions, smart collections (Earth Collections), sensitive-data masking, multi-language UI (English & Simplified Chinese), and verified one-click updates.

> **How it was built**: This project is crafted through **Vibe Coding** — from architecture choices and cross-platform Rust implementation to visual polish and interaction details, iterated end-to-end in collaboration with AI.

## Lightweight & Low-Overhead Architecture

Earth avoids heavy browser or scripting runtimes by design, staying **virtually invisible in the background and instantaneous when summoned**:

- **No Electron / Chromium / WebView overhead**: Compiled from pure Rust to native machine code with hardware-accelerated GPU rendering. A single-architecture Release binary is ~`18 MB`, and the universal macOS `.dmg` installer is ~`18 MB`.
- **Low memory & near-zero idle CPU**: Uses **viewport virtualization** for the history list (only visible rows are rendered) and per-item caching for preview text, timestamps, thumbnail textures, and Quick Action detection. Windows listens via native clipboard change notifications; macOS polls pasteboard change count every 100 ms. Rapid back-to-back copies may keep only the latest item, and source app metadata is captured from the foreground window at capture time.
- **Fast local persistence**: Uses `SQLite` (WAL mode) for text indexing and content-hash deduplication, storing full-size images separately from thumbnails so switching across large images or long text stays smooth.

## Core Features

### 1. Fast Keyboard Flow & Flexible Click Modes
- **Multi-type clipboard history**: Supports **Text** (plain text plus HTML/RTF rich text), **Images** (PNG storage + instant list thumbnails), and **Files / Folders** with a clean, uniform `34 px` row height.
- **Global & in-app hotkeys**:
  - **macOS**: `⇧⌘C` to open Earth, `⌘1`–`⌘9` for instant paste, `⌥P` to pin, `⌥⌫` to delete, `⇧⌘P` to toggle the Workbench, and `Tab / Shift+Tab` to cycle top filter capsules.
  - **Windows**: `Shift+Alt+V` to open Earth, `Alt+1`–`Alt+9` for instant paste, `Alt+P` to pin, `Alt+Delete` to delete, `Ctrl+Shift+P` to toggle the Workbench, and `Tab / Shift+Tab` to cycle top filter capsules.
  - Every shortcut can be customized live under **Preferences -> General**.
  - While the search bar or an inline text editor has focus, pin/delete/clear/workbench shortcuts are suppressed to preserve text editing and IME composition (all actions remain accessible via buttons and right-click menus). Recording rejects common OS-reserved shortcuts, and failing to register a new global shortcut keeps the previous working one.
- **Switchable Single-Click / Double-Click Mode**:
  - Configurable under **Preferences -> General -> Mouse Click on Item**:
    - **Double-Click to Copy (Single-Click Previews)** (default): Single-clicking an item opens the right-hand Quick-Preview Workbench; double-clicking copies/pastes.
    - **Single-Click to Copy Directly**: When the Workbench is closed, single-clicking copies/pastes immediately without expanding the pane. Once the Workbench is opened (via toolbar button, shortcut, or context menu), single-clicking another item switches the preview and clicking the selected item again copies it.
- **Modifier-Aware Paste**:
  - `Enter` / trigger copy: Runs the default paste/copy behavior.
  - `⌥+Enter` (Windows `Alt+Enter`): Copies to clipboard only.
  - `⇧⌘+Enter` (Windows `Ctrl+Shift+Enter`): Copies and pastes as plain text.
- **Instant Search & Match Highlighting**: Supports **Exact / Fuzzy (`nucleo`) / Regex** search across item content, file paths, and custom notes, with bold / italic / underline match highlights.

### 2. Quick-Preview Workbench & Power Features
- **Category Capsules & Earth Collections**: Filter by `All`, `Text`, `Images`, `Files`, `Pinned`, or custom color-coded collections (e.g., *Shell Commands*, *Snippets*). Pinned items and items assigned to a collection are never evicted by the history limit.
- **Split-Pane Quick-Preview Workbench**:
  - **Inline text editing**: Click **Edit** directly in the Workbench to modify an item, then **Save** or **Save & Paste** — character/line/word stats and content detection update automatically.
  - **Notes & searchable aliases**: Attach, edit, or clear a custom note on any item; notes appear as badges in the list and are searchable.
  - **Offline QR code generation & export**: Turn text or URLs into crisp local QR codes, with one-click **Copy QR Image** or **Save Image As...**.
  - **Enhanced image, file & color previews**: Toggle **Fit Window / Crisp Preview (up to 2048 px)** and save images with background decoding that never blocks the UI; file entries verify path existence and open directly in Finder / Explorer; hex/RGB color items offer one-click HEX / RGB copying.
- **Quick Actions (Offline Transforms)**: Transform text offline and copy or paste the result in one click:
  - `Format / Minify JSON`, `Base64 Encode / Decode`, `URL Encode / Decode`, `Unix Timestamp ↔ Local/UTC Date`
  - Identifier casing: `snake_case`, `camelCase`, `UPPER_CASE`
  - Line & utility tools: `Deduplicate Lines`, `Sort Lines`, `Unescape String`, `Trim Whitespace`, `SHA-256 Digest`
- **Privacy & Sensitive Data Protection**:
  - Automatically detects common secrets (Private Keys, JWTs, API Keys, Bearer Tokens, etc.) and masks them as `••••` in the list until you click **Show Plaintext** in the preview pane.
  - Supports app blocklist/allowlist rules, clipboard format/type ignores (such as `ConcealedType`, `TransientType`, `ExcludeClipboardContentFromMonitorProcessing`), regex ignores, **Pause Recording**, and **Ignore Next Copy**.
- **Multi-Language UI (i18n)**:
  - Ships with **English** and **Simplified Chinese (简体中文)** locale packs. Defaults to **Follow System** and can be switched live at any time under **Preferences -> General -> Language** (instantly updating both the UI and the menu bar / system tray).
- **Backup & Verified Updates**:
  - **JSON Export & Import**: Export full JSON backups (with embedded PNGs) and import/merge backups with content-hash deduplication under **Preferences -> Storage**.
  - **Automatic Update Checks & Trusted Installers**: Check automatically on launch or manually via **Check Now**. Builds signed with a trusted publisher identity verify `SHA256SUMS.txt`, installer signatures, publisher identity, and macOS Apple notarization before offering **Restart to Update** (preserving backups and install logs on macOS). Unsigned or development builds keep the manual **Download** button. Pre-release versions follow full SemVer ordering, while stable builds receive stable releases by default.

## Build & Run

Requires a Rust toolchain (`1.95+` / Edition `2024`; currently verified on Rust `1.100.0`):

```bash
# Add ~/.cargo/bin to PATH if needed
export PATH="$HOME/.cargo/bin:$PATH"

# Run debug build
cargo run

# Run unit tests
cargo test

# Verify cross-platform installer SemVer anti-downgrade logic (no npm deps required)
node packaging/semver.test.js

# Build release binary
cargo build --release

# Package macOS development artifacts (outputs Earth.app, DMG, and PKG into dist/)
./scripts/package-macos.sh
```

### Platform Notes
- **macOS**: Runs as a menu-bar accessory app without a Dock icon (`LSUIElement=true` in `Info.plist`). Run `./scripts/package-macos.sh universal` to build a universal (`arm64` + `x86_64`) `dist/Earth.app`, DMG, and PKG, or pass `native` for the host architecture only.
  - By default `EARTH_RELEASE_CHANNEL=development` produces ad-hoc signed, unnotarized builds that are not eligible for in-app auto-updates. Official releases require `EARTH_RELEASE_CHANNEL=official` with Developer ID Application / Installer identities and notarization credentials; any signing, publisher-check, or notarization failure aborts the build. The PKG compares installed versions by full SemVer and prevents downgrades.
  - To use **Auto-Paste on Selection**, grant permission under **System Settings -> Privacy & Security -> Accessibility**. If macOS still reports unauthorized after a re-signed upgrade, select Earth in the Accessibility list, click `-` to remove it, and add it again.
- **Windows**: `src/platform/windows.rs` uses `windows-sys` for DPI-aware popup positioning, system tray menu integration, `SendInput` (`Ctrl+V`) auto-pasting, and login autostart (build and test Windows binaries natively on Windows or with a configured Windows target).
  - `scripts/package-windows.ps1` likewise produces unsigned development installers by default. Official mode Authenticode-signs `Earth.exe` and the Setup installer, verifies the trusted certificate SHA-256, checks PE version metadata, and enforces SemVer anti-downgrade checks via Windows Script Host.

### Downloading Releases & Unsigned CI Artifacts

- **GitHub Releases**: Download the latest macOS (`.dmg` / `.pkg`) and Windows (`Setup.exe` / portable `.zip`) packages from the [Releases](https://github.com/lumingbao/Earth/releases) page.
- **CI Development Builds**: In **Actions → Build & Release (Windows + macOS) → Run workflow**, select a branch, choose `platform: all`, and leave `publish_release` unchecked to download `earth-windows-x64-development` or `earth-macos-universal-development` from the workflow **Artifacts** (no signing certificates required and no Release created).

### Release & Signing Configuration

Versions are read from `Cargo.toml` via `cargo metadata`, and release tags must match `v{version}` strictly. Format, Clippy, and test checks run as independent CI gates. When pushing a version tag (or manually dispatching with `publish_release` checked):

- **Without signing secrets configured**: Each platform falls back to unsigned builds and publishes installers, `SHA256SUMS.txt`, and the GitHub Release (users approve first-run OS prompts manually, and in-app updates stay in manual **Download** mode).
- **With full signing credentials configured**: Builds run in `official` mode with code signing, Apple notarization, and trusted-publisher verification. Partially configured credentials fail fast to prevent accidental silent downgrade.

Optional GitHub Actions repository configuration for official signed releases:

| Platform | Variables | Secrets |
| --- | --- | --- |
| macOS | `EARTH_UPDATE_MACOS_TEAM_ID` (10-char Team ID), `EARTH_MACOS_SIGN_IDENTITY` (Developer ID Application), `EARTH_MACOS_INSTALLER_IDENTITY` (Developer ID Installer) | `EARTH_MACOS_APP_P12_BASE64`, `EARTH_MACOS_INSTALLER_P12_BASE64`, `EARTH_MACOS_P12_PASSWORD`, `EARTH_APPLE_ID`, `EARTH_APPLE_APP_PASSWORD` |
| Windows | `EARTH_UPDATE_WINDOWS_CERT_SHA256` (64-hex SHA-256 of DER publisher cert) | `EARTH_WINDOWS_PFX_BASE64`, `EARTH_WINDOWS_PFX_PASSWORD` |

For local official packaging: macOS also requires `EARTH_NOTARY_PROFILE` (a `notarytool` keychain profile); Windows requires `EARTH_WINDOWS_SIGN_THUMBPRINT` (SHA-1 thumbprint in the current user's certificate store) and Windows SDK `signtool`.

## Storage & Capture Limits

By default each captured item allows up to 2 MiB of text (including HTML/RTF), 20 MiB per image, and 40 megapixels (configurable under **Preferences -> Storage**). Oversized images or text are skipped; oversized rich text falls back to plain text when available. When source-app or format filters are enabled and the OS cannot supply that metadata, the copy event is skipped.

History and JSON backups store clipboard content unencrypted — keep backup files only in locations you trust. Modern backups embed PNG image files; importing legacy JSON backups reports any missing image previews.

[License (MIT)](LICENSE) · [Contributing](CONTRIBUTING.md) · [Security](SECURITY.md)

## Author

- **Author**: 西岚 (Xilan)
- **WeChat / Feedback**: Scan the QR code below

<img src="./assets/wechat_qr.jpg" width="240" alt="Author WeChat QR Code" />
