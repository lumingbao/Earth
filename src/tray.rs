use std::sync::mpsc::{self, Receiver};
use tray_icon::menu::{CheckMenuItem, Menu, MenuEvent, MenuId, MenuItem, PredefinedMenuItem};
use tray_icon::{Icon, MouseButton, MouseButtonState, TrayIcon, TrayIconBuilder, TrayIconEvent};

pub enum TrayAction {
    TogglePopup(Option<(f32, f32)>),
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
    last_tray_logical_pos: Option<(f32, f32)>,
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
        let clear_item = MenuItem::new("清除未置顶记录", true, None);
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

        let rgba = build_earth_tray_rgba(32, paused);
        let icon = Icon::from_rgba(rgba, 32, 32)
            .map_err(|e| format!("创建托盘图标失败: {e}"))?;

        let tray_icon = TrayIconBuilder::new()
            .with_menu(Box::new(menu))
            .with_menu_on_left_click(false)
            .with_tooltip("Earth — 跨平台智能剪贴板")
            .with_icon(icon)
            .build()
            .map_err(|e| format!("创建系统托盘失败: {e}"))?;

        let _ = tray_icon.set_visible(visible);

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
            last_tray_logical_pos: None,
            last_visible: visible,
            last_paused: paused,
            last_ignore_next: ignore_next,
            last_title_or_tip: initial_title_or_tip,
        })
    }

    pub fn last_tray_pos(&self) -> Option<(f32, f32)> {
        self.last_tray_logical_pos
    }

    pub fn sync_state(
        &mut self,
        show_icon: bool,
        paused: bool,
        ignore_next: bool,
        menubar_preview: Option<&str>,
    ) {
        if show_icon != self.last_visible {
            let _ = self.tray_icon.set_visible(show_icon);
            self.last_visible = show_icon;
        }
        if paused != self.last_paused {
            self.pause_item.set_checked(paused);
            let rgba = build_earth_tray_rgba(32, paused);
            if let Ok(icon) = Icon::from_rgba(rgba, 32, 32) {
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
                let scale = dpi_scale.max(1.0) as f64;
                let logical_x = ((rect.position.x + rect.size.width as f64 * 0.5) / scale) as f32;
                let logical_y = ((rect.position.y + rect.size.height as f64) / scale) as f32;
                let pos = if rect.size.width > 0 {
                    (logical_x, logical_y)
                } else {
                    ((position.x / scale) as f32, (position.y / scale) as f32)
                };
                self.last_tray_logical_pos = Some(pos);
                actions.push(TrayAction::TogglePopup(Some(pos)));
            }
        }

        while let Ok(ev) = self.menu_rx.try_recv() {
            if ev.id == self.id_show {
                actions.push(TrayAction::TogglePopup(self.last_tray_logical_pos));
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
