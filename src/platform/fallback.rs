use crate::model::ActiveAppInfo;
use crate::platform::ScreenRect;
use std::path::Path;

pub fn capture_frontmost_app() -> Option<ActiveAppInfo> {
    None
}

pub fn restore_frontmost_app(_app: &ActiveAppInfo) -> bool {
    false
}

pub fn simulate_paste() -> Result<(), String> {
    Err("当前平台暂不支持自动模拟粘贴".to_string())
}

pub fn has_accessibility_permission() -> bool {
    true
}

pub fn open_accessibility_settings() {}

pub fn configure_macos_accessory_mode() {}

pub fn activate_self_window() {}

pub fn hide_self_app() {}

pub fn get_cursor_and_monitor_bounds(_dpi_scale: f32) -> ((f32, f32), ScreenRect) {
    (
        (400.0, 300.0),
        ScreenRect {
            x: 0.0,
            y: 0.0,
            width: 1440.0,
            height: 900.0,
        },
    )
}

pub fn reveal_in_file_manager(path: &Path) {
    let target = path.parent().unwrap_or(path);
    let _ = std::process::Command::new("xdg-open").arg(target).spawn();
}

pub fn open_url(url: &str) {
    let _ = std::process::Command::new("xdg-open").arg(url).spawn();
}
