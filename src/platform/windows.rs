use crate::model::ActiveAppInfo;
use crate::platform::{PreferredSystemFonts, ScreenRect, SystemFontCandidate};
use enigo::{Direction, Enigo, Key, Keyboard, Settings};
use std::path::{Path, PathBuf};
use std::process::Command;
use windows_sys::Win32::Foundation::{CloseHandle, HWND, POINT, RECT};
use windows_sys::Win32::Graphics::Gdi::{
    GetMonitorInfoW, MonitorFromPoint, MONITORINFO, MONITOR_DEFAULTTONEAREST,
};
use windows_sys::Win32::System::Threading::{
    OpenProcess, QueryFullProcessImageNameW, PROCESS_QUERY_LIMITED_INFORMATION,
};
use windows_sys::Win32::UI::HiDpi::GetDpiForSystem;
use windows_sys::Win32::UI::WindowsAndMessaging::{
    GetCursorPos, GetForegroundWindow, GetWindowRect, GetWindowThreadProcessId,
    SetForegroundWindow,
};

pub fn capture_frontmost_app_identity() -> Option<ActiveAppInfo> {
    capture_frontmost_app()
}

pub fn capture_frontmost_app() -> Option<ActiveAppInfo> {
    unsafe {
        let hwnd = GetForegroundWindow();
        if hwnd.is_null() {
            return None;
        }
        let mut pid: u32 = 0;
        GetWindowThreadProcessId(hwnd, &mut pid);
        if pid == 0 || pid == std::process::id() {
            return None;
        }

        let mut exe_name = String::new();
        let handle = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
        if !handle.is_null() {
            let mut buf = [0u16; 1024];
            let mut size = buf.len() as u32;
            if QueryFullProcessImageNameW(handle, 0, buf.as_mut_ptr(), &mut size) != 0 {
                let full_path = String::from_utf16_lossy(&buf[..size as usize]);
                exe_name = Path::new(&full_path)
                    .file_name()
                    .and_then(|n| n.to_str())
                    .unwrap_or(&full_path)
                    .to_string();
            }
            CloseHandle(handle);
        }

        let app_name = exe_name
            .strip_suffix(".exe")
            .unwrap_or(&exe_name)
            .to_string();

        let mut rect = RECT {
            left: 0,
            top: 0,
            right: 0,
            bottom: 0,
        };
        let scale = system_dpi_scale();
        let window_rect = if GetWindowRect(hwnd, &mut rect) != 0 {
            let w = (rect.right - rect.left) as f32 / scale;
            let h = (rect.bottom - rect.top) as f32 / scale;
            if w > 10.0 && h > 10.0 {
                Some((rect.left as f32 / scale, rect.top as f32 / scale, w, h))
            } else {
                None
            }
        } else {
            None
        };

        Some(ActiveAppInfo {
            app_name,
            process_or_bundle: exe_name,
            pid: pid as u64,
            window_rect,
            window_handle: Some(hwnd as isize),
        })
    }
}

pub fn restore_frontmost_app(app: &ActiveAppInfo) -> bool {
    let Some(raw) = app.window_handle else {
        return false;
    };
    let hwnd = raw as HWND;
    if hwnd.is_null() {
        return false;
    }
    unsafe { SetForegroundWindow(hwnd) != 0 }
}

pub fn simulate_paste() -> Result<(), String> {
    std::thread::sleep(std::time::Duration::from_millis(55));
    let mut enigo =
        Enigo::new(&Settings::default()).map_err(|e| format!("初始化输入模拟失败: {e}"))?;
    enigo
        .key(Key::Control, Direction::Press)
        .map_err(|e| format!("模拟 Ctrl 按下失败: {e}"))?;
    enigo
        .key(Key::V, Direction::Click)
        .map_err(|e| format!("模拟 V 键失败: {e}"))?;
    enigo
        .key(Key::Control, Direction::Release)
        .map_err(|e| format!("模拟 Ctrl 释放失败: {e}"))?;
    Ok(())
}

pub fn has_accessibility_permission() -> bool {
    true
}

pub fn open_accessibility_settings() {}

pub fn configure_macos_accessory_mode() {}

pub fn activate_self_window() {}

pub fn hide_self_app() {}

fn system_dpi_scale() -> f32 {
    let dpi = unsafe { GetDpiForSystem() };
    if dpi == 0 {
        1.0
    } else {
        dpi as f32 / 96.0
    }
}

fn resolve_dpi_scale(dpi_scale: f32) -> f32 {
    if dpi_scale > 0.1 {
        dpi_scale
    } else {
        system_dpi_scale()
    }
}

fn monitor_bounds_at_physical_point(pt: POINT, scale: f32) -> ScreenRect {
    unsafe {
        let hmon = MonitorFromPoint(pt, MONITOR_DEFAULTTONEAREST);
        let mut info = MONITORINFO {
            cbSize: std::mem::size_of::<MONITORINFO>() as u32,
            rcMonitor: RECT {
                left: 0,
                top: 0,
                right: 1920,
                bottom: 1080,
            },
            rcWork: RECT {
                left: 0,
                top: 0,
                right: 1920,
                bottom: 1040,
            },
            dwFlags: 0,
        };
        if !hmon.is_null() {
            GetMonitorInfoW(hmon, &mut info);
        }
        let work = info.rcWork;
        ScreenRect {
            x: work.left as f32 / scale,
            y: work.top as f32 / scale,
            width: (work.right - work.left) as f32 / scale,
            height: (work.bottom - work.top) as f32 / scale,
        }
    }
}

pub fn get_cursor_and_monitor_bounds(dpi_scale: f32) -> ((f32, f32), ScreenRect) {
    let scale = resolve_dpi_scale(dpi_scale);
    let mut pt = POINT { x: 0, y: 0 };
    unsafe {
        let _ = GetCursorPos(&mut pt);
    }
    (
        (pt.x as f32 / scale, pt.y as f32 / scale),
        monitor_bounds_at_physical_point(pt, scale),
    )
}

pub fn monitor_bounds_for_point(x: f32, y: f32, dpi_scale: f32) -> ScreenRect {
    let scale = resolve_dpi_scale(dpi_scale);
    let pt = POINT {
        x: (x * scale).round() as i32,
        y: (y * scale).round() as i32,
    };
    monitor_bounds_at_physical_point(pt, scale)
}

pub fn configure_main_window(cc: &eframe::CreationContext<'_>) {
    if let Some(window) = cc.winit_window() {
        use winit::platform::windows::{CornerPreference, WindowExtWindows as _};
        // `egui-winit` enables undecorated drop shadows whenever `decorations` is false, which
        // adds a 1px dark top border and a rectangular DWM frame shadow outside our 18px rounded
        // corners and around empty space during workbench animations.
        window.set_undecorated_shadow(false);
        window.set_corner_preference(CornerPreference::DoNotRound);
    }
}

pub fn invalidate_window_shadow() {}

pub fn set_window_resize_anchor_right(_anchor_right: bool) {}

pub fn set_window_pinned(_pinned: bool) {}

pub fn reveal_in_file_manager(path: &Path) {
    let arg = format!("/select,{}", path.display());
    let _ = Command::new("explorer.exe").arg(arg).spawn();
}

pub fn open_url(url: &str) {
    let _ = Command::new("cmd").args(["/C", "start", "", url]).spawn();
}

pub fn preferred_system_fonts() -> PreferredSystemFonts {
    let windir = std::env::var_os("WINDIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(r"C:\Windows"));
    let fonts_dir = windir.join("Fonts");

    let mut ui = Vec::new();
    for (file, ps) in [
        ("segoeui.ttf", "SegoeUI"),
        ("msyh.ttc", "MicrosoftYaHei"),
    ] {
        let p = fonts_dir.join(file);
        if p.is_file() {
            ui.push(SystemFontCandidate {
                path: p,
                postscript_name: ps,
            });
        }
    }

    let mut monospace = Vec::new();
    let consolas = fonts_dir.join("consola.ttf");
    if consolas.is_file() {
        monospace.push(SystemFontCandidate {
            path: consolas,
            postscript_name: "Consolas",
        });
    }

    PreferredSystemFonts { ui, monospace }
}

pub fn autostart_entry_needs_refresh(expected_app_path: &str, autostart_arg: &str) -> bool {
    let expected_cmd = format!("{expected_app_path} {autostart_arg}");
    let subkey: Vec<u16> = r"SOFTWARE\Microsoft\Windows\CurrentVersion\Run"
        .encode_utf16()
        .chain(std::iter::once(0))
        .collect();
    let value_name: Vec<u16> = "Earth"
        .encode_utf16()
        .chain(std::iter::once(0))
        .collect();

    for root in [
        windows_sys::Win32::System::Registry::HKEY_CURRENT_USER,
        windows_sys::Win32::System::Registry::HKEY_LOCAL_MACHINE,
    ] {
        if let Some(current) = read_reg_sz(root, &subkey, &value_name) {
            return current.trim() != expected_cmd;
        }
    }
    false
}

fn read_reg_sz(
    root: windows_sys::Win32::System::Registry::HKEY,
    subkey: &[u16],
    value_name: &[u16],
) -> Option<String> {
    use windows_sys::Win32::Foundation::ERROR_SUCCESS;
    use windows_sys::Win32::System::Registry::{RegGetValueW, RRF_RT_REG_SZ};

    let mut byte_len: u32 = 0;
    let status = unsafe {
        RegGetValueW(
            root,
            subkey.as_ptr(),
            value_name.as_ptr(),
            RRF_RT_REG_SZ,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            &mut byte_len,
        )
    };
    if status != ERROR_SUCCESS || byte_len < 2 {
        return None;
    }

    let mut buf = vec![0u16; (byte_len as usize).div_ceil(2)];
    let status = unsafe {
        RegGetValueW(
            root,
            subkey.as_ptr(),
            value_name.as_ptr(),
            RRF_RT_REG_SZ,
            std::ptr::null_mut(),
            buf.as_mut_ptr().cast(),
            &mut byte_len,
        )
    };
    if status != ERROR_SUCCESS {
        return None;
    }

    let len = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
    Some(String::from_utf16_lossy(&buf[..len]))
}

static SHORTCUT_RECORDING: std::sync::atomic::AtomicBool =
    std::sync::atomic::AtomicBool::new(false);
static SHORTCUT_ALLOW_SUPER: std::sync::atomic::AtomicBool =
    std::sync::atomic::AtomicBool::new(false);
static SHORTCUT_EVENTS: std::sync::OnceLock<
    std::sync::Mutex<std::collections::VecDeque<crate::platform::NativeShortcutEvent>>,
> = std::sync::OnceLock::new();
static SHORTCUT_REPAINT_CTX: std::sync::OnceLock<std::sync::Mutex<Option<egui::Context>>> =
    std::sync::OnceLock::new();

pub fn install_shortcut_repaint_ctx(ctx: &egui::Context) {
    let slot = SHORTCUT_REPAINT_CTX.get_or_init(|| std::sync::Mutex::new(None));
    if let Ok(mut guard) = slot.lock() {
        *guard = Some(ctx.clone());
    }
}

pub fn set_shortcut_recording(active: bool, allow_super: bool) {
    SHORTCUT_ALLOW_SUPER.store(allow_super, std::sync::atomic::Ordering::Release);
    SHORTCUT_RECORDING.store(active, std::sync::atomic::Ordering::Release);
    if !active && let Some(queue) = SHORTCUT_EVENTS.get()
        && let Ok(mut guard) = queue.lock()
    {
        guard.clear();
    }
}

pub fn poll_native_shortcut_event() -> Option<crate::platform::NativeShortcutEvent> {
    let queue = SHORTCUT_EVENTS.get()?;
    queue.lock().ok()?.pop_front()
}

pub fn native_recording_modifiers() -> Option<(bool, bool, bool, bool)> {
    if !SHORTCUT_RECORDING.load(std::sync::atomic::Ordering::Acquire) {
        return None;
    }
    let (ctrl, alt, shift, super_raw) = read_async_modifiers();
    let allow_super = SHORTCUT_ALLOW_SUPER.load(std::sync::atomic::Ordering::Acquire);
    Some((ctrl, alt, shift, super_raw && allow_super))
}

fn push_shortcut_event(ev: crate::platform::NativeShortcutEvent) {
    let queue =
        SHORTCUT_EVENTS.get_or_init(|| std::sync::Mutex::new(std::collections::VecDeque::new()));
    if let Ok(mut guard) = queue.lock() {
        guard.push_back(ev);
    }
    request_shortcut_repaint();
}

fn request_shortcut_repaint() {
    if let Some(slot) = SHORTCUT_REPAINT_CTX.get()
        && let Ok(guard) = slot.lock()
        && let Some(ctx) = guard.as_ref()
    {
        ctx.request_repaint();
    }
}

fn is_vk_down(vk: u16) -> bool {
    use windows_sys::Win32::UI::Input::KeyboardAndMouse::GetAsyncKeyState;
    (unsafe { GetAsyncKeyState(i32::from(vk)) } as u16 & 0x8000) != 0
}

fn read_async_modifiers() -> (bool, bool, bool, bool) {
    use windows_sys::Win32::UI::Input::KeyboardAndMouse::{
        VK_CONTROL, VK_LWIN, VK_MENU, VK_RWIN, VK_SHIFT,
    };
    (
        is_vk_down(VK_CONTROL),
        is_vk_down(VK_MENU),
        is_vk_down(VK_SHIFT),
        is_vk_down(VK_LWIN) || is_vk_down(VK_RWIN),
    )
}

fn is_modifier_vk(vk: u16) -> bool {
    use windows_sys::Win32::UI::Input::KeyboardAndMouse::{
        VK_CONTROL, VK_LCONTROL, VK_LMENU, VK_LSHIFT, VK_LWIN, VK_MENU, VK_RCONTROL, VK_RMENU,
        VK_RSHIFT, VK_RWIN, VK_SHIFT,
    };
    matches!(
        vk,
        VK_SHIFT
            | VK_LSHIFT
            | VK_RSHIFT
            | VK_CONTROL
            | VK_LCONTROL
            | VK_RCONTROL
            | VK_MENU
            | VK_LMENU
            | VK_RMENU
            | VK_LWIN
            | VK_RWIN
    )
}

#[allow(clippy::not_unsafe_ptr_arg_deref)]
pub fn handle_windows_msg_hook(msg_ptr: *const core::ffi::c_void) -> bool {
    use windows_sys::Win32::UI::Input::KeyboardAndMouse::{
        MapVirtualKeyW, MAPVK_VSC_TO_VK_EX, VK_ESCAPE, VK_PROCESSKEY,
    };
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        MSG, WM_KEYDOWN, WM_KEYUP, WM_SYSKEYDOWN, WM_SYSKEYUP,
    };

    if !SHORTCUT_RECORDING.load(std::sync::atomic::Ordering::Acquire) || msg_ptr.is_null() {
        return false;
    }

    let msg = unsafe { &*msg_ptr.cast::<MSG>() };
    match msg.message {
        WM_KEYDOWN | WM_SYSKEYDOWN => {
            let mut vk = (msg.wParam & 0xffff) as u16;
            if vk == VK_PROCESSKEY {
                let scancode = ((msg.lParam as usize >> 16) & 0xff) as u32;
                let extended = (msg.lParam as usize & (1usize << 24)) != 0;
                let ext_scancode = scancode | if extended { 0xe000 } else { 0 };
                let resolved = unsafe { MapVirtualKeyW(ext_scancode, MAPVK_VSC_TO_VK_EX) } as u16;
                if resolved != 0 {
                    vk = resolved;
                } else {
                    let fallback = unsafe { MapVirtualKeyW(scancode, MAPVK_VSC_TO_VK_EX) } as u16;
                    if fallback != 0 {
                        vk = fallback;
                    }
                }
            }

            if vk == VK_ESCAPE {
                push_shortcut_event(crate::platform::NativeShortcutEvent::Cancelled);
                return true;
            }

            if is_modifier_vk(vk) {
                request_shortcut_repaint();
                return false;
            }

            let is_repeat = (msg.lParam as usize & (1usize << 30)) != 0;
            if is_repeat {
                return true;
            }

            if let Some(token) = crate::hotkey::win32_vk_to_token(vk) {
                let (ctrl, alt, shift, super_raw) = read_async_modifiers();
                let allow_super = SHORTCUT_ALLOW_SUPER.load(std::sync::atomic::Ordering::Acquire);
                let super_key = super_raw && allow_super;
                if let Some(combo) =
                    crate::hotkey::assemble_shortcut_combo(ctrl, alt, shift, super_key, token)
                {
                    push_shortcut_event(crate::platform::NativeShortcutEvent::Captured(combo));
                } else {
                    push_shortcut_event(crate::platform::NativeShortcutEvent::NeedsModifier);
                }
                return true;
            }
            false
        }
        WM_KEYUP | WM_SYSKEYUP => {
            request_shortcut_repaint();
            false
        }
        _ => false,
    }
}
