use crate::model::ActiveAppInfo;
use crate::platform::ScreenRect;
use objc2::MainThreadMarker;
use objc2_app_kit::{
    NSApplication, NSApplicationActivationOptions, NSApplicationActivationPolicy, NSEvent,
    NSRunningApplication, NSScreen, NSWorkspace,
};
use std::ffi::c_void;
use std::path::Path;
use std::process::Command;

type CGEventSourceRef = *mut c_void;
type CGEventRef = *mut c_void;
type CGEventSourceStateID = i32;
type CGEventTapLocation = u32;
type CGEventFlags = u64;
type CGKeyCode = u16;

const K_CG_EVENT_SOURCE_STATE_PRIVATE: CGEventSourceStateID = -1;
const K_CG_HID_EVENT_TAP: CGEventTapLocation = 0;
const K_CG_EVENT_FLAG_MASK_COMMAND: CGEventFlags = 0x0010_0000;
const K_VK_ANSI_V: CGKeyCode = 0x09;

#[link(name = "ApplicationServices", kind = "framework")]
unsafe extern "C" {
    fn AXIsProcessTrusted() -> bool;
    fn AXIsProcessTrustedWithOptions(options: *const c_void) -> bool;
    static kAXTrustedCheckOptionPrompt: *const c_void;
}

#[link(name = "CoreGraphics", kind = "framework")]
unsafe extern "C" {
    fn CGEventSourceCreate(state_id: CGEventSourceStateID) -> CGEventSourceRef;
    fn CGEventCreateKeyboardEvent(
        source: CGEventSourceRef,
        virtual_key: CGKeyCode,
        key_down: bool,
    ) -> CGEventRef;
    fn CGEventSetFlags(event: CGEventRef, flags: CGEventFlags);
    fn CGEventPost(tap: CGEventTapLocation, event: CGEventRef);
}

#[link(name = "CoreFoundation", kind = "framework")]
unsafe extern "C" {
    fn CFRelease(cf: *const c_void);
    fn CFDictionaryCreate(
        allocator: *const c_void,
        keys: *const *const c_void,
        values: *const *const c_void,
        num_values: isize,
        key_callbacks: *const c_void,
        value_callbacks: *const c_void,
    ) -> *const c_void;
    static kCFBooleanTrue: *const c_void;
    static kCFTypeDictionaryKeyCallBacks: c_void;
    static kCFTypeDictionaryValueCallBacks: c_void;
}

struct CfOwned(*mut c_void);

impl Drop for CfOwned {
    fn drop(&mut self) {
        if !self.0.is_null() {
            unsafe { CFRelease(self.0.cast()) };
        }
    }
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

/// Uses CoreGraphics CGEvents directly with ANSI `V` (0x09) + `Command` so background
/// paste execution never calls HIToolbox keyboard layout APIs off the main thread.
fn build_paste_command_v_events() -> Result<(CfOwned, CfOwned, CfOwned), String> {
    let source = unsafe { CGEventSourceCreate(K_CG_EVENT_SOURCE_STATE_PRIVATE) };
    if source.is_null() {
        return Err("创建 CoreGraphics 事件源失败".to_string());
    }
    let source = CfOwned(source);

    let key_down = unsafe { CGEventCreateKeyboardEvent(source.0, K_VK_ANSI_V, true) };
    if key_down.is_null() {
        return Err("创建 ⌘V 按下事件失败".to_string());
    }
    let key_down = CfOwned(key_down);
    unsafe { CGEventSetFlags(key_down.0, K_CG_EVENT_FLAG_MASK_COMMAND) };

    let key_up = unsafe { CGEventCreateKeyboardEvent(source.0, K_VK_ANSI_V, false) };
    if key_up.is_null() {
        return Err("创建 ⌘V 释放事件失败".to_string());
    }
    let key_up = CfOwned(key_up);
    unsafe { CGEventSetFlags(key_up.0, K_CG_EVENT_FLAG_MASK_COMMAND) };

    Ok((source, key_down, key_up))
}

pub fn simulate_paste() -> Result<(), String> {
    if !has_accessibility_permission() {
        return Err("需要「辅助功能」权限才能自动粘贴（已复制，请按 ⌘V 粘贴）".to_string());
    }
    std::thread::sleep(std::time::Duration::from_millis(85));
    let (_source, key_down, key_up) = build_paste_command_v_events()?;
    unsafe {
        CGEventPost(K_CG_HID_EVENT_TAP, key_down.0);
        std::thread::sleep(std::time::Duration::from_millis(5));
        CGEventPost(K_CG_HID_EVENT_TAP, key_up.0);
    }
    Ok(())
}

pub fn has_accessibility_permission() -> bool {
    unsafe { AXIsProcessTrusted() }
}

pub fn prompt_accessibility_permission() -> bool {
    unsafe {
        let keys = [kAXTrustedCheckOptionPrompt];
        let values = [kCFBooleanTrue];
        let dict = CFDictionaryCreate(
            std::ptr::null(),
            keys.as_ptr(),
            values.as_ptr(),
            1,
            &raw const kCFTypeDictionaryKeyCallBacks,
            &raw const kCFTypeDictionaryValueCallBacks,
        );
        let trusted = AXIsProcessTrustedWithOptions(dict);
        if !dict.is_null() {
            CFRelease(dict);
        }
        trusted
    }
}

pub fn open_accessibility_settings() {
    let _ = prompt_accessibility_permission();
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn paste_cgevent_builds_safely_off_main_thread() {
        std::thread::spawn(|| {
            let (source, down, up) = build_paste_command_v_events()
                .expect("CoreGraphics paste events should build on background thread");
            assert!(!source.0.is_null());
            assert!(!down.0.is_null());
            assert!(!up.0.is_null());
        })
        .join()
        .expect("worker thread panicked while building paste events");
    }
}
