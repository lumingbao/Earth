use super::*;

pub(super) fn render_preview_pane(
    ui: &mut Ui,
    item: Option<&Arc<ClipItem>>,
    privacy_masking: bool,
    collections: &[Collection],
    state: &mut PopupState,
    palette: &EarthPalette,
    actions: &mut Vec<PopupAction>,
) {
    let inner_w = (ui.available_width() - 24.0).max(160.0);
    let inner_h = (ui.available_height() - 24.0).max(120.0);
    egui::Frame::NONE
        .fill(palette.surface_bg)
        .stroke(Stroke::new(1.0, palette.border_subtle))
        .corner_radius(CornerRadius::same(16))
        .shadow(card_shadow(palette))
        .inner_margin(egui::Margin::same(12))
        .show(ui, |ui| {
            ui.set_min_size(vec2(inner_w, inner_h));
            ui.set_max_size(vec2(inner_w, inner_h));

            let Some(item) = item else {
                ui.centered_and_justified(|ui| {
                    ui.label(
                        RichText::new(t("workbench.empty_hint"))
                            .size(13.0)
                            .color(palette.text_muted),
                    );
                });
                return;
            };

            // Preview Metadata Header
            let kind_label = match item.kind {
                ClipKind::Text => {
                    if item.detected_color.is_some() {
                        t("workbench.kind.color")
                    } else if item.is_url {
                        t("workbench.kind.url")
                    } else {
                        t("workbench.kind.text")
                    }
                }
                ClipKind::Image => t("workbench.kind.image"),
                ClipKind::Files => t("workbench.kind.files"),
            };
            state.ensure_preview_cache(item);
            let ts_str = state
                .preview_cache
                .as_ref()
                .map(|c| c.formatted_ts.as_str())
                .unwrap_or("");
            let stats_suffix = state
                .preview_cache
                .as_ref()
                .and_then(|c| c.text_stats)
                .map(|(lines, chars, words)| {
                    tf(
                        "workbench.text_stats",
                        &[
                            ("lines", &lines.to_string()),
                            ("chars", &chars.to_string()),
                            ("words", &words.to_string()),
                        ],
                    )
                })
                .unwrap_or_default();

            ui.horizontal(|ui| {
                let (tag_bg, tag_border, tag_fg) = antd_kind_tag_colors(item, palette);
                egui::Frame::NONE
                    .fill(tag_bg)
                    .stroke(Stroke::new(1.0, tag_border))
                    .corner_radius(CornerRadius::same(255))
                    .inner_margin(egui::Margin::symmetric(8, 2))
                    .show(ui, |ui| {
                        ui.label(RichText::new(kind_label).size(11.0).color(tag_fg));
                    });
                let count_s = item.copy_count.to_string();
                let size_s = format_bytes(item.byte_size);
                ui.add(
                    egui::Label::new(
                        RichText::new(tf(
                            "workbench.meta_summary",
                            &[
                                ("ts", ts_str),
                                ("count", &count_s),
                                ("size", &size_s),
                                ("stats", &stats_suffix),
                            ],
                        ))
                        .size(11.2)
                        .color(palette.text_secondary),
                    )
                    .truncate(),
                );
            });

            if let Some(app) = &item.source_app {
                ui.label(
                    RichText::new(tf("workbench.source_app", &[("app", app)]))
                        .size(11.0)
                        .color(palette.text_muted),
                );
            }

            let is_editing_text = state
                .inline_edit_text
                .as_ref()
                .map(|(id, _)| *id == item.id)
                .unwrap_or(false);
            let is_editing_note = state
                .inline_edit_note
                .as_ref()
                .map(|(id, _)| *id == item.id)
                .unwrap_or(false);

            ui.add_space(4.0);
            ui.horizontal(|ui| {
                let pin_btn = if item.pinned {
                    egui::Button::new(
                        RichText::new(t("workbench.pinned"))
                            .size(11.5)
                            .color(palette.warning_text),
                    )
                    .fill(palette.warning_bg)
                    .stroke(Stroke::new(1.0, palette.warning_border))
                    .corner_radius(CornerRadius::same(255))
                } else {
                    egui::Button::new(RichText::new(t("workbench.pin")).size(11.5))
                        .corner_radius(CornerRadius::same(255))
                };
                if ui.add(pin_btn).clicked() {
                    actions.push(PopupAction::TogglePin(item.id));
                }

                // Collection assignment dropdown
                let cur_col = item
                    .collection_id
                    .and_then(|cid| collections.iter().find(|c| c.id == cid))
                    .map(|c| c.name.as_str())
                    .unwrap_or(t("workbench.collection_placeholder"));
                egui::ComboBox::from_id_salt(("preview_col", item.id))
                    .selected_text(cur_col)
                    .width(110.0)
                    .show_ui(ui, |ui| {
                        if ui
                            .selectable_label(
                                item.collection_id.is_none(),
                                t("workbench.collection_none"),
                            )
                            .clicked()
                        {
                            actions.push(PopupAction::SetCollection {
                                item_id: item.id,
                                collection_id: None,
                            });
                        }
                        for col in collections {
                            if ui
                                .selectable_label(item.collection_id == Some(col.id), &col.name)
                                .clicked()
                            {
                                actions.push(PopupAction::SetCollection {
                                    item_id: item.id,
                                    collection_id: Some(col.id),
                                });
                            }
                        }
                    });

                let note_lbl = if item
                    .note
                    .as_deref()
                    .map(str::trim)
                    .is_some_and(|n| !n.is_empty())
                {
                    t("workbench.note")
                } else {
                    t("workbench.add_note")
                };
                if compact_button(ui, note_lbl).clicked() {
                    if is_editing_note {
                        state.inline_edit_note = None;
                    } else {
                        state.inline_edit_note =
                            Some((item.id, item.note.clone().unwrap_or_default()));
                    }
                }

                if item.kind == ClipKind::Text && !is_masked(item, privacy_masking, state) {
                    let edit_lbl = if is_editing_text {
                        t("workbench.exit_edit")
                    } else {
                        t("workbench.edit")
                    };
                    if compact_button(ui, edit_lbl).clicked() {
                        if is_editing_text {
                            state.inline_edit_text = None;
                        } else {
                            let raw = item.text_content.as_deref().unwrap_or("");
                            state.inline_edit_text = Some((item.id, raw.to_string()));
                            state.qr_preview_id = None;
                        }
                    }
                }

                if item.is_sensitive() && privacy_masking {
                    let revealed = state.revealed_sensitive_ids.contains(&item.id);
                    let btn_lbl = if revealed {
                        t("workbench.hide_plain")
                    } else {
                        t("workbench.show_plain")
                    };
                    if compact_button(ui, btn_lbl).clicked() {
                        if revealed {
                            state.clear_sensitive_context();
                        } else {
                            state.revealed_sensitive_ids.insert(item.id);
                        }
                    }
                }

                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    let del_btn = egui::Button::new(
                        RichText::new(t("workbench.delete")).color(palette.danger),
                    )
                    .fill(palette.danger_bg)
                    .stroke(Stroke::new(1.0, palette.danger_border))
                    .corner_radius(CornerRadius::same(255));
                    if padded_button(ui, del_btn, vec2(11.0, 4.5)).clicked() {
                        actions.push(PopupAction::DeleteItem(item.id));
                    }
                });
            });

            // Optional Note Editor / Note Display Bar
            if is_editing_note {
                ui.add_space(3.0);
                let mut save_note = None;
                let mut cancel_note = false;
                if let Some((_, ref mut note_buf)) = state.inline_edit_note {
                    ui.horizontal(|ui| {
                        let edit_w = (ui.available_width() - 116.0).max(80.0);
                        let resp = ui
                            .scope(|ui| {
                                let r = CornerRadius::same(8);
                                ui.visuals_mut().widgets.inactive.corner_radius = r;
                                ui.visuals_mut().widgets.hovered.corner_radius = r;
                                ui.visuals_mut().widgets.active.corner_radius = r;
                                ui.add(
                                    egui::TextEdit::singleline(note_buf)
                                        .hint_text(t("workbench.note_hint"))
                                        .margin(egui::Margin::symmetric(8, 4))
                                        .desired_width(edit_w),
                                )
                            })
                            .inner;
                        let enter_hit =
                            resp.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter));
                        let save_btn = egui::Button::new(
                            RichText::new(t("workbench.save"))
                                .size(11.0)
                                .color(palette.on_accent),
                        )
                        .fill(palette.accent)
                        .stroke(Stroke::NONE)
                        .corner_radius(CornerRadius::same(255));
                        if padded_button(ui, save_btn, vec2(9.0, 3.0)).clicked() || enter_hit {
                            save_note = Some(note_buf.trim().to_string());
                        }
                        if compact_button(ui, t("workbench.clear")).clicked() {
                            save_note = Some(String::new());
                        }
                        if dismiss_button(ui).clicked() {
                            cancel_note = true;
                        }
                    });
                }
                if let Some(txt) = save_note {
                    actions.push(PopupAction::SetItemNote {
                        item_id: item.id,
                        note: (!txt.is_empty()).then_some(txt),
                    });
                    state.inline_edit_note = None;
                } else if cancel_note {
                    state.inline_edit_note = None;
                }
            } else if let Some(note) = item
                .note
                .as_deref()
                .map(str::trim)
                .filter(|n| !n.is_empty())
            {
                ui.add_space(2.0);
                ui.horizontal(|ui| {
                    ui.label(
                        RichText::new(tf("workbench.note_display", &[("note", note)]))
                            .size(11.5)
                            .color(palette.accent_text),
                    );
                    if compact_button(ui, t("workbench.modify")).clicked() {
                        state.inline_edit_note = Some((item.id, note.to_string()));
                    }
                    if dismiss_button(ui)
                        .on_hover_text(t("workbench.clear_note_tip"))
                        .clicked()
                    {
                        actions.push(PopupAction::SetItemNote {
                            item_id: item.id,
                            note: None,
                        });
                    }
                });
            }

            ui.separator();

            // Main Rich Body (leave space for Quick Actions bar at bottom)
            let actions_area_h = match item.kind {
                ClipKind::Text => {
                    if state
                        .active_transform
                        .as_ref()
                        .map(|(id, _, _)| *id == item.id)
                        .unwrap_or(false)
                    {
                        176.0
                    } else {
                        76.0
                    }
                }
                _ => 42.0,
            };
            let body_h = (ui.available_height() - actions_area_h).max(100.0);

            ScrollArea::vertical()
                .id_salt(("preview_body", item.id))
                .max_height(body_h)
                .auto_shrink([false, false])
                .scroll_source(desktop_scroll_source())
                .show(ui, |ui| {
                    ui.set_min_height(body_h);
                    match item.kind {
                        ClipKind::Text => {
                            let raw = item.text_content.as_deref().unwrap_or("");
                            let hidden = is_masked(item, privacy_masking, state);
                            if is_editing_text && !hidden {
                                let mut commit_edit: Option<(String, bool)> = None;
                                let mut cancel_edit = false;
                                if let Some((_, ref mut edit_buf)) = state.inline_edit_text {
                                    ui.horizontal(|ui| {
                                        let save_btn = egui::Button::new(
                                            RichText::new(t("workbench.save_changes"))
                                                .size(11.0)
                                                .color(palette.on_accent),
                                        )
                                        .fill(palette.accent)
                                        .stroke(Stroke::NONE)
                                        .corner_radius(CornerRadius::same(255));
                                        if padded_button(ui, save_btn, vec2(10.0, 3.5)).clicked() {
                                            commit_edit = Some((edit_buf.clone(), false));
                                        }
                                        if compact_button(ui, t("workbench.save_and_paste"))
                                            .clicked()
                                        {
                                            commit_edit = Some((edit_buf.clone(), true));
                                        }
                                        if compact_button(ui, t("workbench.cancel")).clicked() {
                                            cancel_edit = true;
                                        }
                                    });
                                    ui.add_space(4.0);
                                    ui.scope(|ui| {
                                        let r = CornerRadius::same(10);
                                        ui.visuals_mut().widgets.inactive.corner_radius = r;
                                        ui.visuals_mut().widgets.hovered.corner_radius = r;
                                        ui.visuals_mut().widgets.active.corner_radius = r;
                                        ui.add(
                                            egui::TextEdit::multiline(edit_buf)
                                                .font(FontId::monospace(12.2))
                                                .margin(egui::Margin::same(10))
                                                .desired_width(f32::INFINITY)
                                                .desired_rows(8),
                                        );
                                    });
                                }
                                if let Some((new_text, paste_after)) = commit_edit {
                                    actions.push(PopupAction::UpdateItemText {
                                        item_id: item.id,
                                        new_text,
                                        paste_after,
                                    });
                                    state.inline_edit_text = None;
                                } else if cancel_edit {
                                    state.inline_edit_text = None;
                                }
                            } else if hidden {
                                let label = item
                                    .sensitive_label
                                    .as_deref()
                                    .unwrap_or(t("workbench.sensitive_default_label"));
                                ui.label(
                                    RichText::new(tf(
                                        "workbench.sensitive_notice",
                                        &[("label", label)],
                                    ))
                                    .size(12.5)
                                    .color(palette.text_secondary),
                                );
                            } else if state.qr_preview_id == Some(item.id) {
                                let qr_res = state
                                    .preview_cache
                                    .as_ref()
                                    .and_then(|c| c.qr_matrix.clone());
                                if let Some(res) = qr_res {
                                    let texture = state.preview_cache.as_mut().and_then(|cache| {
                                        if cache.qr_texture.is_none()
                                            && let Ok((width, modules)) = &res
                                            && let Ok(pixels) = qr_pixels(*width, modules)
                                        {
                                            cache.qr_texture = Some(ui.ctx().load_texture(
                                                format!("earth-qr:{}", item.id),
                                                pixels,
                                                egui::TextureOptions::NEAREST,
                                            ));
                                        }
                                        cache.qr_texture.clone()
                                    });
                                    render_qr_card(
                                        ui,
                                        &res,
                                        texture.as_ref(),
                                        body_h,
                                        palette,
                                        &mut state.qr_export,
                                    );
                                }
                            } else {
                                if let Some(rgba) = item.detected_color {
                                    render_color_card(ui, rgba, palette, actions);
                                    ui.add_space(6.0);
                                }
                                let animating_wb = state.preview_reveal.is_some_and(|r| r < 0.999);
                                let display = state
                                    .preview_cache
                                    .as_ref()
                                    .map(|c| {
                                        if animating_wb {
                                            c.anim_display_text.as_str()
                                        } else {
                                            c.display_text.as_str()
                                        }
                                    })
                                    .unwrap_or(raw);
                                ui.add(
                                    egui::Label::new(
                                        RichText::new(display)
                                            .font(FontId::monospace(12.2))
                                            .color(palette.text_primary),
                                    )
                                    .wrap(),
                                );
                            }
                        }
                        ClipKind::Image => {
                            let preview_path = if state.image_actual_size {
                                item.image_path.as_deref().or(item.thumb_path.as_deref())
                            } else {
                                item.thumb_path.as_deref().or(item.image_path.as_deref())
                            };
                            if let Some(path) = preview_path {
                                let max_w = ui.available_width().max(120.0);
                                let max_h = (body_h - 32.0).max(100.0);
                                match state.image_preview.texture(ui.ctx(), path) {
                                    Ok(Some(texture)) => {
                                        ui.centered_and_justified(|ui| {
                                            ui.add(
                                                egui::Image::from_texture(&texture)
                                                    .max_size(vec2(max_w, max_h))
                                                    .corner_radius(CornerRadius::same(12)),
                                            );
                                        });
                                    }
                                    Ok(None) => {
                                        ui.spinner();
                                        ui.label(t("workbench.loading_preview"));
                                    }
                                    Err(error) => {
                                        ui.label(RichText::new(error).color(palette.danger));
                                    }
                                }
                            }
                        }
                        ClipKind::Files => {
                            for (idx, f) in item.files.iter().take(MAX_PREVIEW_FILES).enumerate() {
                                let exists = state
                                    .preview_cache
                                    .as_ref()
                                    .and_then(|c| c.file_exists.get(idx).copied())
                                    .unwrap_or(true);
                                egui::Frame::NONE
                                    .fill(palette.card_bg)
                                    .stroke(Stroke::new(1.0, palette.border_subtle))
                                    .corner_radius(CornerRadius::same(12))
                                    .inner_margin(egui::Margin::same(8))
                                    .show(ui, |ui| {
                                        ui.set_width(ui.available_width());
                                        ui.horizontal(|ui| {
                                            ui.label(
                                                RichText::new(if f.is_dir {
                                                    "📁"
                                                } else {
                                                    "📄"
                                                })
                                                .size(13.0),
                                            );
                                            ui.vertical(|ui| {
                                                ui.label(
                                                    RichText::new(&f.name).strong().size(12.5),
                                                );
                                                ui.label(
                                                    RichText::new(&f.path)
                                                        .size(11.0)
                                                        .color(palette.text_muted),
                                                );
                                                let status = if exists {
                                                    if f.is_dir {
                                                        t("workbench.file_dir_valid").to_string()
                                                    } else {
                                                        let size_s = format_bytes(f.size_bytes);
                                                        tf(
                                                            "workbench.file_valid",
                                                            &[("size", &size_s)],
                                                        )
                                                    }
                                                } else {
                                                    t("workbench.file_missing").to_string()
                                                };
                                                ui.label(RichText::new(status).size(10.5).color(
                                                    if exists {
                                                        palette.success_text
                                                    } else {
                                                        palette.danger_text
                                                    },
                                                ));
                                            });
                                            if exists {
                                                ui.with_layout(
                                                    Layout::right_to_left(Align::Center),
                                                    |ui| {
                                                        if compact_button(ui, t("workbench.reveal"))
                                                            .clicked()
                                                        {
                                                            actions.push(PopupAction::RevealFile(
                                                                PathBuf::from(&f.path),
                                                            ));
                                                        }
                                                    },
                                                );
                                            }
                                        });
                                    });
                                ui.add_space(4.0);
                            }
                            if item.files.len() > MAX_PREVIEW_FILES {
                                let rem = (item.files.len() - MAX_PREVIEW_FILES).to_string();
                                ui.label(
                                    RichText::new(tf(
                                        "workbench.files_remaining",
                                        &[("count", &rem)],
                                    ))
                                    .size(11.0)
                                    .color(palette.text_muted),
                                );
                            }
                        }
                    }
                });

            ui.separator();

            // Quick Transform Actions Bar (Earth signature!)
            match item.kind {
                ClipKind::Text => {
                    let raw = item.text_content.as_deref().unwrap_or("");
                    let quick_list = state
                        .preview_cache
                        .as_ref()
                        .map(|c| c.quick_actions.clone())
                        .unwrap_or_default();
                    let hidden = is_masked(item, privacy_masking, state);
                    ui.horizontal_wrapped(|ui| {
                        if item.is_url && compact_button(ui, t("workbench.open_url")).clicked() {
                            actions.push(PopupAction::OpenUrl(raw.trim().to_string()));
                        }
                        if raw.len() <= 2048 && !hidden {
                            let qr_active = state.qr_preview_id == Some(item.id);
                            let qr_lbl = if qr_active {
                                t("workbench.hide_qr")
                            } else {
                                t("workbench.show_qr")
                            };
                            if compact_button(ui, qr_lbl).clicked() {
                                if qr_active {
                                    state.qr_preview_id = None;
                                } else {
                                    state.qr_preview_id = Some(item.id);
                                    state.inline_edit_text = None;
                                }
                                ui.ctx().request_repaint();
                            }
                        }
                        for qa in quick_list.into_iter().filter(|_| !hidden) {
                            if compact_button(ui, qa.label()).clicked() {
                                let res = apply_quick_action(qa, raw);
                                state.active_transform = Some((item.id, qa, res));
                            }
                        }
                    });

                    let mut clear_transform = false;
                    if let Some((tid, qa, ref res)) = state.active_transform
                        && tid == item.id
                        && !hidden
                    {
                        ui.add_space(4.0);
                        egui::Frame::NONE
                            .fill(palette.info_bg)
                            .stroke(Stroke::new(1.0, palette.info_border))
                            .corner_radius(CornerRadius::same(14))
                            .inner_margin(egui::Margin::same(8))
                            .show(ui, |ui| {
                                ui.set_width(ui.available_width());
                                ui.horizontal(|ui| {
                                    ui.label(
                                        RichText::new(tf(
                                            "workbench.qa_result",
                                            &[("action", qa.label())],
                                        ))
                                        .size(11.5)
                                        .strong()
                                        .color(palette.accent_text),
                                    );
                                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                        if dismiss_button(ui).clicked() {
                                            clear_transform = true;
                                        }
                                        if let Ok(txt) = res {
                                            let paste_btn = egui::Button::new(
                                                RichText::new(t("workbench.paste_result"))
                                                    .size(11.0)
                                                    .color(palette.on_accent),
                                            )
                                            .fill(palette.accent)
                                            .stroke(Stroke::NONE)
                                            .corner_radius(CornerRadius::same(255));
                                            if padded_button(ui, paste_btn, vec2(10.0, 4.5))
                                                .clicked()
                                            {
                                                actions.push(PopupAction::PasteTransformedText(
                                                    txt.clone(),
                                                ));
                                            }
                                            if compact_button(ui, t("workbench.copy_result"))
                                                .clicked()
                                            {
                                                actions.push(PopupAction::CopyTransformedText(
                                                    txt.clone(),
                                                ));
                                            }
                                        }
                                    });
                                });
                                match res {
                                    Ok(out) => {
                                        ScrollArea::vertical()
                                            .id_salt("qa_result_scroll")
                                            .max_height(56.0)
                                            .scroll_source(desktop_scroll_source())
                                            .show(ui, |ui| {
                                                ui.add(
                                                    egui::Label::new(
                                                        RichText::new(out)
                                                            .font(FontId::monospace(11.5))
                                                            .color(palette.text_primary),
                                                    )
                                                    .wrap(),
                                                );
                                            });
                                    }
                                    Err(err) => {
                                        ui.label(
                                            RichText::new(err).size(11.5).color(palette.danger),
                                        );
                                    }
                                }
                            });
                    }
                    if clear_transform {
                        state.active_transform = None;
                    }
                }
                ClipKind::Image => {
                    ui.horizontal(|ui| {
                        let zoom_lbl = if state.image_actual_size {
                            t("workbench.image_fit")
                        } else {
                            t("workbench.image_crisp")
                        };
                        if compact_button(ui, zoom_lbl).clicked() {
                            state.image_actual_size = !state.image_actual_size;
                        }
                        if let Some(path) = &item.image_path
                            && compact_button(ui, t("workbench.save_image_as")).clicked()
                        {
                            actions.push(PopupAction::SaveImageAs(path.clone()));
                        }
                        if compact_button(ui, t("workbench.copy_and_paste_image")).clicked() {
                            actions.push(PopupAction::PasteItem {
                                id: item.id,
                                behavior: PasteBehavior::CopyAndPaste,
                            });
                        }
                    });
                }
                ClipKind::Files => {
                    ui.horizontal(|ui| {
                        if compact_button(ui, t("workbench.copy_paths")).clicked() {
                            let paths = item
                                .files
                                .iter()
                                .map(|f| f.path.as_str())
                                .collect::<Vec<_>>()
                                .join("\n");
                            actions.push(PopupAction::CopyTransformedText(paths));
                        }
                        let first_valid_idx = state
                            .preview_cache
                            .as_ref()
                            .and_then(|c| c.file_exists.iter().position(|&ok| ok));
                        if let Some(first) = first_valid_idx.and_then(|i| item.files.get(i))
                            && compact_button(ui, t("workbench.reveal_in_manager")).clicked()
                        {
                            actions.push(PopupAction::RevealFile(PathBuf::from(&first.path)));
                        }
                    });
                }
            }
        });
}
