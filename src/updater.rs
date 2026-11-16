use serde::Deserialize;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::mpsc::Sender;

pub const GITHUB_LATEST_API: &str = "https://api.github.com/repos/lumingbao/Earth/releases/latest";
pub const GITHUB_RELEASES_PAGE: &str = "https://github.com/lumingbao/Earth/releases";
pub const CURRENT_VERSION: &str = env!("CARGO_PKG_VERSION");

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReleaseInfo {
    pub version: String,
    pub tag_name: String,
    pub html_url: String,
    pub download_url: Option<String>,
    pub asset_name: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UpdateCheckOutcome {
    UpdateAvailable(ReleaseInfo),
    UpToDate(String),
    Error(String),
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum UpdateDownloadState {
    #[default]
    Idle,
    Downloading {
        version: String,
    },
    ReadyToInstall {
        version: String,
        staged_path: PathBuf,
    },
    Failed {
        version: String,
        error: String,
    },
}

#[derive(Debug, Clone)]
pub enum UpdaterEvent {
    CheckFinished {
        manual: bool,
        outcome: UpdateCheckOutcome,
    },
    DownloadFinished {
        version: String,
        result: Result<PathBuf, String>,
    },
}

#[derive(Debug, Deserialize)]
struct GithubReleaseDto {
    tag_name: String,
    #[serde(default)]
    html_url: String,
    #[serde(default)]
    draft: bool,
    #[serde(default)]
    prerelease: bool,
    #[serde(default)]
    assets: Vec<GithubAssetDto>,
}

#[derive(Debug, Deserialize)]
struct GithubAssetDto {
    name: String,
    browser_download_url: String,
}

pub fn spawn_update_check(manual: bool, tx: Sender<UpdaterEvent>, ctx: egui::Context) {
    std::thread::spawn(move || {
        let outcome = fetch_latest_github_release();
        let _ = tx.send(UpdaterEvent::CheckFinished { manual, outcome });
        ctx.request_repaint();
    });
}

pub fn spawn_update_download(info: ReleaseInfo, tx: Sender<UpdaterEvent>, ctx: egui::Context) {
    std::thread::spawn(move || {
        let version = info.version.clone();
        let result = download_and_stage_release(&info);
        let _ = tx.send(UpdaterEvent::DownloadFinished { version, result });
        ctx.request_repaint();
    });
}

fn fetch_latest_github_release() -> UpdateCheckOutcome {
    let user_agent = format!("User-Agent: Earth/{CURRENT_VERSION}");
    let mut cmd = Command::new("curl");
    cmd.args([
        "-fsSL",
        "--max-time",
        "8",
        "-H",
        "Accept: application/vnd.github+json",
        "-H",
        &user_agent,
        GITHUB_LATEST_API,
    ]);

    #[cfg(target_os = "windows")]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }

    let output = match cmd.output() {
        Ok(out) => out,
        Err(e) => return UpdateCheckOutcome::Error(format!("无法启动网络请求: {e}")),
    };

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        if stderr.contains("404") {
            return UpdateCheckOutcome::UpToDate(CURRENT_VERSION.to_string());
        }
        return UpdateCheckOutcome::Error("连接 GitHub Releases 接口失败，请检查网络".to_string());
    }

    let body = String::from_utf8_lossy(&output.stdout);
    match parse_github_release(&body, CURRENT_VERSION) {
        Ok(outcome) => outcome,
        Err(err) => UpdateCheckOutcome::Error(err),
    }
}

fn download_and_stage_release(info: &ReleaseInfo) -> Result<PathBuf, String> {
    let url = info
        .download_url
        .as_deref()
        .ok_or_else(|| "该版本未附带可直接下载的安装包".to_string())?;

    let stage_root = std::env::temp_dir().join(format!("earth-update-{}", info.version));
    let _ = fs::remove_dir_all(&stage_root);
    fs::create_dir_all(&stage_root).map_err(|e| format!("创建临时更新目录失败: {e}"))?;

    let default_name = if cfg!(target_os = "macos") {
        format!("Earth-{}-macOS-universal.dmg", info.version)
    } else {
        format!("Earth-{}-setup.exe", info.version)
    };
    let file_name = info.asset_name.as_deref().unwrap_or(&default_name);
    let archive_path = stage_root.join(file_name);
    let archive_str = archive_path.to_string_lossy().into_owned();

    let user_agent = format!("User-Agent: Earth/{CURRENT_VERSION}");
    let mut cmd = Command::new("curl");
    cmd.args([
        "-fL",
        "--retry",
        "2",
        "--connect-timeout",
        "15",
        "-H",
        &user_agent,
        "-o",
        &archive_str,
        url,
    ]);

    #[cfg(target_os = "windows")]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }

    let status = cmd
        .status()
        .map_err(|e| format!("启动下载进程失败: {e}"))?;
    if !status.success() {
        return Err("下载更新安装包失败，请重试或点击「前往下载」".to_string());
    }

    #[cfg(target_os = "macos")]
    {
        if file_name.to_ascii_lowercase().ends_with(".dmg") {
            let mount_dir = stage_root.join("mnt");
            fs::create_dir_all(&mount_dir).map_err(|e| format!("创建挂载目录失败: {e}"))?;
            let mount_str = mount_dir.to_string_lossy().into_owned();

            let attach_status = Command::new("hdiutil")
                .args([
                    "attach",
                    "-nobrowse",
                    "-readonly",
                    "-mountpoint",
                    &mount_str,
                    &archive_str,
                ])
                .status()
                .map_err(|e| format!("挂载更新镜像失败: {e}"))?;
            if !attach_status.success() {
                return Err("挂载 DMG 更新镜像失败".to_string());
            }

            let mounted_app = mount_dir.join("Earth.app");
            let staged_app = stage_root.join("Earth.app");
            let copy_res = Command::new("ditto")
                .args([
                    mounted_app.to_string_lossy().as_ref(),
                    staged_app.to_string_lossy().as_ref(),
                ])
                .status();

            let _ = Command::new("hdiutil")
                .args(["detach", &mount_str, "-force"])
                .status();

            let copy_status = copy_res.map_err(|e| format!("解包 Earth.app 失败: {e}"))?;
            if !copy_status.success() || !staged_app.join("Contents/MacOS/earth").exists() {
                return Err("更新镜像中未找到有效的 Earth.app".to_string());
            }

            let _ = Command::new("xattr")
                .args(["-cr", staged_app.to_string_lossy().as_ref()])
                .status();

            return Ok(staged_app);
        }
    }

    Ok(archive_path)
}

/// Launches a detached updater script/installer that waits for the current Earth process to exit,
/// replaces the application bundle (or runs the installer), and restarts Earth.
pub fn apply_staged_update_and_restart(staged_path: &Path) -> Result<(), String> {
    if !staged_path.exists() {
        return Err("更新包文件不存在，请重新检查更新".to_string());
    }

    #[cfg(target_os = "macos")]
    {
        if staged_path.extension().and_then(|e| e.to_str()) == Some("app") {
            let exe = std::env::current_exe().ok();
            let target_app = exe
                .as_deref()
                .and_then(find_bundle_from_exe_path)
                .unwrap_or_else(|| PathBuf::from("/Applications/Earth.app"));

            let parent_dir = staged_path
                .parent()
                .ok_or_else(|| "无效的更新暂存目录".to_string())?;
            let script_path = parent_dir.join("apply_update.sh");
            let script_content = build_macos_restart_script();
            fs::write(&script_path, script_content)
                .map_err(|e| format!("写入更新脚本失败: {e}"))?;

            let pid = std::process::id().to_string();
            Command::new("/bin/sh")
                .args([
                    script_path.to_string_lossy().as_ref(),
                    &pid,
                    staged_path.to_string_lossy().as_ref(),
                    target_app.to_string_lossy().as_ref(),
                ])
                .spawn()
                .map_err(|e| format!("启动更新替换脚本失败: {e}"))?;
            return Ok(());
        }

        // Fallback if asset is a .pkg or .dmg
        Command::new("open")
            .arg(staged_path)
            .spawn()
            .map_err(|e| format!("打开更新包失败: {e}"))?;
        Ok(())
    }

    #[cfg(not(target_os = "macos"))]
    {
        Command::new("cmd")
            .args(["/C", "start", "", &staged_path.to_string_lossy()])
            .spawn()
            .map_err(|e| format!("启动安装程序失败: {e}"))?;
        Ok(())
    }
}

pub fn find_bundle_from_exe_path(exe_path: &Path) -> Option<PathBuf> {
    for ancestor in exe_path.ancestors() {
        if ancestor.extension().and_then(|e| e.to_str()) == Some("app") {
            return Some(ancestor.to_path_buf());
        }
    }
    None
}

pub fn build_macos_restart_script() -> &'static str {
    "#!/bin/sh
OLD_PID=\"$1\"
STAGED_APP=\"$2\"
TARGET_APP=\"$3\"
i=0
while kill -0 \"$OLD_PID\" 2>/dev/null && [ $i -lt 60 ]; do
    sleep 0.1
    i=$((i + 1))
done
rm -rf \"$TARGET_APP\"
ditto \"$STAGED_APP\" \"$TARGET_APP\"
xattr -cr \"$TARGET_APP\" 2>/dev/null || true
open -n \"$TARGET_APP\"
rm -rf \"$(dirname \"$STAGED_APP\")\"
"
}

pub fn parse_github_release(
    json_str: &str,
    current_version: &str,
) -> Result<UpdateCheckOutcome, String> {
    let dto: GithubReleaseDto =
        serde_json::from_str(json_str).map_err(|e| format!("解析版本信息失败: {e}"))?;

    if dto.draft || dto.prerelease {
        return Ok(UpdateCheckOutcome::UpToDate(current_version.to_string()));
    }

    let remote_ver = normalize_version(&dto.tag_name);
    if remote_ver.is_empty() {
        return Err("远端 Release 缺少有效版本号".to_string());
    }

    if is_newer_version(&remote_ver, current_version) {
        let html_url = if dto.html_url.trim().is_empty() {
            GITHUB_RELEASES_PAGE.to_string()
        } else {
            dto.html_url
        };
        let (download_url, asset_name) = select_preferred_asset(&dto.assets);
        Ok(UpdateCheckOutcome::UpdateAvailable(ReleaseInfo {
            version: remote_ver,
            tag_name: dto.tag_name,
            html_url,
            download_url,
            asset_name,
        }))
    } else {
        Ok(UpdateCheckOutcome::UpToDate(current_version.to_string()))
    }
}

fn select_preferred_asset(assets: &[GithubAssetDto]) -> (Option<String>, Option<String>) {
    #[cfg(target_os = "macos")]
    let preferred_exts = [".dmg", ".pkg"];
    #[cfg(not(target_os = "macos"))]
    let preferred_exts = [".msi", ".exe", ".zip"];

    for ext in preferred_exts {
        if let Some(asset) = assets
            .iter()
            .find(|a| a.name.to_ascii_lowercase().ends_with(ext))
            && !asset.browser_download_url.is_empty()
        {
            return (
                Some(asset.browser_download_url.clone()),
                Some(asset.name.clone()),
            );
        }
    }
    (None, None)
}

pub fn normalize_version(raw: &str) -> String {
    let trimmed = raw.trim();
    trimmed
        .strip_prefix('v')
        .or_else(|| trimmed.strip_prefix('V'))
        .unwrap_or(trimmed)
        .trim()
        .to_string()
}

pub fn is_newer_version(remote: &str, current: &str) -> bool {
    let Some((r_core, r_pre)) = parse_semver(remote) else {
        return false;
    };
    let Some((c_core, c_pre)) = parse_semver(current) else {
        return false;
    };

    if r_core != c_core {
        return r_core > c_core;
    }
    c_pre && !r_pre
}

fn parse_semver(ver: &str) -> Option<((u64, u64, u64), bool)> {
    let norm = normalize_version(ver);
    if norm.is_empty() {
        return None;
    }
    let without_build = norm.split('+').next().unwrap_or(&norm);
    let (core_str, has_prerelease) = match without_build.split_once('-') {
        Some((core, pre)) => (core, !pre.is_empty()),
        None => (without_build, false),
    };

    let mut parts = core_str.split('.');
    let major = parts.next()?.parse::<u64>().ok()?;
    let minor = parts.next().unwrap_or("0").parse::<u64>().ok()?;
    let patch = parts.next().unwrap_or("0").parse::<u64>().ok()?;
    Some(((major, minor, patch), has_prerelease))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn semver_comparison_handles_v_prefix_and_prereleases() {
        assert!(is_newer_version("v0.3.0", "0.2.0"));
        assert!(is_newer_version("0.2.1", "0.2.0"));
        assert!(is_newer_version("1.0.0", "0.9.9"));
        assert!(!is_newer_version("v0.2.0", "0.2.0"));
        assert!(!is_newer_version("v0.1.0", "0.2.0"));
        assert!(!is_newer_version("v0.2.0-beta.1", "0.2.0"));
        assert!(is_newer_version("v0.2.0", "0.2.0-beta.1"));
    }

    #[test]
    fn parses_github_latest_release_json_and_extracts_dmg_asset() {
        let sample = r#"{
            "tag_name": "v0.3.0",
            "html_url": "https://github.com/lumingbao/Earth/releases/tag/v0.3.0",
            "draft": false,
            "prerelease": false,
            "assets": [
                {
                    "name": "Earth-0.3.0-macOS-universal.dmg",
                    "browser_download_url": "https://github.com/lumingbao/Earth/releases/download/v0.3.0/Earth-0.3.0-macOS-universal.dmg"
                }
            ]
        }"#;

        let res = parse_github_release(sample, "0.2.0").unwrap();
        match res {
            UpdateCheckOutcome::UpdateAvailable(info) => {
                assert_eq!(info.version, "0.3.0");
                assert_eq!(info.tag_name, "v0.3.0");
                assert!(info.html_url.contains("v0.3.0"));
                #[cfg(target_os = "macos")]
                {
                    assert_eq!(
                        info.asset_name.as_deref(),
                        Some("Earth-0.3.0-macOS-universal.dmg")
                    );
                    assert!(info.download_url.is_some());
                }
            }
            other => panic!("expected UpdateAvailable, got {other:?}"),
        }

        let up_to_date = parse_github_release(sample, "0.3.0").unwrap();
        assert_eq!(up_to_date, UpdateCheckOutcome::UpToDate("0.3.0".to_string()));
    }

    #[test]
    fn finds_app_bundle_from_executable_path() {
        let p = Path::new("/Applications/Earth.app/Contents/MacOS/earth");
        assert_eq!(
            find_bundle_from_exe_path(p),
            Some(PathBuf::from("/Applications/Earth.app"))
        );
        let dev = Path::new("/Users/test/Earth/target/release/earth");
        assert_eq!(find_bundle_from_exe_path(dev), None);
    }
}
