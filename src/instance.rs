//! Single-instance ownership and one private, local "show the window" signal.
//! This endpoint accepts no commands, paths, clipboard data, or user CLI inputs.
use std::fs::File;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

pub struct InstanceGuard {
    signal: Arc<native::Signal>,
    pending: Arc<AtomicBool>,
    stopping: Arc<AtomicBool>,
    repaint: Arc<Mutex<Option<egui::Context>>>,
    worker: Option<JoinHandle<()>>,
    #[cfg(target_os = "macos")]
    reopen_handler: Option<objc2::rc::Retained<macos::ReopenHandler>>,
    // Drop ownership last, after closing/removing the native signal endpoint.
    _lock: File,
}

impl InstanceGuard {
    /// `None` means a live primary was notified; no storage should be opened.
    pub fn claim(dir: &Path, lock: File) -> Result<Option<Self>, String> {
        match lock.try_lock() {
            Ok(()) => {}
            Err(std::fs::TryLockError::WouldBlock) => {
                // The primary may have acquired its lock just before binding its endpoint.
                let deadline = Instant::now() + Duration::from_secs(2);
                loop {
                    match native::notify(dir) {
                        Ok(()) => return Ok(None),
                        Err(error) if Instant::now() >= deadline => {
                            return Err(format!(
                                "Earth 已在运行，但无法唤醒窗口：{error}\n请通过托盘或全局快捷键打开；若旧实例无响应，请退出后重试。"
                            ));
                        }
                        Err(_) => std::thread::sleep(Duration::from_millis(40)),
                    }
                }
            }
            Err(std::fs::TryLockError::Error(error)) => {
                return Err(format!("无法锁定数据目录：{error}"));
            }
        }
        let signal =
            Arc::new(native::Signal::bind(dir).map_err(|e| format!("无法创建实例唤醒通道：{e}"))?);
        let pending = Arc::new(AtomicBool::new(false));
        let stopping = Arc::new(AtomicBool::new(false));
        let repaint: Arc<Mutex<Option<egui::Context>>> = Arc::new(Mutex::new(None));
        let worker_signal = Arc::clone(&signal);
        let worker_pending = Arc::clone(&pending);
        let worker_stopping = Arc::clone(&stopping);
        let worker_repaint = Arc::clone(&repaint);
        let worker = std::thread::Builder::new()
            .name("earth-activation".into())
            .spawn(move || {
                while worker_signal.wait().is_ok() {
                    if worker_stopping.load(Ordering::Acquire) {
                        break;
                    }
                    worker_pending.store(true, Ordering::Release);
                    if let Ok(ctx) = worker_repaint.lock()
                        && let Some(ctx) = ctx.as_ref()
                    {
                        ctx.request_repaint();
                    }
                }
            })
            .map_err(|e| format!("无法启动实例唤醒监听：{e}"))?;
        Ok(Some(Self {
            _lock: lock,
            signal,
            pending,
            stopping,
            repaint,
            worker: Some(worker),
            #[cfg(target_os = "macos")]
            reopen_handler: None,
        }))
    }

    pub fn attach_context(&mut self, ctx: egui::Context) -> Result<(), String> {
        if let Ok(mut repaint) = self.repaint.lock() {
            *repaint = Some(ctx.clone());
        }
        if self.pending.load(Ordering::Acquire) {
            ctx.request_repaint();
        }
        #[cfg(target_os = "macos")]
        {
            self.reopen_handler = Some(macos::install(Arc::clone(&self.pending), ctx)?);
        }
        Ok(())
    }

    pub fn take_activation(&self) -> bool {
        self.pending.swap(false, Ordering::AcqRel)
    }
}

impl Drop for InstanceGuard {
    fn drop(&mut self) {
        self.stopping.store(true, Ordering::Release);
        if self.signal.wake().is_ok()
            && let Some(worker) = self.worker.take()
        {
            let _ = worker.join();
        }
        // Remove the endpoint before releasing ownership of the data directory.
        self.signal.cleanup();
        #[cfg(target_os = "macos")]
        if self.reopen_handler.take().is_some()
            && let Some(mtm) = objc2::MainThreadMarker::new()
        {
            macos::uninstall(mtm);
        }
    }
}

#[cfg(target_os = "macos")]
mod macos {
    use super::*;
    use objc2::rc::Retained;
    use objc2::runtime::AnyObject;
    use objc2::{DefinedClass, MainThreadMarker, MainThreadOnly, define_class, msg_send, sel};
    use objc2_foundation::{NSAppleEventManager, NSObject, NSObjectProtocol};

    pub(super) const K_CORE_EVENT_CLASS: u32 = u32::from_be_bytes(*b"aevt");
    pub(super) const K_AE_REOPEN_APPLICATION: u32 = u32::from_be_bytes(*b"rapp");

    pub struct ReopenState {
        pending: Arc<AtomicBool>,
        ctx: egui::Context,
    }
    define_class!(
        #[unsafe(super = NSObject)]
        #[thread_kind = MainThreadOnly]
        #[ivars = ReopenState]
        pub struct ReopenHandler;

        unsafe impl NSObjectProtocol for ReopenHandler {}

        impl ReopenHandler {
            #[unsafe(method(handleReopenEvent:withReplyEvent:))]
            fn handle_reopen(
                &self,
                _event: Option<&AnyObject>,
                _reply: Option<&AnyObject>,
            ) {
                self.ivars().pending.store(true, Ordering::Release);
                self.ivars().ctx.request_repaint();
            }
        }
    );

    pub fn install(
        pending: Arc<AtomicBool>,
        ctx: egui::Context,
    ) -> Result<Retained<ReopenHandler>, String> {
        let mtm = MainThreadMarker::new().ok_or("macOS 窗口唤醒须在主线程初始化")?;
        let handler = ReopenHandler::alloc(mtm).set_ivars(ReopenState { pending, ctx });
        let handler: Retained<ReopenHandler> = unsafe { msg_send![super(handler), init] };
        // Install an Apple Event reopen handler rather than replacing NSApplicationDelegate,
        // because winit 0.30 registers its own ApplicationDelegate and requires keeping it.
        let manager = NSAppleEventManager::sharedAppleEventManager();
        let target: &AnyObject = &handler;
        let _: () = unsafe {
            msg_send![
                &*manager,
                setEventHandler: target,
                andSelector: sel!(handleReopenEvent:withReplyEvent:),
                forEventClass: K_CORE_EVENT_CLASS,
                andEventID: K_AE_REOPEN_APPLICATION
            ]
        };
        Ok(handler)
    }

    pub fn uninstall(_mtm: MainThreadMarker) {
        let manager = NSAppleEventManager::sharedAppleEventManager();
        let _: () = unsafe {
            msg_send![
                &*manager,
                removeEventHandlerForEventClass: K_CORE_EVENT_CLASS,
                andEventID: K_AE_REOPEN_APPLICATION
            ]
        };
    }
}

#[cfg(target_os = "macos")]
mod native {
    use std::fs::{self, File, OpenOptions};
    use std::io;
    use std::os::fd::{AsRawFd, FromRawFd, OwnedFd};
    use std::os::unix::fs::{FileExt, OpenOptionsExt, PermissionsExt};
    use std::path::{Path, PathBuf};
    use std::ptr::{null, null_mut};

    pub struct Signal {
        queue: OwnedFd,
        file: File,
        path: PathBuf,
    }
    impl Signal {
        pub fn bind(dir: &Path) -> io::Result<Self> {
            let path = dir.join("activate.signal");
            let file = OpenOptions::new()
                .read(true)
                .write(true)
                .create(true)
                .truncate(false)
                .mode(0o600)
                .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK)
                .open(&path)?;
            if !file.metadata()?.is_file() {
                return Err(io::Error::other(
                    "activation endpoint is not a regular file",
                ));
            }
            file.set_permissions(fs::Permissions::from_mode(0o600))?;
            // Preserve a pending byte written during startup; a new file extends with zero.
            file.set_len(1)?;
            let fd = unsafe { libc::kqueue() };
            if fd == -1 {
                return Err(io::Error::last_os_error());
            }
            let queue = unsafe { OwnedFd::from_raw_fd(fd) };
            let event = libc::kevent {
                ident: file.as_raw_fd() as libc::uintptr_t,
                filter: libc::EVFILT_VNODE,
                flags: libc::EV_ADD | libc::EV_CLEAR,
                fflags: libc::NOTE_WRITE,
                data: 0,
                udata: null_mut(),
            };
            if unsafe { libc::kevent(queue.as_raw_fd(), &event, 1, null_mut(), 0, null()) } == -1 {
                return Err(io::Error::last_os_error());
            }
            Ok(Self { queue, file, path })
        }
        pub fn wait(&self) -> io::Result<()> {
            loop {
                let mut marker = [0];
                self.file.read_exact_at(&mut marker, 0)?;
                if marker[0] == 1 {
                    self.file.write_all_at(&[0], 0)?;
                    return Ok(());
                }
                let mut event = std::mem::MaybeUninit::<libc::kevent>::uninit();
                let result = unsafe {
                    libc::kevent(
                        self.queue.as_raw_fd(),
                        null(),
                        0,
                        event.as_mut_ptr(),
                        1,
                        null(),
                    )
                };
                if result == -1 {
                    let error = io::Error::last_os_error();
                    if error.kind() != io::ErrorKind::Interrupted {
                        return Err(error);
                    }
                } else if result > 0 {
                    let event = unsafe { event.assume_init() };
                    if event.flags & libc::EV_ERROR != 0 {
                        return Err(io::Error::from_raw_os_error(event.data as i32));
                    }
                }
            }
        }
        pub fn wake(&self) -> io::Result<()> {
            self.file.write_all_at(&[1], 0)
        }
        pub fn cleanup(&self) {
            let _ = fs::remove_file(&self.path);
        }
    }
    impl Drop for Signal {
        fn drop(&mut self) {
            self.cleanup();
        }
    }
    pub fn notify(dir: &Path) -> io::Result<()> {
        let file = OpenOptions::new()
            .write(true)
            .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK)
            .open(dir.join("activate.signal"))?;
        if !file.metadata()?.is_file() {
            return Err(io::Error::other(
                "activation endpoint is not a regular file",
            ));
        }
        file.write_all_at(&[1], 0)
    }
}

#[cfg(not(any(target_os = "macos", target_os = "windows")))]
mod native {
    use std::io;
    use std::path::Path;
    pub struct Signal;
    impl Signal {
        pub fn bind(_dir: &Path) -> io::Result<Self> {
            // Keep the existing fallback usable with the directory lock. There
            // is no native window-activation channel on these platforms.
            Ok(Self)
        }
        pub fn wait(&self) -> io::Result<()> {
            Err(io::Error::other("unsupported platform"))
        }
        pub fn wake(&self) -> io::Result<()> {
            Err(io::Error::other("unsupported platform"))
        }
        pub fn cleanup(&self) {}
    }
    pub fn notify(_dir: &Path) -> io::Result<()> {
        Err(io::Error::other(
            "single-instance activation is supported on macOS and Windows",
        ))
    }
}

#[cfg(windows)]
mod native {
    use sha2::{Digest, Sha256};
    use std::fmt::Write;
    use std::io;
    use std::path::Path;
    use std::ptr::null_mut;
    use windows_sys::Win32::Foundation::{
        CloseHandle, ERROR_ALREADY_EXISTS, GetLastError, HANDLE, LocalFree, WAIT_OBJECT_0,
    };
    use windows_sys::Win32::Security::Authorization::{
        ConvertStringSecurityDescriptorToSecurityDescriptorW, SDDL_REVISION_1,
    };
    use windows_sys::Win32::Security::SECURITY_ATTRIBUTES;
    use windows_sys::Win32::System::Threading::{
        CreateEventW, EVENT_MODIFY_STATE, INFINITE, OpenEventW, SetEvent, WaitForSingleObject,
    };

    fn name(dir: &Path) -> io::Result<Vec<u16>> {
        let dir = dir.canonicalize()?.to_string_lossy().to_lowercase();
        let mut name = "Local\\Earth-Activate-".to_owned();
        for byte in Sha256::digest(dir.as_bytes()) {
            let _ = write!(&mut name, "{byte:02x}");
        }
        Ok(name.encode_utf16().chain(Some(0)).collect())
    }
    pub struct Signal(HANDLE);
    // The event handle remains valid while an Arc is held. Win32 permits cross-thread waits/signals.
    unsafe impl Send for Signal {}
    unsafe impl Sync for Signal {}
    impl Signal {
        pub fn bind(dir: &Path) -> io::Result<Self> {
            let name = name(dir)?;
            let sddl: Vec<u16> = "D:P(A;;GA;;;OW)(A;;GA;;;SY)"
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
                return Err(io::Error::last_os_error());
            }
            let attributes = SECURITY_ATTRIBUTES {
                nLength: std::mem::size_of::<SECURITY_ATTRIBUTES>() as u32,
                lpSecurityDescriptor: descriptor,
                bInheritHandle: 0,
            };
            let handle = unsafe { CreateEventW(&attributes, 0, 0, name.as_ptr()) };
            let error = unsafe { GetLastError() };
            unsafe { LocalFree(descriptor) };
            if handle.is_null() {
                return Err(io::Error::from_raw_os_error(error as i32));
            }
            let signal = Self(handle);
            if error == ERROR_ALREADY_EXISTS {
                return Err(io::Error::other(
                    "activation event already exists without a primary lock",
                ));
            }
            Ok(signal)
        }
        pub fn wait(&self) -> io::Result<()> {
            if unsafe { WaitForSingleObject(self.0, INFINITE) } == WAIT_OBJECT_0 {
                Ok(())
            } else {
                Err(io::Error::last_os_error())
            }
        }
        pub fn wake(&self) -> io::Result<()> {
            if unsafe { SetEvent(self.0) } != 0 {
                Ok(())
            } else {
                Err(io::Error::last_os_error())
            }
        }
        pub fn cleanup(&self) {}
    }
    impl Drop for Signal {
        fn drop(&mut self) {
            unsafe { CloseHandle(self.0) };
        }
    }
    pub fn notify(dir: &Path) -> io::Result<()> {
        let name = name(dir)?;
        let handle = unsafe { OpenEventW(EVENT_MODIFY_STATE, 0, name.as_ptr()) };
        if handle.is_null() {
            return Err(io::Error::last_os_error());
        }
        Signal(handle).wake()
    }
}

#[cfg(all(test, any(target_os = "macos", target_os = "windows")))]
mod tests {
    use super::*;
    use std::fs::{self, OpenOptions};
    use std::sync::atomic::AtomicU64;

    fn temp_dir() -> std::path::PathBuf {
        static SEQUENCE: AtomicU64 = AtomicU64::new(0);
        let base = std::env::temp_dir();
        let dir = base.join(format!(
            "earth-instance-{}-{}",
            std::process::id(),
            SEQUENCE.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&dir).unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&dir, fs::Permissions::from_mode(0o700)).unwrap();
        }
        dir
    }

    fn consume_activation(primary: &InstanceGuard) {
        let deadline = Instant::now() + Duration::from_secs(2);
        while !primary.pending.load(Ordering::Acquire) && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(5));
        }
        assert!(primary.take_activation());
        assert!(!primary.take_activation());
    }

    #[test]
    fn second_launch_activates_primary_and_released_lock_can_be_reclaimed() {
        let dir = temp_dir();
        let lock = || {
            OpenOptions::new()
                .read(true)
                .write(true)
                .create(true)
                .truncate(false)
                .open(dir.join("instance.lock"))
                .unwrap()
        };
        let primary = InstanceGuard::claim(&dir, lock()).unwrap().unwrap();
        assert!(!primary.take_activation());
        assert!(InstanceGuard::claim(&dir, lock()).unwrap().is_none());
        consume_activation(&primary);
        drop(primary);
        let replacement = InstanceGuard::claim(&dir, lock()).unwrap().unwrap();
        assert!(!replacement.take_activation());
        drop(replacement);
        fs::remove_dir_all(dir).unwrap();
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn macos_activation_handles_long_paths_and_repeated_notifications() {
        use std::os::unix::fs::PermissionsExt;

        let base = temp_dir();
        let dir = base.join("long-data-directory-".repeat(8));
        fs::create_dir(&dir).unwrap();
        let lock = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(dir.join("instance.lock"))
            .unwrap();
        let primary = InstanceGuard::claim(&dir, lock).unwrap().unwrap();
        let endpoint = dir.join("activate.signal");
        let metadata = fs::metadata(&endpoint).unwrap();
        assert_eq!(metadata.len(), 1);
        assert_eq!(metadata.permissions().mode() & 0o777, 0o600);
        for _ in 0..16 {
            native::notify(&dir).unwrap();
            consume_activation(&primary);
        }
        drop(primary);
        assert!(!endpoint.exists());
        fs::remove_dir_all(base).unwrap();
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn macos_activation_preserves_startup_signal_and_rejects_symlinks() {
        use std::os::unix::fs::symlink;

        let dir = temp_dir();
        let endpoint = dir.join("activate.signal");
        // A second process can write after the primary takes its lock and before
        // its listener starts waiting.
        fs::write(&endpoint, [1]).unwrap();
        let lock = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(dir.join("instance.lock"))
            .unwrap();
        let primary = InstanceGuard::claim(&dir, lock).unwrap().unwrap();
        consume_activation(&primary);
        assert_eq!(fs::read(&endpoint).unwrap(), [0]);
        drop(primary);
        let target = dir.join("unrelated");
        fs::write(&target, b"unchanged").unwrap();
        symlink(&target, &endpoint).unwrap();
        assert!(native::Signal::bind(&dir).is_err());
        assert!(native::notify(&dir).is_err());
        assert_eq!(fs::read(&target).unwrap(), b"unchanged");
        fs::remove_dir_all(dir).unwrap();
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn macos_reopen_handler_registers_expected_selector_and_fourcc() {
        use objc2::ClassType;

        assert_eq!(macos::K_CORE_EVENT_CLASS, 0x6165_7674);
        assert_eq!(macos::K_AE_REOPEN_APPLICATION, 0x7261_7070);
        assert!(
            macos::ReopenHandler::class()
                .instance_method(objc2::sel!(handleReopenEvent:withReplyEvent:))
                .is_some()
        );
    }
}
