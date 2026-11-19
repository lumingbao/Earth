use crate::hotkey::{consume_egui_shortcut, format_shortcut_display};
use crate::i18n::{t, tf};
use crate::model::{
    AppSettings, CategoryFilter, ClipItem, ClipKind, Collection, CopyTriggerMode, PasteBehavior,
    SearchBoxVisibility, SearchMode, format_bytes, truncate_chars,
};
use crate::search::SearchResultItem;
use crate::transforms::{
    QuickActionKind, apply_quick_action, available_quick_actions, format_color_hex,
    format_color_hsl, format_color_rgb, mask_sensitive_preview, text_stats,
};
use crate::ui::icons::{
    draw_earth_emblem, draw_file_badge, draw_gear_glyph, draw_link_badge, draw_pause_glyph,
    draw_pin_badge, draw_play_glyph, draw_search_glyph, draw_shield_glyph, draw_split_pane_glyph,
    draw_window_pin_glyph, icon_button,
};
use crate::ui::media::{ImagePreview, QrExport, ThumbnailCache, qr_pixels};
use crate::ui::theme::{
    EarthPalette, StatusToast, build_highlighted_layout_job, card_shadow, compact_button,
    desktop_scroll_source, dismiss_button, padded_button, parse_hex_color32,
};
use crate::updater::{CURRENT_VERSION, ReleaseInfo, UpdateDownloadState};
use chrono::{DateTime, Local};
use egui::{
    Align, Color32, CornerRadius, FontId, Key, Layout, Modifiers, RichText, ScrollArea, Sense,
    Stroke, Ui, pos2, vec2,
};
use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct VerticalNavHold {
    /// `+1` for ArrowDown, `-1` for ArrowUp.
    pub dir: i32,
    /// `egui` input clock (seconds) when the key was initially pressed.
    pub pressed_at: f64,
    /// `egui` input clock (seconds) when the next accelerated repeat step should fire.
    pub next_step_at: f64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum WorkbenchSide {
    #[default]
    Right,
    Left,
}

pub enum PopupAction {
    HidePopup,
    PasteItem {
        id: i64,
        behavior: PasteBehavior,
    },
    CopyTransformedText(String),
    PasteTransformedText(String),
    TogglePin(i64),
    SetCollection {
        item_id: i64,
        collection_id: Option<i64>,
    },
    SetItemNote {
        item_id: i64,
        note: Option<String>,
    },
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
    ToggleWindowPin,
    SettingsChanged,
    SaveImageAs(String),
    CopyQrImage(Vec<u8>),
    SaveQrImageAs(Vec<u8>),
    RevealFile(PathBuf),
    OpenUrl(String),
    StartUpdateDownload(ReleaseInfo),
    ApplyUpdateAndRestart(PathBuf),
    OpenAccessibilitySettings,
    Quit,
}

const MAX_PREVIEW_FILES: usize = 64;

pub struct CachedPreviewItem {
    pub source: Arc<ClipItem>,
    pub formatted_ts: String,
    pub display_text: String,
    pub anim_display_text: String,
    pub text_stats: Option<(usize, usize, usize)>,
    pub quick_actions: Vec<QuickActionKind>,
    pub file_exists: Vec<bool>,
    pub qr_matrix: Option<Result<(usize, Vec<bool>), String>>,
    pub qr_texture: Option<egui::TextureHandle>,
}

pub struct PopupState {
    pub search_query: String,
    pub filter: CategoryFilter,
    pub selected_id: Option<i64>,
    pub preview_open: bool,
    pub window_pinned: bool,
    pub workbench_side: WorkbenchSide,
    pub preview_reveal: Option<f32>,
    pub request_search_focus: bool,
    pub selection_changed_at: Instant,
    pub revealed_sensitive_ids: HashSet<i64>,
    pub image_actual_size: bool,
    pub active_transform: Option<(i64, QuickActionKind, Result<String, String>)>,
    pub inline_edit_text: Option<(i64, String)>,
    pub inline_edit_note: Option<(i64, String)>,
    pub qr_preview_id: Option<i64>,
    pub preview_cache: Option<CachedPreviewItem>,
    pub image_preview: ImagePreview,
    pub thumbnail_cache: ThumbnailCache,
    pub qr_export: QrExport,
    pub show_about_modal: bool,
    pub status_banner: Option<StatusToast>,
    pub service_error: Option<String>,
    pub available_update: Option<ReleaseInfo>,
    pub update_download: UpdateDownloadState,
    pub dismissed_update_version: Option<String>,
    pub dismissed_accessibility_banner: bool,
    pub ime_composing: bool,
    pub search_focused: bool,
    pub scroll_to_selection: bool,
    pub scroll_to_filter: bool,
    pub nav_hold: Option<VerticalNavHold>,
    pub list_scroll_y: f32,
    pub scroll_anim_target: Option<f32>,
    pub selection_visual_y: Option<f32>,
    pub external_selection_bar: bool,
    pub selection_glide_blend: f32,
}

impl Default for PopupState {
    fn default() -> Self {
        Self {
            search_query: String::new(),
            filter: CategoryFilter::All,
            selected_id: None,
            preview_open: false,
            window_pinned: false,
            workbench_side: WorkbenchSide::Right,
            preview_reveal: None,
            request_search_focus: true,
            selection_changed_at: Instant::now(),
            revealed_sensitive_ids: HashSet::new(),
            image_actual_size: false,
            active_transform: None,
            inline_edit_text: None,
            inline_edit_note: None,
            qr_preview_id: None,
            preview_cache: None,
            image_preview: ImagePreview::default(),
            thumbnail_cache: ThumbnailCache::default(),
            qr_export: QrExport::default(),
            show_about_modal: false,
            status_banner: None,
            service_error: None,
            available_update: None,
            update_download: UpdateDownloadState::Idle,
            dismissed_update_version: None,
            dismissed_accessibility_banner: false,
            ime_composing: false,
            search_focused: false,
            scroll_to_selection: false,
            scroll_to_filter: false,
            nav_hold: None,
            list_scroll_y: 0.0,
            scroll_anim_target: None,
            selection_visual_y: None,
            external_selection_bar: false,
            selection_glide_blend: 1.0,
        }
    }
}

impl PopupState {
    pub fn clear_sensitive_context(&mut self) {
        self.revealed_sensitive_ids.clear();
        self.inline_edit_text = None;
        self.active_transform = None;
        self.qr_preview_id = None;
        self.preview_cache = None;
        self.image_preview.clear();
    }

    pub fn release_images(&mut self) {
        self.preview_cache = None;
        self.image_preview.clear();
        self.thumbnail_cache.clear();
    }

    pub fn poll_qr_export(&mut self) -> Option<PopupAction> {
        match self.qr_export.poll()? {
            Ok((true, png)) => Some(PopupAction::SaveQrImageAs(png)),
            Ok((false, png)) => Some(PopupAction::CopyQrImage(png)),
            Err(error) => {
                self.set_status_banner(error, true);
                None
            }
        }
    }

    fn select_item(&mut self, selected_id: Option<i64>) {
        if self.selected_id != selected_id {
            self.clear_sensitive_context();
            self.selected_id = selected_id;
        }
    }

    /// Displays a transient status toast banner that auto-dismisses after 3 seconds.
    pub fn set_status_banner(&mut self, message: impl Into<String>, is_warn: bool) {
        self.status_banner = Some(StatusToast::new(message, is_warn));
    }

    /// Clears transient vertical key-hold and list scroll/indicator glide animation state.
    pub fn reset_list_navigation(&mut self) {
        self.nav_hold = None;
        self.scroll_anim_target = None;
        self.selection_visual_y = None;
        self.selection_glide_blend = 1.0;
    }

    /// Updates workbench open state while seeding `preview_reveal` so the toggle frame never snaps before animating.
    pub fn set_preview_open(&mut self, open: bool) {
        if self.preview_open != open {
            if self.preview_reveal.is_none() {
                self.preview_reveal = Some(if self.preview_open { 1.0 } else { 0.0 });
            }
            self.preview_open = open;
            if !open {
                self.preview_cache = None;
                self.image_preview.clear();
            }
        }
    }

    pub fn toggle_preview(&mut self) {
        self.set_preview_open(!self.preview_open);
    }

    pub fn ensure_preview_cache(&mut self, item: &Arc<ClipItem>) -> &CachedPreviewItem {
        const MAX_PREVIEW_TEXT_CHARS: usize = 4000;
        const MAX_ANIM_PREVIEW_CHARS: usize = 900;
        let needs_rebuild = self
            .preview_cache
            .as_ref()
            .map(|c| !Arc::ptr_eq(&c.source, item))
            .unwrap_or(true);
        if needs_rebuild {
            let formatted_ts = format_epoch_millis(item.last_copied_at);
            let (display_text, anim_display_text, stats, quick_actions) =
                if item.kind == ClipKind::Text {
                    let raw = item.text_content.as_deref().unwrap_or("");
                    let display = match raw.char_indices().nth(MAX_PREVIEW_TEXT_CHARS) {
                        Some((byte_idx, _)) => format!(
                            "{}{}",
                            &raw[..byte_idx],
                            tf(
                                "workbench.preview_truncated",
                                &[("max", &MAX_PREVIEW_TEXT_CHARS.to_string())]
                            )
                        ),
                        None => raw.to_string(),
                    };
                    let anim_display = match display.char_indices().nth(MAX_ANIM_PREVIEW_CHARS) {
                        Some((byte_idx, _)) => display[..byte_idx].to_string(),
                        None => display.clone(),
                    };
                    (
                        display,
                        anim_display,
                        Some(text_stats(raw)),
                        available_quick_actions(raw),
                    )
                } else {
                    (String::new(), String::new(), None, Vec::new())
                };
            let file_exists = if item.kind == ClipKind::Files {
                item.files
                    .iter()
                    .take(MAX_PREVIEW_FILES)
                    .map(|f| f.exists())
                    .collect()
            } else {
                Vec::new()
            };
            self.preview_cache = Some(CachedPreviewItem {
                source: Arc::clone(item),
                formatted_ts,
                display_text,
                anim_display_text,
                text_stats: stats,
                quick_actions,
                file_exists,
                qr_matrix: None,
                qr_texture: None,
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
        return Err(t("qr.err_empty").to_string());
    }
    if text.len() > 2048 {
        return Err(t("qr.err_too_long").to_string());
    }
    let code = qrcode::QrCode::new(text.as_bytes())
        .map_err(|e| tf("qr.err_generate", &[("error", &e.to_string())]))?;
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

mod cards;
mod footer;
mod header;
mod input;
mod launcher;
mod list;
mod row;
#[cfg(test)]
mod tests;
mod workbench;

use cards::*;
use footer::*;
use header::*;
use input::*;
pub use launcher::render_popup_view;
use list::*;
use row::*;
use workbench::*;
