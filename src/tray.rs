use crate::platform::TrayPosition;
use std::sync::mpsc::{self, Receiver};
use tray_icon::menu::{CheckMenuItem, Menu, MenuEvent, MenuId, MenuItem, PredefinedMenuItem};
use tray_icon::{Icon, MouseButton, MouseButtonState, TrayIcon, TrayIconBuilder, TrayIconEvent};

pub enum TrayAction {
    TogglePopup(Option<TrayPosition>),
    TogglePause,
    IgnoreNextEvent,
    OpenSettings,
    ClearHistory,
    Quit,
}

pub struct EarthTray {
    tray_icon: TrayIcon,
    pause_item: CheckMenuItem,
    ignore_next_item: CheckMenuItem,
    id_show: MenuId,
    id_pause: MenuId,
    id_ignore_next: MenuId,
    id_settings: MenuId,
    id_clear: MenuId,
    id_quit: MenuId,
    tray_rx: Receiver<TrayIconEvent>,
    menu_rx: Receiver<MenuEvent>,
    // Physical desktop pixels on Windows, platform points elsewhere. Never
    // cache Windows positions in units belonging to a movable egui viewport.
    last_tray_native_pos: Option<TrayPosition>,
    last_visible: bool,
    last_paused: bool,
    last_ignore_next: bool,
    last_title_or_tip: Option<String>,
}

impl EarthTray {
    pub fn new(
        repaint_ctx: egui::Context,
        visible: bool,
        paused: bool,
        ignore_next: bool,
    ) -> Result<Self, String> {
        let menu = Menu::new();
        let show_item = MenuItem::new("打开 Earth 剪贴板", true, None);
        let pause_item = CheckMenuItem::new("暂停记录新复制", true, paused, None);
        let ignore_next_item = CheckMenuItem::new("仅忽略下一次复制", true, ignore_next, None);
        let settings_item = MenuItem::new("偏好设置...", true, None);
        let clear_item = MenuItem::new("清除未固定记录", true, None);
        let quit_item = MenuItem::new("退出 Earth", true, None);

        let id_show = show_item.id().clone();
        let id_pause = pause_item.id().clone();
        let id_ignore_next = ignore_next_item.id().clone();
        let id_settings = settings_item.id().clone();
        let id_clear = clear_item.id().clone();
        let id_quit = quit_item.id().clone();

        menu.append(&show_item).map_err(|e| e.to_string())?;
        menu.append(&PredefinedMenuItem::separator())
            .map_err(|e| e.to_string())?;
        menu.append(&pause_item).map_err(|e| e.to_string())?;
        menu.append(&ignore_next_item).map_err(|e| e.to_string())?;
        menu.append(&PredefinedMenuItem::separator())
            .map_err(|e| e.to_string())?;
        menu.append(&settings_item).map_err(|e| e.to_string())?;
        menu.append(&clear_item).map_err(|e| e.to_string())?;
        menu.append(&PredefinedMenuItem::separator())
            .map_err(|e| e.to_string())?;
        menu.append(&quit_item).map_err(|e| e.to_string())?;

        let (tray_tx, tray_rx) = mpsc::channel();
        let ctx_t = repaint_ctx.clone();
        TrayIconEvent::set_event_handler(Some(move |ev| {
            let _ = tray_tx.send(ev);
            ctx_t.request_repaint();
        }));

        let (menu_tx, menu_rx) = mpsc::channel();
        let ctx_m = repaint_ctx;
        MenuEvent::set_event_handler(Some(move |ev| {
            let _ = menu_tx.send(ev);
            ctx_m.request_repaint();
        }));

        let icon = build_tray_icon(paused)?;

        let builder = TrayIconBuilder::new()
            .with_menu(Box::new(menu))
            .with_menu_on_left_click(false)
            .with_tooltip("Earth — 跨平台智能剪贴板");

        #[cfg(target_os = "macos")]
        let builder = builder.with_icon_templated(icon);
        #[cfg(not(target_os = "macos"))]
        let builder = builder.with_icon(icon);

        let tray_icon = builder
            .build()
            .map_err(|e| format!("创建系统托盘失败: {e}"))?;

        tray_icon
            .set_visible(visible)
            .map_err(|e| format!("设置托盘可见性失败: {e}"))?;

        let initial_title_or_tip = if cfg!(target_os = "macos") {
            None
        } else {
            Some("Earth — 跨平台智能剪贴板".to_string())
        };

        Ok(Self {
            tray_icon,
            pause_item,
            ignore_next_item,
            id_show,
            id_pause,
            id_ignore_next,
            id_settings,
            id_clear,
            id_quit,
            tray_rx,
            menu_rx,
            last_tray_native_pos: None,
            last_visible: visible,
            last_paused: paused,
            last_ignore_next: ignore_next,
            last_title_or_tip: initial_title_or_tip,
        })
    }

    pub fn last_tray_pos(&self) -> Option<TrayPosition> {
        self.last_tray_native_pos
    }

    pub fn is_visible(&self) -> bool {
        self.last_visible
    }

    pub fn sync_state(
        &mut self,
        show_icon: bool,
        paused: bool,
        ignore_next: bool,
        menubar_preview: Option<&str>,
    ) -> Result<(), String> {
        if show_icon != self.last_visible {
            self.tray_icon
                .set_visible(show_icon)
                .map_err(|e| format!("设置托盘可见性失败: {e}"))?;
            self.last_visible = show_icon;
        }
        if paused != self.last_paused {
            self.pause_item.set_checked(paused);
            if let Ok(icon) = build_tray_icon(paused) {
                #[cfg(target_os = "macos")]
                let _ = self.tray_icon.set_icon_templated(Some(icon));
                #[cfg(not(target_os = "macos"))]
                let _ = self.tray_icon.set_icon(Some(icon));
            }
            self.last_paused = paused;
        }
        if ignore_next != self.last_ignore_next {
            self.ignore_next_item.set_checked(ignore_next);
            self.last_ignore_next = ignore_next;
        }
        #[cfg(target_os = "macos")]
        {
            let next_title = if let Some(preview) = menubar_preview {
                Some(preview.to_string())
            } else if paused {
                Some("⏸".to_string())
            } else {
                None
            };
            if next_title != self.last_title_or_tip {
                self.tray_icon.set_title(next_title.as_deref());
                self.last_title_or_tip = next_title;
            }
        }
        #[cfg(not(target_os = "macos"))]
        {
            let next_tip = if let Some(preview) = menubar_preview {
                Some(format!("Earth · {preview}"))
            } else if paused {
                Some("Earth — 剪贴板（已暂停）".to_string())
            } else {
                Some("Earth — 跨平台智能剪贴板".to_string())
            };
            if next_tip != self.last_title_or_tip {
                let _ = self.tray_icon.set_tooltip(next_tip.as_deref());
                self.last_title_or_tip = next_tip;
            }
        }
        Ok(())
    }

    pub fn poll_actions(&mut self, dpi_scale: f32) -> Vec<TrayAction> {
        let mut actions = Vec::new();
        while let Ok(ev) = self.tray_rx.try_recv() {
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                position,
                rect,
                ..
            } = ev
            {
                let physical = tray_anchor_physical(
                    (position.x, position.y),
                    (rect.position.x, rect.position.y),
                    (rect.size.width, rect.size.height),
                );
                #[cfg(target_os = "windows")]
                let pos = {
                    let _ = dpi_scale;
                    physical
                };
                #[cfg(not(target_os = "windows"))]
                let pos = {
                    let scale = dpi_scale.max(1.0);
                    TrayPosition {
                        popup_anchor: (
                            physical.popup_anchor.0 / scale,
                            physical.popup_anchor.1 / scale,
                        ),
                        monitor_anchor: (
                            physical.monitor_anchor.0 / scale,
                            physical.monitor_anchor.1 / scale,
                        ),
                    }
                };
                self.last_tray_native_pos = Some(pos);
                actions.push(TrayAction::TogglePopup(Some(pos)));
            }
        }

        while let Ok(ev) = self.menu_rx.try_recv() {
            if ev.id == self.id_show {
                actions.push(TrayAction::TogglePopup(self.last_tray_native_pos));
            } else if ev.id == self.id_pause {
                actions.push(TrayAction::TogglePause);
            } else if ev.id == self.id_ignore_next {
                actions.push(TrayAction::IgnoreNextEvent);
            } else if ev.id == self.id_settings {
                actions.push(TrayAction::OpenSettings);
            } else if ev.id == self.id_clear {
                actions.push(TrayAction::ClearHistory);
            } else if ev.id == self.id_quit {
                actions.push(TrayAction::Quit);
            }
        }
        actions
    }
}

fn tray_anchor_physical(
    click: (f64, f64),
    rect_position: (f64, f64),
    rect_size: (u32, u32),
) -> TrayPosition {
    if rect_size.0 > 0 && rect_size.1 > 0 {
        let center_x = (rect_position.0 + f64::from(rect_size.0) * 0.5) as f32;
        TrayPosition {
            popup_anchor: (center_x, (rect_position.1 + f64::from(rect_size.1)) as f32),
            monitor_anchor: (
                center_x,
                (rect_position.1 + f64::from(rect_size.1) * 0.5) as f32,
            ),
        }
    } else {
        let point = (click.0 as f32, click.1 as f32);
        TrayPosition {
            popup_anchor: point,
            monitor_anchor: point,
        }
    }
}

/// Generates an anti-aliased RGBA Earth globe + orbital ring icon.
pub fn build_earth_tray_rgba(size: u32, paused: bool) -> Vec<u8> {
    let mut data = vec![0u8; (size * size * 4) as usize];
    let center = (size as f32 - 1.0) * 0.5;
    let radius = size as f32 * 0.34;
    let orbit_rx = size as f32 * 0.45;
    let orbit_ry = size as f32 * 0.16;

    for y in 0..size {
        for x in 0..size {
            let dx = x as f32 - center;
            let dy = y as f32 - center;
            let dist = (dx * dx + dy * dy).sqrt();

            let mut r = 0.0f32;
            let mut g = 0.0f32;
            let mut b = 0.0f32;
            let mut a = 0.0f32;

            // Globe disc
            let globe_alpha = ((radius + 0.8 - dist) / 1.4).clamp(0.0, 1.0);
            if globe_alpha > 0.0 {
                // Subtle ocean-emerald gradient + stylized meridian band
                let t = ((dy / radius) * 0.5 + 0.5).clamp(0.0, 1.0);
                let (c1, c2) = if paused {
                    ([148.0, 163.0, 184.0], [100.0, 116.0, 139.0])
                } else {
                    ([16.0, 185.0, 129.0], [14.0, 165.0, 233.0])
                };
                r = c1[0] * (1.0 - t) + c2[0] * t;
                g = c1[1] * (1.0 - t) + c2[1] * t;
                b = c1[2] * (1.0 - t) + c2[2] * t;

                // Subtle latitude/longitude highlight lines
                let nx = dx / radius;
                let ny = dy / radius;
                let mer_dist = (nx * 1.4).abs().min((ny).abs());
                if mer_dist < 0.09 {
                    r = r * 0.72 + 255.0 * 0.28;
                    g = g * 0.72 + 255.0 * 0.28;
                    b = b * 0.72 + 255.0 * 0.28;
                }
                a = globe_alpha;
            }

            // Tilted orbital ring (-24 deg)
            let angle = -0.42f32;
            let rx = dx * angle.cos() - dy * angle.sin();
            let ry = dx * angle.sin() + dy * angle.cos();
            let ell = ((rx / orbit_rx).powi(2) + (ry / orbit_ry).powi(2)).sqrt();
            let ring_dist = (ell - 1.0).abs() * orbit_rx * 0.45;
            // Front half (ry > 0) draws over globe; back half draws behind
            let ring_alpha = ((1.3 - ring_dist) / 1.2).clamp(0.0, 1.0);
            if ring_alpha > 0.0 && (ry > 0.0 || a < 0.5) {
                let (rr, rg, rb) = if paused {
                    (226.0, 232.0, 240.0)
                } else {
                    (236.0, 253.0, 245.0)
                };
                let blend = ring_alpha;
                r = r * (1.0 - blend) + rr * blend;
                g = g * (1.0 - blend) + rg * blend;
                b = b * (1.0 - blend) + rb * blend;
                a = a.max(blend);
            }

            let idx = ((y * size + x) * 4) as usize;
            data[idx] = r.round().clamp(0.0, 255.0) as u8;
            data[idx + 1] = g.round().clamp(0.0, 255.0) as u8;
            data[idx + 2] = b.round().clamp(0.0, 255.0) as u8;
            data[idx + 3] = (a * 255.0).round().clamp(0.0, 255.0) as u8;
        }
    }
    data
}

fn build_tray_icon(paused: bool) -> Result<Icon, String> {
    #[cfg(target_os = "macos")]
    const TRAY_ICON_SIZE: u32 = 44; // 2x Retina for 22pt macOS status bar cap
    #[cfg(not(target_os = "macos"))]
    const TRAY_ICON_SIZE: u32 = 32;

    #[cfg(target_os = "macos")]
    let rgba = build_earth_tray_monochrome_rgba(TRAY_ICON_SIZE, paused);
    #[cfg(not(target_os = "macos"))]
    let rgba = build_earth_tray_rgba(TRAY_ICON_SIZE, paused);

    Icon::from_rgba(rgba, TRAY_ICON_SIZE, TRAY_ICON_SIZE)
        .map_err(|e| format!("创建托盘图标失败: {e}"))
}

#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
fn aa_fill(dist: f32, radius: f32) -> f32 {
    (radius + 0.5 - dist).clamp(0.0, 1.0)
}

#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
fn aa_stroke(dist: f32, half_width: f32) -> f32 {
    (half_width + 0.5 - dist).clamp(0.0, 1.0)
}

#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
fn ellipse_dist(x: f32, y: f32, rx: f32, ry: f32) -> f32 {
    let nx = x / rx;
    let ny = y / ry;
    let f = nx * nx + ny * ny - 1.0;
    let gx = (2.0 * x) / (rx * rx);
    let gy = (2.0 * y) / (ry * ry);
    let grad = (gx * gx + gy * gy).sqrt();
    if grad < 1e-5 {
        rx.min(ry)
    } else {
        f.abs() / grad
    }
}

/// Generates an anti-aliased monochrome white Earth globe + orbital ring silhouette
/// suitable for macOS menu bar template rendering (`isTemplate = true`).
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
pub fn build_earth_tray_monochrome_rgba(size: u32, paused: bool) -> Vec<u8> {
    let mut data = vec![0u8; (size * size * 4) as usize];
    let s = size as f32;
    let center = (s - 1.0) * 0.5;
    let radius = s * 0.31;
    let orbit_rx = s * 0.45;
    let orbit_ry = s * 0.13;

    let w_outer = (s * 0.032).max(0.9);
    let w_orbit = (s * 0.029).max(0.85);
    let w_inner = (s * 0.021).max(0.65);
    let w_gap = (s * 0.028).max(0.9);

    let angle = -0.50f32;
    let cos_a = angle.cos();
    let sin_a = angle.sin();
    let opacity = if paused { 0.50 } else { 1.0 };

    for y in 0..size {
        for x in 0..size {
            let dx = x as f32 - center;
            let dy = y as f32 - center;
            let dist = (dx * dx + dy * dy).sqrt();

            // Globe outline + interior meridian/equator lines clipped cleanly to the sphere.
            let outer_cov = aa_stroke((dist - radius).abs(), w_outer);
            let globe_fill = aa_fill(dist, radius);
            let mer_cov =
                aa_stroke(ellipse_dist(dx, dy, radius * 0.47, radius), w_inner) * globe_fill;
            let eq_cov = aa_stroke(dy.abs(), w_inner) * globe_fill;
            let globe_cov = outer_cov.max(mer_cov).max(eq_cov);

            // Tilted orbital ring (-28.6 deg) with negative-space cutouts separating front and back arcs.
            let rx = dx * cos_a - dy * sin_a;
            let ry = dx * sin_a + dy * cos_a;
            let d_orbit = ellipse_dist(rx, ry, orbit_rx, orbit_ry);
            let orbit_cov = aa_stroke(d_orbit, w_orbit);

            let (front_orbit, back_orbit, front_halo) = if ry > 0.0 {
                (orbit_cov, 0.0, aa_stroke(d_orbit, w_orbit + w_gap))
            } else {
                let outside_globe = 1.0 - aa_fill(dist, radius + w_outer + w_gap);
                (0.0, orbit_cov * outside_globe, 0.0)
            };

            let globe_masked = globe_cov * (1.0 - front_halo);
            let cov = globe_masked.max(front_orbit).max(back_orbit);
            let alpha = (cov * opacity * 255.0).round().clamp(0.0, 255.0) as u8;

            if alpha > 0 {
                let idx = ((y * size + x) * 4) as usize;
                data[idx] = 255;
                data[idx + 1] = 255;
                data[idx + 2] = 255;
                data[idx + 3] = alpha;
            }
        }
    }
    data
}

#[cfg(test)]
mod tests {
    use super::{build_earth_tray_monochrome_rgba, build_earth_tray_rgba, tray_anchor_physical};

    #[test]
    fn tray_anchor_uses_physical_icon_bounds_including_negative_origins() {
        let position = tray_anchor_physical((900.0, 600.0), (-1900.0, -80.0), (32, 40));
        assert_eq!(position.popup_anchor, (-1884.0, -40.0));
        assert_eq!(position.monitor_anchor, (-1884.0, -60.0));
        let fallback = tray_anchor_physical((-50.0, 1060.0), (0.0, 0.0), (0, 0));
        assert_eq!(fallback.popup_anchor, (-50.0, 1060.0));
        assert_eq!(fallback.monitor_anchor, fallback.popup_anchor);
    }

    #[test]
    fn tray_monitor_anchor_stays_inside_icon_at_stacked_screen_boundary() {
        let position = tray_anchor_physical((200.0, 1060.0), (1840.0, 1040.0), (40, 40));
        assert_eq!(position.popup_anchor, (1860.0, 1080.0));
        assert!(position.monitor_anchor.1 >= 1040.0 && position.monitor_anchor.1 < 1080.0);
    }

    #[test]
    fn monochrome_tray_icon_is_pure_white_with_alpha_silhouette() {
        let active = build_earth_tray_monochrome_rgba(44, false);
        assert_eq!(active.len(), 44 * 44 * 4);

        let mut visible_pixels = 0usize;
        let mut transparent_pixels = 0usize;
        for px in active.as_chunks::<4>().0 {
            if px[3] == 0 {
                transparent_pixels += 1;
                assert_eq!(*px, [0, 0, 0, 0]);
            } else {
                visible_pixels += 1;
                assert_eq!(&px[..3], &[255, 255, 255]);
            }
        }
        assert!(visible_pixels > 150);
        assert!(transparent_pixels > 1000);

        let paused = build_earth_tray_monochrome_rgba(44, true);
        let active_max_a = active
            .as_chunks::<4>()
            .0
            .iter()
            .map(|px| px[3])
            .max()
            .unwrap_or(0);
        let paused_max_a = paused
            .as_chunks::<4>()
            .0
            .iter()
            .map(|px| px[3])
            .max()
            .unwrap_or(0);
        assert_eq!(active_max_a, 255);
        assert!(paused_max_a < 130 && paused_max_a > 90);
    }

    #[test]
    fn window_earth_icon_stays_colorful() {
        let rgba = build_earth_tray_rgba(64, false);
        let has_color = rgba
            .as_chunks::<4>()
            .0
            .iter()
            .any(|px| px[3] > 200 && (px[0] != px[1] || px[1] != px[2]));
        assert!(has_color);
    }
}
