use crate::model::ActiveAppInfo;
use crate::platform::ScreenRect;
use enigo::{Direction, Enigo, Key, Keyboard, Settings};
use objc2::MainThreadMarker;
use objc2_app_kit::{
    NSApplication, NSApplicationActivationOptions, NSApplicationActivationPolicy, NSEvent,
    NSRunningApplication, NSScreen, NSWorkspace,
};
use std::path::Path;
use std::process::Command;

#[link(name = "ApplicationServices", kind = "framework")]
unsafe extern "C" {
    fn AXIsProcessTrusted() -> bool;
}

pub fn capture_frontmost_app() -> Option<ActiveAppInfo> {
    let self_pid = std::process::id() as i32;
    let workspace = NSWorkspace::sharedWorkspace();
    let app = workspace.frontmostApplication()?;
    let pid = app.processIdentifier();
    if pid == self_pid {
        return None;
    }

    let app_name = app
        .localizedName()
        .map(|s| s.to_string())
        .unwrap_or_default();
    let bundle_id = app
        .bundleIdentifier()
        .map(|s| s.to_string())
        .unwrap_or_else(|| app_name.clone());

    let window_rect = active_win_pos_rs::get_position().ok().and_then(|pos| {
        if pos.width > 10.0 && pos.height > 10.0 {
            Some((pos.x as f32, pos.y as f32, pos.width as f32, pos.height as f32))
        } else {
            None
        }
    });

    Some(ActiveAppInfo {
        app_name,
        process_or_bundle: bundle_id,
        pid: pid as u64,
        window_rect,
        window_handle: None,
    })
}

pub fn restore_frontmost_app(app: &ActiveAppInfo) -> bool {
    let pid = app.pid as i32;
    if pid <= 0 {
        return false;
    }
    if let Some(running) = NSRunningApplication::runningApplicationWithProcessIdentifier(pid) {
        #[allow(deprecated)]
        let opts = NSApplicationActivationOptions::ActivateIgnoringOtherApps;
        return running.activateWithOptions(opts);
    }
    false
}

pub fn simulate_paste() -> Result<(), String> {
    if !has_accessibility_permission() {
        return Err("需要「辅助功能」权限才能自动粘贴（已复制，请按 ⌘V 粘贴）".to_string());
    }
    std::thread::sleep(std::time::Duration::from_millis(75));
    let mut enigo =
        Enigo::new(&Settings::default()).map_err(|e| format!("初始化输入模拟失败: {e}"))?;
    enigo
        .key(Key::Meta, Direction::Press)
        .map_err(|e| format!("模拟 Cmd 按下失败: {e}"))?;
    enigo
        .key(Key::Unicode('v'), Direction::Click)
        .map_err(|e| format!("模拟 V 键失败: {e}"))?;
    enigo
        .key(Key::Meta, Direction::Release)
        .map_err(|e| format!("模拟 Cmd 释放失败: {e}"))?;
    Ok(())
}

pub fn has_accessibility_permission() -> bool {
    unsafe { AXIsProcessTrusted() }
}

pub fn open_accessibility_settings() {
    let _ = Command::new("open")
        .arg("x-apple.systempreferences:com.apple.preference.security?Privacy_Accessibility")
        .spawn();
}

pub fn configure_macos_accessory_mode() {
    if let Some(mtm) = MainThreadMarker::new() {
        let app = NSApplication::sharedApplication(mtm);
        app.setActivationPolicy(NSApplicationActivationPolicy::Accessory);
    }
}

pub fn activate_self_window() {
    if let Some(mtm) = MainThreadMarker::new() {
        let app = NSApplication::sharedApplication(mtm);
        #[allow(deprecated)]
        app.activateIgnoringOtherApps(true);
    }
}

pub fn hide_self_app() {
    if let Some(mtm) = MainThreadMarker::new() {
        let app = NSApplication::sharedApplication(mtm);
        app.hide(None);
    }
}

pub fn get_cursor_and_monitor_bounds(_dpi_scale: f32) -> ((f32, f32), ScreenRect) {
    let fallback_rect = ScreenRect {
        x: 0.0,
        y: 0.0,
        width: 1440.0,
        height: 900.0,
    };

    let mouse = NSEvent::mouseLocation();
    let Some(mtm) = MainThreadMarker::new() else {
        return ((mouse.x as f32, mouse.y as f32), fallback_rect);
    };

    let screens = NSScreen::screens(mtm);
    if screens.is_empty() {
        return ((mouse.x as f32, mouse.y as f32), fallback_rect);
    }

    let primary_height = screens.objectAtIndex(0).frame().size.height;
    let cursor_x = mouse.x as f32;
    let cursor_y = (primary_height - mouse.y) as f32;

    for i in 0..screens.len() {
        let screen = screens.objectAtIndex(i);
        let frame = screen.frame();
        let in_x = mouse.x >= frame.origin.x && mouse.x <= frame.origin.x + frame.size.width;
        let in_y = mouse.y >= frame.origin.y && mouse.y <= frame.origin.y + frame.size.height;
        if in_x && in_y {
            let vis = screen.visibleFrame();
            let rect = ScreenRect {
                x: vis.origin.x as f32,
                y: (primary_height - (vis.origin.y + vis.size.height)) as f32,
                width: vis.size.width as f32,
                height: vis.size.height as f32,
            };
            return ((cursor_x, cursor_y), rect);
        }
    }

    let vis = screens.objectAtIndex(0).visibleFrame();
    let rect = ScreenRect {
        x: vis.origin.x as f32,
        y: (primary_height - (vis.origin.y + vis.size.height)) as f32,
        width: vis.size.width as f32,
        height: vis.size.height as f32,
    };
    ((cursor_x, cursor_y), rect)
}

pub fn reveal_in_file_manager(path: &Path) {
    let _ = Command::new("open").arg("-R").arg(path).spawn();
}

pub fn open_url(url: &str) {
    let _ = Command::new("open").arg(url).spawn();
}
