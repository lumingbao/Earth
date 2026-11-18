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
