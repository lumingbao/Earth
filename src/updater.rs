use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::fs;
use std::io::Read;
#[cfg(target_os = "macos")]
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::Sender;
use std::time::{SystemTime, UNIX_EPOCH};

mod http;
use http::download_to;

pub const GITHUB_LATEST_API: &str = "https://api.github.com/repos/lumingbao/Earth/releases/latest";
pub const GITHUB_RELEASES_PAGE: &str = "https://github.com/lumingbao/Earth/releases";
pub const CURRENT_VERSION: &str = env!("CARGO_PKG_VERSION");
const RELEASE_DOWNLOAD_PREFIX: &str = "https://github.com/lumingbao/Earth/releases/download/";
const MAX_METADATA_BYTES: u64 = 2 * 1024 * 1024;
const MAX_ARCHIVE_BYTES: u64 = 512 * 1024 * 1024;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReleaseInfo {
    pub version: String,
    pub tag_name: String,
    pub html_url: String,
    pub download_url: Option<String>,
    pub asset_name: Option<String>,
    pub checksum_url: Option<String>,
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
    let prerelease_channel = parse_semver(CURRENT_VERSION).is_some_and(|v| !v.pre.is_empty());
    let api = if prerelease_channel {
        "https://api.github.com/repos/lumingbao/Earth/releases?per_page=100"
    } else {
        GITHUB_LATEST_API
    };
    let mut body = Vec::new();
    let result = download_to(api, &mut body, MAX_METADATA_BYTES).and_then(|()| {
        let body = std::str::from_utf8(&body).map_err(|_| "更新服务返回无效文本".to_string())?;
        if prerelease_channel {
            parse_github_release_list(body, CURRENT_VERSION)
        } else {
            parse_github_release(body, CURRENT_VERSION)
        }
    });
    result.unwrap_or_else(UpdateCheckOutcome::Error)
}

// The trust root is compiled into the installed application, never learned from a release.
fn trusted_publisher() -> Result<String, String> {
    #[cfg(target_os = "macos")]
    let value = option_env!("EARTH_UPDATE_MACOS_TEAM_ID").unwrap_or("");
    #[cfg(target_os = "windows")]
    let value = option_env!("EARTH_UPDATE_WINDOWS_CERT_SHA256").unwrap_or("");
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    let value = "";
    if valid_publisher_pin(value) {
        Ok(value.to_string())
    } else {
        Err(crate::i18n::t("updater.unconfigured_publisher").into())
    }
}

fn valid_publisher_pin(value: &str) -> bool {
    #[cfg(target_os = "macos")]
    return value.len() == 10
        && value
            .bytes()
            .all(|c| c.is_ascii_uppercase() || c.is_ascii_digit());
    #[cfg(target_os = "windows")]
    return value.len() == 64 && value.bytes().all(|c| c.is_ascii_hexdigit());
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    {
        let _ = value;
        false
    }
}

fn safe_asset_name(name: &str) -> bool {
    !name.is_empty()
        && !name.starts_with('.')
        && name.len() <= 200
        && name
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, b'.' | b'-' | b'_'))
}

fn trusted_asset_url(tag: &str, name: &str, url: &str) -> bool {
    safe_asset_name(name)
        && parse_semver(tag).is_some()
        && url == format!("{RELEASE_DOWNLOAD_PREFIX}{tag}/{name}")
}

fn expected_checksum(manifest: &str, file_name: &str) -> Result<String, String> {
    let mut expected = None;
    for line in manifest.lines().filter(|line| !line.trim().is_empty()) {
        let Some(hash) = line.get(..64) else {
            return Err("SHA256SUMS 格式无效".into());
        };
        let separator = line.get(64..66);
        let name = line.get(66..).unwrap_or("");
        if !hash.bytes().all(|c| c.is_ascii_hexdigit())
            || !matches!(separator, Some("  " | " *"))
            || !safe_asset_name(name)
        {
            return Err("SHA256SUMS 格式无效".into());
        }
        if name == file_name && expected.replace(hash.to_ascii_lowercase()).is_some() {
            return Err("SHA256SUMS 包含重复的安装包校验值".into());
        }
    }
    expected.ok_or_else(|| "SHA256SUMS 缺少当前安装包的校验值".into())
}

fn digest_hex(bytes: &[u8]) -> String {
    use std::fmt::Write as _;
    let mut output = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        let _ = write!(&mut output, "{byte:02x}");
    }
    output
}

fn verify_checksum(path: &Path, expected: &str) -> Result<(), String> {
    let mut file = fs::File::open(path).map_err(|e| format!("读取更新包失败: {e}"))?;
    let mut hasher = Sha256::new();
    let mut buffer = [0u8; 65536];
    loop {
        let len = file
            .read(&mut buffer)
            .map_err(|e| format!("校验更新包失败: {e}"))?;
        if len == 0 {
            break;
        }
        hasher.update(&buffer[..len]);
    }
    let actual = digest_hex(&hasher.finalize());
    if actual != expected {
        return Err("安装包 SHA-256 校验失败，已拒绝安装，请重新下载".into());
    }
    Ok(())
}

struct StageDirectory {
    path: PathBuf,
    keep: bool,
}

#[cfg(not(target_os = "windows"))]
fn create_private_stage_dir(path: &Path) -> std::io::Result<()> {
    let mut builder = fs::DirBuilder::new();
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt;
        builder.mode(0o700);
    }
    builder.create(path)
}

#[cfg(target_os = "windows")]
fn create_private_stage_dir(path: &Path) -> std::io::Result<()> {
    use std::os::windows::ffi::OsStrExt;
    use std::ptr::null_mut;
    use windows_sys::Win32::Foundation::LocalFree;
    use windows_sys::Win32::Security::Authorization::{
        ConvertStringSecurityDescriptorToSecurityDescriptorW, SDDL_REVISION_1,
    };
    use windows_sys::Win32::Security::SECURITY_ATTRIBUTES;
    use windows_sys::Win32::Storage::FileSystem::CreateDirectoryW;

    // Protected DACL: only the owner and SYSTEM, inherited by files/subdirectories.
    // Apply it atomically at creation even if TEMP points to a shared directory.
    let sddl: Vec<u16> = "D:P(A;OICI;FA;;;OW)(A;OICI;FA;;;SY)"
        .encode_utf16()
        .chain(Some(0))
        .collect();
    let mut descriptor = null_mut();
    if unsafe {
        ConvertStringSecurityDescriptorToSecurityDescriptorW(
            sddl.as_ptr(),
            SDDL_REVISION_1,
            &mut descriptor,
            null_mut(),
        )
    } == 0
    {
        return Err(std::io::Error::last_os_error());
    }
    let attributes = SECURITY_ATTRIBUTES {
        nLength: std::mem::size_of::<SECURITY_ATTRIBUTES>() as u32,
        lpSecurityDescriptor: descriptor,
        bInheritHandle: 0,
    };
    let path: Vec<u16> = path.as_os_str().encode_wide().chain(Some(0)).collect();
    let result = if unsafe { CreateDirectoryW(path.as_ptr(), &attributes) } == 0 {
        Err(std::io::Error::last_os_error())
    } else {
        Ok(())
    };
    unsafe { LocalFree(descriptor) };
    result
}

impl StageDirectory {
    fn new() -> Result<Self, String> {
        static SEQUENCE: AtomicU64 = AtomicU64::new(0);
        let time = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|e| format!("读取系统时间失败: {e}"))?
            .as_nanos();
        for _ in 0..32 {
            let path = std::env::temp_dir().join(format!(
                "earth-update-{}-{time}-{}",
                std::process::id(),
                SEQUENCE.fetch_add(1, Ordering::Relaxed)
            ));
            match create_private_stage_dir(&path) {
                Ok(()) => return Ok(Self { path, keep: false }),
                Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
                Err(e) => return Err(format!("创建私有更新目录失败: {e}")),
            }
        }
        Err("无法创建唯一更新目录".into())
    }
}
impl Drop for StageDirectory {
    fn drop(&mut self) {
        if !self.keep {
            #[cfg(target_os = "macos")]
            {
                use std::os::unix::fs::MetadataExt;
                if let (Ok(root), Ok(mount)) = (
                    fs::metadata(&self.path),
                    fs::metadata(self.path.join("mnt")),
                ) && root.dev() != mount.dev()
                {
                    // If both detach attempts failed, never traverse the mounted image.
                    return;
                }
            }
            let _ = fs::remove_dir_all(&self.path);
        }
    }
}

fn download_and_stage_release(info: &ReleaseInfo) -> Result<PathBuf, String> {
    let publisher = trusted_publisher()?;
    let file_name = info.asset_name.as_deref().ok_or("该版本仅支持手动下载")?;
    let url = info.download_url.as_deref().ok_or("该版本仅支持手动下载")?;
    let checksum_url = info
        .checksum_url
        .as_deref()
        .ok_or("发布缺少 SHA256SUMS.txt，无法自动更新")?;
    if !trusted_asset_url(&info.tag_name, file_name, url)
        || !trusted_asset_url(&info.tag_name, "SHA256SUMS.txt", checksum_url)
        || !supported_installer_name(file_name, &info.version)
        || normalize_version(&info.tag_name) != info.version
        || !is_newer_version(&info.version, CURRENT_VERSION)
    {
        return Err("更新包来源、名称或版本不符合可信发布规则".into());
    }
    let mut manifest = Vec::new();
    download_to(checksum_url, &mut manifest, MAX_METADATA_BYTES)?;
    let manifest = std::str::from_utf8(&manifest).map_err(|_| "SHA256SUMS 不是有效文本")?;
    let expected = expected_checksum(manifest, file_name)?;
    let mut stage = StageDirectory::new()?;
    let archive = stage.path.join(file_name);
    let mut file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&archive)
        .map_err(|e| format!("创建更新包文件失败: {e}"))?;
    download_to(url, &mut file, MAX_ARCHIVE_BYTES)?;
    file.sync_all()
        .map_err(|e| format!("保存更新包失败: {e}"))?;
    drop(file);
    verify_checksum(&archive, &expected)?;
    let staged_path = stage_verified_archive(&archive, &stage.path, &publisher, &info.version)?;
    stage.keep = true;
    Ok(staged_path)
}

#[cfg(target_os = "macos")]
fn macos_requirement(team_id: &str) -> String {
    format!(
        "anchor apple generic and identifier \"io.earth.Earth\" and certificate 1[field.1.2.840.113635.100.6.2.6] exists and certificate leaf[field.1.2.840.113635.100.6.1.13] exists and certificate leaf[subject.OU] = \"{team_id}\""
    )
}

#[cfg(target_os = "macos")]
fn verify_macos_app(path: &Path, publisher: &str) -> Result<(), String> {
    if fs::symlink_metadata(path)
        .map_err(|e| e.to_string())?
        .file_type()
        .is_symlink()
        || !path.join("Contents/MacOS/earth").is_file()
    {
        return Err("更新包不是有效的 Earth.app".into());
    }
    for (program, args) in [
        (
            "/usr/bin/codesign",
            vec![
                "--verify",
                "--deep",
                "--strict",
                "--all-architectures",
                "--test-requirement",
            ],
        ),
        ("/usr/sbin/spctl", vec!["--assess", "--type", "execute"]),
    ] {
        let mut command = Command::new(program);
        command.args(args);
        if program.ends_with("codesign") {
            command.arg(macos_requirement(publisher));
        }
        let output = command
            .arg(path)
            .output()
            .map_err(|e| format!("无法校验发布者: {e}"))?;
        if !output.status.success() {
            return Err("安装包签名、发布者身份或 Apple 公证校验失败，请前往发布页手动检查".into());
        }
    }
    Ok(())
}

#[cfg(target_os = "macos")]
fn macos_app_version(path: &Path) -> Result<String, String> {
    for key in ["EarthVersion", "CFBundleShortVersionString"] {
        let output = Command::new("/usr/libexec/PlistBuddy")
            .args(["-c", &format!("Print :{key}")])
            .arg(path.join("Contents/Info.plist"))
            .output()
            .map_err(|e| format!("读取应用版本失败: {e}"))?;
        if output.status.success() {
            let version = String::from_utf8(output.stdout).map_err(|_| "应用版本信息无效")?;
            let version = version.trim().to_string();
            parse_semver(&version).ok_or("应用版本信息无效")?;
            return Ok(version);
        }
    }
    Err("应用缺少版本信息".into())
}

#[cfg(target_os = "macos")]
fn stage_verified_archive(
    archive: &Path,
    root: &Path,
    publisher: &str,
    version: &str,
) -> Result<PathBuf, String> {
    let mount = root.join("mnt");
    fs::create_dir(&mount).map_err(|e| e.to_string())?;
    let status = Command::new("/usr/bin/hdiutil")
        .args(["attach", "-nobrowse", "-readonly", "-mountpoint"])
        .arg(&mount)
        .arg(archive)
        .status()
        .map_err(|e| format!("挂载更新镜像失败: {e}"))?;
    if !status.success() {
        return Err("挂载 DMG 更新镜像失败".into());
    }
    let result = (|| {
        let app = mount.join("Earth.app");
        verify_macos_app(&app, publisher)?;
        if macos_app_version(&app)? != version {
            return Err("签名应用版本与发布版本不一致".into());
        }
        let staged = root.join("Earth.app");
        let status = Command::new("/usr/bin/ditto")
            .arg(&app)
            .arg(&staged)
            .status()
            .map_err(|e| e.to_string())?;
        if !status.success() {
            return Err("解包 Earth.app 失败".into());
        }
        verify_macos_app(&staged, publisher)?;
        Ok(staged)
    })();
    let detached = Command::new("/usr/bin/hdiutil")
        .arg("detach")
        .arg(&mount)
        .status();
    if !detached.is_ok_and(|status| status.success()) {
        // Do not recursively remove a still-mounted image on error.
        let _ = Command::new("/usr/bin/hdiutil")
            .args(["detach", "-force"])
            .arg(&mount)
            .status();
    }
    result
}

#[cfg(target_os = "windows")]
fn verify_windows_installer(path: &Path, publisher: &str) -> Result<String, String> {
    use std::os::windows::process::CommandExt;
    // Get-AuthenticodeSignature validates Authenticode through Windows trust APIs. Its
    // Valid status alone is insufficient: pin the leaf certificate's SHA-256 as well.
    const SCRIPT: &str = r#"$ErrorActionPreference = 'Stop'
$sig = Get-AuthenticodeSignature -LiteralPath $env:EARTH_VERIFY_PATH
if ($sig.Status -ne 'Valid' -or $null -eq $sig.SignerCertificate) { exit 1 }
$sha = [System.Security.Cryptography.SHA256]::Create()
try { $hash = [BitConverter]::ToString($sha.ComputeHash($sig.SignerCertificate.RawData)).Replace('-', '') } finally { $sha.Dispose() }
if ($hash -cne $env:EARTH_VERIFY_PUBLISHER) { exit 1 }
$version = [System.Diagnostics.FileVersionInfo]::GetVersionInfo($env:EARTH_VERIFY_PATH)
if ($version.ProductName -cne 'Earth') { exit 1 }
[Console]::Write($version.ProductVersion)
"#;
    let powershell =
        PathBuf::from(std::env::var_os("SystemRoot").ok_or("无法定位 Windows 系统目录")?)
            .join("System32/WindowsPowerShell/v1.0/powershell.exe");
    let output = Command::new(powershell)
        .args(["-NoProfile", "-NonInteractive", "-Command", SCRIPT])
        .env("EARTH_VERIFY_PATH", path)
        .env("EARTH_VERIFY_PUBLISHER", publisher.to_ascii_uppercase())
        .creation_flags(0x0800_0000)
        .output()
        .map_err(|e| format!("无法校验 Windows 安装包: {e}"))?;
    if !output.status.success() {
        return Err("安装包 Authenticode 签名或发布者证书不可信，已拒绝安装".into());
    }
    let version = String::from_utf8(output.stdout).map_err(|_| "安装器版本信息无效")?;
    if !is_newer_version(&version, CURRENT_VERSION) {
        return Err("安装器版本未高于当前版本，已拒绝覆盖".into());
    }
    Ok(version)
}

#[cfg(target_os = "windows")]
fn stage_verified_archive(
    archive: &Path,
    _root: &Path,
    publisher: &str,
    version: &str,
) -> Result<PathBuf, String> {
    if verify_windows_installer(archive, publisher)? != version {
        return Err("签名安装器版本与发布版本不一致".into());
    }
    Ok(archive.to_path_buf())
}

#[cfg(not(any(target_os = "macos", target_os = "windows")))]
fn stage_verified_archive(
    _archive: &Path,
    _root: &Path,
    _publisher: &str,
    _version: &str,
) -> Result<PathBuf, String> {
    Err("当前平台仅支持手动更新".into())
}

/// Recheck publisher identity at the execution boundary, including after the UI has
/// held a staged path for an arbitrary amount of time.
pub fn apply_staged_update_and_restart(staged_path: &Path) -> Result<(), String> {
    let publisher = trusted_publisher()?;
    #[cfg(target_os = "macos")]
    {
        if staged_path.file_name().and_then(|s| s.to_str()) != Some("Earth.app") {
            return Err("无效的更新应用路径".into());
        }
        verify_macos_app(staged_path, &publisher)?;
        let version = macos_app_version(staged_path)?;
        if !is_newer_version(&version, CURRENT_VERSION) {
            return Err("更新版本未高于当前版本，已拒绝覆盖".into());
        }
        let exe = std::env::current_exe().map_err(|e| e.to_string())?;
        let target = find_bundle_from_exe_path(&exe)
            .ok_or("当前运行方式不支持替换应用，请从发布页手动安装")?;
        verify_macos_app(&target, &publisher)?;
        let root = staged_path.parent().ok_or("无效的更新暂存目录")?;
        let script = root.join("apply_update.sh");
        let mut file = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&script)
            .map_err(|e| format!("创建更新脚本失败: {e}"))?;
        file.write_all(build_macos_restart_script().as_bytes())
            .map_err(|e| e.to_string())?;
        let log = fs::OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(root.join("install.log"))
            .map_err(|e| e.to_string())?;
        let stderr = log.try_clone().map_err(|e| e.to_string())?;
        Command::new("/bin/sh")
            .arg(&script)
            .arg(std::process::id().to_string())
            .arg(staged_path)
            .arg(target)
            .arg(macos_requirement(&publisher))
            .arg(version)
            .stdout(log)
            .stderr(stderr)
            .spawn()
            .map_err(|e| format!("启动更新脚本失败: {e}"))?;
        Ok(())
    }
    #[cfg(target_os = "windows")]
    {
        if staged_path.extension().and_then(|s| s.to_str()) != Some("exe") {
            return Err("自动更新仅支持已签名的 EXE 安装器，请手动下载其他格式".into());
        }
        verify_windows_installer(staged_path, &publisher)?;
        // No command interpreter: paths containing spaces or shell metacharacters remain data.
        Command::new(staged_path)
            .spawn()
            .map_err(|e| format!("启动安装程序失败: {e}"))?;
        Ok(())
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    {
        let _ = (staged_path, publisher);
        Err("当前平台仅支持手动更新".into())
    }
}

#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
pub fn find_bundle_from_exe_path(exe_path: &Path) -> Option<PathBuf> {
    exe_path
        .ancestors()
        .find(|path| path.extension().and_then(|e| e.to_str()) == Some("app"))
        .map(Path::to_path_buf)
}

#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
pub fn build_macos_restart_script() -> &'static str {
    r#"#!/bin/sh
set -eu
PATH=/usr/bin:/bin:/usr/sbin:/sbin
export PATH
OLD_PID="$1"
STAGED_APP="$2"
TARGET_APP="$3"
REQUIREMENT="$4"
EXPECTED_VERSION="$5"
NEW_APP="${TARGET_APP}.new.$$"
BACKUP_APP="${TARGET_APP}.bak.$$"
fail() { printf '%s\n' "$1" >&2; exit 1; }
verify() {
    /usr/bin/codesign --verify --deep --strict --all-architectures --test-requirement "$REQUIREMENT" "$1" &&
    /usr/sbin/spctl --assess --type execute "$1"
}
verify_version() {
    version=$(/usr/libexec/PlistBuddy -c 'Print :EarthVersion' "$1/Contents/Info.plist" 2>/dev/null ||
        /usr/libexec/PlistBuddy -c 'Print :CFBundleShortVersionString' "$1/Contents/Info.plist") || return 1
    [ "$version" = "$EXPECTED_VERSION" ]
}
i=0
while kill -0 "$OLD_PID" 2>/dev/null; do
    [ "$i" -lt 300 ] || fail 'Earth did not exit within 30 seconds; installation cancelled. Please exit Earth and retry.'
    sleep 0.1
    i=$((i + 1))
done
[ ! -e "$NEW_APP" ] && [ ! -L "$NEW_APP" ] || fail 'Replacement path already exists.'
[ ! -e "$BACKUP_APP" ] && [ ! -L "$BACKUP_APP" ] || fail 'A previous backup exists; inspect it before retrying.'
[ ! -L "$TARGET_APP" ] && [ ! -L "$STAGED_APP" ] || fail 'Application paths must not be symlinks.'
verify "$STAGED_APP" || fail 'Staged application failed publisher verification.'
verify_version "$STAGED_APP" || fail 'Staged application version changed; installation cancelled.'
/usr/bin/ditto "$STAGED_APP" "$NEW_APP" || fail 'Could not prepare replacement; original application is intact.'
verify "$NEW_APP" || fail 'Replacement failed publisher verification; original application is intact.'
verify_version "$NEW_APP" || fail 'Replacement version changed; original application is intact.'
/bin/mv "$TARGET_APP" "$BACKUP_APP" || fail 'Could not preserve the original application.'
if ! /bin/mv "$NEW_APP" "$TARGET_APP"; then
    /bin/mv "$BACKUP_APP" "$TARGET_APP" || fail 'Rollback failed; original application is preserved at .bak.'
    fail 'Replacement failed; original application restored.'
fi
if ! verify "$TARGET_APP" || ! /usr/bin/open "$TARGET_APP"; then
    /bin/mv "$TARGET_APP" "$NEW_APP" && /bin/mv "$BACKUP_APP" "$TARGET_APP" || fail 'Rollback failed; inspect .bak before retrying.'
    fail 'Restart failed; original application restored.'
fi
# Keep the backup and install.log for recovery. Never strip quarantine attributes.
printf '%s\n' 'Update installed. Previous application is preserved at .bak.'
"#
}

pub fn parse_github_release(
    json_str: &str,
    current_version: &str,
) -> Result<UpdateCheckOutcome, String> {
    let dto: GithubReleaseDto =
        serde_json::from_str(json_str).map_err(|e| format!("解析版本信息失败: {e}"))?;
    release_outcome(dto, current_version, trusted_publisher().is_ok())
}

fn parse_github_release_list(
    json_str: &str,
    current_version: &str,
) -> Result<UpdateCheckOutcome, String> {
    let releases: Vec<GithubReleaseDto> =
        serde_json::from_str(json_str).map_err(|e| format!("解析版本列表失败: {e}"))?;
    let newest = releases
        .into_iter()
        .filter(|release| !release.draft)
        .filter_map(|release| parse_semver(&release.tag_name).map(|version| (version, release)))
        .max_by(|(a, _), (b, _)| a.cmp_precedence(b))
        .map(|(_, release)| release);
    match newest {
        Some(release) => release_outcome(release, current_version, trusted_publisher().is_ok()),
        None => Err("未找到有效的发布版本，请前往 Releases 页面查看".into()),
    }
}

fn release_outcome(
    dto: GithubReleaseDto,
    current_version: &str,
    publisher_configured: bool,
) -> Result<UpdateCheckOutcome, String> {
    let current = parse_semver(current_version).ok_or("当前版本号格式无效")?;
    if dto.draft || (dto.prerelease && current.pre.is_empty()) {
        return Ok(UpdateCheckOutcome::UpToDate(current_version.to_string()));
    }
    let remote = parse_semver(&dto.tag_name).ok_or("远端 Release 缺少有效版本号")?;
    if (!remote.pre.is_empty() && current.pre.is_empty()) || remote.cmp_precedence(&current).is_le()
    {
        return Ok(UpdateCheckOutcome::UpToDate(current_version.to_string()));
    }
    let version = normalize_version(&dto.tag_name);
    let checksum = dto.assets.iter().find(|a| {
        a.name == "SHA256SUMS.txt"
            && trusted_asset_url(&dto.tag_name, &a.name, &a.browser_download_url)
    });
    let asset = dto.assets.iter().find(|a| {
        supported_installer_name(&a.name, &version)
            && trusted_asset_url(&dto.tag_name, &a.name, &a.browser_download_url)
    });
    let automatic = publisher_configured && checksum.is_some() && asset.is_some();
    Ok(UpdateCheckOutcome::UpdateAvailable(ReleaseInfo {
        version,
        html_url: format!("{GITHUB_RELEASES_PAGE}/tag/{}", dto.tag_name),
        tag_name: dto.tag_name,
        download_url: automatic.then(|| asset.unwrap().browser_download_url.clone()),
        asset_name: automatic.then(|| asset.unwrap().name.clone()),
        checksum_url: automatic.then(|| checksum.unwrap().browser_download_url.clone()),
    }))
}

fn supported_installer_name(name: &str, version: &str) -> bool {
    #[cfg(target_os = "macos")]
    {
        name == format!("Earth-{version}-macOS-universal.dmg")
    }
    #[cfg(target_os = "windows")]
    {
        name == format!("Earth-{version}-Windows-x64-Setup.exe")
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    {
        let _ = (name, version);
        false
    }
}

pub fn normalize_version(raw: &str) -> String {
    let trimmed = raw.trim();
    trimmed
        .strip_prefix('v')
        .or_else(|| trimmed.strip_prefix('V'))
        .unwrap_or(trimmed)
        .to_string()
}

pub fn is_newer_version(remote: &str, current: &str) -> bool {
    match (parse_semver(remote), parse_semver(current)) {
        (Some(remote), Some(current)) => remote.cmp_precedence(&current).is_gt(),
        _ => false,
    }
}

fn parse_semver(version: &str) -> Option<semver::Version> {
    semver::Version::parse(&normalize_version(version)).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn semver_precedence_and_invalid_versions() {
        for (new, old) in [
            ("v0.3.0", "0.2.0"),
            ("0.5.0-beta.2", "0.5.0-beta.1"),
            ("0.5.0-beta.10", "0.5.0-beta.9"),
            ("0.5.0-rc.1", "0.5.0-beta.2"),
            ("0.5.0", "0.5.0-rc.1"),
        ] {
            assert!(is_newer_version(new, old));
        }
        assert!(!is_newer_version("1.0.0+build2", "1.0.0+build1"));
        assert!(!is_newer_version("0.5.0-beta.2", "0.5.0"));
        for invalid in ["", "1.0.0.2", "1.0.0-", "../1.0.0", "01.0.0"] {
            assert!(parse_semver(invalid).is_none());
        }
    }

    #[test]
    fn checksum_manifest_requires_an_exact_unique_name_and_valid_digest() {
        let digest = "a".repeat(64);
        let valid = format!("{digest}  Earth.dmg\n");
        assert_eq!(expected_checksum(&valid, "Earth.dmg").unwrap(), digest);
        assert!(expected_checksum(&valid, "Other.dmg").is_err());
        assert!(expected_checksum(&(valid.clone() + &valid), "Earth.dmg").is_err());
        assert!(expected_checksum("invalid  Earth.dmg", "Earth.dmg").is_err());
        assert!(!safe_asset_name("../Earth.dmg"));
        assert!(!safe_asset_name("Earth&Other.exe"));
        assert!(!trusted_asset_url(
            "v1.0.0",
            "Earth.dmg",
            "http://github.com/lumingbao/Earth/releases/download/v1.0.0/Earth.dmg"
        ));
    }

    #[test]
    fn checksum_accepts_unchanged_data_and_rejects_changed_data() {
        let stage = StageDirectory::new().unwrap();
        let file = stage.path.join("fixture.bin");
        fs::write(&file, b"ordinary update fixture").unwrap();
        let digest = digest_hex(&Sha256::digest(b"ordinary update fixture"));
        verify_checksum(&file, &digest).unwrap();
        fs::write(&file, b"different fixture").unwrap();
        assert!(verify_checksum(&file, &digest).is_err());
    }

    #[test]
    fn unsuccessful_staging_removes_private_temporary_files() {
        let path = {
            let stage = StageDirectory::new().unwrap();
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                assert_eq!(
                    fs::metadata(&stage.path).unwrap().permissions().mode() & 0o777,
                    0o700
                );
            }
            fs::write(stage.path.join("incomplete.bin"), b"partial download").unwrap();
            stage.path.clone()
        };
        assert!(!path.exists());
    }

    #[cfg(any(target_os = "macos", target_os = "windows"))]
    #[test]
    fn trusted_release_requires_matching_platform_asset_and_manifest() {
        #[cfg(target_os = "macos")]
        let file_name = "Earth-9.0.0-macOS-universal.dmg";
        #[cfg(target_os = "windows")]
        let file_name = "Earth-9.0.0-Windows-x64-Setup.exe";
        let release = |include_manifest: bool, configured: bool| {
            let mut assets = vec![GithubAssetDto {
                name: file_name.into(),
                browser_download_url: format!("{RELEASE_DOWNLOAD_PREFIX}v9.0.0/{file_name}"),
            }];
            if include_manifest {
                assets.push(GithubAssetDto {
                    name: "SHA256SUMS.txt".into(),
                    browser_download_url: format!("{RELEASE_DOWNLOAD_PREFIX}v9.0.0/SHA256SUMS.txt"),
                });
            }
            let dto = GithubReleaseDto {
                tag_name: "v9.0.0".into(),
                draft: false,
                prerelease: false,
                assets,
            };
            let UpdateCheckOutcome::UpdateAvailable(info) =
                release_outcome(dto, "1.0.0", configured).unwrap()
            else {
                panic!("valid newer releases remain discoverable");
            };
            info
        };
        let automatic = release(true, true);
        assert_eq!(automatic.asset_name.as_deref(), Some(file_name));
        assert!(automatic.download_url.is_some() && automatic.checksum_url.is_some());
        assert!(release(false, true).download_url.is_none());
        assert!(release(true, false).download_url.is_none());
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn macos_bundle_retains_full_semver_and_rejects_unsigned_content() {
        let stage = StageDirectory::new().unwrap();
        let app = stage.path.join("Earth.app");
        fs::create_dir_all(app.join("Contents/MacOS")).unwrap();
        fs::write(
            app.join("Contents/Info.plist"),
            br#"<?xml version="1.0" encoding="UTF-8"?>
<plist version="1.0"><dict>
<key>CFBundleShortVersionString</key><string>0.5.0</string>
<key>EarthVersion</key><string>0.5.0-beta.2+build1</string>
</dict></plist>"#,
        )
        .unwrap();
        fs::write(
            app.join("Contents/MacOS/earth"),
            b"non-executable test fixture",
        )
        .unwrap();
        assert_eq!(macos_app_version(&app).unwrap(), "0.5.0-beta.2+build1");
        assert!(verify_macos_app(&app, "ABCDEFGHIJ").is_err());
    }

    #[test]
    fn unconfigured_build_keeps_manual_download_and_disables_install() {
        let json = r#"{"tag_name":"v9.0.0","assets":[]}"#;
        let dto = serde_json::from_str(json).unwrap();
        let UpdateCheckOutcome::UpdateAvailable(release) =
            release_outcome(dto, "1.0.0", false).unwrap()
        else {
            panic!()
        };
        assert_eq!(
            release.html_url,
            "https://github.com/lumingbao/Earth/releases/tag/v9.0.0"
        );
        assert!(release.download_url.is_none());
        assert!(!valid_publisher_pin(""));
        assert!(!valid_publisher_pin("invalid publisher"));
    }

    #[test]
    fn prerelease_channel_selects_newest_and_stable_channel_excludes_it() {
        let json = r#"[{"tag_name":"v0.5.0-beta.2","prerelease":true},{"tag_name":"v0.5.0-beta.10","prerelease":true}]"#;
        let UpdateCheckOutcome::UpdateAvailable(release) =
            parse_github_release_list(json, "0.5.0-beta.1").unwrap()
        else {
            panic!()
        };
        assert_eq!(release.version, "0.5.0-beta.10");
        assert!(matches!(
            parse_github_release(
                r#"{"tag_name":"v0.5.0-beta.10","prerelease":true}"#,
                "0.4.0"
            )
            .unwrap(),
            UpdateCheckOutcome::UpToDate(_)
        ));
    }

    #[test]
    fn http_missing_release_and_rate_limit_are_errors() {
        assert!(http::check_status(404).unwrap_err().contains("未找到"));
        assert!(http::check_status(429).unwrap_err().contains("频率"));
    }

    #[test]
    fn finds_only_the_running_bundle() {
        assert_eq!(
            find_bundle_from_exe_path(Path::new("/Applications/Earth.app/Contents/MacOS/earth")),
            Some(PathBuf::from("/Applications/Earth.app"))
        );
        assert_eq!(find_bundle_from_exe_path(Path::new("/tmp/earth")), None);
    }
}
