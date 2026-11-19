use super::*;

pub fn render_popup_view(
    ui: &mut Ui,
    ctx: &egui::Context,
    data: PopupViewData<'_>,
    state: &mut PopupState,
) -> Vec<PopupAction> {
    let PopupViewData {
        settings,
        results,
        collections,
        palette,
        storage_bytes,
        total_items,
        has_accessibility,
    } = data;
    let mut actions = Vec::new();

    // Ensure a valid selected item
    if !results.is_empty() {
        let valid = state
            .selected_id
            .map(|sid| results.iter().any(|r| r.item.id == sid))
            .unwrap_or(false);
        if !valid {
            state.select_item(Some(results[0].item.id));
            state.selection_changed_at = Instant::now();
            state.reset_list_navigation();
        }
    } else {
        state.select_item(None);
        state.reset_list_navigation();
    }

    // Track macOS / Windows IME composition state so Chinese Pinyin works from the 1st keystroke
    ctx.input(|i| {
        for ev in &i.events {
            if let egui::Event::Ime(ime) = ev {
                match ime {
                    egui::ImeEvent::Preedit { text, .. } => {
                        state.ime_composing = !text.is_empty();
                    }
                    egui::ImeEvent::Commit(_) => {
                        state.ime_composing = false;
                    }
                    _ => {}
                }
            }
        }
    });

    // Handle keyboard shortcuts BEFORE TextEdit steals Up/Down/Enter/Esc (unless IME is composing)
    if !state.ime_composing {
        handle_popup_keyboard(ctx, settings, results, collections, state, &mut actions);
    } else {
        state.nav_hold = None;
    }

    // Top Header Bar
    render_header(ui, settings, state, palette, &mut actions);
    ui.add_space(5.0);

    // Filter Capsules + Collections
    render_filter_strip(ui, collections, state, palette);
    ui.add_space(5.0);

    // Optional macOS accessibility / status banner (auto-dismisses after 3s; also closable manually)
    let now_secs = ctx.input(|i| i.time);
    let active_toast = state.status_banner.as_mut().and_then(|toast| {
        toast
            .tick(now_secs)
            .map(|rem| (toast.message.clone(), toast.is_alert, rem))
    });
    if state.status_banner.is_some() && active_toast.is_none() {
        state.status_banner = None;
        ctx.request_repaint();
    }
    if let Some((msg, is_warn, remaining)) = active_toast {
        StatusToast::schedule_repaint(ctx, remaining);
        let opacity = StatusToast::opacity_for_remaining(remaining);
        let mut clear_banner = false;
        ui.scope(|ui| {
            if opacity < 0.999 {
                ui.multiply_opacity(opacity);
            }
            if render_banner(ui, &msg, is_warn, !has_accessibility, palette, &mut actions) {
                clear_banner = true;
            }
        });
        if clear_banner {
            state.status_banner = None;
        }
    } else if cfg!(target_os = "macos")
        && !has_accessibility
        && settings.auto_paste
        && !state.dismissed_accessibility_banner
    {
        egui::Frame::NONE
            .fill(palette.warning_bg)
            .stroke(Stroke::new(1.0, palette.warning_border))
            .corner_radius(CornerRadius::same(14))
            .shadow(card_shadow(palette))
            .inner_margin(egui::Margin::symmetric(10, 6))
            .show(ui, |ui| {
                ui.set_max_width(ui.available_width());
                ui.horizontal(|ui| {
                    let msg_w = (ui.available_width() - 96.0).max(120.0);
                    ui.allocate_ui(vec2(msg_w, 18.0), |ui| {
                        ui.set_max_width(msg_w);
                        ui.add(
                            egui::Label::new(
                                RichText::new(
                                    "⚠ 辅助功能未生效（若已开启，请在设置中点「-」移除后重加）",
                                )
                                .size(11.0)
                                .color(palette.text_primary),
                            )
                            .truncate(),
                        );
                    });
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        if dismiss_button(ui).clicked() {
                            state.dismissed_accessibility_banner = true;
                        }
                        if compact_button(ui, "去授权").clicked() {
                            actions.push(PopupAction::OpenAccessibilitySettings);
                        }
                    });
                });
            });
        ui.add_space(4.0);
    } else if settings.check_updates
        && let Some(rel) = state.available_update.clone()
        && state.dismissed_update_version.as_deref() != Some(rel.version.as_str())
    {
        let dl_state = state.update_download.clone();
        let banner_text = match &dl_state {
            UpdateDownloadState::Downloading { version } if version == &rel.version => {
                format!("⬇ 正在自动下载新版本 v{}…", rel.version)
            }
            UpdateDownloadState::ReadyToInstall { version, .. } if version == &rel.version => {
                format!("🎉 新版本 v{} 已下载就绪", rel.version)
            }
            UpdateDownloadState::Failed { version, .. } if version == &rel.version => {
                format!("⚠ v{} 自动下载失败，可重试或前往下载", rel.version)
            }
            _ => format!("🎉 发现新版本 v{}（当前 v{CURRENT_VERSION}）", rel.version),
        };

        egui::Frame::NONE
            .fill(palette.info_bg)
            .stroke(Stroke::new(1.0, palette.info_border))
            .corner_radius(CornerRadius::same(14))
            .shadow(card_shadow(palette))
            .inner_margin(egui::Margin::symmetric(10, 6))
            .show(ui, |ui| {
                ui.set_max_width(ui.available_width());
                ui.horizontal(|ui| {
                    let msg_w = (ui.available_width() - 188.0).max(96.0);
                    ui.allocate_ui(vec2(msg_w, 18.0), |ui| {
                        ui.set_max_width(msg_w);
                        ui.add(
                            egui::Label::new(
                                RichText::new(banner_text)
                                    .size(11.5)
                                    .color(palette.accent_text),
                            )
                            .truncate(),
                        );
                    });
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        if dismiss_button(ui).clicked() {
                            state.dismissed_update_version = Some(rel.version.clone());
                        }
                        if compact_button(ui, "前往下载").clicked() {
                            actions.push(PopupAction::OpenUrl(rel.html_url.clone()));
                        }
                        match dl_state {
                            UpdateDownloadState::ReadyToInstall {
                                version,
                                staged_path,
                            } if version == rel.version => {
                                let restart_btn = egui::Button::new(
                                    RichText::new("重启完成更新")
                                        .size(11.0)
                                        .color(palette.on_accent),
                                )
                                .fill(palette.accent)
                                .stroke(Stroke::NONE)
                                .corner_radius(CornerRadius::same(255));
                                if padded_button(ui, restart_btn, vec2(10.0, 3.5)).clicked() {
                                    actions.push(PopupAction::ApplyUpdateAndRestart(staged_path));
                                }
                            }
                            UpdateDownloadState::Downloading { version }
                                if version == rel.version => {}
                            UpdateDownloadState::Failed { version, .. }
                                if version == rel.version && rel.download_url.is_some() =>
                            {
                                if compact_button(ui, "重试下载").clicked() {
                                    actions.push(PopupAction::StartUpdateDownload(rel.clone()));
                                }
                            }
                            _ if rel.download_url.is_some() => {
                                let auto_btn = egui::Button::new(
                                    RichText::new("自动更新")
                                        .size(11.0)
                                        .color(palette.on_accent),
                                )
                                .fill(palette.accent)
                                .stroke(Stroke::NONE)
                                .corner_radius(CornerRadius::same(255));
                                if padded_button(ui, auto_btn, vec2(10.0, 3.5)).clicked() {
                                    actions.push(PopupAction::StartUpdateDownload(rel.clone()));
                                }
                            }
                            _ => {}
                        }
                    });
                });
            });
        ui.add_space(4.0);
    }
    if let Some(error) = &state.service_error {
        ui.label(RichText::new(error).size(11.5).color(palette.danger));
        ui.add_space(4.0);
    }

    // Main Split Area: History List + Optional Quick-Peek Workbench (Left or Right depending on screen bounds)
    let footer_reserved = if settings.show_footer_menu { 44.0 } else { 0.0 };
    let body_height = (ui.available_height() - footer_reserved).max(160.0);
    let total_width = ui.available_width();
    let split_gap = 8.0;

    let (body_rect, _) = ui.allocate_exact_size(vec2(total_width, body_height), Sense::hover());
    let reveal = state
        .preview_reveal
        .unwrap_or(if state.preview_open { 1.0 } else { 0.0 })
        .clamp(0.0, 1.0);
    let target_open = state.preview_open || reveal > 0.001;
    // Wait until the OS viewport has expanded past compact width (424 - 26 = 398) before revealing
    // the second pane, keeping workbench layout at full target width so text never wraps narrowly.
    let split_ready = target_open && body_rect.width() > 416.0;
    if state.preview_open && !split_ready {
        ctx.request_repaint();
    }

    let (list_rect, workbench_slot) = if split_ready {
        let compact_body_w = 398.0_f32.min(body_rect.width());
        let full_list_w = (734.0_f32 * 0.45).clamp(240.0, 320.0);
        let full_wb_w = (734.0_f32 - full_list_w - split_gap).max(320.0);
        let target_list_w = if reveal >= 0.999 {
            (body_rect.width() * 0.45).clamp(240.0, 320.0)
        } else {
            full_list_w
        };
        let cur_list_w = if reveal >= 0.999 {
            target_list_w
        } else {
            (compact_body_w + (target_list_w - compact_body_w) * reveal)
                .min((body_rect.width() - split_gap - 8.0).max(240.0))
                .max(240.0)
        };
        let wb_avail_w = (body_rect.width() - cur_list_w - split_gap).max(0.0);
        let layout_wb_w = if reveal >= 0.999 {
            wb_avail_w
        } else {
            full_wb_w
        };

        let (l_rect, wb_clip, wb_layout) = match state.workbench_side {
            WorkbenchSide::Right => {
                let l_rect = egui::Rect::from_min_max(
                    body_rect.min,
                    pos2(body_rect.min.x + cur_list_w, body_rect.max.y),
                );
                let wb_clip = egui::Rect::from_min_max(
                    pos2(body_rect.min.x + cur_list_w + split_gap, body_rect.min.y),
                    body_rect.max,
                );
                let slide = (1.0 - reveal) * 18.0;
                let wb_layout = egui::Rect::from_min_size(
                    pos2(wb_clip.min.x + slide, body_rect.min.y),
                    vec2(layout_wb_w, body_rect.height()),
                );
                (l_rect, wb_clip, wb_layout)
            }
            WorkbenchSide::Left => {
                let l_rect = egui::Rect::from_min_max(
                    pos2(body_rect.max.x - cur_list_w, body_rect.min.y),
                    body_rect.max,
                );
                let wb_clip = egui::Rect::from_min_max(
                    body_rect.min,
                    pos2(
                        (body_rect.max.x - cur_list_w - split_gap).max(body_rect.min.x),
                        body_rect.max.y,
                    ),
                );
                let slide = -(1.0 - reveal) * 18.0;
                let wb_layout = egui::Rect::from_min_size(
                    pos2(wb_clip.min.x + slide, body_rect.min.y),
                    vec2(layout_wb_w, body_rect.height()),
                );
                (l_rect, wb_clip, wb_layout)
            }
        };

        let wb_opacity = (reveal * 1.18).clamp(0.0, 1.0);
        let slot = (wb_clip.width() >= 8.0 && wb_opacity > 0.01)
            .then_some((wb_clip, wb_layout, wb_opacity));
        (l_rect, slot)
    } else {
        (body_rect, None)
    };

    let render_list = |ui: &mut Ui, state: &mut PopupState, actions: &mut Vec<PopupAction>| {
        ui.scope_builder(
            egui::UiBuilder::new()
                .max_rect(list_rect)
                .layout(Layout::top_down(Align::Min)),
            |ui| {
                ui.set_clip_rect(ui.clip_rect().intersect(list_rect));
                ui.set_min_size(list_rect.size());
                ui.set_max_size(list_rect.size());
                render_items_list(ui, settings, results, collections, state, palette, actions);
            },
        );
    };

    let render_workbench = |ui: &mut Ui,
                            state: &mut PopupState,
                            actions: &mut Vec<PopupAction>,
                            wb_clip: egui::Rect,
                            wb_layout: egui::Rect,
                            wb_opacity: f32| {
        let clip = ui.clip_rect().intersect(wb_clip);
        ui.scope_builder(
            egui::UiBuilder::new()
                .max_rect(wb_layout)
                .layout(Layout::top_down(Align::Min)),
            |ui| {
                ui.set_clip_rect(clip);
                ui.set_min_size(wb_layout.size());
                ui.set_max_size(wb_layout.size());
                let selected_item = state
                    .selected_id
                    .and_then(|sid| results.iter().find(|r| r.item.id == sid))
                    .map(|r| &r.item);
                render_preview_pane(
                    ui,
                    selected_item,
                    settings.enable_privacy_masking,
                    collections,
                    state,
                    palette,
                    actions,
                );
            },
        );
        // Fade the sliding workbench with a single GPU veil quad instead of `multiply_opacity`
        // so text colors remain constant and egui's galley cache stays warm across all frames.
        if wb_opacity < 0.999 {
            let veil_alpha = ((1.0 - wb_opacity) * 255.0).round().clamp(0.0, 255.0) as u8;
            if veil_alpha > 0 {
                let bg = palette.window_bg;
                ui.painter().with_clip_rect(clip).rect_filled(
                    wb_clip,
                    CornerRadius::same(14),
                    Color32::from_rgba_unmultiplied(bg.r(), bg.g(), bg.b(), veil_alpha),
                );
            }
        }
    };

    if target_open
        && let Some(sid) = state.selected_id
        && let Some(sel) = results.iter().find(|r| r.item.id == sid)
    {
        state.ensure_preview_cache(&sel.item);
    }

    match state.workbench_side {
        WorkbenchSide::Right => {
            render_list(ui, state, &mut actions);
            if let Some((wb_clip, wb_layout, wb_opacity)) = workbench_slot {
                render_workbench(ui, state, &mut actions, wb_clip, wb_layout, wb_opacity);
            }
        }
        WorkbenchSide::Left => {
            if let Some((wb_clip, wb_layout, wb_opacity)) = workbench_slot {
                render_workbench(ui, state, &mut actions, wb_clip, wb_layout, wb_opacity);
            }
            render_list(ui, state, &mut actions);
        }
    }

    if settings.show_footer_menu {
        render_footer(
            ui,
            settings,
            state,
            palette,
            storage_bytes,
            total_items,
            &mut actions,
        );
    }

    if state.show_about_modal {
        render_about_overlay(ctx, state, palette);
    }

    actions
}
