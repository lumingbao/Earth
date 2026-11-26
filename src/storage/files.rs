use std::fs::{self, File, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

static NEXT_TEMP: AtomicU64 = AtomicU64::new(0);

pub(super) fn private_dir(path: &Path) -> io::Result<()> {
    let mut builder = fs::DirBuilder::new();
    builder.recursive(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt;
        builder.mode(0o700);
    }
    builder.create(path)?;
    let metadata = fs::symlink_metadata(path)?;
    if !metadata.is_dir() || metadata.file_type().is_symlink() {
        return Err(io::Error::other(
            "data directory must be an ordinary directory",
        ));
    }
    restrict_permissions(path, &metadata, 0o700)
}

pub(super) fn private_file(path: &Path) -> io::Result<()> {
    let metadata = fs::symlink_metadata(path)?;
    if !metadata.is_file() || metadata.file_type().is_symlink() {
        return Err(io::Error::other("data file must be an ordinary file"));
    }
    restrict_permissions(path, &metadata, 0o600)
}

fn restrict_permissions(path: &Path, metadata: &fs::Metadata, mode: u32) -> io::Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if metadata.permissions().mode() & 0o777 != mode {
            fs::set_permissions(path, fs::Permissions::from_mode(mode))?;
            eprintln!("Earth: tightened local data permissions to {mode:o}");
        }
    }
    #[cfg(not(unix))]
    let _ = (path, metadata, mode); // Windows inherits the user's application-data ACL.
    Ok(())
}

pub(super) fn create_private_file(path: &Path) -> io::Result<File> {
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    options.open(path)
}

pub(super) fn ensure_private_file(path: &Path) -> io::Result<()> {
    match create_private_file(path) {
        Ok(_) => Ok(()),
        Err(err) if err.kind() == io::ErrorKind::AlreadyExists => private_file(path),
        Err(err) => Err(err),
    }
}

pub(super) fn temporary_path(parent: &Path, stem: &str) -> PathBuf {
    let counter = NEXT_TEMP.fetch_add(1, Ordering::Relaxed);
    parent.join(format!(".{stem}.{}.{}.tmp", std::process::id(), counter))
}

/// Publish a complete file only after its bytes have reached disk. The old file
/// stays intact if serialization, writing, syncing, or replacement fails.
pub(super) fn atomic_private_write(path: &Path, bytes: &[u8]) -> io::Result<()> {
    let parent = path
        .parent()
        .ok_or_else(|| io::Error::other("missing parent directory"))?;
    let temp = temporary_path(parent, "earth-write");
    let result = (|| {
        if path.try_exists()? {
            private_file(path)?;
        } else if fs::symlink_metadata(path).is_ok() {
            return Err(io::Error::other("destination is not an ordinary file"));
        }
        let mut file = create_private_file(&temp)?;
        file.write_all(bytes)?;
        file.sync_all()?;
        drop(file);
        replace_file(&temp, path)?;
        #[cfg(unix)]
        File::open(parent)?.sync_all()?;
        Ok(())
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temp);
    }
    result
}

#[cfg(not(target_os = "windows"))]
fn replace_file(source: &Path, destination: &Path) -> io::Result<()> {
    fs::rename(source, destination)
}

#[cfg(target_os = "windows")]
fn replace_file(source: &Path, destination: &Path) -> io::Result<()> {
    use std::os::windows::ffi::OsStrExt;
    // MoveFileExW replaces the destination without the delete/rename gap.
    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn MoveFileExW(existing: *const u16, new: *const u16, flags: u32) -> i32;
    }
    let source: Vec<u16> = source.as_os_str().encode_wide().chain(Some(0)).collect();
    let destination: Vec<u16> = destination
        .as_os_str()
        .encode_wide()
        .chain(Some(0))
        .collect();
    if unsafe { MoveFileExW(source.as_ptr(), destination.as_ptr(), 0x1 | 0x8) } == 0 {
        Err(io::Error::last_os_error())
    } else {
        Ok(())
    }
}

/// Database paths and imported legacy paths never grant authority outside the
/// dedicated media directory. Symlinks, nested paths and non-PNG files are not
/// owned media and are never read, chmod'ed, exported or deleted.
pub(super) fn managed_media_path(directory: &Path, raw: &str) -> Option<PathBuf> {
    let path = Path::new(raw);
    if path.extension()?.to_str()? != "png" {
        return None;
    }
    let metadata = fs::symlink_metadata(path).ok()?;
    if !metadata.is_file() || metadata.file_type().is_symlink() {
        return None;
    }
    if path.parent() == Some(directory)
        && matches!(path.file_name(), Some(name) if name != "." && name != "..")
    {
        return Some(path.to_path_buf());
    }
    let root = directory.canonicalize().ok()?;
    let resolved = path.canonicalize().ok()?;
    (resolved.parent() == Some(root.as_path())).then_some(resolved)
}

/// Tracks newly created media until the corresponding SQLite transaction commits.
#[derive(Default)]
pub(super) struct PendingMedia(Vec<PathBuf>);

impl PendingMedia {
    pub(super) fn write(
        &mut self,
        directory: &Path,
        bytes: &[u8],
        suffix: &str,
    ) -> Result<String, String> {
        let name = format!("{}{suffix}.png", super::sha256_hex(bytes));
        let path = directory.join(name);
        match create_private_file(&path) {
            Ok(mut file) => {
                self.0.push(path.clone());
                file.write_all(bytes)
                    .and_then(|_| file.sync_all())
                    .map_err(|e| format!("保存图片失败: {e}"))?;
                #[cfg(unix)]
                File::open(directory)
                    .and_then(|dir| dir.sync_all())
                    .map_err(|e| format!("同步图片目录失败: {e}"))?;
            }
            Err(err) if err.kind() == io::ErrorKind::AlreadyExists => {
                let owned = managed_media_path(directory, &path.to_string_lossy())
                    .ok_or_else(|| "图片目标不是受管理的普通文件".to_string())?;
                private_file(&owned).map_err(|e| format!("保护图片权限失败: {e}"))?;
                if fs::read(&owned).map_err(|e| format!("读取已有图片失败: {e}"))? != bytes
                {
                    return Err("已有图片内容与其文件名不一致，已停止写入".to_string());
                }
            }
            Err(err) => return Err(format!("创建图片失败: {err}")),
        }
        Ok(path.to_string_lossy().into_owned())
    }

    pub(super) fn commit(mut self) {
        self.0.clear();
    }
}

impl Drop for PendingMedia {
    fn drop(&mut self) {
        for path in &self.0 {
            let _ = fs::remove_file(path);
        }
    }
}
