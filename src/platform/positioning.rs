use super::ScreenRect;
use crate::model::PopupPosition;

/// All positions and rectangles use one desktop coordinate space. On Windows it
/// is physical pixels; on macOS it is Cocoa points with a top-left origin.
pub(super) struct PopupGeometry {
    pub cursor: (f32, f32),
    pub monitor: ScreenRect,
    pub previous_window: Option<(f32, f32, f32, f32)>,
    pub tray_position: Option<(f32, f32)>,
    /// Desktop units per egui point on the destination monitor.
    pub target_scale: f32,
}

pub(super) fn valid_scale(scale: f32) -> f32 {
    if scale.is_finite() && scale > 0.1 {
        scale
    } else {
        1.0
    }
}

/// Select the monitor from the anchor, not from a cursor that may have moved
/// since a tray click or may be on a different screen than the foreground app.
#[cfg(any(target_os = "windows", test))]
pub(super) fn anchor_point(
    mode: PopupPosition,
    cursor: (f32, f32),
    previous_window: Option<(f32, f32, f32, f32)>,
    tray_position: Option<(f32, f32)>,
) -> (f32, f32) {
    match mode {
        PopupPosition::WindowCenter => previous_window
            .map(|(x, y, width, height)| (x + width * 0.5, y + height * 0.5))
            .unwrap_or(cursor),
        PopupPosition::Tray => tray_position.unwrap_or(cursor),
        _ => cursor,
    }
}

pub(super) fn popup_origin(
    mode: PopupPosition,
    window_size: (f32, f32),
    geometry: PopupGeometry,
    tray_above: bool,
) -> (f32, f32) {
    let PopupGeometry {
        cursor: (cursor_x, cursor_y),
        monitor,
        previous_window,
        tray_position,
        target_scale,
    } = geometry;
    let scale = valid_scale(target_scale);
    let (win_w, win_h) = (window_size.0 * scale, window_size.1 * scale);
    let screen_center = (
        monitor.x + (monitor.width - win_w) * 0.5,
        monitor.y + (monitor.height - win_h) * 0.42,
    );
    let (raw_x, raw_y) = match mode {
        PopupPosition::Cursor => (cursor_x + 8.0 * scale, cursor_y + 10.0 * scale),
        PopupPosition::ScreenCenter => screen_center,
        PopupPosition::WindowCenter => previous_window
            .map(|(x, y, width, height)| (x + (width - win_w) * 0.5, y + (height - win_h) * 0.5))
            .unwrap_or(screen_center),
        PopupPosition::Tray => match tray_position {
            Some((x, y)) if tray_above => (x - win_w * 0.5, y - win_h - 12.0 * scale),
            Some((x, y)) => (x - win_w * 0.5, y + 8.0 * scale),
            None if tray_above => (
                monitor.x + monitor.width - win_w - 24.0 * scale,
                monitor.y + monitor.height - win_h - 24.0 * scale,
            ),
            None => (
                monitor.x + monitor.width - win_w - 24.0 * scale,
                monitor.y + 36.0 * scale,
            ),
        },
    };
    let margin = 8.0 * scale;
    let min_x = monitor.x + margin;
    let max_x = (monitor.x + monitor.width - win_w - margin).max(min_x);
    let min_y = monitor.y + margin;
    let max_y = (monitor.y + monitor.height - win_h - margin).max(min_y);
    (raw_x.clamp(min_x, max_x), raw_y.clamp(min_y, max_y))
}

/// egui-winit multiplies `OuterPosition` by the *current* viewport scale. The
/// destination monitor's scale must never be used to divide desktop origins:
/// doing that moves negative and secondary-monitor coordinates to another screen.
#[cfg(any(target_os = "windows", test))]
pub(super) fn physical_to_viewport(
    physical: (f32, f32),
    current_pixels_per_point: f32,
) -> (f32, f32) {
    let scale = valid_scale(current_pixels_per_point);
    (physical.0 / scale, physical.1 / scale)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn geometry(monitor: ScreenRect, target_scale: f32) -> PopupGeometry {
        PopupGeometry {
            cursor: (
                monitor.x + monitor.width - 20.0,
                monitor.y + monitor.height - 20.0,
            ),
            monitor,
            previous_window: None,
            tray_position: None,
            target_scale,
        }
    }

    fn assert_close(actual: (f32, f32), expected: (f32, f32)) {
        assert!(
            (actual.0 - expected.0).abs() < 0.01,
            "{actual:?} != {expected:?}"
        );
        assert!(
            (actual.1 - expected.1).abs() < 0.01,
            "{actual:?} != {expected:?}"
        );
    }

    #[test]
    fn mixed_dpi_clamps_to_destination_size_and_preserves_physical_position() {
        // The viewport was on a 150% laptop. The cursor is now on a 100% screen.
        let screen = ScreenRect {
            x: 1920.0,
            y: 0.0,
            width: 1920.0,
            height: 1040.0,
        };
        let physical = popup_origin(
            PopupPosition::Cursor,
            (424.0, 520.0),
            geometry(screen, 1.0),
            true,
        );
        assert_close(physical, (3408.0, 512.0));
        let command = physical_to_viewport(physical, 1.5);
        assert_close((command.0 * 1.5, command.1 * 1.5), physical);

        // Reversing the scales must reserve the full 636×780 physical pixels.
        let physical = popup_origin(
            PopupPosition::Cursor,
            (424.0, 520.0),
            geometry(screen, 1.5),
            true,
        );
        assert_close(physical, (3192.0, 248.0));
        assert_close(physical_to_viewport(physical, 1.0), physical);
    }

    #[test]
    fn negative_desktop_origins_survive_200_to_125_percent_conversion() {
        let screen = ScreenRect {
            x: -2560.0,
            y: -1440.0,
            width: 2560.0,
            height: 1400.0,
        };
        let physical = popup_origin(
            PopupPosition::Cursor,
            (424.0, 520.0),
            geometry(screen, 2.0),
            true,
        );
        assert_close(physical, (-864.0, -1096.0));
        let command = physical_to_viewport(physical, 1.25);
        assert_close((command.0 * 1.25, command.1 * 1.25), physical);
    }

    #[test]
    fn tray_and_foreground_window_choose_their_own_monitor_anchor() {
        let cursor = (500.0, 400.0);
        let previous = Some((-1800.0, 200.0, 1200.0, 900.0));
        let tray = Some((3500.0, 1080.0));
        assert_close(
            anchor_point(PopupPosition::WindowCenter, cursor, previous, tray),
            (-1200.0, 650.0),
        );
        assert_close(
            anchor_point(PopupPosition::Tray, cursor, previous, tray),
            (3500.0, 1080.0),
        );
        assert_close(
            anchor_point(PopupPosition::Cursor, cursor, previous, tray),
            cursor,
        );
        assert_close(
            anchor_point(PopupPosition::WindowCenter, cursor, None, None),
            cursor,
        );
    }

    #[test]
    fn tray_cache_keeps_physical_anchor_when_viewport_scale_changes() {
        let screen = ScreenRect {
            x: 1920.0,
            y: 0.0,
            width: 1920.0,
            height: 1040.0,
        };
        let mut input = geometry(screen, 1.25);
        input.tray_position = Some((3000.0, 1060.0));
        let physical = popup_origin(PopupPosition::Tray, (424.0, 520.0), input, true);
        assert_close(physical, (2735.0, 380.0));
        for current_scale in [1.0, 1.25, 1.5, 2.0] {
            let command = physical_to_viewport(physical, current_scale);
            assert_close(
                (command.0 * current_scale, command.1 * current_scale),
                physical,
            );
        }
    }

    #[test]
    fn foreground_window_center_uses_physical_window_rect() {
        let screen = ScreenRect {
            x: -1920.0,
            y: 0.0,
            width: 1920.0,
            height: 1040.0,
        };
        let mut input = geometry(screen, 1.25);
        input.previous_window = Some((-1800.0, 100.0, 1600.0, 900.0));
        assert_close(
            popup_origin(PopupPosition::WindowCenter, (424.0, 520.0), input, true),
            (-1265.0, 225.0),
        );
    }

    #[test]
    fn portrait_screen_and_oversized_popup_have_stable_bounds() {
        let screen = ScreenRect {
            x: -1080.0,
            y: 0.0,
            width: 1080.0,
            height: 1880.0,
        };
        let physical = popup_origin(
            PopupPosition::ScreenCenter,
            (424.0, 520.0),
            geometry(screen, 1.5),
            true,
        );
        assert_close(physical, (-858.0, 462.0));
        let small = ScreenRect {
            x: -300.0,
            y: -200.0,
            width: 300.0,
            height: 200.0,
        };
        assert_close(
            popup_origin(
                PopupPosition::Cursor,
                (424.0, 520.0),
                geometry(small, 2.0),
                true,
            ),
            (-284.0, -184.0),
        );
    }
}
