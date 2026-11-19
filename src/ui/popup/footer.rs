use super::*;

pub(super) fn render_footer(
    ui: &mut Ui,
    settings: &AppSettings,
    state: &mut PopupState,
    palette: &EarthPalette,
    storage_bytes: u64,
    total_items: usize,
    actions: &mut Vec<PopupAction>,
) {
    ui.separator();
    ui.horizontal(|ui| {
        let count_s = total_items.to_string();
        let size_s = format_bytes(storage_bytes);
        ui.label(
            RichText::new(tf(
                "footer.stats",
                &[("count", &count_s), ("size", &size_s)],
            ))
            .size(11.5)
            .color(palette.text_muted),
        );

        let footer_btn = |ui: &mut Ui, label: RichText| {
            padded_button(
                ui,
                egui::Button::new(label)
                    .fill(palette.surface_bg)
                    .stroke(Stroke::new(1.0, palette.border_subtle))
                    .corner_radius(CornerRadius::same(255)),
                vec2(8.0, 4.0),
            )
        };
        let ignore_lbl = if settings.ignore_next_event {
            t("footer.ignore_next_active")
        } else {
            t("footer.ignore_next")
        };
        if footer_btn(
            ui,
            RichText::new(ignore_lbl)
                .size(11.0)
                .color(if settings.ignore_next_event {
                    palette.warning_text
                } else {
                    palette.text_secondary
                }),
        )
        .on_hover_text(t("footer.ignore_next_tip"))
        .clicked()
        {
            actions.push(PopupAction::ToggleIgnoreNext);
        }

        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            #[cfg(target_os = "macos")]
            let (quit_s, prefs_s, clear_s) = ("⌘Q", "⌘,", "⌥⌘⌫");
            #[cfg(not(target_os = "macos"))]
            let (quit_s, prefs_s, clear_s) = ("Ctrl+Q", "Ctrl+,", "Ctrl+Alt+⌫");

            if footer_btn(ui, RichText::new(t("footer.quit")).size(11.5))
                .on_hover_text(tf("footer.quit_tip", &[("shortcut", quit_s)]))
                .clicked()
            {
                actions.push(PopupAction::Quit);
            }
            if footer_btn(ui, RichText::new(t("footer.about")).size(11.5)).clicked() {
                state.show_about_modal = true;
            }
            if footer_btn(ui, RichText::new(t("footer.settings")).size(11.5))
                .on_hover_text(tf("footer.settings_tip", &[("shortcut", prefs_s)]))
                .clicked()
            {
                actions.push(PopupAction::OpenSettings);
            }
            if footer_btn(
                ui,
                RichText::new(t("footer.clear"))
                    .size(11.5)
                    .color(palette.danger),
            )
            .on_hover_text(tf("footer.clear_tip", &[("shortcut", clear_s)]))
            .clicked()
            {
                actions.push(PopupAction::ClearUnpinned);
            }
        });
    });
}

pub(super) fn antd_kind_tag_colors(
    item: &ClipItem,
    palette: &EarthPalette,
) -> (Color32, Color32, Color32) {
    match item.kind {
        ClipKind::Text => {
            if item.detected_color.is_some() {
                if palette.is_dark {
                    (
                        Color32::from_rgb(48, 34, 58),
                        Color32::from_rgb(96, 62, 120),
                        Color32::from_rgb(191, 90, 242),
                    )
                } else {
                    (
                        Color32::from_rgb(245, 236, 252),
                        Color32::from_rgb(218, 184, 242),
                        Color32::from_rgb(137, 68, 171),
                    )
                }
            } else if item.is_url {
                if palette.is_dark {
                    (
                        Color32::from_rgb(26, 42, 62),
                        Color32::from_rgb(46, 82, 124),
                        Color32::from_rgb(100, 178, 255),
                    )
                } else {
                    (
                        Color32::from_rgb(232, 242, 255),
                        Color32::from_rgb(179, 215, 255),
                        Color32::from_rgb(0, 102, 214),
                    )
                }
            } else {
                (palette.info_bg, palette.info_border, palette.accent_text)
            }
        }
        ClipKind::Image => {
            if palette.is_dark {
                (
                    Color32::from_rgb(24, 46, 52),
                    Color32::from_rgb(44, 96, 108),
                    Color32::from_rgb(100, 210, 255),
                )
            } else {
                (
                    Color32::from_rgb(230, 247, 250),
                    Color32::from_rgb(174, 224, 235),
                    Color32::from_rgb(0, 130, 153),
                )
            }
        }
        ClipKind::Files => (
            palette.success_bg,
            palette.success_border,
            palette.success_text,
        ),
    }
}

pub(super) fn tint(c: Color32, alpha: u8) -> Color32 {
    Color32::from_rgba_unmultiplied(c.r(), c.g(), c.b(), alpha)
}

pub(super) fn render_banner(
    ui: &mut Ui,
    msg: &str,
    is_warn: bool,
    show_ax_btn: bool,
    palette: &EarthPalette,
    actions: &mut Vec<PopupAction>,
) -> bool {
    let mut close_clicked = false;
    let (bg, border) = if is_warn {
        (palette.warning_bg, palette.warning_border)
    } else {
        (palette.success_bg, palette.success_border)
    };
    let has_ax_action = show_ax_btn && is_warn && cfg!(target_os = "macos");
    egui::Frame::NONE
        .fill(bg)
        .stroke(Stroke::new(1.0, border))
        .corner_radius(CornerRadius::same(14))
        .shadow(card_shadow(palette))
        .inner_margin(egui::Margin::symmetric(10, 6))
        .show(ui, |ui| {
            let inner_w = ui.available_width();
            ui.set_min_width(inner_w);
            ui.set_max_width(inner_w);
            let text_w = (inner_w - 28.0).max(80.0);
            ui.horizontal(|ui| {
                ui.allocate_ui_with_layout(
                    vec2(text_w, 18.0),
                    Layout::top_down(Align::Min),
                    |ui| {
                        ui.set_max_width(text_w);
                        ui.add(
                            egui::Label::new(
                                RichText::new(msg).size(11.5).color(palette.text_primary),
                            )
                            .wrap(),
                        );
                    },
                );
                ui.with_layout(Layout::right_to_left(Align::Min), |ui| {
                    if dismiss_button(ui).clicked() {
                        close_clicked = true;
                    }
                });
            });
            if has_ax_action {
                ui.add_space(4.0);
                if compact_button(ui, t("footer.open_accessibility")).clicked() {
                    actions.push(PopupAction::OpenAccessibilitySettings);
                }
            }
        });
    ui.add_space(4.0);
    close_clicked
}

pub(super) fn render_about_overlay(
    ctx: &egui::Context,
    state: &mut PopupState,
    palette: &EarthPalette,
) {
    egui::Window::new(t("about.window_title"))
        .collapsible(false)
        .resizable(false)
        .anchor(egui::Align2::CENTER_CENTER, vec2(0.0, 0.0))
        .fixed_size(vec2(280.0, 158.0))
        .show(ctx, |ui| {
            crate::app::handle_background_window_drag(
                ui,
                ctx,
                ui.max_rect(),
                "earth_about_modal_drag",
            );
            ui.vertical_centered(|ui| {
                ui.add_space(4.0);
                if draw_earth_emblem(ui, 42.0, palette, false).drag_started() {
                    ctx.send_viewport_cmd(egui::ViewportCommand::StartDrag);
                    ctx.stop_dragging();
                }
                ui.add_space(6.0);
                ui.label(RichText::new(t("about.app_name")).size(16.0).strong());
                ui.label(
                    RichText::new(concat!("v", env!("CARGO_PKG_VERSION"), " · Pure Rust"))
                        .size(11.5)
                        .color(palette.text_secondary),
                );
                ui.add_space(4.0);
                ui.label(
                    RichText::new(t("about.author"))
                        .size(12.0)
                        .color(palette.text_muted),
                );
                ui.add_space(12.0);
                let close_btn =
                    egui::Button::new(RichText::new(t("about.close")).color(palette.on_accent))
                        .fill(palette.accent)
                        .stroke(Stroke::NONE)
                        .corner_radius(CornerRadius::same(255));
                if ui.add(close_btn).clicked() {
                    state.show_about_modal = false;
                }
            });
        });
}

pub(super) fn path_to_file_uri(path: &str) -> String {
    let normalized = path.replace('\\', "/");
    let normalized = normalized
        .split('/')
        .map(|segment| urlencoding::encode(segment).replace("%3A", ":"))
        .collect::<Vec<_>>()
        .join("/");
    if normalized.starts_with('/') {
        format!("file://{normalized}")
    } else {
        format!("file:///{normalized}")
    }
}

pub(super) fn format_epoch_millis(millis: i64) -> String {
    if let Some(utc) = DateTime::from_timestamp_millis(millis) {
        let local: DateTime<Local> = utc.into();
        return local.format("%m-%d %H:%M:%S").to_string();
    }
    String::new()
}
