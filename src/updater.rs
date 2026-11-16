use serde::Deserialize;
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
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UpdateCheckOutcome {
    UpdateAvailable(ReleaseInfo),
    UpToDate(String),
    Error(String),
}

#[derive(Debug, Clone)]
pub struct UpdateCheckEvent {
    pub manual: bool,
    pub outcome: UpdateCheckOutcome,
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

pub fn spawn_update_check(manual: bool, tx: Sender<UpdateCheckEvent>, ctx: egui::Context) {
    std::thread::spawn(move || {
        let outcome = fetch_latest_github_release();
        let _ = tx.send(UpdateCheckEvent { manual, outcome });
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
        let download_url = select_preferred_asset(&dto.assets);
        Ok(UpdateCheckOutcome::UpdateAvailable(ReleaseInfo {
            version: remote_ver,
            tag_name: dto.tag_name,
            html_url,
            download_url,
        }))
    } else {
        Ok(UpdateCheckOutcome::UpToDate(current_version.to_string()))
    }
}

fn select_preferred_asset(assets: &[GithubAssetDto]) -> Option<String> {
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
            return Some(asset.browser_download_url.clone());
        }
    }
    None
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
    // Same major.minor.patch: stable (no pre-release) is newer than pre-release
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
    fn parses_github_latest_release_json() {
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
            }
            other => panic!("expected UpdateAvailable, got {other:?}"),
        }

        let up_to_date = parse_github_release(sample, "0.3.0").unwrap();
        assert_eq!(up_to_date, UpdateCheckOutcome::UpToDate("0.3.0".to_string()));
    }
}
