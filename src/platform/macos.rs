use crate::model::ActiveAppInfo;
use crate::platform::{PreferredSystemFonts, ScreenRect, SystemFontCandidate};
use objc2::rc::Retained;
use objc2::runtime::ProtocolObject;
use objc2::MainThreadMarker;
use objc2_app_kit::{
    NSApplication, NSApplicationActivationOptions, NSApplicationActivationPolicy, NSEvent,
    NSFloatingWindowLevel, NSRunningApplication, NSScreen, NSViewLayerContentsPlacement, NSWindow,
    NSWindowCollectionBehavior, NSWindowLevel, NSWorkspace,
};
use objc2_foundation::{ns_string, NSDictionary, NSNull};
use objc2_quartz_core::{CAAction, CATransaction, kCAGravityLeft, kCAGravityRight};
use std::ffi::c_void;
use std::path::{Path, PathBuf};
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
    fn CFStringCreateWithBytes(
        alloc: *const c_void,
        bytes: *const u8,
        num_bytes: isize,
        encoding: u32,
        is_external_representation: bool,
    ) -> *const c_void;
    fn CFStringGetCString(
        the_string: *const c_void,
        buffer: *mut u8,
        buffer_size: isize,
        encoding: u32,
    ) -> bool;
    fn CFURLCopyFileSystemPath(url: *const c_void, path_style: isize) -> *const c_void;
    static kCFBooleanTrue: *const c_void;
    static kCFTypeDictionaryKeyCallBacks: c_void;
    static kCFTypeDictionaryValueCallBacks: c_void;
}

#[link(name = "CoreText", kind = "framework")]
unsafe extern "C" {
    fn CTFontCreateWithName(
        name: *const c_void,
        size: f64,
        matrix: *const c_void,
    ) -> *const c_void;
    fn CTFontCopyPostScriptName(font: *const c_void) -> *const c_void;
    fn CTFontCopyAttribute(font: *const c_void, attribute: *const c_void) -> *const c_void;
    static kCTFontURLAttribute: *const c_void;
}

const K_CF_STRING_ENCODING_UTF8: u32 = 0x0800_0100;
const K_CF_URL_POSIX_PATH_STYLE: isize = 0;

struct CfOwned(*mut c_void);

impl Drop for CfOwned {
    fn drop(&mut self) {
        if !self.0.is_null() {
            unsafe { CFRelease(self.0.cast()) };
        }
    }
}

pub fn capture_frontmost_app_identity() -> Option<ActiveAppInfo> {
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

    Some(ActiveAppInfo {
        app_name,
        process_or_bundle: bundle_id,
        pid: pid as u64,
        window_rect: None,
        window_handle: None,
    })
}

pub fn capture_frontmost_app() -> Option<ActiveAppInfo> {
    let mut info = capture_frontmost_app_identity()?;
    info.window_rect = active_win_pos_rs::get_position().ok().and_then(|pos| {
        if pos.width > 10.0 && pos.height > 10.0 {
            Some((pos.x as f32, pos.y as f32, pos.width as f32, pos.height as f32))
        } else {
            None
        }
    });
    Some(info)
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

fn screen_bounds_at_cocoa_point(mtm: MainThreadMarker, pt: objc2_foundation::NSPoint) -> ScreenRect {
    let fallback_rect = ScreenRect {
        x: 0.0,
        y: 0.0,
        width: 1440.0,
        height: 900.0,
    };

    let screens = NSScreen::screens(mtm);
    if screens.is_empty() {
        return fallback_rect;
    }

    let primary_height = screens.objectAtIndex(0).frame().size.height;
    let to_screen_rect = |screen: objc2::rc::Retained<NSScreen>| {
        let vis = screen.visibleFrame();
        ScreenRect {
            x: vis.origin.x as f32,
            y: (primary_height - (vis.origin.y + vis.size.height)) as f32,
            width: vis.size.width as f32,
            height: vis.size.height as f32,
        }
    };

    for i in 0..screens.len() {
        let screen = screens.objectAtIndex(i);
        let frame = screen.frame();
        let in_x = pt.x >= frame.origin.x && pt.x <= frame.origin.x + frame.size.width;
        let in_y = pt.y >= frame.origin.y && pt.y <= frame.origin.y + frame.size.height;
        if in_x && in_y {
            return to_screen_rect(screen);
        }
    }

    to_screen_rect(screens.objectAtIndex(0))
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
    ((cursor_x, cursor_y), screen_bounds_at_cocoa_point(mtm, mouse))
}

pub fn monitor_bounds_for_point(x: f32, y: f32, _dpi_scale: f32) -> ScreenRect {
    let fallback_rect = ScreenRect {
        x: 0.0,
        y: 0.0,
        width: 1440.0,
        height: 900.0,
    };
    let Some(mtm) = MainThreadMarker::new() else {
        return fallback_rect;
    };
    let screens = NSScreen::screens(mtm);
    if screens.is_empty() {
        return fallback_rect;
    }
    let primary_height = screens.objectAtIndex(0).frame().size.height;
    let pt = objc2_foundation::NSPoint::new(x as f64, primary_height - y as f64);
    screen_bounds_at_cocoa_point(mtm, pt)
}

fn earth_ns_window(mtm: MainThreadMarker) -> Option<Retained<NSWindow>> {
    let app = NSApplication::sharedApplication(mtm);
    app.keyWindow().or_else(|| {
        let windows = app.windows();
        (0..windows.len())
            .map(|i| windows.objectAtIndex(i))
            .find(|w| w.title().to_string() == "Earth")
            .or_else(|| (!windows.is_empty()).then(|| windows.objectAtIndex(0)))
    })
}

pub fn invalidate_window_shadow() {
    if let Some(mtm) = MainThreadMarker::new()
        && let Some(window) = earth_ns_window(mtm)
    {
        window.invalidateShadow();
    }
}

/// Anchors AppKit/Metal layer contents to the right or left edge during OS viewport resizes
/// so collapsing or expanding from the left never squashes or shifts the card rightward.
pub fn set_window_resize_anchor_right(anchor_right: bool) {
    let Some(mtm) = MainThreadMarker::new() else {
        return;
    };
    let Some(window) = earth_ns_window(mtm) else {
        return;
    };
    let Some(view) = window.contentView() else {
        return;
    };
    let (placement, gravity) = if anchor_right {
        unsafe { (NSViewLayerContentsPlacement::Right, kCAGravityRight) }
    } else {
        unsafe { (NSViewLayerContentsPlacement::Left, kCAGravityLeft) }
    };

    CATransaction::begin();
    CATransaction::setDisableActions(true);
    view.setLayerContentsPlacement(placement);
    if let Some(root_layer) = view.layer() {
        root_layer.setContentsGravity(gravity);
        root_layer.setMasksToBounds(true);
        if let Some(sublayers) = unsafe { root_layer.sublayers() } {
            let null_action = NSNull::null();
            let action_obj: &ProtocolObject<dyn CAAction> = ProtocolObject::from_ref(&*null_action);
            let keys = [
                ns_string!("bounds"),
                ns_string!("position"),
                ns_string!("contentsGravity"),
            ];
            let vals = [action_obj, action_obj, action_obj];
            let no_actions = NSDictionary::from_slices(&keys, &vals);
            for i in 0..sublayers.len() {
                let sub = sublayers.objectAtIndex(i);
                sub.setContentsGravity(gravity);
                sub.setMasksToBounds(true);
                sub.setActions(Some(&no_actions));
            }
        }
    }
    CATransaction::commit();
}

/// Elevates the Earth NSWindow above normal, floating, and modal windows across Spaces while
/// ensuring it never hides when clicking other applications.
pub fn set_window_pinned(pinned: bool) {
    const UTILITY_WINDOW_LEVEL: NSWindowLevel = 19;
    let Some(mtm) = MainThreadMarker::new() else {
        return;
    };
    let Some(window) = earth_ns_window(mtm) else {
        return;
    };
    window.setHidesOnDeactivate(false);
    if pinned {
        window.setLevel(UTILITY_WINDOW_LEVEL);
        window.setCollectionBehavior(
            NSWindowCollectionBehavior::CanJoinAllSpaces
                | NSWindowCollectionBehavior::FullScreenAuxiliary,
        );
        window.orderFrontRegardless();
    } else {
        window.setLevel(NSFloatingWindowLevel);
        window.setCollectionBehavior(NSWindowCollectionBehavior::MoveToActiveSpace);
    }
}

pub fn reveal_in_file_manager(path: &Path) {
    let _ = Command::new("open").arg("-R").arg(path).spawn();
}

pub fn open_url(url: &str) {
    let _ = Command::new("open").arg(url).spawn();
}

fn cf_string_to_rust(cf_str: *const c_void) -> Option<String> {
    if cf_str.is_null() {
        return None;
    }
    let mut buf = vec![0u8; 2048];
    let ok = unsafe {
        CFStringGetCString(
            cf_str,
            buf.as_mut_ptr(),
            buf.len() as isize,
            K_CF_STRING_ENCODING_UTF8,
        )
    };
    if !ok {
        return None;
    }
    let end = buf.iter().position(|&b| b == 0).unwrap_or(buf.len());
    String::from_utf8(buf[..end].to_vec()).ok()
}

pub fn resolve_font_path_by_postscript_name(ps_name: &str) -> Option<PathBuf> {
    let cf_name = unsafe {
        CFStringCreateWithBytes(
            std::ptr::null(),
            ps_name.as_ptr(),
            ps_name.len() as isize,
            K_CF_STRING_ENCODING_UTF8,
            false,
        )
    };
    if cf_name.is_null() {
        return None;
    }
    let _name_guard = CfOwned(cf_name.cast_mut());

    let ct_font = unsafe { CTFontCreateWithName(cf_name, 0.0, std::ptr::null()) };
    if ct_font.is_null() {
        return None;
    }
    let _font_guard = CfOwned(ct_font.cast_mut());

    let actual_ps = unsafe { CTFontCopyPostScriptName(ct_font) };
    if actual_ps.is_null() {
        return None;
    }
    let _ps_guard = CfOwned(actual_ps.cast_mut());
    if cf_string_to_rust(actual_ps).as_deref() != Some(ps_name) {
        return None;
    }

    let url_ref = unsafe { CTFontCopyAttribute(ct_font, kCTFontURLAttribute) };
    if url_ref.is_null() {
        return None;
    }
    let _url_guard = CfOwned(url_ref.cast_mut());

    let path_cf = unsafe { CFURLCopyFileSystemPath(url_ref, K_CF_URL_POSIX_PATH_STYLE) };
    if path_cf.is_null() {
        return None;
    }
    let _path_guard = CfOwned(path_cf.cast_mut());

    let path = PathBuf::from(cf_string_to_rust(path_cf)?);
    path.is_file().then_some(path)
}

pub fn preferred_system_fonts() -> PreferredSystemFonts {
    let mut ui = Vec::new();
    for (ps, fallback_path) in [
        ("PingFangSC-Regular", None),
        (
            "HiraginoSansGB-W3",
            Some("/System/Library/Fonts/Hiragino Sans GB.ttc"),
        ),
        (
            "ArialUnicodeMS",
            Some("/Library/Fonts/Arial Unicode.ttf"),
        ),
    ] {
        let resolved = resolve_font_path_by_postscript_name(ps).or_else(|| {
            fallback_path
                .map(PathBuf::from)
                .filter(|p| p.is_file())
        });
        if let Some(path) = resolved {
            ui.push(SystemFontCandidate {
                path,
                postscript_name: ps,
            });
            break;
        }
    }

    let mut monospace = Vec::new();
    if let Some(path) = resolve_font_path_by_postscript_name("Menlo-Regular")
        .or_else(|| Some(PathBuf::from("/System/Library/Fonts/Menlo.ttc")).filter(|p| p.is_file()))
    {
        monospace.push(SystemFontCandidate {
            path,
            postscript_name: "Menlo-Regular",
        });
    }

    PreferredSystemFonts { ui, monospace }
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

    #[test]
    fn resolves_pingfang_and_menlo_via_coretext() {
        let fonts = preferred_system_fonts();
        assert!(!fonts.ui.is_empty(), "expected macOS UI font candidate");
        assert!(fonts.ui[0].path.is_file());
        assert!(!fonts.monospace.is_empty(), "expected macOS Menlo font candidate");
        assert!(fonts.monospace[0].path.is_file());
    }
}
