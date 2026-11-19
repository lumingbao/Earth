use super::*;

pub(super) const LIST_ROW_HEIGHT: f32 = 34.0;

#[allow(clippy::too_many_arguments)]
pub(super) fn render_item_row(
    ui: &mut Ui,
    res: &SearchResultItem,
    shortcut_num: Option<usize>,
    selected: bool,
    settings: &AppSettings,
    collections: &[Collection],
    state: &mut PopupState,
    palette: &EarthPalette,
    actions: &mut Vec<PopupAction>,
) -> egui::Response {
    let item = &res.item;
    let row_height = LIST_ROW_HEIGHT;

    let row_fill = if selected {
        palette.row_selected_bg
    } else {
        palette.surface_bg
    };

    let row_outer_w = ui.available_width();
    let row_inner_w = (row_outer_w - 16.0).max(80.0);
    let row_inner_h = (row_height - 8.0).max(20.0);

    let frame = egui::Frame::NONE
        .fill(row_fill)
        .stroke(Stroke::NONE)
        .corner_radius(CornerRadius::same(12))
        .inner_margin(egui::Margin::symmetric(8, 4));

    let inner = frame.show(ui, |ui| {
        // Disable text-selection cursor/sensing on row labels so the whole row acts as a clickable item
        ui.style_mut().interaction.selectable_labels = false;
        ui.set_min_size(vec2(row_inner_w, row_inner_h));
        ui.set_max_size(vec2(row_inner_w, row_inner_h));
        ui.spacing_mut().interact_size.y = row_inner_h;
        ui.horizontal_centered(|ui| {
            ui.set_max_width(row_inner_w);
            // Left selection bar slot (always allocated so text X-alignment and widget IDs stay stable)
            let (slot, _) = ui.allocate_exact_size(vec2(3.5, row_inner_h), Sense::hover());
            if selected && !state.external_selection_bar {
                let bar_h = (row_height - 16.0).max(16.0);
                let bar = egui::Rect::from_center_size(slot.center(), vec2(3.5, bar_h));
                ui.painter()
                    .rect_filled(bar, CornerRadius::same(2), palette.accent);
            }

            // Type / Color / Image thumbnail badge
            match item.kind {
                ClipKind::Text => {
                    if settings.show_hex_color_swatch
                        && let Some(rgba) = item.detected_color
                    {
                        let (swatch, swatch_resp) =
                            ui.allocate_exact_size(vec2(16.0, 16.0), Sense::hover());
                        paint_color_swatch(ui.painter(), swatch, 4, rgba, palette.border_strong);
                        let hex_tip = format_color_hex(rgba);
                        let rgb_tip = format_color_rgb(rgba);
                        let hsl_tip = format_color_hsl(rgba);
                        swatch_resp.on_hover_text(format!("{hex_tip}\n{rgb_tip}\n{hsl_tip}"));
                    }
                    if item.is_url {
                        let (r, _) = ui.allocate_exact_size(vec2(14.0, 14.0), Sense::hover());
                        draw_link_badge(ui.painter(), r, palette.accent);
                    } else if item.is_sensitive() && settings.enable_privacy_masking {
                        let (r, _) = ui.allocate_exact_size(vec2(14.0, 14.0), Sense::hover());
                        draw_shield_glyph(ui.painter(), r, palette.pin_amber);
                    }
                }
                ClipKind::Files => {
                    let (r, _) = ui.allocate_exact_size(vec2(15.0, 15.0), Sense::hover());
                    draw_file_badge(ui.painter(), r, palette.success);
                }
                ClipKind::Image => {
                    if let Some(thumb) = item.thumb_path.as_deref() {
                        let uri = path_to_file_uri(thumb);
                        state.thumbnail_cache.track(ui.ctx(), &uri);
                        ui.add(
                            egui::Image::from_uri(uri)
                                .max_size(vec2(36.0, 22.0))
                                .maintain_aspect_ratio(true)
                                .corner_radius(CornerRadius::same(4)),
                        );
                    }
                }
            }

            // Right reserved width for pin + app badge + shortcut pill
            let mut right_reserved = 6.0;
            if shortcut_num.is_some() {
                right_reserved += 42.0;
            }
            if item.pinned {
                right_reserved += 18.0;
            }
            if item.collection_id.is_some() {
                right_reserved += 14.0;
            }
            if settings.show_source_app_badge && item.source_app.is_some() {
                right_reserved += 74.0;
            }

            let text_w = (ui.available_width() - right_reserved).max(40.0);
            ui.allocate_ui(vec2(text_w, row_inner_h), |ui| {
                ui.set_min_size(vec2(text_w, row_inner_h));
                ui.set_max_size(vec2(text_w, row_inner_h));
                ui.with_layout(Layout::left_to_right(Align::Center), |ui| {
                    ui.set_max_width(text_w);
                    if let Some(note) = item
                        .note
                        .as_deref()
                        .map(str::trim)
                        .filter(|n| !n.is_empty())
                    {
                        let short_note = truncate_chars(note, 12);
                        egui::Frame::NONE
                            .fill(palette.accent_soft)
                            .stroke(Stroke::new(1.0, palette.border_strong))
                            .corner_radius(CornerRadius::same(255))
                            .inner_margin(egui::Margin::symmetric(6, 1))
                            .show(ui, |ui| {
                                ui.label(
                                    RichText::new(short_note)
                                        .size(10.5)
                                        .color(palette.accent_text),
                                );
                            });
                    }
                    let rem_text_w = ui.available_width().max(24.0);
                    match item.kind {
                        ClipKind::Text => {
                            let raw = item.text_content.as_deref().unwrap_or("");
                            if is_masked(item, settings.enable_privacy_masking, state) {
                                let masked = mask_sensitive_preview(raw);
                                let lbl = item.sensitive_label.as_deref().unwrap_or("敏感");
                                ui.add(
                                    egui::Label::new(
                                        RichText::new(format!("[{lbl}] {masked}"))
                                            .size(12.8)
                                            .color(palette.text_secondary),
                                    )
                                    .truncate(),
                                );
                            } else {
                                let clipped = crate::search::display_line(
                                    item,
                                    settings.show_special_symbols,
                                );
                                let job = build_highlighted_layout_job(
                                    &clipped,
                                    &res.matched_indices,
                                    settings.match_highlight,
                                    palette,
                                    13.0,
                                    rem_text_w,
                                    selected,
                                );
                                ui.add(egui::Label::new(job).truncate());
                            }
                        }
                        ClipKind::Image => {
                            let summary = item.summary_line(40);
                            ui.add(
                                egui::Label::new(
                                    RichText::new(summary)
                                        .size(12.2)
                                        .color(palette.text_secondary),
                                )
                                .truncate(),
                            );
                        }
                        ClipKind::Files => {
                            let summary = item.summary_line(60);
                            let job = build_highlighted_layout_job(
                                &summary,
                                &res.matched_indices,
                                settings.match_highlight,
                                palette,
                                13.0,
                                rem_text_w,
                                selected,
                            );
                            ui.add(egui::Label::new(job).truncate());
                        }
                    }
                });
            });

            // Right metadata cluster
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if let Some(num) = shortcut_num {
                    #[cfg(target_os = "macos")]
                    let label = format!("⌘ {num}");
                    #[cfg(not(target_os = "macos"))]
                    let label = format!("Alt+{num}");

                    let fg = if selected {
                        palette.on_accent
                    } else {
                        palette.keycap_text
                    };
                    let galley =
                        ui.fonts_mut(|f| f.layout_no_wrap(label, FontId::proportional(10.5), fg));
                    let pill_size = vec2(
                        (galley.size().x + 12.0).max(28.0),
                        (galley.size().y + 6.0).clamp(20.0, 22.0),
                    );
                    let (pill_rect, _) = ui.allocate_exact_size(pill_size, Sense::hover());
                    ui.painter().rect_filled(
                        pill_rect,
                        CornerRadius::same(255),
                        if selected {
                            palette.accent
                        } else {
                            palette.keycap_bg
                        },
                    );
                    if !selected {
                        ui.painter().rect_stroke(
                            pill_rect,
                            CornerRadius::same(255),
                            Stroke::new(1.0, palette.border_subtle),
                            egui::StrokeKind::Inside,
                        );
                    }
                    let text_pos = pill_rect.center() - galley.size() * 0.5;
                    ui.painter().galley(text_pos, galley, fg);
                }

                if item.pinned {
                    let (r, _) = ui.allocate_exact_size(vec2(12.0, 12.0), Sense::hover());
                    draw_pin_badge(ui.painter(), r, palette.pin_amber);
                }

                if let Some(cid) = item.collection_id
                    && let Some(col) = collections.iter().find(|c| c.id == cid)
                {
                    let c = parse_hex_color32(&col.color_hex).unwrap_or(palette.accent);
                    let (r, resp) = ui.allocate_exact_size(vec2(10.0, 10.0), Sense::hover());
                    ui.painter().circle_filled(r.center(), 4.0, c);
                    resp.on_hover_text(&col.name);
                }

                if settings.show_source_app_badge
                    && let Some(app) = &item.source_app
                {
                    let short = truncate_chars(app, 10);
                    ui.label(RichText::new(short).size(10.5).color(palette.text_muted));
                }
            });
        });
    });

    let resp = inner
        .response
        .interact(Sense::click())
        .on_hover_cursor(egui::CursorIcon::PointingHand);
    let pointer_in_row = resp.hovered() || ui.rect_contains_pointer(resp.rect);
    if pointer_in_row {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    let border_stroke = if selected {
        let blend = if state.external_selection_bar {
            state.selection_glide_blend.clamp(0.0, 1.0)
        } else {
            1.0
        };
        let accent = palette.accent;
        let alpha = (75.0 + 180.0 * blend).round() as u8;
        Stroke::new(
            1.2,
            Color32::from_rgba_unmultiplied(accent.r(), accent.g(), accent.b(), alpha),
        )
    } else if pointer_in_row && state.selected_id != Some(item.id) {
        Stroke::new(1.0, palette.border_strong)
    } else {
        Stroke::new(1.0, palette.border_subtle)
    };
    ui.painter().rect_stroke(
        resp.rect,
        CornerRadius::same(12),
        border_stroke,
        egui::StrokeKind::Inside,
    );

    match settings.copy_trigger_mode {
        CopyTriggerMode::DoubleClick => {
            if resp.double_clicked() {
                // Double click: copy / paste using the default behavior (modifiers still respected)
                state.select_item(Some(item.id));
                state.selection_changed_at = Instant::now();
                let mods = ui.ctx().input(|i| i.modifiers);
                let behavior = behavior_from_modifiers(mods);
                actions.push(PopupAction::PasteItem {
                    id: item.id,
                    behavior,
                });
            } else if resp.clicked() {
                // Single click: select the record and expand the quick-peek workbench (skip when dragging window)
                if state.selected_id != Some(item.id) {
                    state.select_item(Some(item.id));
                    state.selection_changed_at = Instant::now();
                    state.image_actual_size = false;
                }
                if !state.preview_open {
                    state.set_preview_open(true);

                    ui.ctx().request_repaint();
                }
            }
        }
        CopyTriggerMode::SingleClick => {
            if !state.preview_open {
                // Workbench is closed: single click directly copies/pastes without opening workbench
                if resp.clicked() {
                    state.select_item(Some(item.id));
                    state.selection_changed_at = Instant::now();
                    let mods = ui.ctx().input(|i| i.modifiers);
                    let behavior = behavior_from_modifiers(mods);
                    actions.push(PopupAction::PasteItem {
                        id: item.id,
                        behavior,
                    });
                }
            } else if resp.double_clicked() {
                // Workbench is open: double click always copies/pastes
                state.select_item(Some(item.id));
                state.selection_changed_at = Instant::now();
                let mods = ui.ctx().input(|i| i.modifiers);
                let behavior = behavior_from_modifiers(mods);
                actions.push(PopupAction::PasteItem {
                    id: item.id,
                    behavior,
                });
            } else if resp.clicked() {
                // Workbench is open: clicking another item switches the workbench preview;
                // clicking the already-selected item copies/pastes it.
                if state.selected_id != Some(item.id) {
                    state.select_item(Some(item.id));
                    state.selection_changed_at = Instant::now();
                    state.image_actual_size = false;
                } else {
                    let mods = ui.ctx().input(|i| i.modifiers);
                    let behavior = behavior_from_modifiers(mods);
                    actions.push(PopupAction::PasteItem {
                        id: item.id,
                        behavior,
                    });
                }
            }
        }
    }

    resp.context_menu(|ui| {
        if ui.button("复制并粘贴").clicked() {
            actions.push(PopupAction::PasteItem {
                id: item.id,
                behavior: PasteBehavior::CopyAndPaste,
            });
            ui.close();
        }
        if ui.button("仅复制到剪贴板").clicked() {
            actions.push(PopupAction::PasteItem {
                id: item.id,
                behavior: PasteBehavior::CopyOnly,
            });
            ui.close();
        }
        if item.kind == ClipKind::Text && ui.button("纯文本粘贴").clicked() {
            actions.push(PopupAction::PasteItem {
                id: item.id,
                behavior: PasteBehavior::CopyAndPastePlainText,
            });
            ui.close();
        }
        ui.separator();
        let pin_label = if item.pinned {
            "取消固定"
        } else {
            "固定记录"
        };
        if ui.button(pin_label).clicked() {
            actions.push(PopupAction::TogglePin(item.id));
            ui.close();
        }
        ui.menu_button("加入收藏分组", |ui| {
            if item.collection_id.is_some() && ui.button("移出收藏分组").clicked() {
                actions.push(PopupAction::SetCollection {
                    item_id: item.id,
                    collection_id: None,
                });
                ui.close();
            }
            for col in collections {
                if ui.button(&col.name).clicked() {
                    actions.push(PopupAction::SetCollection {
                        item_id: item.id,
                        collection_id: Some(col.id),
                    });
                    ui.close();
                }
            }
        });
        if ui.button("在右侧速览面板查看").clicked() {
            state.select_item(Some(item.id));
            state.set_preview_open(true);

            ui.close();
        }
        ui.separator();
        if ui
            .button(RichText::new("删除此记录").color(palette.danger))
            .clicked()
        {
            actions.push(PopupAction::DeleteItem(item.id));
            ui.close();
        }
    });

    resp
}
