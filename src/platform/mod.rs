use crate::model::{ActiveAppInfo, PopupPosition};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone)]
pub struct SystemFontCandidate {
    pub path: PathBuf,
    pub postscript_name: &'static str,
}

#[derive(Debug, Clone, Default)]
pub struct PreferredSystemFonts {
    pub ui: Vec<SystemFontCandidate>,
    pub monospace: Vec<SystemFontCandidate>,
}

#[cfg(target_os = "macos")]
mod macos;
#[cfg(target_os = "windows")]
mod windows;
#[cfg(not(any(target_os = "macos", target_os = "windows")))]
mod fallback;

#[cfg(target_os = "macos")]
use macos as imp;
#[cfg(target_os = "windows")]
use windows as imp;
#[cfg(not(any(target_os = "macos", target_os = "windows")))]
use fallback as imp;

#[derive(Debug, Clone, Copy)]
pub struct ScreenRect {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
}

pub fn capture_frontmost_app() -> Option<ActiveAppInfo> {
    imp::capture_frontmost_app()
}

pub fn capture_frontmost_app_identity() -> Option<ActiveAppInfo> {
    imp::capture_frontmost_app_identity()
}

pub fn restore_frontmost_app(app: &ActiveAppInfo) -> bool {
    imp::restore_frontmost_app(app)
}

pub fn simulate_paste() -> Result<(), String> {
    imp::simulate_paste()
}

pub fn has_accessibility_permission() -> bool {
    imp::has_accessibility_permission()
}

pub fn open_accessibility_settings() {
    imp::open_accessibility_settings();
}

pub fn configure_macos_accessory_mode() {
    imp::configure_macos_accessory_mode();
}

pub fn configure_main_window(cc: &eframe::CreationContext<'_>) {
    imp::configure_main_window(cc);
}

pub fn activate_self_window() {
    imp::activate_self_window();
}

pub fn hide_self_app() {
    imp::hide_self_app();
}

pub fn get_cursor_and_monitor_bounds(dpi_scale: f32) -> ((f32, f32), ScreenRect) {
    imp::get_cursor_and_monitor_bounds(dpi_scale)
}

pub fn monitor_bounds_for_point(x: f32, y: f32, dpi_scale: f32) -> ScreenRect {
    imp::monitor_bounds_for_point(x, y, dpi_scale)
}

pub fn invalidate_window_shadow() {
    imp::invalidate_window_shadow();
}

pub fn set_window_resize_anchor_right(anchor_right: bool) {
    imp::set_window_resize_anchor_right(anchor_right);
}

pub fn set_window_pinned(pinned: bool) {
    imp::set_window_pinned(pinned);
}

pub fn reveal_in_file_manager(path: &Path) {
    imp::reveal_in_file_manager(path);
}

pub fn open_url(url: &str) {
    imp::open_url(url);
}

pub fn preferred_system_fonts() -> PreferredSystemFonts {
    imp::preferred_system_fonts()
}

pub const AUTOSTART_ARG: &str = "--autostart";

pub fn should_start_hidden_in_args<'a>(args: impl IntoIterator<Item = &'a str>) -> bool {
    args.into_iter()
        .any(|arg| matches!(arg, "--autostart" | "--hidden" | "--minimized"))
}

pub fn should_start_hidden_from_args() -> bool {
    let args: Vec<String> = std::env::args().skip(1).collect();
    should_start_hidden_in_args(args.iter().map(String::as_str))
}

fn autostart_app_path(exe_path: &Path) -> String {
    let raw = exe_path.to_string_lossy().into_owned();
    #[cfg(target_os = "windows")]
    {
        format_windows_autostart_path(&raw)
    }
    #[cfg(not(target_os = "windows"))]
    {
        raw
    }
}

#[cfg(any(target_os = "windows", test))]
fn format_windows_autostart_path(raw: &str) -> String {
    let trimmed = raw.trim();
    if trimmed.starts_with('"') && trimmed.ends_with('"') && trimmed.len() >= 2 {
        trimmed.to_string()
    } else {
        format!("\"{trimmed}\"")
    }
}

fn build_autostart_manager() -> Result<(auto_launch::AutoLaunch, String), String> {
    let exe_path = std::env::current_exe()
        .map_err(|e| format!("无法获取当前可执行文件路径: {e}"))?;
    let app_path = autostart_app_path(&exe_path);
    let auto = auto_launch::AutoLaunchBuilder::new()
        .set_app_name("Earth")
        .set_app_path(&app_path)
        .set_args(&[AUTOSTART_ARG])
        .build()
        .map_err(|e| format!("初始化开机自启配置失败: {e}"))?;
    Ok((auto, app_path))
}

pub fn sync_autostart(enabled: bool) -> Result<(), String> {
    let (auto, _) = build_autostart_manager()?;

    if enabled {
        auto.enable().map_err(|e| format!("开启开机自启失败: {e}"))?;
    } else if auto.is_enabled().unwrap_or(false) {
        auto.disable().map_err(|e| format!("关闭开机自启失败: {e}"))?;
    }
    Ok(())
}

pub fn refresh_autostart_if_enabled() -> Result<(), String> {
    let (auto, app_path) = build_autostart_manager()?;
    if !auto.is_enabled().unwrap_or(false) {
        return Ok(());
    }
    if imp::autostart_entry_needs_refresh(&app_path, AUTOSTART_ARG) {
        auto.enable().map_err(|e| format!("更新开机自启配置失败: {e}"))?;
    }
    Ok(())
}

pub fn compute_popup_origin(
    mode: PopupPosition,
    window_size: (f32, f32),
    prev_app: Option<&ActiveAppInfo>,
    tray_pos: Option<(f32, f32)>,
    dpi_scale: f32,
) -> (f32, f32) {
    let ((cursor_x, cursor_y), monitor) = get_cursor_and_monitor_bounds(dpi_scale);
    let (win_w, win_h) = window_size;

    let (raw_x, raw_y) = match mode {
        PopupPosition::Cursor => (cursor_x + 8.0, cursor_y + 10.0),
        PopupPosition::ScreenCenter => (
            monitor.x + (monitor.width - win_w) * 0.5,
            monitor.y + (monitor.height - win_h) * 0.42,
        ),
        PopupPosition::WindowCenter => {
            if let Some((wx, wy, ww, wh)) = prev_app.and_then(|a| a.window_rect) {
                (wx + (ww - win_w) * 0.5, wy + (wh - win_h) * 0.5)
            } else {
                (
                    monitor.x + (monitor.width - win_w) * 0.5,
                    monitor.y + (monitor.height - win_h) * 0.42,
                )
            }
        }
        PopupPosition::Tray => {
            if let Some((tx, ty)) = tray_pos {
                #[cfg(target_os = "macos")]
                {
                    (tx - win_w * 0.5, ty + 8.0)
                }
                #[cfg(not(target_os = "macos"))]
                {
                    (tx - win_w * 0.5, ty - win_h - 12.0)
                }
            } else {
                #[cfg(target_os = "macos")]
                {
                    (monitor.x + monitor.width - win_w - 24.0, monitor.y + 36.0)
                }
                #[cfg(not(target_os = "macos"))]
                {
                    (
                        monitor.x + monitor.width - win_w - 24.0,
                        monitor.y + monitor.height - win_h - 24.0,
                    )
                }
            }
        }
    };

    let min_x = monitor.x + 8.0;
    let max_x = (monitor.x + monitor.width - win_w - 8.0).max(min_x);
    let min_y = monitor.y + 8.0;
    let max_y = (monitor.y + monitor.height - win_h - 8.0).max(min_y);

    (raw_x.clamp(min_x, max_x), raw_y.clamp(min_y, max_y))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn should_start_hidden_detects_autostart_flags() {
        assert!(!should_start_hidden_in_args(std::iter::empty()));
        assert!(!should_start_hidden_in_args(["--foo", "bar"]));
        assert!(should_start_hidden_in_args([AUTOSTART_ARG]));
        assert!(should_start_hidden_in_args(["--other", "--hidden"]));
        assert!(should_start_hidden_in_args(["--minimized"]));
    }

    #[test]
    fn windows_autostart_quoting_handles_spaces_without_double_quoting() {
        assert_eq!(
            format_windows_autostart_path(r"C:\Program Files\Earth\Earth.exe"),
            r#""C:\Program Files\Earth\Earth.exe""#
        );
        assert_eq!(
            format_windows_autostart_path(r#""C:\Program Files\Earth\Earth.exe""#),
            r#""C:\Program Files\Earth\Earth.exe""#
        );
    }
}
