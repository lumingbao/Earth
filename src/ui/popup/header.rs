use super::*;

pub(super) fn render_header(
    ui: &mut Ui,
    settings: &mut AppSettings,
    state: &mut PopupState,
    palette: &EarthPalette,
    actions: &mut Vec<PopupAction>,
) {
    ui.horizontal(|ui| {
        let show_search = settings.search_box_visibility == SearchBoxVisibility::Always
            || !state.search_query.is_empty()
            || state.ime_composing;

        let right_controls_width = 166.0;
        let search_outer_w = (ui.available_width() - right_controls_width).max(90.0);

        let frame_fill = if show_search {
            palette.card_bg
        } else {
            Color32::TRANSPARENT
        };
        let hint = if show_search {
            "搜索记录..."
        } else {
            "直接打字即可搜索..."
        };

        let search_row_h = 22.0;
        let search_margin = egui::Margin {
            left: 10,
            right: 4,
            top: 4,
            bottom: 4,
        };
        let search_inner_w = (search_outer_w - search_margin.sum().x).max(74.0);

        let mut search_focused = false;
        let mut search_hovered = false;
        let search_frame = egui::Frame::NONE
            .fill(frame_fill)
            .stroke(Stroke::NONE)
            .corner_radius(CornerRadius::same(255))
            .inner_margin(search_margin)
            .show(ui, |ui| {
                ui.set_min_size(vec2(search_inner_w, search_row_h));
                ui.set_max_size(vec2(search_inner_w, search_row_h));
                ui.spacing_mut().interact_size.y = search_row_h;
                ui.horizontal_centered(|ui| {
                    if show_search {
                        let (icon_rect, _) =
                            ui.allocate_exact_size(vec2(14.0, 14.0), Sense::hover());
                        draw_search_glyph(ui.painter(), icon_rect, palette.text_secondary);
                    }

                    let reserve_right = if !show_search {
                        4.0
                    } else if state.search_query.is_empty() {
                        46.0
                    } else {
                        74.0
                    };
                    let edit_w = (ui.available_width() - reserve_right).max(40.0);
                    let mut output = egui::TextEdit::singleline(&mut state.search_query)
                        .id_salt("earth_search_input")
                        .hint_text(hint)
                        .frame(egui::Frame::NONE)
                        .margin(egui::Margin::symmetric(2, 0))
                        .align(egui::Align2::LEFT_CENTER)
                        .min_size(vec2(edit_w, search_row_h))
                        .desired_width(edit_w)
                        .show(ui);
                    search_focused = output.response.has_focus();
                    search_hovered = output.response.hovered();
                    if state.request_search_focus {
                        output.response.request_focus();
                        let end = egui::text::CCursor::new(state.search_query.chars().count());
                        output
                            .state
                            .cursor
                            .set_char_range(Some(egui::text::CCursorRange::one(end)));
                        output.state.store(ui.ctx(), output.response.id);
                        state.request_search_focus = false;
                        search_focused = true;
                    }

                    if show_search {
                        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                            // GOSTOCK pill badge for search mode indicator (right-aligned)
                            let mode_btn = egui::Button::new(
                                RichText::new(settings.search_mode.label())
                                    .size(10.5)
                                    .color(palette.accent_text),
                            )
                            .fill(palette.surface_bg)
                            .stroke(Stroke::new(1.0, palette.border_strong))
                            .corner_radius(CornerRadius::same(255));
                            if padded_button(ui, mode_btn, vec2(8.0, 2.0))
                                .on_hover_text("点击切换搜索模式：精确 / 模糊 / 正则")
                                .clicked()
                            {
                                settings.search_mode = match settings.search_mode {
                                    SearchMode::Exact => SearchMode::Fuzzy,
                                    SearchMode::Fuzzy => SearchMode::Regex,
                                    SearchMode::Regex => SearchMode::Exact,
                                };
                                actions.push(PopupAction::SettingsChanged);
                            }

                            if !state.search_query.is_empty() && dismiss_button(ui).clicked() {
                                state.search_query.clear();
                                state.request_search_focus = true;
                            }
                        });
                    }
                });
            });

        state.search_focused = search_focused;

        if show_search {
            let sr = search_frame.response.rect;
            let border_color = if search_focused {
                palette.accent
            } else if search_hovered || search_frame.response.hovered() {
                palette.border_strong
            } else {
                palette.border_subtle
            };
            if search_focused {
                ui.painter().rect_stroke(
                    sr.expand(1.5),
                    CornerRadius::same(255),
                    Stroke::new(2.0, tint(palette.accent, 34)),
                    egui::StrokeKind::Inside,
                );
            }
            ui.painter().rect_stroke(
                sr,
                CornerRadius::same(255),
                Stroke::new(1.0, border_color),
                egui::StrokeKind::Inside,
            );
        }

        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            ui.spacing_mut().item_spacing.x = 5.0;
            if icon_button(ui, 28.0, false, "偏好设置 (⌘,)", palette, draw_gear_glyph).clicked()
            {
                actions.push(PopupAction::OpenSettings);
            }

            let preview_tip = format!(
                "切换速览工作台 ({})",
                format_shortcut_display(&settings.hotkey_preview)
            );
            if icon_button(
                ui,
                28.0,
                state.preview_open,
                &preview_tip,
                palette,
                draw_split_pane_glyph,
            )
            .clicked()
            {
                state.toggle_preview();
            }

            if icon_button(
                ui,
                28.0,
                settings.enable_privacy_masking,
                "隐私脱敏保护（自动遮罩密钥/Token）",
                palette,
                draw_shield_glyph,
            )
            .clicked()
            {
                settings.enable_privacy_masking = !settings.enable_privacy_masking;
                state.clear_sensitive_context();
                actions.push(PopupAction::SettingsChanged);
            }

            let pause_tip = if settings.paused {
                "已暂停记录新复制（点击恢复）"
            } else {
                "正在监听剪贴板（点击暂停）"
            };
            if icon_button(
                ui,
                28.0,
                settings.paused,
                pause_tip,
                palette,
                if settings.paused {
                    draw_play_glyph
                } else {
                    draw_pause_glyph
                },
            )
            .clicked()
            {
                actions.push(PopupAction::TogglePause);
            }

            let pin_tip = if state.window_pinned {
                "已钉住窗口（失焦不关闭并始终置顶，点击取消）"
            } else {
                "钉住窗口（点击其他界面不自动关闭并始终置顶）"
            };
            let pinned_now = state.window_pinned;
            if icon_button(ui, 28.0, pinned_now, pin_tip, palette, |p, r, c| {
                draw_window_pin_glyph(p, r, c, pinned_now);
            })
            .clicked()
            {
                state.window_pinned = !state.window_pinned;
                actions.push(PopupAction::ToggleWindowPin);
            }
        });
    });
}

pub(super) fn render_filter_strip(
    ui: &mut Ui,
    collections: &[Collection],
    state: &mut PopupState,
    palette: &EarthPalette,
) {
    let strip_w = ui.available_width();
    ScrollArea::horizontal()
        .id_salt("earth_filter_capsules")
        .max_width(strip_w)
        .auto_shrink([false, true])
        .scroll_source(desktop_scroll_source())
        .scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::AlwaysHidden)
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 4.5;
                let pills = [
                    (CategoryFilter::All, "全部"),
                    (CategoryFilter::Text, "文本"),
                    (CategoryFilter::Image, "图片"),
                    (CategoryFilter::Files, "文件"),
                    (CategoryFilter::Pinned, "固定"),
                ];

                for (filter, label) in pills {
                    let active = state.filter == filter;
                    let resp = capsule_button(ui, active, label, None, palette, || {
                        if state.filter != filter {
                            state.filter = filter;
                            state.reset_list_navigation();
                        }
                    });
                    if active && state.scroll_to_filter {
                        resp.scroll_to_me(None);
                        state.scroll_to_filter = false;
                    }
                }

                for col in collections {
                    let dot = parse_hex_color32(&col.color_hex);
                    let f = CategoryFilter::Collection(col.id);
                    let active = state.filter == f;
                    let resp = capsule_button(ui, active, &col.name, dot, palette, || {
                        if state.filter != f {
                            state.filter = f;
                            state.reset_list_navigation();
                        }
                    });
                    if active && state.scroll_to_filter {
                        resp.scroll_to_me(None);
                        state.scroll_to_filter = false;
                    }
                }
            });
        });
}

pub(super) fn capsule_button(
    ui: &mut Ui,
    active: bool,
    label: &str,
    dot_color: Option<Color32>,
    palette: &EarthPalette,
    on_click: impl FnOnce(),
) -> egui::Response {
    let fill = if active {
        palette.accent
    } else {
        palette.card_bg
    };
    let stroke = Stroke::NONE;
    let text_color = if active {
        palette.on_accent
    } else {
        palette.text_secondary
    };

    let font = FontId::proportional(11.5);
    let mut job = egui::text::LayoutJob::default();
    if let Some(dot) = dot_color {
        job.append(
            "● ",
            0.0,
            egui::TextFormat {
                font_id: font.clone(),
                color: if active { palette.on_accent } else { dot },
                ..Default::default()
            },
        );
    }
    job.append(
        label,
        0.0,
        egui::TextFormat {
            font_id: font,
            color: text_color,
            ..Default::default()
        },
    );

    let btn = egui::Button::new(job)
        .fill(fill)
        .stroke(stroke)
        .corner_radius(CornerRadius::same(255))
        .min_size(vec2(0.0, 23.0));
    let resp = padded_button(ui, btn, vec2(9.0, 4.5));
    if resp.clicked() {
        on_click();
    }
    resp
}
