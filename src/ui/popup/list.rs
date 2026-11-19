use super::*;

/// Computes the smooth scroll offset needed to keep the selected row inside the viewport
/// with ~1.25 rows of lookahead breathing margin so users can see upcoming items while holding Up/Down.
pub(super) fn compute_smooth_scroll_target(
    cur_scroll: f32,
    row_top: f32,
    row_h: f32,
    viewport_h: f32,
    max_scroll: f32,
) -> f32 {
    if max_scroll <= 0.0 || viewport_h <= 0.0 {
        return 0.0;
    }
    let cur = cur_scroll.clamp(0.0, max_scroll);
    let max_margin = ((viewport_h - row_h) * 0.5).max(0.0);
    let margin = ((row_h + 5.0) * 1.25)
        .min(viewport_h * 0.35)
        .min(max_margin);
    let row_bottom = row_top + row_h;
    if row_top < cur + margin {
        (row_top - margin).clamp(0.0, max_scroll)
    } else if row_bottom > cur + viewport_h - margin {
        (row_bottom + margin - viewport_h).clamp(0.0, max_scroll)
    } else {
        cur
    }
}

pub(super) fn render_items_list(
    ui: &mut Ui,
    settings: &AppSettings,
    results: &[SearchResultItem],
    collections: &[Collection],
    state: &mut PopupState,
    palette: &EarthPalette,
    actions: &mut Vec<PopupAction>,
) {
    if results.is_empty() {
        state.selection_visual_y = None;
        state.scroll_anim_target = None;
        state.scroll_to_selection = false;
        egui::Frame::NONE
            .fill(palette.surface_bg)
            .stroke(Stroke::new(1.0, palette.border_subtle))
            .corner_radius(CornerRadius::same(16))
            .shadow(card_shadow(palette))
            .inner_margin(egui::Margin::same(20))
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.vertical_centered(|ui| {
                    ui.add_space(40.0);
                    ui.label(
                        RichText::new(if state.search_query.is_empty() {
                            t("list.empty_all")
                        } else {
                            t("list.empty_search")
                        })
                        .size(14.0)
                        .color(palette.text_secondary),
                    );
                    ui.add_space(6.0);
                    ui.label(
                        RichText::new(t("list.empty_hint"))
                            .size(12.0)
                            .color(palette.text_muted),
                    );
                    ui.add_space(40.0);
                });
            });
        return;
    }

    let dt = ui.input(|i| i.stable_dt).clamp(1.0 / 240.0, 0.05);
    let manual_scroll = ui.input(|i| i.is_scrolling() || i.smooth_scroll_delta.y.abs() > 0.1)
        || ui.ctx().egui_is_using_pointer();
    if manual_scroll {
        state.scroll_anim_target = None;
    }

    let mut applied_scroll_y: Option<f32> = None;
    if let Some(target) = state.scroll_anim_target {
        let cur = state.list_scroll_y;
        let diff = target - cur;
        if diff.abs() <= 0.5 {
            state.list_scroll_y = target;
            state.scroll_anim_target = None;
            applied_scroll_y = Some(target);
        } else {
            let rate = if state.nav_hold.is_some() { 24.0 } else { 18.0 };
            let alpha = 1.0 - (-rate * dt).exp();
            let next = cur + diff * alpha;
            state.list_scroll_y = next;
            applied_scroll_y = Some(next);
            ui.ctx().request_repaint();
        }
    }

    let list_avail_w = ui.available_width();
    let mut scroll_area = ScrollArea::vertical()
        .id_salt("earth_history_scroll")
        .auto_shrink([false, false])
        .scroll_source(desktop_scroll_source())
        .max_width(list_avail_w);
    if let Some(y) = applied_scroll_y {
        scroll_area = scroll_area.vertical_scroll_offset(y);
    }

    let mut selected_row_span: Option<(f32, f32)> = None;
    let scroll_out = scroll_area.show_viewport(ui, |ui, viewport| {
        ui.set_max_width(list_avail_w);
        let origin_y = ui.cursor().min.y;
        let animating_wb = state.preview_reveal.is_some_and(|r| r > 0.001 && r < 0.999);
        let overscan = if animating_wb { 40.0 } else { 240.0 };
        let vis_min_y = viewport.min.y - overscan;
        let vis_max_y = viewport.max.y + overscan;
        let mut seen_pinned_sep = false;
        let mut prev_pinned: Option<bool> = None;
        let mut selected_row_rect: Option<egui::Rect> = None;

        state.external_selection_bar = true;
        for (idx, res) in results.iter().enumerate() {
            let item = &res.item;
            if let Some(prev) = prev_pinned
                && prev != item.pinned
                && !seen_pinned_sep
            {
                seen_pinned_sep = true;
                ui.add_space(2.0);
                ui.separator();
                ui.add_space(2.0);
            }
            prev_pinned = Some(item.pinned);

            let selected = state.selected_id == Some(item.id);
            let row_height = LIST_ROW_HEIGHT;
            let rel_y = ui.cursor().min.y - origin_y;
            let in_viewport = (rel_y + row_height >= vis_min_y && rel_y <= vis_max_y) || selected;

            if selected {
                let est_target_cy = rel_y + row_height * 0.5;
                let est_dist = state
                    .selection_visual_y
                    .map(|vy| (est_target_cy - vy).abs())
                    .unwrap_or(0.0);
                state.selection_glide_blend = 1.0 - (est_dist / 16.0).clamp(0.0, 1.0);
            }

            let resp = if in_viewport {
                let shortcut_num = if idx < 9 { Some(idx + 1) } else { None };
                render_item_row(
                    ui,
                    res,
                    shortcut_num,
                    selected,
                    settings,
                    collections,
                    state,
                    palette,
                    actions,
                )
            } else {
                ui.allocate_exact_size(vec2(list_avail_w, row_height), Sense::hover())
                    .1
            };

            if selected {
                selected_row_rect = Some(resp.rect);
                selected_row_span = Some((resp.rect.min.y - origin_y, resp.rect.height()));
            }
        }
        state.external_selection_bar = false;
        state.selection_glide_blend = 1.0;

        // Paint silky content-space gliding selection bar + subtle focus capsule
        if let Some(sel_rect) = selected_row_rect {
            let target_rel_cy = sel_rect.center().y - origin_y;
            let visual_rel_cy = match state.selection_visual_y {
                Some(mut prev_cy) => {
                    let jump_limit = (viewport.height() * 1.1).max(LIST_ROW_HEIGHT * 6.0);
                    if (target_rel_cy - prev_cy).abs() > jump_limit {
                        let sign = (target_rel_cy - prev_cy).signum();
                        prev_cy = target_rel_cy - sign * (jump_limit * 0.55);
                    }
                    let diff = target_rel_cy - prev_cy;
                    if diff.abs() <= 0.35 {
                        target_rel_cy
                    } else {
                        let rate = if state.nav_hold.is_some() { 24.0 } else { 28.0 };
                        let alpha = 1.0 - (-rate * dt).exp();
                        ui.ctx().request_repaint();
                        prev_cy + diff * alpha
                    }
                }
                None => target_rel_cy,
            };
            state.selection_visual_y = Some(visual_rel_cy);

            let dist = (target_rel_cy - visual_rel_cy).abs();
            let screen_cy = origin_y + visual_rel_cy;
            let painter = ui.painter();

            if dist > 0.6 {
                let activity = (dist / 10.0).clamp(0.0, 1.0);
                let capsule_rect = egui::Rect::from_center_size(
                    pos2(sel_rect.center().x, screen_cy),
                    sel_rect.size(),
                );
                let accent = palette.accent;
                let fill_alpha =
                    (activity * if palette.is_dark { 26.0 } else { 16.0 }).round() as u8;
                let stroke_alpha = (activity * 195.0).round() as u8;
                painter.rect_filled(
                    capsule_rect,
                    CornerRadius::same(12),
                    Color32::from_rgba_unmultiplied(accent.r(), accent.g(), accent.b(), fill_alpha),
                );
                painter.rect_stroke(
                    capsule_rect,
                    CornerRadius::same(12),
                    Stroke::new(
                        1.2,
                        Color32::from_rgba_unmultiplied(
                            accent.r(),
                            accent.g(),
                            accent.b(),
                            stroke_alpha,
                        ),
                    ),
                    egui::StrokeKind::Inside,
                );
            }

            let base_bar_h = (LIST_ROW_HEIGHT - 16.0).max(16.0);
            let stretch = (dist * 0.28).min(16.0);
            let bar_h = base_bar_h + stretch;
            let bar_center_x = sel_rect.min.x + 8.0 + 3.5 * 0.5;
            let bar_rect =
                egui::Rect::from_center_size(pos2(bar_center_x, screen_cy), vec2(3.5, bar_h));
            painter.rect_filled(bar_rect, CornerRadius::same(2), palette.accent);
        } else {
            state.selection_visual_y = None;
        }
    });

    let actual_scroll_y = scroll_out.state.clamped_offset().y;
    state.list_scroll_y = actual_scroll_y;
    let max_scroll = (scroll_out.content_size.y - scroll_out.inner_rect.height()).max(0.0);
    if let Some(target) = state.scroll_anim_target {
        state.scroll_anim_target = Some(target.clamp(0.0, max_scroll));
    }

    if state.scroll_to_selection {
        state.scroll_to_selection = false;
        if let Some((sel_top, sel_h)) = selected_row_span {
            let target = compute_smooth_scroll_target(
                actual_scroll_y,
                sel_top,
                sel_h,
                scroll_out.inner_rect.height(),
                max_scroll,
            );
            if (target - actual_scroll_y).abs() > 0.5 {
                state.scroll_anim_target = Some(target);
                ui.ctx().request_repaint();
            } else {
                state.scroll_anim_target = None;
            }
        }
    }
}
