use crate::model::ActiveAppInfo;
use crate::platform::{PreferredSystemFonts, ScreenRect, SystemFontCandidate};
use std::path::{Path, PathBuf};

pub fn capture_frontmost_app_identity() -> Option<ActiveAppInfo> {
    None
}

pub fn capture_frontmost_app() -> Option<ActiveAppInfo> {
    None
}

pub fn restore_frontmost_app(_app: &ActiveAppInfo) -> bool {
    false
}

pub fn simulate_paste(_target: &ActiveAppInfo) -> Result<(), String> {
    Err("当前平台暂不支持自动模拟粘贴".to_string())
}

pub fn has_accessibility_permission() -> bool {
    true
}

pub fn open_accessibility_settings() {}

pub fn configure_macos_accessory_mode() {}

pub fn configure_main_window(_cc: &eframe::CreationContext<'_>) {}

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

pub fn monitor_bounds_for_point(_x: f32, _y: f32, _dpi_scale: f32) -> ScreenRect {
    ScreenRect {
        x: 0.0,
        y: 0.0,
        width: 1440.0,
        height: 900.0,
    }
}

pub fn invalidate_window_shadow() {}

pub fn set_window_resize_anchor_right(_anchor_right: bool) {}

pub fn set_window_pinned(_pinned: bool) {}

pub fn reveal_in_file_manager(path: &Path) {
    let target = path.parent().unwrap_or(path);
    let _ = std::process::Command::new("xdg-open").arg(target).spawn();
}

pub fn open_url(url: &str) {
    let _ = std::process::Command::new("xdg-open").arg(url).spawn();
}

pub fn preferred_system_fonts() -> PreferredSystemFonts {
    let mut ui = Vec::new();
    for (path, ps) in [
        (
            "/usr/share/fonts/opentype/noto/NotoSansCJK-Regular.ttc",
            "NotoSansCJKsc-Regular",
        ),
        (
            "/usr/share/fonts/noto-cjk/NotoSansCJK-Regular.ttc",
            "NotoSansCJKsc-Regular",
        ),
        (
            "/usr/share/fonts/truetype/wqy/wqy-microhei.ttc",
            "WenQuanYiMicroHei",
        ),
    ] {
        let p = PathBuf::from(path);
        if p.is_file() {
            ui.push(SystemFontCandidate {
                path: p,
                postscript_name: ps,
            });
            break;
        }
    }
    PreferredSystemFonts {
        ui,
        monospace: Vec::new(),
    }
}

pub fn autostart_entry_needs_refresh(expected_app_path: &str, autostart_arg: &str) -> bool {
    let Some(base_dirs) = directories::BaseDirs::new() else {
        return false;
    };
    let desktop_path = base_dirs.home_dir().join(".config/autostart/Earth.desktop");
    let Ok(contents) = std::fs::read_to_string(&desktop_path) else {
        return false;
    };
    !contents.contains(expected_app_path) || !contents.contains(autostart_arg)
}

pub fn install_shortcut_repaint_ctx(_ctx: &egui::Context) {}

pub fn install_system_theme_observer(_ctx: &egui::Context) {}

pub fn system_is_dark() -> Option<bool> {
    None
}

pub fn set_shortcut_recording(_active: bool, _allow_super: bool) {}

pub fn poll_native_shortcut_event() -> Option<crate::platform::NativeShortcutEvent> {
    None
}

pub fn native_recording_modifiers() -> Option<(bool, bool, bool, bool)> {
    None
}
