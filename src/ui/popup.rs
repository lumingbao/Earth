use crate::hotkey::{format_shortcut_display, matches_egui_shortcut};
use crate::model::{
    format_bytes, truncate_chars, AppSettings, CategoryFilter, ClipItem, ClipKind, Collection,
    PasteBehavior, SearchBoxVisibility, SearchMode,
};
use crate::search::SearchResultItem;
use crate::transforms::{
    apply_quick_action, available_quick_actions, format_display_line, mask_sensitive_preview,
    QuickActionKind,
};
use crate::ui::icons::{
    draw_earth_emblem, draw_file_badge, draw_gear_glyph, draw_link_badge, draw_pause_glyph,
    draw_pin_badge, draw_play_glyph, draw_search_glyph, draw_shield_glyph, draw_split_pane_glyph,
    icon_button,
};
use crate::ui::theme::{build_highlighted_layout_job, parse_hex_color32, EarthPalette};
use chrono::{DateTime, Local};
use egui::{
    pos2, vec2, Align, Color32, CornerRadius, FontId, Key, Layout, Modifiers, RichText, ScrollArea,
    Sense, Stroke, Ui,
};
use std::collections::HashSet;
use std::path::PathBuf;
use std::time::Instant;

pub enum PopupAction {
    HidePopup,
    PasteItem { id: i64, behavior: PasteBehavior },
    CopyTransformedText(String),
    PasteTransformedText(String),
    TogglePin(i64),
    SetCollection { item_id: i64, collection_id: Option<i64> },
    DeleteItem(i64),
    ClearUnpinned,
    OpenSettings,
    TogglePause,
    ToggleIgnoreNext,
    SettingsChanged,
    SaveImageAs(String),
    RevealFile(PathBuf),
    OpenUrl(String),
    OpenAccessibilitySettings,
    Quit,
}

pub struct PopupState {
    pub search_query: String,
    pub filter: CategoryFilter,
    pub selected_id: Option<i64>,
    pub preview_open: bool,
    pub preview_manually_toggled: bool,
    pub request_search_focus: bool,
    pub selection_changed_at: Instant,
    pub revealed_sensitive_ids: HashSet<i64>,
    pub image_actual_size: bool,
    pub active_transform: Option<(i64, QuickActionKind, Result<String, String>)>,
    pub show_about_modal: bool,
    pub status_banner: Option<(String, bool)>,
    pub scroll_to_selection: bool,
}

impl Default for PopupState {
    fn default() -> Self {
        Self {
            search_query: String::new(),
            filter: CategoryFilter::All,
            selected_id: None,
            preview_open: false,
            preview_manually_toggled: false,
            request_search_focus: true,
            selection_changed_at: Instant::now(),
            revealed_sensitive_ids: HashSet::new(),
            image_actual_size: false,
            active_transform: None,
            show_about_modal: false,
            status_banner: None,
            scroll_to_selection: false,
        }
    }
}

pub struct PopupViewData<'a> {
    pub settings: &'a mut AppSettings,
    pub results: &'a [SearchResultItem],
    pub collections: &'a [Collection],
    pub palette: &'a EarthPalette,
    pub storage_bytes: u64,
    pub total_items: usize,
    pub has_accessibility: bool,
}

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
            state.selected_id = Some(results[0].item.id);
            state.selection_changed_at = Instant::now();
        }
    } else {
        state.selected_id = None;
    }

    // Auto-open preview timer (Maccy-parity)
    if settings.auto_open_preview
        && !state.preview_open
        && !state.preview_manually_toggled
        && state.selected_id.is_some()
    {
        let delay = std::time::Duration::from_millis(settings.preview_delay_ms);
        let elapsed = state.selection_changed_at.elapsed();
        if elapsed >= delay {
            state.preview_open = true;
        } else {
            ctx.request_repaint_after(delay - elapsed);
        }
    }

    // Handle keyboard shortcuts BEFORE TextEdit steals Up/Down/Enter/Esc
    handle_popup_keyboard(ctx, settings, results, state, &mut actions);

    // Capture typed characters when search box is set to WhileTyping
    if settings.search_box_visibility == SearchBoxVisibility::WhileTyping
        && state.search_query.is_empty()
    {
        let typed: String = ctx.input(|i| {
            i.events
                .iter()
                .filter_map(|e| match e {
                    egui::Event::Text(t) if !i.modifiers.command && !i.modifiers.ctrl && !i.modifiers.alt => {
                        Some(t.clone())
                    }
                    _ => None,
                })
                .collect()
        });
        if !typed.is_empty() {
            state.search_query.push_str(&typed);
            state.request_search_focus = true;
        }
    }

    // Top Header Bar
    render_header(ui, ctx, settings, state, palette, &mut actions);
    ui.add_space(5.0);

    // Filter Capsules + Collections
    render_filter_strip(ui, collections, state, palette);
    ui.add_space(5.0);

    // Optional macOS accessibility / status banner
    let mut clear_banner = false;
    if let Some((msg, is_warn)) = state.status_banner.clone() {
        if render_banner(ui, &msg, is_warn, !has_accessibility, palette, &mut actions) {
            clear_banner = true;
        }
        if ui.input(|i| i.key_pressed(Key::Escape)) {
            clear_banner = true;
        }
        if clear_banner {
            state.status_banner = None;
        }
    } else if cfg!(target_os = "macos") && !has_accessibility && settings.auto_paste {
        egui::Frame::NONE
            .fill(Color32::from_rgba_unmultiplied(245, 158, 11, 26))
            .stroke(Stroke::new(1.0, palette.pin_amber))
            .corner_radius(CornerRadius::same(8))
            .inner_margin(egui::Margin::symmetric(8, 5))
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.label(
                        RichText::new("⚠ 未授权「辅助功能」权限，自动粘贴将退化为仅复制")
                            .size(11.5)
                            .color(palette.pin_amber),
                    );
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        if ui.small_button("去授权").clicked() {
                            actions.push(PopupAction::OpenAccessibilitySettings);
                        }
                    });
                });
            });
        ui.add_space(4.0);
    }

    // Main Split Area: Left History List + Optional Right Quick-Peek Workbench
    let footer_reserved = if settings.show_footer_menu { 34.0 } else { 4.0 };
    let body_height = (ui.available_height() - footer_reserved).max(160.0);

    ui.allocate_ui_with_layout(
        vec2(ui.available_width(), body_height),
        Layout::left_to_right(Align::Min),
        |ui| {
            let list_width = if state.preview_open {
                (ui.available_width() * 0.45).clamp(290.0, 390.0)
            } else {
                ui.available_width()
            };

            ui.allocate_ui_with_layout(
                vec2(list_width, body_height),
                Layout::top_down(Align::Min),
                |ui| {
                    render_items_list(ui, settings, results, collections, state, palette, &mut actions);
                },
            );

            if state.preview_open {
                ui.add_space(8.0);
                let preview_width = ui.available_width().max(260.0);
                ui.allocate_ui_with_layout(
                    vec2(preview_width, body_height),
                    Layout::top_down(Align::Min),
                    |ui| {
                        let selected_item = state
                            .selected_id
                            .and_then(|sid| results.iter().find(|r| r.item.id == sid))
                            .map(|r| &r.item);
                        render_preview_pane(
                            ui,
                            selected_item,
                            collections,
                            state,
                            palette,
                            &mut actions,
                        );
                    },
                );
            }
        },
    );

    if settings.show_footer_menu {
        ui.add_space(4.0);
        render_footer(ui, settings, state, palette, storage_bytes, total_items, &mut actions);
    }

    if state.show_about_modal {
        render_about_overlay(ctx, state, palette);
    }

    actions
}

fn handle_popup_keyboard(
    ctx: &egui::Context,
    settings: &mut AppSettings,
    results: &[SearchResultItem],
    state: &mut PopupState,
    actions: &mut Vec<PopupAction>,
) {
    if state.show_about_modal {
        if ctx.input_mut(|i| i.consume_key(Modifiers::NONE, Key::Escape)) {
            state.show_about_modal = false;
        }
        return;
    }

    // Esc clears search first, or closes preview/popup
    if ctx.input_mut(|i| i.consume_key(Modifiers::NONE, Key::Escape)) {
        if !state.search_query.is_empty() {
            state.search_query.clear();
            state.request_search_focus = true;
        } else {
            actions.push(PopupAction::HidePopup);
        }
        return;
    }

    // Navigation: Up / Down
    if !results.is_empty() {
        let cur_idx = state
            .selected_id
            .and_then(|id| results.iter().position(|r| r.item.id == id))
            .unwrap_or(0);

        if ctx.input_mut(|i| i.consume_key(Modifiers::NONE, Key::ArrowDown)) {
            let next = (cur_idx + 1).min(results.len() - 1);
            state.selected_id = Some(results[next].item.id);
            state.selection_changed_at = Instant::now();
            state.scroll_to_selection = true;
            state.active_transform = None;
        } else if ctx.input_mut(|i| i.consume_key(Modifiers::NONE, Key::ArrowUp)) {
            let prev = cur_idx.saturating_sub(1);
            state.selected_id = Some(results[prev].item.id);
            state.selection_changed_at = Instant::now();
            state.scroll_to_selection = true;
            state.active_transform = None;
        } else if ctx.input_mut(|i| i.consume_key(Modifiers::NONE, Key::PageDown)) {
            let next = (cur_idx + 8).min(results.len() - 1);
            state.selected_id = Some(results[next].item.id);
            state.selection_changed_at = Instant::now();
            state.scroll_to_selection = true;
        } else if ctx.input_mut(|i| i.consume_key(Modifiers::NONE, Key::PageUp)) {
            let prev = cur_idx.saturating_sub(8);
            state.selected_id = Some(results[prev].item.id);
            state.selection_changed_at = Instant::now();
            state.scroll_to_selection = true;
        }
    }

    // Enter -> Paste or Copy depending on held modifiers (Maccy parity)
    let enter_info = ctx.input_mut(|i| {
        let mods = i.modifiers;
        let pressed = i.consume_key(Modifiers::NONE, Key::Enter)
            || i.consume_key(Modifiers::ALT, Key::Enter)
            || i.consume_key(Modifiers::COMMAND, Key::Enter)
            || i.consume_key(Modifiers::MAC_CMD, Key::Enter)
            || i.consume_key(Modifiers::COMMAND | Modifiers::SHIFT, Key::Enter)
            || i.consume_key(Modifiers::MAC_CMD | Modifiers::SHIFT, Key::Enter)
            || i.consume_key(Modifiers::CTRL | Modifiers::SHIFT, Key::Enter);
        (pressed, mods)
    });

    if enter_info.0
        && let Some(id) = state.selected_id {
            let behavior = behavior_from_modifiers(enter_info.1);
            actions.push(PopupAction::PasteItem { id, behavior });
            return;
        }

    // Quick number keys: Cmd+1..9 (macOS) / Alt+1..9 (Windows)
    let num_keys = [
        Key::Num1,
        Key::Num2,
        Key::Num3,
        Key::Num4,
        Key::Num5,
        Key::Num6,
        Key::Num7,
        Key::Num8,
        Key::Num9,
    ];
    for (idx, key) in num_keys.into_iter().enumerate() {
        #[cfg(target_os = "macos")]
        let triggered = ctx.input_mut(|i| {
            i.consume_key(Modifiers::MAC_CMD, key) || i.consume_key(Modifiers::COMMAND, key)
        });
        #[cfg(not(target_os = "macos"))]
        let triggered = ctx.input_mut(|i| i.consume_key(Modifiers::ALT, key));

        if triggered
            && let Some(item) = results.get(idx) {
                actions.push(PopupAction::PasteItem {
                    id: item.item.id,
                    behavior: PasteBehavior::Default,
                });
                return;
            }
    }

    // Pin, Delete, Preview, Settings, Clear, Quit shortcuts
    let (pin_hit, del_hit, preview_hit, settings_hit, clear_hit, quit_hit) = ctx.input(|i| {
        let pin = matches_egui_shortcut(i, &settings.hotkey_pin);
        let del = matches_egui_shortcut(i, &settings.hotkey_delete);
        let prev = matches_egui_shortcut(i, &settings.hotkey_preview);
        #[cfg(target_os = "macos")]
        let cmd = i.modifiers.mac_cmd || i.modifiers.command;
        #[cfg(not(target_os = "macos"))]
        let cmd = i.modifiers.ctrl || i.modifiers.command;

        let prefs = cmd && !i.modifiers.alt && !i.modifiers.shift && i.key_pressed(Key::Comma);
        let clear = cmd && i.modifiers.alt && i.key_pressed(Key::Backspace);
        let quit = cmd && !i.modifiers.alt && !i.modifiers.shift && i.key_pressed(Key::Q);
        (pin, del, prev, prefs, clear, quit)
    });

    if pin_hit
        && let Some(id) = state.selected_id {
            actions.push(PopupAction::TogglePin(id));
        }
    if del_hit
        && let Some(id) = state.selected_id {
            actions.push(PopupAction::DeleteItem(id));
        }
    if preview_hit {
        state.preview_open = !state.preview_open;
        state.preview_manually_toggled = true;
    }
    if settings_hit {
        actions.push(PopupAction::OpenSettings);
    }
    if clear_hit {
        actions.push(PopupAction::ClearUnpinned);
    }
    if quit_hit {
        actions.push(PopupAction::Quit);
    }
}

fn behavior_from_modifiers(mods: Modifiers) -> PasteBehavior {
    #[cfg(target_os = "macos")]
    let cmd = mods.mac_cmd || mods.command;
    #[cfg(not(target_os = "macos"))]
    let cmd = mods.ctrl || mods.command;

    if cmd && mods.shift {
        PasteBehavior::CopyAndPastePlainText
    } else if mods.alt {
        PasteBehavior::CopyOnly
    } else if cmd {
        PasteBehavior::CopyAndPaste
    } else {
        PasteBehavior::Default
    }
}

fn render_header(
    ui: &mut Ui,
    ctx: &egui::Context,
    settings: &mut AppSettings,
    state: &mut PopupState,
    palette: &EarthPalette,
    actions: &mut Vec<PopupAction>,
) {
    ui.horizontal(|ui| {
        // Draggable Earth brand badge
        let emblem_resp = draw_earth_emblem(ui, 24.0, palette, settings.paused);
        if emblem_resp.hovered() {
            emblem_resp.on_hover_text("拖动移动窗口");
        }

        if settings.show_app_name_in_search {
            let title_resp = ui.add(
                egui::Label::new(
                    RichText::new("Earth")
                        .size(14.5)
                        .strong()
                        .color(palette.text_primary),
                )
                .sense(Sense::drag()),
            );
            if title_resp.drag_started() {
                ctx.send_viewport_cmd(egui::ViewportCommand::StartDrag);
            }
        }

        let show_search = settings.search_box_visibility == SearchBoxVisibility::Always
            || !state.search_query.is_empty();

        let right_controls_width = 166.0;
        let search_avail = (ui.available_width() - right_controls_width).max(90.0);

        if show_search {
            egui::Frame::NONE
                .fill(palette.surface_bg)
                .stroke(Stroke::new(1.0, palette.border))
                .corner_radius(CornerRadius::same(9))
                .inner_margin(egui::Margin::symmetric(8, 5))
                .show(ui, |ui| {
                    ui.set_width(search_avail);
                    ui.horizontal(|ui| {
                        let (icon_rect, _) = ui.allocate_exact_size(vec2(14.0, 14.0), Sense::hover());
                        draw_search_glyph(ui.painter(), icon_rect, palette.text_muted);

                        let edit_w = (ui.available_width() - 56.0).max(50.0);
                        let resp = ui.add(
                            egui::TextEdit::singleline(&mut state.search_query)
                                .hint_text("搜索记录...")
                                .frame(egui::Frame::NONE)
                                .desired_width(edit_w),
                        );
                        if state.request_search_focus {
                            resp.request_focus();
                            state.request_search_focus = false;
                        }

                        if !state.search_query.is_empty() && ui.small_button("×").clicked() {
                            state.search_query.clear();
                            state.request_search_focus = true;
                        }

                        // Cycle Exact / Fuzzy / Regex
                        let mode_btn = egui::Button::new(
                            RichText::new(settings.search_mode.label())
                                .size(10.5)
                                .color(palette.accent),
                        )
                        .fill(palette.accent_soft)
                        .stroke(Stroke::NONE)
                        .corner_radius(CornerRadius::same(5));
                        if ui
                            .add(mode_btn)
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
                    });
                });
        } else {
            let drag = ui.allocate_response(vec2(search_avail, 28.0), Sense::drag());
            if drag.drag_started() {
                ctx.send_viewport_cmd(egui::ViewportCommand::StartDrag);
            }
            ui.painter().text(
                pos2(drag.rect.left() + 6.0, drag.rect.center().y),
                egui::Align2::LEFT_CENTER,
                "直接打字即可搜索...",
                FontId::proportional(12.0),
                palette.text_muted,
            );
        }

        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            if icon_button(ui, 28.0, false, "偏好设置 (⌘,)", palette, draw_gear_glyph).clicked() {
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
                state.preview_open = !state.preview_open;
                state.preview_manually_toggled = true;
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
        });
    });
}

fn render_filter_strip(
    ui: &mut Ui,
    collections: &[Collection],
    state: &mut PopupState,
    palette: &EarthPalette,
) {
    ScrollArea::horizontal()
        .id_salt("earth_filter_capsules")
        .scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::AlwaysHidden)
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 5.0;
                let pills = [
                    (CategoryFilter::All, "全部"),
                    (CategoryFilter::Text, "文本"),
                    (CategoryFilter::Image, "图片"),
                    (CategoryFilter::Files, "文件"),
                    (CategoryFilter::Color, "颜色"),
                    (CategoryFilter::Link, "链接"),
                    (CategoryFilter::Pinned, "置顶"),
                ];

                for (filter, label) in pills {
                    capsule_button(ui, state.filter == filter, label, None, palette, || {
                        state.filter = filter;
                    });
                }

                for col in collections {
                    let dot = parse_hex_color32(&col.color_hex);
                    let f = CategoryFilter::Collection(col.id);
                    capsule_button(ui, state.filter == f, &col.name, dot, palette, || {
                        state.filter = f;
                    });
                }
            });
        });
}

fn capsule_button(
    ui: &mut Ui,
    active: bool,
    label: &str,
    dot_color: Option<Color32>,
    palette: &EarthPalette,
    on_click: impl FnOnce(),
) {
    let fill = if active {
        palette.accent_soft
    } else {
        palette.card_bg
    };
    let stroke = if active {
        Stroke::new(1.0, palette.accent)
    } else {
        Stroke::new(1.0, palette.border)
    };
    let text_color = if active {
        palette.accent
    } else {
        palette.text_secondary
    };

    let text = if dot_color.is_some() {
        format!("● {label}")
    } else {
        label.to_string()
    };

    let mut rich = RichText::new(text).size(11.5).color(text_color);
    if active {
        rich = rich.strong();
    }
    let btn = egui::Button::new(rich)
        .fill(fill)
        .stroke(stroke)
        .corner_radius(CornerRadius::same(12))
        .min_size(vec2(0.0, 22.0));
    if ui.add(btn).clicked() {
        on_click();
    }
}

fn render_items_list(
    ui: &mut Ui,
    settings: &AppSettings,
    results: &[SearchResultItem],
    collections: &[Collection],
    state: &mut PopupState,
    palette: &EarthPalette,
    actions: &mut Vec<PopupAction>,
) {
    if results.is_empty() {
        egui::Frame::NONE
            .fill(palette.surface_bg)
            .stroke(Stroke::new(1.0, palette.border))
            .corner_radius(CornerRadius::same(10))
            .inner_margin(egui::Margin::same(20))
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.vertical_centered(|ui| {
                    ui.add_space(40.0);
                    ui.label(
                        RichText::new(if state.search_query.is_empty() {
                            "暂无剪贴板记录"
                        } else {
                            "没有找到匹配的记录"
                        })
                        .size(14.0)
                        .color(palette.text_secondary),
                    );
                    ui.add_space(6.0);
                    ui.label(
                        RichText::new("复制文字、图片或文件后将自动出现在这里")
                            .size(12.0)
                            .color(palette.text_muted),
                    );
                    ui.add_space(40.0);
                });
            });
        return;
    }

    ScrollArea::vertical()
        .id_salt("earth_history_scroll")
        .auto_shrink([false, false])
        .show(ui, |ui| {
            let mut seen_pinned_sep = false;
            let mut prev_pinned: Option<bool> = None;

            for (idx, res) in results.iter().enumerate() {
                let item = &res.item;
                if let Some(prev) = prev_pinned
                    && prev != item.pinned && !seen_pinned_sep {
                        seen_pinned_sep = true;
                        ui.add_space(2.0);
                        ui.separator();
                        ui.add_space(2.0);
                    }
                prev_pinned = Some(item.pinned);

                let selected = state.selected_id == Some(item.id);
                let shortcut_num = if idx < 9 { Some(idx + 1) } else { None };
                let resp = render_item_row(
                    ui,
                    res,
                    shortcut_num,
                    selected,
                    settings,
                    collections,
                    state,
                    palette,
                    actions,
                );

                if selected && state.scroll_to_selection {
                    resp.scroll_to_me(Some(Align::Center));
                    state.scroll_to_selection = false;
                }
            }
        });
}

#[allow(clippy::too_many_arguments)]
fn render_item_row(
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
    let row_height = match item.kind {
        ClipKind::Image => settings.image_height.max(28.0) + 12.0,
        _ => 34.0,
    };

    let row_fill = if selected {
        palette.row_selected_bg
    } else {
        Color32::TRANSPARENT
    };
    let row_stroke = if selected {
        Stroke::new(1.0, palette.accent)
    } else {
        Stroke::NONE
    };

    let frame = egui::Frame::NONE
        .fill(row_fill)
        .stroke(row_stroke)
        .corner_radius(CornerRadius::same(8))
        .inner_margin(egui::Margin::symmetric(8, 4));

    let inner = frame.show(ui, |ui| {
        ui.set_width((ui.available_width() - 4.0).max(120.0));
        ui.set_min_height(row_height - 8.0);
        ui.horizontal(|ui| {
            // Left selection bar
            if selected {
                let (bar, _) = ui.allocate_exact_size(vec2(3.0, row_height - 14.0), Sense::hover());
                ui.painter().rect_filled(bar, CornerRadius::same(2), palette.accent);
            }

            // Type / Color / Image thumbnail badge
            match item.kind {
                ClipKind::Text => {
                    if settings.show_hex_color_swatch
                        && let Some(rgba) = item.detected_color {
                            let (swatch, _) = ui.allocate_exact_size(vec2(16.0, 16.0), Sense::hover());
                            ui.painter().rect_filled(
                                swatch,
                                CornerRadius::same(4),
                                Color32::from_rgba_unmultiplied(rgba[0], rgba[1], rgba[2], rgba[3]),
                            );
                            ui.painter().rect_stroke(
                                swatch,
                                CornerRadius::same(4),
                                Stroke::new(1.0, palette.border_strong),
                                egui::StrokeKind::Inside,
                            );
                        }
                    if item.is_url {
                        let (r, _) = ui.allocate_exact_size(vec2(14.0, 14.0), Sense::hover());
                        draw_link_badge(ui.painter(), r, palette.accent_secondary);
                    } else if item.is_sensitive() && settings.enable_privacy_masking {
                        let (r, _) = ui.allocate_exact_size(vec2(14.0, 14.0), Sense::hover());
                        draw_shield_glyph(ui.painter(), r, palette.pin_amber);
                    }
                }
                ClipKind::Files => {
                    let (r, _) = ui.allocate_exact_size(vec2(15.0, 15.0), Sense::hover());
                    draw_file_badge(ui.painter(), r, palette.accent_secondary);
                }
                ClipKind::Image => {
                    if let Some(thumb) = item.thumb_path.as_deref().or(item.image_path.as_deref()) {
                        let uri = path_to_file_uri(thumb);
                        let h = settings.image_height.max(24.0);
                        ui.add(
                            egui::Image::from_uri(uri)
                                .max_size(vec2(h * 2.2, h))
                                .corner_radius(CornerRadius::same(5)),
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

            let text_w = (ui.available_width() - right_reserved).max(80.0);
            ui.allocate_ui(vec2(text_w, row_height - 10.0), |ui| {
                ui.with_layout(Layout::left_to_right(Align::Center), |ui| {
                    match item.kind {
                        ClipKind::Text => {
                            let raw = item.text_content.as_deref().unwrap_or("");
                            let is_revealed = state.revealed_sensitive_ids.contains(&item.id);
                            if item.is_sensitive() && settings.enable_privacy_masking && !is_revealed {
                                let masked = mask_sensitive_preview(raw);
                                let lbl = item.sensitive_label.as_deref().unwrap_or("敏感");
                                ui.label(
                                    RichText::new(format!("[{lbl}] {masked}"))
                                        .size(12.8)
                                        .color(palette.text_secondary),
                                );
                            } else {
                                let formatted = format_display_line(raw, settings.show_special_symbols);
                                let clipped = truncate_chars(&formatted, 88);
                                let job = build_highlighted_layout_job(
                                    &clipped,
                                    &res.matched_indices,
                                    settings.match_highlight,
                                    palette,
                                    13.0,
                                    selected,
                                );
                                ui.label(job);
                            }
                        }
                        ClipKind::Image => {
                            let summary = item.summary_line(40);
                            ui.label(
                                RichText::new(summary)
                                    .size(12.2)
                                    .color(palette.text_secondary),
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
                                selected,
                            );
                            ui.label(job);
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

                    egui::Frame::NONE
                        .fill(palette.keycap_bg)
                        .corner_radius(CornerRadius::same(5))
                        .inner_margin(egui::Margin::symmetric(5, 2))
                        .show(ui, |ui| {
                            ui.label(
                                RichText::new(label)
                                    .size(10.5)
                                    .color(palette.keycap_text),
                            );
                        });
                }

                if item.pinned {
                    let (r, _) = ui.allocate_exact_size(vec2(12.0, 12.0), Sense::hover());
                    draw_pin_badge(ui.painter(), r, palette.pin_amber);
                }

                if let Some(cid) = item.collection_id
                    && let Some(col) = collections.iter().find(|c| c.id == cid) {
                        let c = parse_hex_color32(&col.color_hex).unwrap_or(palette.accent);
                        let (r, resp) = ui.allocate_exact_size(vec2(10.0, 10.0), Sense::hover());
                        ui.painter().circle_filled(r.center(), 4.0, c);
                        resp.on_hover_text(&col.name);
                    }

                if settings.show_source_app_badge
                    && let Some(app) = &item.source_app {
                        let short = truncate_chars(app, 10);
                        ui.label(
                            RichText::new(short)
                                .size(10.5)
                                .color(palette.text_muted),
                        );
                    }
            });
        });
    });

    let resp = inner.response.interact(Sense::click());
    if resp.hovered() && state.selected_id != Some(item.id) && !selected {
        ui.painter().rect_stroke(
            resp.rect,
            CornerRadius::same(8),
            Stroke::new(1.0, palette.border_strong),
            egui::StrokeKind::Inside,
        );
    }

    if resp.clicked() {
        state.selected_id = Some(item.id);
        state.selection_changed_at = Instant::now();
        let mods = ui.ctx().input(|i| i.modifiers);
        let behavior = behavior_from_modifiers(mods);
        actions.push(PopupAction::PasteItem {
            id: item.id,
            behavior,
        });
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
        let pin_label = if item.pinned { "取消置顶" } else { "置顶记录" };
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
            state.selected_id = Some(item.id);
            state.preview_open = true;
            state.preview_manually_toggled = true;
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

fn render_preview_pane(
    ui: &mut Ui,
    item: Option<&ClipItem>,
    collections: &[Collection],
    state: &mut PopupState,
    palette: &EarthPalette,
    actions: &mut Vec<PopupAction>,
) {
    egui::Frame::NONE
        .fill(palette.surface_bg)
        .stroke(Stroke::new(1.0, palette.border))
        .corner_radius(CornerRadius::same(10))
        .inner_margin(egui::Margin::same(12))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.set_height(ui.available_height());

            let Some(item) = item else {
                ui.centered_and_justified(|ui| {
                    ui.label(
                        RichText::new("选择左侧记录以预览详情与快捷加工")
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
                        "颜色色值"
                    } else if item.is_url {
                        "网页链接"
                    } else {
                        "文本"
                    }
                }
                ClipKind::Image => "图像",
                ClipKind::Files => "文件",
            };
            let ts_str = format_epoch_millis(item.last_copied_at);

            ui.horizontal(|ui| {
                egui::Frame::NONE
                    .fill(palette.accent_soft)
                    .corner_radius(CornerRadius::same(6))
                    .inner_margin(egui::Margin::symmetric(7, 3))
                    .show(ui, |ui| {
                        ui.label(RichText::new(kind_label).size(11.0).color(palette.accent));
                    });
                ui.label(
                    RichText::new(format!(
                        "{ts_str} · 复制 {} 次 · {}",
                        item.copy_count,
                        format_bytes(item.byte_size)
                    ))
                    .size(11.5)
                    .color(palette.text_secondary),
                );
            });

            if let Some(app) = &item.source_app {
                ui.label(
                    RichText::new(format!("来源应用：{app}"))
                        .size(11.0)
                        .color(palette.text_muted),
                );
            }

            ui.add_space(4.0);
            ui.horizontal(|ui| {
                let pin_btn = if item.pinned { "已置顶" } else { "置顶" };
                if ui.small_button(pin_btn).clicked() {
                    actions.push(PopupAction::TogglePin(item.id));
                }

                // Collection assignment dropdown
                let cur_col = item
                    .collection_id
                    .and_then(|cid| collections.iter().find(|c| c.id == cid))
                    .map(|c| c.name.as_str())
                    .unwrap_or("加入收藏分组");
                egui::ComboBox::from_id_salt(("preview_col", item.id))
                    .selected_text(cur_col)
                    .width(120.0)
                    .show_ui(ui, |ui| {
                        if ui
                            .selectable_label(item.collection_id.is_none(), "未分组")
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

                if item.is_sensitive() {
                    let revealed = state.revealed_sensitive_ids.contains(&item.id);
                    let btn_lbl = if revealed { "隐藏明文" } else { "显示明文" };
                    if ui.small_button(btn_lbl).clicked() {
                        if revealed {
                            state.revealed_sensitive_ids.remove(&item.id);
                        } else {
                            state.revealed_sensitive_ids.insert(item.id);
                        }
                    }
                }

                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if ui
                        .small_button(RichText::new("删除").color(palette.danger))
                        .clicked()
                    {
                        actions.push(PopupAction::DeleteItem(item.id));
                    }
                });
            });

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
                        165.0
                    } else {
                        68.0
                    }
                }
                _ => 42.0,
            };
            let body_h = (ui.available_height() - actions_area_h).max(110.0);

            ScrollArea::both()
                .id_salt(("preview_body", item.id))
                .max_height(body_h)
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    ui.set_min_height(body_h);
                    match item.kind {
                        ClipKind::Text => {
                            let raw = item.text_content.as_deref().unwrap_or("");
                            let hidden = item.is_sensitive()
                                && !state.revealed_sensitive_ids.contains(&item.id);
                            if let Some(rgba) = item.detected_color {
                                render_color_card(ui, raw.trim(), rgba, palette, actions);
                                ui.add_space(6.0);
                            }
                            if hidden {
                                ui.label(
                                    RichText::new(format!(
                                        "🔒 检测到敏感内容（{}），已默认脱敏保护。\n点击上方「显示明文」按钮可临时查看完整内容。",
                                        item.sensitive_label.as_deref().unwrap_or("凭据")
                                    ))
                                    .size(12.5)
                                    .color(palette.text_secondary),
                                );
                            } else {
                                let mut display = raw.to_string();
                                ui.add(
                                    egui::TextEdit::multiline(&mut display)
                                        .font(FontId::monospace(12.2))
                                        .desired_width(f32::INFINITY)
                                        .frame(egui::Frame::NONE)
                                        .interactive(false),
                                );
                            }
                        }
                        ClipKind::Image => {
                            if let Some(path) = item.image_path.as_deref() {
                                let uri = path_to_file_uri(path);
                                let max_w = ui.available_width().max(120.0);
                                let max_h = (body_h - 32.0).max(100.0);
                                let img = egui::Image::from_uri(uri)
                                    .corner_radius(CornerRadius::same(8));
                                let img = if state.image_actual_size {
                                    img
                                } else {
                                    img.max_size(vec2(max_w, max_h))
                                };
                                ui.centered_and_justified(|ui| {
                                    ui.add(img);
                                });
                            }
                        }
                        ClipKind::Files => {
                            for f in &item.files {
                                let exists = f.exists();
                                egui::Frame::NONE
                                    .fill(palette.card_bg)
                                    .corner_radius(CornerRadius::same(7))
                                    .inner_margin(egui::Margin::same(8))
                                    .show(ui, |ui| {
                                        ui.set_width(ui.available_width());
                                        ui.horizontal(|ui| {
                                            ui.label(
                                                RichText::new(if f.is_dir { "📁" } else { "📄" })
                                                    .size(13.0),
                                            );
                                            ui.vertical(|ui| {
                                                ui.label(RichText::new(&f.name).strong().size(12.5));
                                                ui.label(
                                                    RichText::new(&f.path)
                                                        .size(11.0)
                                                        .color(palette.text_muted),
                                                );
                                                let status = if exists {
                                                    if f.is_dir {
                                                        "文件夹 · 有效".to_string()
                                                    } else {
                                                        format!("{} · 有效", format_bytes(f.size_bytes))
                                                    }
                                                } else {
                                                    "⚠ 原路径不存在".to_string()
                                                };
                                                ui.label(
                                                    RichText::new(status)
                                                        .size(10.5)
                                                        .color(if exists {
                                                            palette.accent
                                                        } else {
                                                            palette.danger
                                                        }),
                                                );
                                            });
                                            if exists {
                                                ui.with_layout(
                                                    Layout::right_to_left(Align::Center),
                                                    |ui| {
                                                        if ui.small_button("定位").clicked() {
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
                        }
                    }
                });

            ui.separator();

            // Quick Transform Actions Bar (Earth signature!)
            match item.kind {
                ClipKind::Text => {
                    let raw = item.text_content.as_deref().unwrap_or("");
                    let quick_list = available_quick_actions(raw);
                    ui.horizontal_wrapped(|ui| {
                        if item.is_url && ui.small_button("🌐 打开链接").clicked() {
                            actions.push(PopupAction::OpenUrl(raw.trim().to_string()));
                        }
                        for qa in quick_list {
                            if ui.small_button(qa.label()).clicked() {
                                let res = apply_quick_action(qa, raw);
                                state.active_transform = Some((item.id, qa, res));
                            }
                        }
                    });

                    if let Some((tid, qa, res)) = state.active_transform.clone()
                        && tid == item.id {
                            ui.add_space(4.0);
                            egui::Frame::NONE
                                .fill(palette.card_bg)
                                .stroke(Stroke::new(1.0, palette.accent))
                                .corner_radius(CornerRadius::same(7))
                                .inner_margin(egui::Margin::same(8))
                                .show(ui, |ui| {
                                    ui.set_width(ui.available_width());
                                    ui.horizontal(|ui| {
                                        ui.label(
                                            RichText::new(format!("⚡ {} 结果", qa.label()))
                                                .size(11.5)
                                                .strong()
                                                .color(palette.accent),
                                        );
                                        ui.with_layout(
                                            Layout::right_to_left(Align::Center),
                                            |ui| {
                                                if ui.small_button("×").clicked() {
                                                    state.active_transform = None;
                                                }
                                                if let Ok(ref txt) = res {
                                                    if ui.small_button("粘贴结果").clicked() {
                                                        actions.push(
                                                            PopupAction::PasteTransformedText(
                                                                txt.clone(),
                                                            ),
                                                        );
                                                    }
                                                    if ui.small_button("复制结果").clicked() {
                                                        actions.push(
                                                            PopupAction::CopyTransformedText(
                                                                txt.clone(),
                                                            ),
                                                        );
                                                    }
                                                }
                                            },
                                        );
                                    });
                                    match res {
                                        Ok(mut out) => {
                                            ScrollArea::vertical()
                                                .id_salt("qa_result_scroll")
                                                .max_height(56.0)
                                                .show(ui, |ui| {
                                                    ui.add(
                                                        egui::TextEdit::multiline(&mut out)
                                                            .font(FontId::monospace(11.5))
                                                            .desired_width(f32::INFINITY)
                                                            .frame(egui::Frame::NONE)
                                                            .interactive(false),
                                                    );
                                                });
                                        }
                                        Err(err) => {
                                            ui.label(
                                                RichText::new(err)
                                                    .size(11.5)
                                                    .color(palette.danger),
                                            );
                                        }
                                    }
                                });
                        }
                }
                ClipKind::Image => {
                    ui.horizontal(|ui| {
                        let zoom_lbl = if state.image_actual_size {
                            "适应窗口"
                        } else {
                            "原始尺寸"
                        };
                        if ui.small_button(zoom_lbl).clicked() {
                            state.image_actual_size = !state.image_actual_size;
                        }
                        if let Some(path) = &item.image_path
                            && ui.small_button("另存为图片...").clicked() {
                                actions.push(PopupAction::SaveImageAs(path.clone()));
                            }
                        if ui.small_button("复制并粘贴图片").clicked() {
                            actions.push(PopupAction::PasteItem {
                                id: item.id,
                                behavior: PasteBehavior::CopyAndPaste,
                            });
                        }
                    });
                }
                ClipKind::Files => {
                    ui.horizontal(|ui| {
                        if ui.small_button("复制路径文本").clicked() {
                            let paths = item
                                .files
                                .iter()
                                .map(|f| f.path.as_str())
                                .collect::<Vec<_>>()
                                .join("\n");
                            actions.push(PopupAction::CopyTransformedText(paths));
                        }
                        if let Some(first) = item.files.iter().find(|f| f.exists())
                            && ui.small_button("在文件管理器中定位").clicked() {
                                actions.push(PopupAction::RevealFile(PathBuf::from(&first.path)));
                            }
                    });
                }
            }
        });
}

fn render_color_card(
    ui: &mut Ui,
    raw_hex: &str,
    rgba: [u8; 4],
    palette: &EarthPalette,
    actions: &mut Vec<PopupAction>,
) {
    let hex_norm = if raw_hex.starts_with('#') {
        raw_hex.to_uppercase()
    } else {
        format!("#{}", raw_hex.to_uppercase())
    };
    let rgb_str = format!("rgb({}, {}, {})", rgba[0], rgba[1], rgba[2]);

    egui::Frame::NONE
        .fill(palette.card_bg)
        .corner_radius(CornerRadius::same(8))
        .inner_margin(egui::Margin::same(8))
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                let (r, _) = ui.allocate_exact_size(vec2(38.0, 38.0), Sense::hover());
                ui.painter().rect_filled(
                    r,
                    CornerRadius::same(6),
                    Color32::from_rgba_unmultiplied(rgba[0], rgba[1], rgba[2], rgba[3]),
                );
                ui.vertical(|ui| {
                    ui.horizontal(|ui| {
                        ui.label(RichText::new(&hex_norm).strong().size(13.0));
                        if ui.small_button("复制 HEX").clicked() {
                            actions.push(PopupAction::CopyTransformedText(hex_norm.clone()));
                        }
                    });
                    ui.horizontal(|ui| {
                        ui.label(
                            RichText::new(&rgb_str)
                                .size(11.5)
                                .color(palette.text_secondary),
                        );
                        if ui.small_button("复制 RGB").clicked() {
                            actions.push(PopupAction::CopyTransformedText(rgb_str));
                        }
                    });
                });
            });
        });
}

fn render_footer(
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
        ui.label(
            RichText::new(format!(
                "{total_items} 条 · {}",
                format_bytes(storage_bytes)
            ))
            .size(11.5)
            .color(palette.text_muted),
        );

        let ignore_lbl = if settings.ignore_next_event {
            "✓ 将忽略下次复制"
        } else {
            "忽略下次"
        };
        if ui
            .small_button(RichText::new(ignore_lbl).size(11.0))
            .clicked()
        {
            actions.push(PopupAction::ToggleIgnoreNext);
        }

        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            #[cfg(target_os = "macos")]
            let (quit_s, prefs_s, clear_s) = ("⌘Q", "⌘,", "⌥⌘⌫");
            #[cfg(not(target_os = "macos"))]
            let (quit_s, prefs_s, clear_s) = ("Ctrl+Q", "Ctrl+,", "Ctrl+Alt+⌫");

            if ui
                .small_button(RichText::new(format!("退出 {quit_s}")).size(11.5))
                .clicked()
            {
                actions.push(PopupAction::Quit);
            }
            if ui.small_button(RichText::new("关于").size(11.5)).clicked() {
                state.show_about_modal = true;
            }
            if ui
                .small_button(RichText::new(format!("偏好设置 {prefs_s}")).size(11.5))
                .clicked()
            {
                actions.push(PopupAction::OpenSettings);
            }
            if ui
                .small_button(
                    RichText::new(format!("清除 {clear_s}"))
                        .size(11.5)
                        .color(palette.danger),
                )
                .clicked()
            {
                actions.push(PopupAction::ClearUnpinned);
            }
        });
    });
}

fn render_banner(
    ui: &mut Ui,
    msg: &str,
    is_warn: bool,
    show_ax_btn: bool,
    palette: &EarthPalette,
    actions: &mut Vec<PopupAction>,
) -> bool {
    let mut close_clicked = false;
    let fg = if is_warn {
        palette.pin_amber
    } else {
        palette.accent
    };
    let bg = if is_warn {
        Color32::from_rgba_unmultiplied(245, 158, 11, 26)
    } else {
        palette.accent_soft
    };
    egui::Frame::NONE
        .fill(bg)
        .stroke(Stroke::new(1.0, fg))
        .corner_radius(CornerRadius::same(8))
        .inner_margin(egui::Margin::symmetric(8, 5))
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.label(RichText::new(msg).size(11.5).color(fg));
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if ui.small_button("×").clicked() {
                        close_clicked = true;
                    }
                    if show_ax_btn && is_warn && cfg!(target_os = "macos") && ui.small_button("打开辅助功能设置").clicked() {
                        actions.push(PopupAction::OpenAccessibilitySettings);
                    }
                });
            });
        });
    ui.add_space(4.0);
    close_clicked
}

fn render_about_overlay(ctx: &egui::Context, state: &mut PopupState, palette: &EarthPalette) {
    egui::Window::new("关于 Earth")
        .collapsible(false)
        .resizable(false)
        .anchor(egui::Align2::CENTER_CENTER, vec2(0.0, 0.0))
        .fixed_size(vec2(320.0, 190.0))
        .show(ctx, |ui| {
            ui.vertical_centered(|ui| {
                draw_earth_emblem(ui, 42.0, palette, false);
                ui.add_space(6.0);
                ui.label(RichText::new("Earth 剪贴板").size(16.0).strong());
                ui.label(
                    RichText::new("v0.1.0 · Pure Rust (macOS & Windows)")
                        .size(11.5)
                        .color(palette.text_secondary),
                );
                ui.add_space(8.0);
                ui.label(
                    RichText::new(
                        "支持文字、图片、文件的极速剪贴板工作台。\n内置速览预览、快捷数据加工、收藏分组与本地敏感信息脱敏。",
                    )
                    .size(12.0)
                    .color(palette.text_muted),
                );
                ui.add_space(12.0);
                if ui.button("关闭").clicked() {
                    state.show_about_modal = false;
                }
            });
        });
}

fn path_to_file_uri(path: &str) -> String {
    let normalized = path.replace('\\', "/");
    if normalized.starts_with('/') {
        format!("file://{normalized}")
    } else {
        format!("file:///{normalized}")
    }
}

fn format_epoch_millis(millis: i64) -> String {
    if let Some(utc) = DateTime::from_timestamp_millis(millis) {
        let local: DateTime<Local> = utc.into();
        return local.format("%m-%d %H:%M:%S").to_string();
    }
    String::new()
}
