use crate::hotkey::{format_shortcut_display, matches_egui_shortcut};
use crate::model::{
    format_bytes, truncate_chars, AppSettings, CategoryFilter, ClipItem, ClipKind, Collection,
    CopyTriggerMode, PasteBehavior, SearchBoxVisibility, SearchMode,
};
use crate::search::SearchResultItem;
use crate::transforms::{
    apply_quick_action, available_quick_actions, format_display_line, mask_sensitive_preview,
    text_stats, QuickActionKind,
};
use crate::ui::icons::{
    draw_earth_emblem, draw_file_badge, draw_gear_glyph, draw_link_badge, draw_pause_glyph,
    draw_pin_badge, draw_play_glyph, draw_search_glyph, draw_shield_glyph, draw_split_pane_glyph,
    icon_button,
};
use crate::ui::theme::{
    build_highlighted_layout_job, card_shadow, compact_button, dismiss_button, padded_button,
    parse_hex_color32, EarthPalette,
};
use crate::updater::{ReleaseInfo, UpdateDownloadState, CURRENT_VERSION};
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
    SetItemNote { item_id: i64, note: Option<String> },
    UpdateItemText {
        item_id: i64,
        new_text: String,
        paste_after: bool,
    },
    DeleteItem(i64),
    ClearUnpinned,
    OpenSettings,
    TogglePause,
    ToggleIgnoreNext,
    SettingsChanged,
    SaveImageAs(String),
    RevealFile(PathBuf),
    OpenUrl(String),
    StartUpdateDownload(ReleaseInfo),
    ApplyUpdateAndRestart(PathBuf),
    OpenAccessibilitySettings,
    Quit,
}

pub struct CachedPreviewItem {
    pub item_id: i64,
    pub last_copied_at: i64,
    pub byte_size: u64,
    pub formatted_ts: String,
    pub display_text: String,
    pub text_stats: Option<(usize, usize, usize)>,
    pub quick_actions: Vec<QuickActionKind>,
    pub file_exists: Vec<bool>,
    pub qr_matrix: Option<Result<(usize, Vec<bool>), String>>,
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
    pub inline_edit_text: Option<(i64, String)>,
    pub inline_edit_note: Option<(i64, String)>,
    pub qr_preview_id: Option<i64>,
    pub preview_cache: Option<CachedPreviewItem>,
    pub show_about_modal: bool,
    pub status_banner: Option<(String, bool)>,
    pub available_update: Option<ReleaseInfo>,
    pub update_download: UpdateDownloadState,
    pub dismissed_update_version: Option<String>,
    pub dismissed_accessibility_banner: bool,
    pub ime_composing: bool,
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
            inline_edit_text: None,
            inline_edit_note: None,
            qr_preview_id: None,
            preview_cache: None,
            show_about_modal: false,
            status_banner: None,
            available_update: None,
            update_download: UpdateDownloadState::Idle,
            dismissed_update_version: None,
            dismissed_accessibility_banner: false,
            ime_composing: false,
            scroll_to_selection: false,
        }
    }
}

impl PopupState {
    pub fn ensure_preview_cache(&mut self, item: &ClipItem) -> &CachedPreviewItem {
        const MAX_PREVIEW_TEXT_CHARS: usize = 4000;
        let needs_rebuild = self
            .preview_cache
            .as_ref()
            .map(|c| {
                c.item_id != item.id
                    || c.last_copied_at != item.last_copied_at
                    || c.byte_size != item.byte_size
            })
            .unwrap_or(true);
        if needs_rebuild {
            let formatted_ts = format_epoch_millis(item.last_copied_at);
            let (display_text, stats, quick_actions) = if item.kind == ClipKind::Text {
                let raw = item.text_content.as_deref().unwrap_or("");
                let display = match raw.char_indices().nth(MAX_PREVIEW_TEXT_CHARS) {
                    Some((byte_idx, _)) => format!(
                        "{}\n\n…（内容较长，速览仅展示前 {} 个字符；复制与粘贴仍保留完整内容）",
                        &raw[..byte_idx],
                        MAX_PREVIEW_TEXT_CHARS
                    ),
                    None => raw.to_string(),
                };
                (
                    display,
                    Some(text_stats(raw)),
                    available_quick_actions(raw),
                )
            } else {
                (String::new(), None, Vec::new())
            };
            let file_exists = if item.kind == ClipKind::Files {
                item.files.iter().map(|f| f.exists()).collect()
            } else {
                Vec::new()
            };
            self.preview_cache = Some(CachedPreviewItem {
                item_id: item.id,
                last_copied_at: item.last_copied_at,
                byte_size: item.byte_size,
                formatted_ts,
                display_text,
                text_stats: stats,
                quick_actions,
                file_exists,
                qr_matrix: None,
            });
        }
        if self.qr_preview_id == Some(item.id)
            && let Some(cache) = self.preview_cache.as_mut()
            && cache.qr_matrix.is_none()
        {
            let raw = item.text_content.as_deref().unwrap_or("").trim();
            cache.qr_matrix = Some(build_qr_matrix(raw));
        }
        self.preview_cache.as_ref().unwrap()
    }
}

fn build_qr_matrix(text: &str) -> Result<(usize, Vec<bool>), String> {
    if text.is_empty() {
        return Err("空文本无法生成二维码".to_string());
    }
    if text.len() > 2048 {
        return Err("文本长度超过二维码容量上限（2048 字节）".to_string());
    }
    let code = qrcode::QrCode::new(text.as_bytes())
        .map_err(|e| format!("生成二维码失败: {e}"))?;
    let width = code.width();
    let colors = code
        .into_colors()
        .into_iter()
        .map(|c| c == qrcode::Color::Dark)
        .collect();
    Ok((width, colors))
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
                                RichText::new("⚠ 辅助功能未生效（若已开启，请在设置中点「-」移除后重加）")
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

    // Main Split Area: Left History List + Optional Right Quick-Peek Workbench
    let footer_reserved = if settings.show_footer_menu { 44.0 } else { 0.0 };
    let body_height = (ui.available_height() - footer_reserved).max(160.0);
    let total_width = ui.available_width();
    let split_gap = 8.0;

    let (body_rect, _) = ui.allocate_exact_size(vec2(total_width, body_height), Sense::hover());
    // Wait until the OS viewport has expanded before splitting into two columns so we never
    // layout/wrap both panes at cramped intermediate widths (240px + 150px) inside the 424px frame.
    let split_ready = state.preview_open && body_rect.width() >= 560.0;
    if state.preview_open && !split_ready {
        ctx.request_repaint();
    }
    let (left_rect, right_rect) = if split_ready {
        let lw = (body_rect.width() * 0.45).clamp(240.0, 320.0);
        let l = egui::Rect::from_min_max(
            body_rect.min,
            pos2(body_rect.min.x + lw, body_rect.max.y),
        );
        let r = egui::Rect::from_min_max(
            pos2(body_rect.min.x + lw + split_gap, body_rect.min.y),
            body_rect.max,
        );
        (l, Some(r))
    } else {
        (body_rect, None)
    };

    ui.scope_builder(
        egui::UiBuilder::new()
            .max_rect(left_rect)
            .layout(Layout::top_down(Align::Min)),
        |ui| {
            ui.set_clip_rect(left_rect);
            ui.set_min_size(left_rect.size());
            ui.set_max_size(left_rect.size());
            render_items_list(ui, settings, results, collections, state, palette, &mut actions);
        },
    );

    if state.preview_open
        && let Some(sid) = state.selected_id
        && let Some(sel) = results.iter().find(|r| r.item.id == sid)
    {
        state.ensure_preview_cache(&sel.item);
    }

    if let Some(r_rect) = right_rect {
        ui.scope_builder(
            egui::UiBuilder::new()
                .max_rect(r_rect)
                .layout(Layout::top_down(Align::Min)),
            |ui| {
                ui.set_clip_rect(r_rect);
                ui.set_min_size(r_rect.size());
                ui.set_max_size(r_rect.size());
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

    if settings.show_footer_menu {
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
    collections: &[Collection],
    state: &mut PopupState,
    actions: &mut Vec<PopupAction>,
) {
    if state.show_about_modal {
        if ctx.input_mut(|i| i.consume_key(Modifiers::NONE, Key::Escape)) {
            state.show_about_modal = false;
        }
        return;
    }

    let editing_workbench = state
        .inline_edit_text
        .as_ref()
        .map(|(id, _)| Some(*id) == state.selected_id)
        .unwrap_or(false)
        || state
            .inline_edit_note
            .as_ref()
            .map(|(id, _)| Some(*id) == state.selected_id)
            .unwrap_or(false);

    // Esc cancels workbench inline editing first, then clears search, or closes popup
    if ctx.input_mut(|i| i.consume_key(Modifiers::NONE, Key::Escape)) {
        if editing_workbench {
            state.inline_edit_text = None;
            state.inline_edit_note = None;
        } else if !state.search_query.is_empty() {
            state.search_query.clear();
            state.request_search_focus = true;
        } else {
            actions.push(PopupAction::HidePopup);
        }
        return;
    }

    // While editing text/note inside the workbench, let the editor handle arrows/Enter/Tab
    if editing_workbench {
        return;
    }

    // Tab / Shift+Tab cycles through category & collection filter capsules
    let shift_tab = ctx.input_mut(|i| i.consume_key(Modifiers::SHIFT, Key::Tab));
    let plain_tab = !shift_tab && ctx.input_mut(|i| i.consume_key(Modifiers::NONE, Key::Tab));
    if shift_tab || plain_tab {
        let mut filters = vec![
            CategoryFilter::All,
            CategoryFilter::Text,
            CategoryFilter::Image,
            CategoryFilter::Files,
            CategoryFilter::Pinned,
        ];
        for col in collections {
            filters.push(CategoryFilter::Collection(col.id));
        }
        let cur = filters
            .iter()
            .position(|&f| f == state.filter)
            .unwrap_or(0);
        let next = if shift_tab {
            if cur == 0 {
                filters.len() - 1
            } else {
                cur - 1
            }
        } else {
            (cur + 1) % filters.len()
        };
        state.filter = filters[next];
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
        if emblem_resp.drag_started() {
            ctx.send_viewport_cmd(egui::ViewportCommand::StartDrag);
        }
        if emblem_resp.hovered() {
            emblem_resp.on_hover_text("拖动移动窗口");
        }

        let show_search = settings.search_box_visibility == SearchBoxVisibility::Always
            || !state.search_query.is_empty()
            || state.ime_composing;

        let right_controls_width = 136.0;
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
    let strip_w = ui.available_width();
    ScrollArea::horizontal()
        .id_salt("earth_filter_capsules")
        .max_width(strip_w)
        .auto_shrink([false, true])
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
    if padded_button(ui, btn, vec2(9.0, 4.5)).clicked() {
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

    let list_avail_w = ui.available_width();
    ScrollArea::vertical()
        .id_salt("earth_history_scroll")
        .auto_shrink([false, false])
        .max_width(list_avail_w)
        .show_viewport(ui, |ui, viewport| {
            ui.set_max_width(list_avail_w);
            let origin_y = ui.cursor().min.y;
            let vis_min_y = viewport.min.y - 240.0;
            let vis_max_y = viewport.max.y + 240.0;
            let mut seen_pinned_sep = false;
            let mut prev_pinned: Option<bool> = None;

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
                let row_height = match item.kind {
                    ClipKind::Image => settings.image_height.max(28.0) + 12.0,
                    _ => 34.0,
                };
                let rel_y = ui.cursor().min.y - origin_y;
                let in_viewport =
                    (rel_y + row_height >= vis_min_y && rel_y <= vis_max_y) || selected;

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
            if selected {
                let bar_h = (row_height - 16.0).max(16.0);
                let bar = egui::Rect::from_center_size(slot.center(), vec2(3.5, bar_h));
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
                                Stroke::new(1.0, palette.border),
                                egui::StrokeKind::Inside,
                            );
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
                    if let Some(thumb) = item.thumb_path.as_deref().or(item.image_path.as_deref()) {
                        let uri = path_to_file_uri(thumb);
                        let h = settings.image_height.max(24.0);
                        ui.add(
                            egui::Image::from_uri(uri)
                                .max_size(vec2(h * 2.2, h))
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
                    if let Some(note) = item.note.as_deref().map(str::trim).filter(|n| !n.is_empty())
                    {
                        let short_note = truncate_chars(note, 12);
                        egui::Frame::NONE
                            .fill(palette.accent_soft)
                            .stroke(Stroke::new(1.0, palette.border_strong))
                            .corner_radius(CornerRadius::same(255))
                            .inner_margin(egui::Margin::symmetric(6, 1))
                            .show(ui, |ui| {
                                ui.label(
                                    RichText::new(format!("🏷 {short_note}"))
                                        .size(10.5)
                                        .color(palette.accent_text),
                                );
                            });
                    }
                    let rem_text_w = ui.available_width().max(24.0);
                    match item.kind {
                        ClipKind::Text => {
                            let raw = item.text_content.as_deref().unwrap_or("");
                            let is_revealed = state.revealed_sensitive_ids.contains(&item.id);
                            if item.is_sensitive() && settings.enable_privacy_masking && !is_revealed {
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
                                let formatted = format_display_line(raw, settings.show_special_symbols);
                                let clipped = truncate_chars(&formatted, 88);
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

                    egui::Frame::NONE
                        .fill(if selected { palette.accent } else { palette.keycap_bg })
                        .stroke(if selected {
                            Stroke::NONE
                        } else {
                            Stroke::new(1.0, palette.border_subtle)
                        })
                        .corner_radius(CornerRadius::same(255))
                        .inner_margin(egui::Margin::symmetric(6, 2))
                        .show(ui, |ui| {
                            ui.label(
                                RichText::new(label)
                                    .size(10.5)
                                    .color(if selected {
                                        palette.on_accent
                                    } else {
                                        palette.keycap_text
                                    }),
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

    let resp = inner
        .response
        .interact(Sense::click())
        .on_hover_cursor(egui::CursorIcon::PointingHand);
    let pointer_in_row = resp.hovered() || ui.rect_contains_pointer(resp.rect);
    if pointer_in_row {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    let border_stroke = if selected {
        Stroke::new(1.2, palette.accent)
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

    let primary_pressed =
        (resp.is_pointer_button_down_on() || pointer_in_row)
            && ui.input(|i| i.pointer.primary_pressed());

    match settings.copy_trigger_mode {
        CopyTriggerMode::DoubleClick => {
            if resp.double_clicked() {
                // Double click: copy / paste using the default behavior (modifiers still respected)
                state.selected_id = Some(item.id);
                state.selection_changed_at = Instant::now();
                let mods = ui.ctx().input(|i| i.modifiers);
                let behavior = behavior_from_modifiers(mods);
                actions.push(PopupAction::PasteItem {
                    id: item.id,
                    behavior,
                });
            } else if primary_pressed || resp.clicked() {
                // Single click: select the record and expand the quick-peek workbench immediately on press
                if state.selected_id != Some(item.id) {
                    state.selected_id = Some(item.id);
                    state.selection_changed_at = Instant::now();
                    state.image_actual_size = false;
                }
                if !state.preview_open {
                    state.preview_open = true;
                    state.preview_manually_toggled = true;
                    ui.ctx().request_repaint();
                }
            }
        }
        CopyTriggerMode::SingleClick => {
            if !state.preview_open {
                // Workbench is closed: single click directly copies/pastes without opening workbench
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
            } else if resp.double_clicked() {
                // Workbench is open: double click always copies/pastes
                state.selected_id = Some(item.id);
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
                    state.selected_id = Some(item.id);
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
        let pin_label = if item.pinned { "取消固定" } else { "固定记录" };
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
                .map(|(lines, chars, words)| format!(" · {lines} 行 · {chars} 字符 · {words} 词"))
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
                ui.add(
                    egui::Label::new(
                        RichText::new(format!(
                            "{ts_str} · 复制 {} 次 · {}{stats_suffix}",
                            item.copy_count,
                            format_bytes(item.byte_size)
                        ))
                        .size(11.2)
                        .color(palette.text_secondary),
                    )
                    .truncate(),
                );
            });

            if let Some(app) = &item.source_app {
                ui.label(
                    RichText::new(format!("来源应用：{app}"))
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
                    egui::Button::new(RichText::new("已固定").size(11.5).color(palette.warning_text))
                        .fill(palette.warning_bg)
                        .stroke(Stroke::new(1.0, palette.warning_border))
                        .corner_radius(CornerRadius::same(255))
                } else {
                    egui::Button::new(RichText::new("固定").size(11.5))
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
                    .unwrap_or("加入收藏分组");
                egui::ComboBox::from_id_salt(("preview_col", item.id))
                    .selected_text(cur_col)
                    .width(110.0)
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

                let note_lbl = if item.note.as_deref().map(str::trim).is_some_and(|n| !n.is_empty())
                {
                    "备注"
                } else {
                    "+ 备注"
                };
                if compact_button(ui, note_lbl).clicked() {
                    if is_editing_note {
                        state.inline_edit_note = None;
                    } else {
                        state.inline_edit_note =
                            Some((item.id, item.note.clone().unwrap_or_default()));
                    }
                }

                if item.kind == ClipKind::Text {
                    let edit_lbl = if is_editing_text { "退出编辑" } else { "编辑" };
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

                if item.is_sensitive() {
                    let revealed = state.revealed_sensitive_ids.contains(&item.id);
                    let btn_lbl = if revealed { "隐藏明文" } else { "显示明文" };
                    if compact_button(ui, btn_lbl).clicked() {
                        if revealed {
                            state.revealed_sensitive_ids.remove(&item.id);
                        } else {
                            state.revealed_sensitive_ids.insert(item.id);
                        }
                    }
                }

                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    let del_btn = egui::Button::new(RichText::new("删除").color(palette.danger))
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
                        ui.label(
                            RichText::new("🏷")
                                .size(12.0)
                                .color(palette.accent_text),
                        );
                        let edit_w = (ui.available_width() - 116.0).max(80.0);
                        let resp = ui
                            .scope(|ui| {
                                let r = CornerRadius::same(8);
                                ui.visuals_mut().widgets.inactive.corner_radius = r;
                                ui.visuals_mut().widgets.hovered.corner_radius = r;
                                ui.visuals_mut().widgets.active.corner_radius = r;
                                ui.add(
                                    egui::TextEdit::singleline(note_buf)
                                        .hint_text("输入备注别名（支持搜索匹配）…")
                                        .margin(egui::Margin::symmetric(8, 4))
                                        .desired_width(edit_w),
                                )
                            })
                            .inner;
                        let enter_hit = resp.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter));
                        let save_btn = egui::Button::new(
                            RichText::new("保存").size(11.0).color(palette.on_accent),
                        )
                        .fill(palette.accent)
                        .stroke(Stroke::NONE)
                        .corner_radius(CornerRadius::same(255));
                        if padded_button(ui, save_btn, vec2(9.0, 3.0)).clicked() || enter_hit {
                            save_note = Some(note_buf.trim().to_string());
                        }
                        if compact_button(ui, "清除").clicked() {
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
            } else if let Some(note) = item.note.as_deref().map(str::trim).filter(|n| !n.is_empty())
            {
                ui.add_space(2.0);
                ui.horizontal(|ui| {
                    ui.label(
                        RichText::new(format!("🏷 备注：{note}"))
                            .size(11.5)
                            .color(palette.accent_text),
                    );
                    if compact_button(ui, "修改").clicked() {
                        state.inline_edit_note = Some((item.id, note.to_string()));
                    }
                    if dismiss_button(ui).on_hover_text("清除备注").clicked() {
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
                        165.0
                    } else {
                        68.0
                    }
                }
                _ => 42.0,
            };
            let body_h = (ui.available_height() - actions_area_h).max(100.0);

            ScrollArea::vertical()
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
                            if state.qr_preview_id == Some(item.id) {
                                let qr_res = state
                                    .preview_cache
                                    .as_ref()
                                    .and_then(|c| c.qr_matrix.clone());
                                if let Some(res) = qr_res {
                                    render_qr_card(ui, &res, palette);
                                    ui.add_space(6.0);
                                }
                            }
                            if is_editing_text {
                                let mut commit_edit: Option<(String, bool)> = None;
                                let mut cancel_edit = false;
                                if let Some((_, ref mut edit_buf)) = state.inline_edit_text {
                                    ui.horizontal(|ui| {
                                        let save_btn = egui::Button::new(
                                            RichText::new("保存修改")
                                                .size(11.0)
                                                .color(palette.on_accent),
                                        )
                                        .fill(palette.accent)
                                        .stroke(Stroke::NONE)
                                        .corner_radius(CornerRadius::same(255));
                                        if padded_button(ui, save_btn, vec2(10.0, 3.5)).clicked() {
                                            commit_edit = Some((edit_buf.clone(), false));
                                        }
                                        if compact_button(ui, "保存并粘贴").clicked() {
                                            commit_edit = Some((edit_buf.clone(), true));
                                        }
                                        if compact_button(ui, "取消").clicked() {
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
                                ui.label(
                                    RichText::new(format!(
                                        "🔒 检测到敏感内容（{}），已默认脱敏保护。\n点击上方「显示明文」按钮可临时查看完整内容。",
                                        item.sensitive_label.as_deref().unwrap_or("凭据")
                                    ))
                                    .size(12.5)
                                    .color(palette.text_secondary),
                                );
                            } else {
                                let display = state
                                    .preview_cache
                                    .as_ref()
                                    .map(|c| c.display_text.as_str())
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
                                let uri = path_to_file_uri(path);
                                let max_w = ui.available_width().max(120.0);
                                let max_h = (body_h - 32.0).max(100.0);
                                let img = egui::Image::from_uri(uri)
                                    .corner_radius(CornerRadius::same(12));
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
                            for (idx, f) in item.files.iter().enumerate() {
                                let exists = state
                                    .preview_cache
                                    .as_ref()
                                    .and_then(|c| c.file_exists.get(idx).copied())
                                    .unwrap_or_else(|| f.exists());
                                egui::Frame::NONE
                                    .fill(palette.card_bg)
                                    .stroke(Stroke::new(1.0, palette.border_subtle))
                                    .corner_radius(CornerRadius::same(12))
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
                                                            palette.success_text
                                                        } else {
                                                            palette.danger_text
                                                        }),
                                                );
                                            });
                                            if exists {
                                                ui.with_layout(
                                                    Layout::right_to_left(Align::Center),
                                                    |ui| {
                                                        if compact_button(ui, "定位").clicked() {
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
                    let quick_list = state
                        .preview_cache
                        .as_ref()
                        .map(|c| c.quick_actions.clone())
                        .unwrap_or_default();
                    ui.horizontal_wrapped(|ui| {
                        if item.is_url && compact_button(ui, "🌐 打开链接").clicked() {
                            actions.push(PopupAction::OpenUrl(raw.trim().to_string()));
                        }
                        if raw.len() <= 2048 {
                            let qr_active = state.qr_preview_id == Some(item.id);
                            let qr_lbl = if qr_active { "收起二维码" } else { "📱 二维码" };
                            if compact_button(ui, qr_lbl).clicked() {
                                state.qr_preview_id = if qr_active { None } else { Some(item.id) };
                            }
                        }
                        for qa in quick_list {
                            if compact_button(ui, qa.label()).clicked() {
                                let res = apply_quick_action(qa, raw);
                                state.active_transform = Some((item.id, qa, res));
                            }
                        }
                    });

                    let mut clear_transform = false;
                    if let Some((tid, qa, ref res)) = state.active_transform
                        && tid == item.id {
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
                                            RichText::new(format!("⚡ {} 结果", qa.label()))
                                                .size(11.5)
                                                .strong()
                                                .color(palette.accent_text),
                                        );
                                        ui.with_layout(
                                            Layout::right_to_left(Align::Center),
                                            |ui| {
                                                if dismiss_button(ui).clicked() {
                                                    clear_transform = true;
                                                }
                                                if let Ok(txt) = res {
                                                    let paste_btn = egui::Button::new(
                                                        RichText::new("粘贴结果")
                                                            .size(11.0)
                                                            .color(palette.on_accent),
                                                    )
                                                    .fill(palette.accent)
                                                    .stroke(Stroke::NONE)
                                                    .corner_radius(CornerRadius::same(255));
                                                    if padded_button(
                                                        ui,
                                                        paste_btn,
                                                        vec2(10.0, 4.5),
                                                    )
                                                    .clicked()
                                                    {
                                                        actions.push(
                                                            PopupAction::PasteTransformedText(
                                                                txt.clone(),
                                                            ),
                                                        );
                                                    }
                                                    if compact_button(ui, "复制结果").clicked() {
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
                                        Ok(out) => {
                                            ScrollArea::vertical()
                                                .id_salt("qa_result_scroll")
                                                .max_height(56.0)
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
                                                RichText::new(err)
                                                    .size(11.5)
                                                    .color(palette.danger),
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
                            "适应窗口"
                        } else {
                            "原始尺寸"
                        };
                        if compact_button(ui, zoom_lbl).clicked() {
                            state.image_actual_size = !state.image_actual_size;
                        }
                        if let Some(path) = &item.image_path
                            && compact_button(ui, "另存为图片...").clicked() {
                                actions.push(PopupAction::SaveImageAs(path.clone()));
                            }
                        if compact_button(ui, "复制并粘贴图片").clicked() {
                            actions.push(PopupAction::PasteItem {
                                id: item.id,
                                behavior: PasteBehavior::CopyAndPaste,
                            });
                        }
                    });
                }
                ClipKind::Files => {
                    ui.horizontal(|ui| {
                        if compact_button(ui, "复制路径文本").clicked() {
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
                            && compact_button(ui, "在文件管理器中定位").clicked() {
                                actions.push(PopupAction::RevealFile(PathBuf::from(&first.path)));
                            }
                    });
                }
            }
        });
}

fn render_qr_card(
    ui: &mut Ui,
    qr_res: &Result<(usize, Vec<bool>), String>,
    palette: &EarthPalette,
) {
    egui::Frame::NONE
        .fill(palette.card_bg)
        .stroke(Stroke::new(1.0, palette.border_subtle))
        .corner_radius(CornerRadius::same(14))
        .inner_margin(egui::Margin::same(10))
        .show(ui, |ui| match qr_res {
            Ok((width, modules)) if *width > 0 => {
                let quiet_zone = 2usize;
                let total_modules = (*width + quiet_zone * 2) as f32;
                let canvas_px = 148.0f32;
                let cell_px = (canvas_px / total_modules).max(2.0);
                let actual_px = cell_px * total_modules;

                ui.horizontal(|ui| {
                    let (qr_rect, _) =
                        ui.allocate_exact_size(vec2(actual_px, actual_px), Sense::hover());
                    ui.painter().rect_filled(
                        qr_rect,
                        CornerRadius::same(8),
                        Color32::WHITE,
                    );
                    ui.painter().rect_stroke(
                        qr_rect,
                        CornerRadius::same(8),
                        Stroke::new(1.0, palette.border),
                        egui::StrokeKind::Inside,
                    );

                    let origin_x = qr_rect.min.x + quiet_zone as f32 * cell_px;
                    let origin_y = qr_rect.min.y + quiet_zone as f32 * cell_px;
                    let dark_color = Color32::from_rgb(5, 9, 20);
                    for y in 0..*width {
                        for x in 0..*width {
                            if modules[y * *width + x] {
                                let min = pos2(
                                    origin_x + x as f32 * cell_px,
                                    origin_y + y as f32 * cell_px,
                                );
                                let max = pos2(min.x + cell_px + 0.35, min.y + cell_px + 0.35);
                                ui.painter().rect_filled(
                                    egui::Rect::from_min_max(min, max),
                                    CornerRadius::ZERO,
                                    dark_color,
                                );
                            }
                        }
                    }

                    ui.vertical(|ui| {
                        ui.label(
                            RichText::new("📱 扫码获取当前内容")
                                .size(12.5)
                                .strong()
                                .color(palette.text_primary),
                        );
                        ui.add_space(4.0);
                        ui.label(
                            RichText::new("使用手机相机或微信扫一扫，即可直接在手机端打开链接或复制文本（本地离线生成）。")
                                .size(11.0)
                                .color(palette.text_muted),
                        );
                    });
                });
            }
            Err(err) => {
                ui.label(RichText::new(err).size(11.5).color(palette.danger));
            }
            _ => {}
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
        .stroke(Stroke::new(1.0, palette.border_subtle))
        .corner_radius(CornerRadius::same(14))
        .inner_margin(egui::Margin::same(8))
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                let (r, _) = ui.allocate_exact_size(vec2(38.0, 38.0), Sense::hover());
                ui.painter().rect_filled(
                    r,
                    CornerRadius::same(10),
                    Color32::from_rgba_unmultiplied(rgba[0], rgba[1], rgba[2], rgba[3]),
                );
                ui.vertical(|ui| {
                    ui.horizontal(|ui| {
                        ui.label(RichText::new(&hex_norm).strong().size(13.0));
                        if compact_button(ui, "复制 HEX").clicked() {
                            actions.push(PopupAction::CopyTransformedText(hex_norm.clone()));
                        }
                    });
                    ui.horizontal(|ui| {
                        ui.label(
                            RichText::new(&rgb_str)
                                .size(11.5)
                                .color(palette.text_secondary),
                        );
                        if compact_button(ui, "复制 RGB").clicked() {
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
            "✓ 忽略下次"
        } else {
            "忽略下次"
        };
        if footer_btn(
            ui,
            RichText::new(ignore_lbl).size(11.0).color(
                if settings.ignore_next_event {
                    palette.warning_text
                } else {
                    palette.text_secondary
                },
            ),
        )
        .on_hover_text("忽略下一次系统剪贴板复制")
        .clicked()
        {
            actions.push(PopupAction::ToggleIgnoreNext);
        }

        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            #[cfg(target_os = "macos")]
            let (quit_s, prefs_s, clear_s) = ("⌘Q", "⌘,", "⌥⌘⌫");
            #[cfg(not(target_os = "macos"))]
            let (quit_s, prefs_s, clear_s) = ("Ctrl+Q", "Ctrl+,", "Ctrl+Alt+⌫");

            if footer_btn(ui, RichText::new("退出").size(11.5))
                .on_hover_text(format!("退出 Earth ({quit_s})"))
                .clicked()
            {
                actions.push(PopupAction::Quit);
            }
            if footer_btn(ui, RichText::new("关于").size(11.5)).clicked() {
                state.show_about_modal = true;
            }
            if footer_btn(ui, RichText::new("设置").size(11.5))
                .on_hover_text(format!("偏好设置 ({prefs_s})"))
                .clicked()
            {
                actions.push(PopupAction::OpenSettings);
            }
            if footer_btn(
                ui,
                RichText::new("清除")
                    .size(11.5)
                    .color(palette.danger),
            )
            .on_hover_text(format!("清除未固定记录 ({clear_s})"))
            .clicked()
            {
                actions.push(PopupAction::ClearUnpinned);
            }
        });
    });
}

fn antd_kind_tag_colors(
    item: &ClipItem,
    palette: &EarthPalette,
) -> (Color32, Color32, Color32) {
    match item.kind {
        ClipKind::Text => {
            if item.detected_color.is_some() {
                if palette.is_dark {
                    (
                        Color32::from_rgb(26, 19, 37),
                        Color32::from_rgb(48, 28, 77),
                        Color32::from_rgb(146, 84, 222),
                    )
                } else {
                    (
                        Color32::from_rgb(249, 240, 255),
                        Color32::from_rgb(211, 173, 247),
                        Color32::from_rgb(83, 29, 171),
                    )
                }
            } else if item.is_url {
                if palette.is_dark {
                    (
                        Color32::from_rgb(19, 22, 41),
                        Color32::from_rgb(28, 39, 85),
                        Color32::from_rgb(89, 126, 247),
                    )
                } else {
                    (
                        Color32::from_rgb(240, 245, 255),
                        Color32::from_rgb(173, 198, 255),
                        Color32::from_rgb(29, 57, 196),
                    )
                }
            } else {
                (palette.info_bg, palette.info_border, palette.accent_text)
            }
        }
        ClipKind::Image => {
            if palette.is_dark {
                (
                    Color32::from_rgb(17, 33, 35),
                    Color32::from_rgb(20, 72, 72),
                    Color32::from_rgb(51, 188, 183),
                )
            } else {
                (
                    Color32::from_rgb(230, 255, 251),
                    Color32::from_rgb(135, 232, 222),
                    Color32::from_rgb(8, 151, 156),
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

fn tint(c: Color32, alpha: u8) -> Color32 {
    Color32::from_rgba_unmultiplied(c.r(), c.g(), c.b(), alpha)
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
    let (bg, border) = if is_warn {
        (palette.warning_bg, palette.warning_border)
    } else {
        (palette.success_bg, palette.success_border)
    };
    egui::Frame::NONE
        .fill(bg)
        .stroke(Stroke::new(1.0, border))
        .corner_radius(CornerRadius::same(14))
        .shadow(card_shadow(palette))
        .inner_margin(egui::Margin::symmetric(10, 6))
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.label(RichText::new(msg).size(11.5).color(palette.text_primary));
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if dismiss_button(ui).clicked() {
                        close_clicked = true;
                    }
                    if show_ax_btn
                        && is_warn
                        && cfg!(target_os = "macos")
                        && compact_button(ui, "打开辅助功能设置").clicked()
                    {
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
                    RichText::new(concat!(
                        "v",
                        env!("CARGO_PKG_VERSION"),
                        " · Pure Rust (macOS & Windows)"
                    ))
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
                let close_btn = egui::Button::new(
                    RichText::new("关闭").color(palette.on_accent),
                )
                .fill(palette.accent)
                .stroke(Stroke::NONE)
                .corner_radius(CornerRadius::same(255));
                if ui.add(close_btn).clicked() {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn footer_fits_within_launcher_width() {
        let ctx = egui::Context::default();
        let palette = EarthPalette::resolve(crate::model::ThemeMode::Light, false);
        palette.apply_to_ctx(&ctx);

        let mut settings = AppSettings::default();
        let collections = vec![
            Collection { id: 1, name: "常用指令".into(), color_hex: "#456EFD".into(), created_at: 0 },
            Collection { id: 2, name: "代码片段".into(), color_hex: "#22C55E".into(), created_at: 0 },
        ];
        let results: Vec<SearchResultItem> = (1..=10)
            .map(|id| SearchResultItem {
                item: ClipItem {
                    id,
                    content_hash: format!("h{id}"),
                    kind: ClipKind::Text,
                    text_content: Some("https://www.behance.net/gallery/218352145".into()),
                    html_content: None,
                    rtf_content: None,
                    image_path: None,
                    thumb_path: None,
                    image_width: None,
                    image_height: None,
                    files: Vec::new(),
                    byte_size: 128,
                    source_app: Some("Safari浏览器".into()),
                    source_bundle_id: None,
                    pinned: false,
                    collection_id: None,
                    first_copied_at: 0,
                    last_copied_at: 0,
                    copy_count: 1,
                    sensitive_label: None,
                    detected_color: None,
                    is_url: true,
                    note: None,
                },
                score: 100,
                matched_indices: HashSet::new(),
            })
            .collect();
        let mut state = PopupState::default();

        if std::path::Path::new("/Library/Fonts/Arial Unicode.ttf").exists()
            && let Ok(bytes) = std::fs::read("/Library/Fonts/Arial Unicode.ttf")
        {
            let mut fonts = egui::FontDefinitions::default();
            fonts.font_data.insert(
                "arial_unicode".into(),
                egui::FontData::from_owned(bytes).into(),
            );
            if let Some(vec) = fonts.families.get_mut(&egui::FontFamily::Proportional) {
                vec.insert(0, "arial_unicode".into());
            }
            ctx.set_fonts(fonts);
        }

        let raw = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(egui::pos2(0.0, 0.0), vec2(424.0, 520.0))),
            ..Default::default()
        };

        let mut min_r = egui::Rect::NOTHING;
        let mut max_shape_right = 0.0f32;
        for _ in 0..2 {
            let mut out = ctx.run_ui(raw.clone(), |ui| {
                let ctx = ui.ctx().clone();
                #[allow(deprecated)]
                egui::CentralPanel::default()
                    .frame(egui::Frame::NONE)
                    .show_inside(ui, |ui| {
                        let panel_rect = ui.max_rect();
                        let outer_rect = panel_rect.shrink(1.0);
                        let content_rect = outer_rect.shrink(12.0);
                        let resp = ui.scope_builder(
                            egui::UiBuilder::new()
                                .max_rect(content_rect)
                                .layout(Layout::top_down(Align::Min)),
                            |ui| {
                                ui.set_min_size(content_rect.size());
                                ui.set_max_size(content_rect.size());
                                let _ = render_popup_view(
                                    ui,
                                    &ctx,
                                    PopupViewData {
                                        settings: &mut settings,
                                        results: &results,
                                        collections: &collections,
                                        palette: &palette,
                                        storage_bytes: 109_500,
                                        total_items: 53,
                                        has_accessibility: true,
                                    },
                                    &mut state,
                                );
                            },
                        );
                        min_r = resp.response.rect;
                    });
            });
            max_shape_right = out
                .shapes
                .iter()
                .map(|cs| cs.shape.visual_bounding_rect().right())
                .filter(|r| r.is_finite())
                .fold(0.0f32, f32::max);
            out.textures_delta.clear();
        }
        assert!(
            min_r.right() <= 411.5 && max_shape_right <= 411.5,
            "popup content exceeded right edge: min_r={min_r:?} max_shape_right={max_shape_right}"
        );
    }

    #[test]
    fn workbench_preview_caches_and_renders_large_item_without_lag() {
        let ctx = egui::Context::default();
        let palette = EarthPalette::resolve(crate::model::ThemeMode::Light, false);
        palette.apply_to_ctx(&ctx);

        let mut settings = AppSettings::default();
        let collections = Vec::new();
        let large_text = "{\"key\":\"value\",\"list\":[1,2,3]}\n".repeat(4000);
        let results = vec![SearchResultItem {
            item: ClipItem {
                id: 1,
                content_hash: "h1".into(),
                kind: ClipKind::Text,
                text_content: Some(large_text),
                html_content: None,
                rtf_content: None,
                image_path: None,
                thumb_path: None,
                image_width: None,
                image_height: None,
                files: Vec::new(),
                byte_size: 120_000,
                source_app: Some("Safari".into()),
                source_bundle_id: None,
                pinned: false,
                collection_id: None,
                first_copied_at: 1_700_000_000_000,
                last_copied_at: 1_700_000_000_000,
                copy_count: 1,
                sensitive_label: None,
                detected_color: None,
                is_url: false,
                note: None,
            },
            score: 100,
            matched_indices: HashSet::new(),
        }];

        let mut state = PopupState {
            selected_id: Some(1),
            preview_open: true,
            ..Default::default()
        };

        let raw = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(egui::pos2(0.0, 0.0), vec2(760.0, 520.0))),
            ..Default::default()
        };

        for _ in 0..2 {
            let mut out = ctx.run_ui(raw.clone(), |ui| {
                let ctx = ui.ctx().clone();
                #[allow(deprecated)]
                egui::CentralPanel::default()
                    .frame(egui::Frame::NONE)
                    .show_inside(ui, |ui| {
                        let content_rect = ui.max_rect().shrink(13.0);
                        ui.scope_builder(
                            egui::UiBuilder::new()
                                .max_rect(content_rect)
                                .layout(Layout::top_down(Align::Min)),
                            |ui| {
                                ui.set_min_size(content_rect.size());
                                ui.set_max_size(content_rect.size());
                                let _ = render_popup_view(
                                    ui,
                                    &ctx,
                                    PopupViewData {
                                        settings: &mut settings,
                                        results: &results,
                                        collections: &collections,
                                        palette: &palette,
                                        storage_bytes: 120_000,
                                        total_items: 1,
                                        has_accessibility: true,
                                    },
                                    &mut state,
                                );
                            },
                        );
                    });
            });
            out.textures_delta.clear();
        }

        let cache = state.preview_cache.as_ref().expect("preview cache populated");
        assert_eq!(cache.item_id, 1);
        assert!(cache.display_text.chars().count() < 4200);
    }

    #[test]
    fn search_bar_hint_vertically_centered_and_mode_right_aligned() {
        let ctx = egui::Context::default();
        let palette = EarthPalette::resolve(crate::model::ThemeMode::Light, false);
        palette.apply_to_ctx(&ctx);

        if std::path::Path::new("/Library/Fonts/Arial Unicode.ttf").exists()
            && let Ok(bytes) = std::fs::read("/Library/Fonts/Arial Unicode.ttf")
        {
            let mut fonts = egui::FontDefinitions::default();
            fonts.font_data.insert(
                "arial_unicode".into(),
                egui::FontData::from_owned(bytes).into(),
            );
            if let Some(vec) = fonts.families.get_mut(&egui::FontFamily::Proportional) {
                vec.insert(0, "arial_unicode".into());
            }
            ctx.set_fonts(fonts);
        }

        let mut settings = AppSettings::default();
        let mut state = PopupState::default();
        let mut actions = Vec::new();

        let raw = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(egui::pos2(0.0, 0.0), vec2(424.0, 520.0))),
            ..Default::default()
        };

        let mut out = ctx.run_ui(raw.clone(), |ui| {
            let ctx = ui.ctx().clone();
            #[allow(deprecated)]
            egui::CentralPanel::default()
                .frame(egui::Frame::NONE)
                .show_inside(ui, |ui| {
                    let content_rect = ui.max_rect().shrink(13.0);
                    ui.scope_builder(
                        egui::UiBuilder::new()
                            .max_rect(content_rect)
                            .layout(Layout::top_down(Align::Min)),
                        |ui| {
                            ui.set_min_size(content_rect.size());
                            ui.set_max_size(content_rect.size());
                            render_header(ui, &ctx, &mut settings, &mut state, &palette, &mut actions);
                        },
                    );
                });
        });
        out.textures_delta.clear();

        let mut search_box_rect = None;
        let mut hint_text_rect = None;
        let mut mode_text_rect = None;
        let mut mode_pill_rect = None;

        for cs in &out.shapes {
            match &cs.shape {
                egui::epaint::Shape::Rect(r) => {
                    if r.rect.width() > 180.0 && r.rect.height() >= 20.0 && r.rect.height() <= 40.0 {
                        search_box_rect = Some(r.rect);
                    } else if r.rect.width() >= 28.0
                        && r.rect.width() <= 60.0
                        && r.rect.height() >= 14.0
                        && r.rect.height() <= 26.0
                        && r.rect.left() > 150.0
                        && r.rect.left() < 280.0
                    {
                        mode_pill_rect = Some(r.rect);
                    }
                }
                egui::epaint::Shape::Text(t) => {
                    let txt = t.galley.text();
                    let rect = egui::Rect::from_min_size(t.pos, t.galley.size());
                    if txt.contains("搜索记录") {
                        hint_text_rect = Some(rect);
                    } else if txt.contains("模糊") {
                        mode_text_rect = Some(rect);
                    }
                }
                _ => {}
            }
        }

        let sb = search_box_rect.expect("search_box_rect found");
        let ht = hint_text_rect.expect("hint_text_rect found");
        let mt = mode_text_rect.expect("mode_text_rect found");
        let mp = mode_pill_rect.expect("mode_pill_rect found");

        println!(
            "search_box={sb:?} center_y={:.2} | hint={ht:?} center_y={:.2} | mode_pill={mp:?} center_y={:.2} right_gap={:.2} | mode_text={mt:?} center_y={:.2}",
            sb.center().y,
            ht.center().y,
            mp.center().y,
            sb.right() - mp.right(),
            mt.center().y
        );

        assert!(
            (sb.center().y - ht.center().y).abs() <= 1.5,
            "hint text not vertically centered: box_cy={} hint_cy={}",
            sb.center().y,
            ht.center().y
        );
        assert!(
            sb.right() - mp.right() <= 14.0,
            "mode pill not right aligned: sb.right()={} mp.right()={}",
            sb.right(),
            mp.right()
        );
    }

    #[test]
    fn selected_row_bar_vertically_centered_and_hover_uses_pointing_hand() {
        let ctx = egui::Context::default();
        let palette = EarthPalette::resolve(crate::model::ThemeMode::Light, false);
        palette.apply_to_ctx(&ctx);

        let settings = AppSettings::default();
        let collections = Vec::new();
        let res = SearchResultItem {
            item: ClipItem {
                id: 1,
                content_hash: "h1".into(),
                kind: ClipKind::Text,
                text_content: Some("物化生缺，语文，数学，政治不缺".into()),
                html_content: None,
                rtf_content: None,
                image_path: None,
                thumb_path: None,
                image_width: None,
                image_height: None,
                files: Vec::new(),
                byte_size: 48,
                source_app: Some("微信".into()),
                source_bundle_id: None,
                pinned: false,
                collection_id: None,
                first_copied_at: 0,
                last_copied_at: 0,
                copy_count: 1,
                sensitive_label: None,
                detected_color: None,
                is_url: false,
                note: None,
            },
            score: 100,
            matched_indices: HashSet::new(),
        };
        let mut state = PopupState {
            selected_id: Some(1),
            ..Default::default()
        };
        let mut actions = Vec::new();

        // Hover directly over the text label area of the row
        let raw = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(egui::pos2(0.0, 0.0), vec2(424.0, 520.0))),
            events: vec![egui::Event::PointerMoved(egui::pos2(120.0, 26.0))],
            ..Default::default()
        };

        let mut row_rect = egui::Rect::NOTHING;
        let mut out = ctx.run_ui(raw, |ui| {
            #[allow(deprecated)]
            egui::CentralPanel::default()
                .frame(egui::Frame::NONE)
                .show_inside(ui, |ui| {
                    let content_rect = egui::Rect::from_min_size(egui::pos2(13.0, 10.0), vec2(398.0, 40.0));
                    ui.scope_builder(
                        egui::UiBuilder::new()
                            .max_rect(content_rect)
                            .layout(Layout::top_down(Align::Min)),
                        |ui| {
                            ui.set_min_size(content_rect.size());
                            ui.set_max_size(content_rect.size());
                            let resp = render_item_row(
                                ui,
                                &res,
                                Some(1),
                                true,
                                &settings,
                                &collections,
                                &mut state,
                                &palette,
                                &mut actions,
                            );
                            row_rect = resp.rect;
                        },
                    );
                });
        });
        out.textures_delta.clear();

        let mut bar_rect = None;
        for cs in &out.shapes {
            if let egui::epaint::Shape::Rect(r) = &cs.shape
                && (r.rect.width() - 3.5).abs() < 0.2
                && r.rect.height() >= 15.0
            {
                bar_rect = Some(r.rect);
            }
        }

        let br = bar_rect.expect("selection bar found");
        assert!(
            (row_rect.center().y - br.center().y).abs() < 0.5,
            "selection bar not vertically centered: row_cy={} bar_cy={}",
            row_rect.center().y,
            br.center().y
        );
        assert_eq!(
            out.platform_output.cursor_icon,
            egui::CursorIcon::PointingHand,
            "expected PointingHand cursor on row hover, got {:?}",
            out.platform_output.cursor_icon
        );
    }

    #[test]
    fn single_click_vs_double_click_copy_mode_avoids_workbench_conflict() {
        let ctx = egui::Context::default();
        let palette = EarthPalette::resolve(crate::model::ThemeMode::Light, false);
        palette.apply_to_ctx(&ctx);

        let mut settings = AppSettings {
            copy_trigger_mode: CopyTriggerMode::SingleClick,
            ..Default::default()
        };
        let collections = Vec::new();
        let res = SearchResultItem {
            item: ClipItem {
                id: 2,
                content_hash: "h2".into(),
                kind: ClipKind::Text,
                text_content: Some("single click test".into()),
                html_content: None,
                rtf_content: None,
                image_path: None,
                thumb_path: None,
                image_width: None,
                image_height: None,
                files: Vec::new(),
                byte_size: 17,
                source_app: None,
                source_bundle_id: None,
                pinned: false,
                collection_id: None,
                first_copied_at: 0,
                last_copied_at: 0,
                copy_count: 1,
                sensitive_label: None,
                detected_color: None,
                is_url: false,
                note: None,
            },
            score: 100,
            matched_indices: HashSet::new(),
        };

        let mut sim_time = 10.0f64;
        let mut run_click = |ctx: &egui::Context,
                             settings: &AppSettings,
                             state: &mut PopupState,
                             actions: &mut Vec<PopupAction>| {
            sim_time += 1.0;
            let pos = egui::pos2(100.0, 24.0);
            for (dt, events) in [
                (0.0, vec![egui::Event::PointerMoved(pos)]),
                (
                    0.02,
                    vec![egui::Event::PointerButton {
                        pos,
                        button: egui::PointerButton::Primary,
                        pressed: true,
                        modifiers: Modifiers::NONE,
                    }],
                ),
                (
                    0.04,
                    vec![egui::Event::PointerButton {
                        pos,
                        button: egui::PointerButton::Primary,
                        pressed: false,
                        modifiers: Modifiers::NONE,
                    }],
                ),
            ] {
                let raw = egui::RawInput {
                    time: Some(sim_time + dt),
                    screen_rect: Some(egui::Rect::from_min_size(egui::pos2(0.0, 0.0), vec2(424.0, 520.0))),
                    events,
                    ..Default::default()
                };
                let mut out = ctx.run_ui(raw, |ui| {
                    #[allow(deprecated)]
                    egui::CentralPanel::default()
                        .frame(egui::Frame::NONE)
                        .show_inside(ui, |ui| {
                            let content_rect =
                                egui::Rect::from_min_size(egui::pos2(13.0, 10.0), vec2(398.0, 40.0));
                            ui.scope_builder(
                                egui::UiBuilder::new()
                                    .max_rect(content_rect)
                                    .layout(Layout::top_down(Align::Min)),
                                |ui| {
                                    ui.set_min_size(content_rect.size());
                                    ui.set_max_size(content_rect.size());
                                    let selected = state.selected_id == Some(res.item.id);
                                    let _ = render_item_row(
                                        ui,
                                        &res,
                                        Some(1),
                                        selected,
                                        settings,
                                        &collections,
                                        state,
                                        &palette,
                                        actions,
                                    );
                                },
                            );
                        });
                });
                out.textures_delta.clear();
            }
        };

        // 1. SingleClick mode with workbench closed: click copies immediately without opening workbench
        let mut state = PopupState {
            selected_id: Some(1),
            preview_open: false,
            ..Default::default()
        };
        let mut actions = Vec::new();
        run_click(&ctx, &settings, &mut state, &mut actions);
        assert!(!state.preview_open, "SingleClick must not auto-open workbench");
        assert!(
            actions
                .iter()
                .any(|a| matches!(a, PopupAction::PasteItem { id: 2, .. })),
            "SingleClick with closed workbench should emit PasteItem"
        );

        // 2. SingleClick mode with workbench already open:
        //    First click on unselected item switches preview; second click copies.
        let mut state = PopupState {
            selected_id: Some(1),
            preview_open: true,
            ..Default::default()
        };
        let mut actions = Vec::new();
        run_click(&ctx, &settings, &mut state, &mut actions);
        assert_eq!(state.selected_id, Some(2));
        assert!(
            actions.is_empty(),
            "clicking unselected row while workbench open should switch preview first"
        );
        run_click(&ctx, &settings, &mut state, &mut actions);
        assert!(
            actions
                .iter()
                .any(|a| matches!(a, PopupAction::PasteItem { id: 2, .. })),
            "clicking already-selected row while workbench open should emit PasteItem"
        );

        // 3. DoubleClick mode: single click opens workbench without copying
        settings.copy_trigger_mode = CopyTriggerMode::DoubleClick;
        let mut state = PopupState {
            selected_id: Some(1),
            preview_open: false,
            ..Default::default()
        };
        let mut actions = Vec::new();
        run_click(&ctx, &settings, &mut state, &mut actions);
        assert!(state.preview_open, "DoubleClick mode single click should open workbench");
        assert!(actions.is_empty(), "DoubleClick mode single click should not copy");
    }
}
