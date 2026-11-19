use super::*;

pub(super) fn draw_qr_square(ui: &mut Ui, canvas_px: f32, texture: &egui::TextureHandle) {
    ui.add(egui::Image::from_texture(texture).fit_to_exact_size(vec2(canvas_px, canvas_px)));
}

pub(super) fn draw_qr_action_buttons(
    ui: &mut Ui,
    width: usize,
    modules: &[bool],
    palette: &EarthPalette,
    export: &mut QrExport,
) {
    let copy_btn = egui::Button::new(
        RichText::new(t("qr.copy_image"))
            .size(11.5)
            .color(palette.on_accent),
    )
    .fill(palette.accent)
    .stroke(Stroke::NONE)
    .corner_radius(CornerRadius::same(255));
    if ui
        .add_enabled_ui(!export.is_busy(), |ui| {
            padded_button(ui, copy_btn, vec2(11.0, 4.5))
        })
        .inner
        .clicked()
        && let Err(error) = export.start(ui.ctx(), width, modules.to_vec(), false)
    {
        ui.label(error);
    }
}

pub(super) fn draw_qr_save_button(
    ui: &mut Ui,
    width: usize,
    modules: &[bool],
    export: &mut QrExport,
) {
    if ui
        .add_enabled_ui(!export.is_busy(), |ui| {
            compact_button(ui, RichText::new(t("qr.save_image")).size(11.5))
        })
        .inner
        .clicked()
        && let Err(error) = export.start(ui.ctx(), width, modules.to_vec(), true)
    {
        ui.label(error);
    }
}

pub(super) fn render_qr_card(
    ui: &mut Ui,
    qr_res: &Result<(usize, Vec<bool>), String>,
    texture: Option<&egui::TextureHandle>,
    body_h: f32,
    palette: &EarthPalette,
    export: &mut QrExport,
) {
    let card_outer_w = ui.available_width().max(160.0);
    let card_outer_h = (body_h - 2.0).max(116.0);
    let card_inner_w = (card_outer_w - 20.0).max(140.0);
    let card_inner_h = (card_outer_h - 20.0).max(96.0);

    egui::Frame::NONE
        .fill(palette.card_bg)
        .stroke(Stroke::new(1.0, palette.border_subtle))
        .corner_radius(CornerRadius::same(14))
        .inner_margin(egui::Margin::same(10))
        .show(ui, |ui| {
            ui.set_min_size(vec2(card_inner_w, card_inner_h));
            ui.set_max_size(vec2(card_inner_w, card_inner_h));

            match qr_res {
                Ok((width, modules)) if *width > 0 => {
                    let font_id = FontId::proportional(11.5);
                    let (w1, w2, btn_h) = ui.fonts_mut(|f| {
                        let g1 = f.layout_no_wrap(
                            t("qr.copy_image").into(),
                            font_id.clone(),
                            Color32::WHITE,
                        );
                        let g2 =
                            f.layout_no_wrap(t("qr.save_image").into(), font_id, Color32::WHITE);
                        (
                            g1.size().x + 22.0,
                            g2.size().x + 22.0,
                            g1.size().y.max(g2.size().y) + 9.0,
                        )
                    });

                    let quiet_zone = 4usize;
                    let total_modules = (*width + quiet_zone * 2) as f32;

                    if card_inner_h >= 150.0 {
                        // Centered vertical stack: crisp QR square above centered action pills
                        let v_gap = 12.0;
                        let h_gap = 8.0;
                        let target_px = (card_inner_h - btn_h - v_gap - 8.0).clamp(112.0, 156.0);
                        let cell_px = target_px / total_modules;
                        let actual_px = cell_px * total_modules;
                        let total_h = actual_px + v_gap + btn_h;
                        let top_pad = ((card_inner_h - total_h) * 0.5).max(0.0);

                        ui.vertical(|ui| {
                            ui.spacing_mut().item_spacing.y = 0.0;
                            ui.add_space(top_pad);
                            ui.horizontal(|ui| {
                                ui.spacing_mut().item_spacing.x = 0.0;
                                ui.add_space(((card_inner_w - actual_px) * 0.5).max(0.0));
                                if let Some(texture) = texture {
                                    draw_qr_square(ui, target_px, texture);
                                }
                            });
                            ui.add_space(v_gap);
                            ui.horizontal(|ui| {
                                ui.spacing_mut().item_spacing.x = 0.0;
                                let total_btn_w = w1 + h_gap + w2;
                                ui.add_space(((card_inner_w - total_btn_w) * 0.5).max(0.0));
                                draw_qr_action_buttons(ui, *width, modules, palette, export);
                                ui.add_space(h_gap);
                                draw_qr_save_button(ui, *width, modules, export);
                            });
                        });
                    } else {
                        // Compact centered horizontal group when transform drawer reduces height
                        let target_px = (card_inner_h - 6.0).clamp(76.0, 132.0);
                        let cell_px = target_px / total_modules;
                        let actual_px = cell_px * total_modules;
                        let col_w = w1.max(w2);
                        let h_gap = 16.0;
                        let total_w = actual_px + h_gap + col_w;
                        let col_h = btn_h * 2.0 + 8.0;
                        let lead_x = ((card_inner_w - total_w) * 0.5).max(0.0);

                        ui.horizontal(|ui| {
                            ui.spacing_mut().item_spacing.x = 0.0;
                            ui.add_space(lead_x);
                            ui.vertical(|ui| {
                                ui.spacing_mut().item_spacing.y = 0.0;
                                ui.add_space(((card_inner_h - actual_px) * 0.5).max(0.0));
                                if let Some(texture) = texture {
                                    draw_qr_square(ui, target_px, texture);
                                }
                            });
                            ui.add_space(h_gap);
                            ui.vertical(|ui| {
                                ui.spacing_mut().item_spacing.y = 0.0;
                                ui.add_space(((card_inner_h - col_h) * 0.5).max(0.0));
                                draw_qr_action_buttons(ui, *width, modules, palette, export);
                                ui.add_space(8.0);
                                draw_qr_save_button(ui, *width, modules, export);
                            });
                        });
                    }
                }
                Err(err) => {
                    ui.centered_and_justified(|ui| {
                        ui.label(RichText::new(err).size(11.5).color(palette.danger));
                    });
                }
                _ => {}
            }
        });
}

pub(super) fn paint_color_swatch(
    painter: &egui::Painter,
    rect: egui::Rect,
    radius: u8,
    rgba: [u8; 4],
    border_color: Color32,
) {
    let rounding = CornerRadius::same(radius);
    if rgba[3] < 255 {
        painter.rect_filled(rect, rounding, Color32::from_rgb(235, 235, 240));
        let inset = (radius as f32 * 0.30).max(1.0);
        let inner = rect.shrink(inset);
        if inner.width() >= 4.0 && inner.height() >= 4.0 {
            let cells: usize = if inner.width() >= 28.0 { 6 } else { 3 };
            let cell_w = inner.width() / cells as f32;
            let cell_h = inner.height() / cells as f32;
            let dark_tile = Color32::from_rgb(196, 196, 204);
            for row in 0..cells {
                for col in 0..cells {
                    if (row + col) % 2 == 1 {
                        let min = pos2(
                            inner.min.x + col as f32 * cell_w,
                            inner.min.y + row as f32 * cell_h,
                        );
                        let tile = egui::Rect::from_min_size(min, vec2(cell_w, cell_h));
                        painter.rect_filled(tile, CornerRadius::ZERO, dark_tile);
                    }
                }
            }
        }
    }
    painter.rect_filled(
        rect,
        rounding,
        Color32::from_rgba_unmultiplied(rgba[0], rgba[1], rgba[2], rgba[3]),
    );
    painter.rect_stroke(
        rect,
        rounding,
        Stroke::new(1.0, border_color),
        egui::StrokeKind::Inside,
    );
}

pub(super) fn render_color_card(
    ui: &mut Ui,
    rgba: [u8; 4],
    palette: &EarthPalette,
    actions: &mut Vec<PopupAction>,
) {
    let hex_str = format_color_hex(rgba);
    let rgb_str = format_color_rgb(rgba);
    let hsl_str = format_color_hsl(rgba);
    let rgb_btn = if rgba[3] == 255 {
        t("color.copy_rgb")
    } else {
        t("color.copy_rgba")
    };
    let hsl_btn = if rgba[3] == 255 {
        t("color.copy_hsl")
    } else {
        t("color.copy_hsla")
    };

    egui::Frame::NONE
        .fill(palette.card_bg)
        .stroke(Stroke::new(1.0, palette.border_subtle))
        .corner_radius(CornerRadius::same(14))
        .inner_margin(egui::Margin::same(10))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal(|ui| {
                ui.vertical(|ui| {
                    let swatch_size = if rgba[3] < 255 { 48.0 } else { 54.0 };
                    let (r, _) =
                        ui.allocate_exact_size(vec2(swatch_size, swatch_size), Sense::hover());
                    paint_color_swatch(ui.painter(), r, 10, rgba, palette.border_strong);
                    if rgba[3] < 255 {
                        let alpha_pct = ((rgba[3] as f32 / 255.0) * 100.0).round() as u8;
                        ui.add_space(2.0);
                        ui.label(
                            RichText::new(tf("color.alpha", &[("pct", &alpha_pct.to_string())]))
                                .size(10.0)
                                .color(palette.text_muted),
                        );
                    }
                });
                ui.add_space(8.0);
                ui.vertical(|ui| {
                    ui.spacing_mut().item_spacing.y = 4.0;
                    let rows: [(&str, &str, bool); 3] = [
                        (&hex_str, t("color.copy_hex"), true),
                        (&rgb_str, rgb_btn, false),
                        (&hsl_str, hsl_btn, false),
                    ];
                    let col_w = ui.available_width().max(120.0);
                    for (val, btn_lbl, primary) in rows {
                        ui.allocate_ui_with_layout(
                            vec2(col_w, 22.0),
                            Layout::right_to_left(Align::Center),
                            |ui| {
                                if compact_button(ui, btn_lbl).clicked() {
                                    actions.push(PopupAction::CopyTransformedText(val.to_string()));
                                }
                                ui.with_layout(Layout::left_to_right(Align::Center), |ui| {
                                    let label = if primary {
                                        RichText::new(val)
                                            .font(FontId::monospace(12.6))
                                            .strong()
                                            .color(palette.text_primary)
                                    } else {
                                        RichText::new(val)
                                            .font(FontId::monospace(11.4))
                                            .color(palette.text_secondary)
                                    };
                                    ui.add(egui::Label::new(label).truncate());
                                });
                            },
                        );
                    }
                });
            });
        });
}
