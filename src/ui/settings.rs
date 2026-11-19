use crate::hotkey::{
    ShortcutCapture, capture_egui_shortcut, format_live_modifiers, format_shortcut_display,
    validate_shortcut,
};
use crate::i18n::{AppLanguage, t, tf};
use crate::model::{
    AppSettings, Collection, CopyTriggerMode, MatchHighlight, PinPosition, PopupPosition,
    SearchBoxVisibility, SearchMode, SortOrder, ThemeMode, format_bytes,
};
use crate::platform::{self, NativeShortcutEvent};
use crate::ui::theme::{
    EarthPalette, StatusToast, card_shadow, compact_button, desktop_scroll_source, dismiss_button,
    padded_button, parse_hex_color32,
};
use crate::updater::{ReleaseInfo, UpdateDownloadState};
use egui::{Align, CornerRadius, Layout, RichText, ScrollArea, Stroke, Ui, vec2};
use std::path::PathBuf;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SettingsTab {
    General,
    Storage,
    Appearance,
    Ignore,
    Advanced,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IgnoreSubTab {
    Apps,
    ClipboardTypes,
    Regexes,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RecordingTarget {
    Open,
    Pin,
    Delete,
    Preview,
}

pub enum SettingsAction {
    CloseSettings,
    SettingsChanged,
    OpenHotkeyChanged(String),
    ToggleAutostart(bool),
    CheckUpdatesNow,
    StartUpdateDownload(ReleaseInfo),
    ApplyUpdateAndRestart(PathBuf),
    OpenUrl(String),
    ClearHistory(bool),
    ExportJson,
    ImportJson,
    CreateCollection {
        name: String,
        color_hex: String,
    },
    RenameCollection {
        id: i64,
        name: String,
    },
    DeleteCollection(i64),
    #[cfg_attr(not(target_os = "macos"), allow(dead_code))]
    OpenAccessibilitySettings,
}

pub struct SettingsState {
    pub active_tab: SettingsTab,
    pub ignore_sub_tab: IgnoreSubTab,
    pub recording: Option<RecordingTarget>,
    pub recording_suppress_reclick: bool,
    pub new_ignore_app: String,
    pub new_ignore_type: String,
    pub new_ignore_regex: String,
    pub new_collection_name: String,
    pub new_collection_color: String,
    pub editing_collection_id: Option<i64>,
    pub editing_collection_name: String,
    pub status_message: Option<StatusToast>,
    pub hotkey_error: Option<String>,
    pub service_error: Option<String>,
    pub checking_updates: bool,
    pub available_update: Option<ReleaseInfo>,
    pub update_download: UpdateDownloadState,
}

impl Default for SettingsState {
    fn default() -> Self {
        Self {
            active_tab: SettingsTab::General,
            ignore_sub_tab: IgnoreSubTab::Apps,
            recording: None,
            recording_suppress_reclick: false,
            new_ignore_app: String::new(),
            new_ignore_type: String::new(),
            new_ignore_regex: String::new(),
            new_collection_name: String::new(),
            new_collection_color: "#456EFD".to_string(),
            editing_collection_id: None,
            editing_collection_name: String::new(),
            status_message: None,
            hotkey_error: None,
            service_error: None,
            checking_updates: false,
            available_update: None,
            update_download: UpdateDownloadState::Idle,
        }
    }
}

impl SettingsState {
    /// Displays a transient settings toast banner that auto-dismisses after 3 seconds.
    pub fn set_status_message(&mut self, message: impl Into<String>, is_err: bool) {
        self.status_message = Some(StatusToast::new(message, is_err));
    }
}

pub struct SettingsViewData<'a> {
    pub settings: &'a mut AppSettings,
    pub collections: &'a [Collection],
    pub palette: &'a EarthPalette,
    pub storage_bytes: u64,
    pub item_count: usize,
    pub has_accessibility: bool,
}

pub fn render_settings_view(
    ui: &mut Ui,
    ctx: &egui::Context,
    data: SettingsViewData<'_>,
    state: &mut SettingsState,
) -> Vec<SettingsAction> {
    let SettingsViewData {
        settings,
        collections,
        palette,
        storage_bytes,
        item_count,
        has_accessibility,
    } = data;
    let mut actions = Vec::new();

    // Handle active shortcut recording
    state.recording_suppress_reclick = state.recording.is_some();
    if let Some(target) = state.recording {
        enum CapturePoll {
            Combo(String),
            Cancel,
            NeedsModifier,
            None,
        }

        let captured = match platform::poll_native_shortcut_event() {
            Some(NativeShortcutEvent::Captured(combo)) => CapturePoll::Combo(combo),
            Some(NativeShortcutEvent::Cancelled) => CapturePoll::Cancel,
            Some(NativeShortcutEvent::NeedsModifier) => CapturePoll::NeedsModifier,
            None => ctx.input(|input| {
                if input.key_pressed(egui::Key::Escape) {
                    CapturePoll::Cancel
                } else {
                    match capture_egui_shortcut(input) {
                        Some(ShortcutCapture::Combo(combo)) => CapturePoll::Combo(combo),
                        Some(ShortcutCapture::NeedsModifier) => CapturePoll::NeedsModifier,
                        None => CapturePoll::None,
                    }
                }
            }),
        };

        match captured {
            CapturePoll::Cancel => {
                state.recording = None;
            }
            CapturePoll::NeedsModifier => {
                state.set_status_message(t("settings.general.shortcut_needs_mod"), true);
            }
            CapturePoll::Combo(combo) => {
                if let Err(error) = validate_shortcut(&combo, target == RecordingTarget::Open) {
                    state.hotkey_error = Some(error);
                    return actions;
                }
                state.hotkey_error = None;
                match target {
                    RecordingTarget::Open => {
                        actions.push(SettingsAction::OpenHotkeyChanged(combo));
                    }
                    RecordingTarget::Pin => {
                        settings.hotkey_pin = combo;
                        actions.push(SettingsAction::SettingsChanged);
                    }
                    RecordingTarget::Delete => {
                        settings.hotkey_delete = combo;
                        actions.push(SettingsAction::SettingsChanged);
                    }
                    RecordingTarget::Preview => {
                        settings.hotkey_preview = combo;
                        actions.push(SettingsAction::SettingsChanged);
                    }
                }
                state.recording = None;
            }
            CapturePoll::None => {}
        }
    } else if ctx.input(|input| input.key_pressed(egui::Key::Escape)) {
        if state.editing_collection_id.is_some() {
            state.editing_collection_id = None;
            state.editing_collection_name.clear();
        } else {
            actions.push(SettingsAction::CloseSettings);
        }
    }

    // Header bar (draggable)
    ui.horizontal(|ui| {
        let back_btn = egui::Button::new(RichText::new(t("settings.back")).size(12.5))
            .fill(palette.surface_bg)
            .stroke(Stroke::new(1.0, palette.border))
            .corner_radius(CornerRadius::same(255));
        if ui.add(back_btn).clicked() {
            actions.push(SettingsAction::CloseSettings);
        }

        let drag_resp = ui.allocate_response(
            vec2((ui.available_width() - 90.0).max(40.0), 28.0),
            egui::Sense::drag(),
        );
        if drag_resp.drag_started() {
            ctx.send_viewport_cmd(egui::ViewportCommand::StartDrag);
            ctx.stop_dragging();
        }
        ui.painter().text(
            drag_resp.rect.center(),
            egui::Align2::CENTER_CENTER,
            t("settings.title"),
            egui::FontId::proportional(14.5),
            palette.text_primary,
        );

        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            ui.label(
                RichText::new(concat!("v", env!("CARGO_PKG_VERSION")))
                    .size(11.5)
                    .color(palette.text_muted),
            );
        });
    });

    ui.add_space(6.0);

    for error in [&state.service_error, &state.hotkey_error]
        .into_iter()
        .flatten()
    {
        ui.label(RichText::new(error).size(11.5).color(palette.danger));
    }

    // GOSTOCK Segmented Pill Tabs (#F1F4F9 track with #456EFD active pill)
    egui::Frame::NONE
        .fill(palette.card_bg)
        .corner_radius(CornerRadius::same(16))
        .inner_margin(egui::Margin::same(3))
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 4.0;
                let tabs = [
                    (SettingsTab::General, t("settings.tab.general")),
                    (SettingsTab::Storage, t("settings.tab.storage")),
                    (SettingsTab::Appearance, t("settings.tab.appearance")),
                    (SettingsTab::Ignore, t("settings.tab.ignore")),
                    (SettingsTab::Advanced, t("settings.tab.advanced")),
                ];
                let font_id = egui::FontId::proportional(12.0);
                let measured: Vec<f32> = tabs
                    .iter()
                    .map(|(_, label)| {
                        ui.painter()
                            .layout_no_wrap(
                                (*label).to_string(),
                                font_id.clone(),
                                palette.text_secondary,
                            )
                            .size()
                            .x
                    })
                    .collect();
                let total_text_w: f32 = measured.iter().sum();
                let spacing_total = 4.0 * (tabs.len().saturating_sub(1)) as f32;
                let total_avail = (ui.available_width() - spacing_total).max(0.0);
                let extra_per_tab = ((total_avail - total_text_w) / tabs.len() as f32).max(6.0);
                for ((tab, label), text_w) in tabs.into_iter().zip(measured) {
                    let active = state.active_tab == tab;
                    let tab_width = text_w + extra_per_tab;
                    let (rect, resp) =
                        ui.allocate_exact_size(vec2(tab_width, 28.0), egui::Sense::click());
                    if active {
                        ui.painter()
                            .rect_filled(rect, CornerRadius::same(13), palette.accent);
                    } else if resp.hovered() {
                        ui.painter()
                            .rect_filled(rect, CornerRadius::same(13), palette.accent_soft);
                    }
                    let text_color = if active {
                        palette.on_accent
                    } else if resp.hovered() {
                        palette.accent_text
                    } else {
                        palette.text_secondary
                    };
                    ui.painter().text(
                        rect.center(),
                        egui::Align2::CENTER_CENTER,
                        label,
                        font_id.clone(),
                        text_color,
                    );
                    if resp.clicked() {
                        state.active_tab = tab;
                        state.recording = None;
                    }
                }
            });
        });

    ui.add_space(6.0);
    ui.separator();
    ui.add_space(6.0);

    let now_secs = ctx.input(|i| i.time);
    let active_status = state.status_message.as_mut().and_then(|toast| {
        toast
            .tick(now_secs)
            .map(|rem| (toast.message.clone(), toast.is_alert, rem))
    });
    if state.status_message.is_some() && active_status.is_none() {
        state.status_message = None;
        ctx.request_repaint();
    }
    if let Some((msg, is_err, remaining)) = active_status {
        StatusToast::schedule_repaint(ctx, remaining);
        let opacity = StatusToast::opacity_for_remaining(remaining);
        let mut clear_status = false;
        let (bg, border) = if is_err {
            (palette.danger_bg, palette.danger_border)
        } else {
            (palette.success_bg, palette.success_border)
        };
        ui.scope(|ui| {
            if opacity < 0.999 {
                ui.multiply_opacity(opacity);
            }
            egui::Frame::NONE
                .fill(bg)
                .stroke(Stroke::new(1.0, border))
                .corner_radius(CornerRadius::same(14))
                .shadow(card_shadow(palette))
                .inner_margin(egui::Margin::symmetric(10, 6))
                .show(ui, |ui| {
                    ui.horizontal(|ui| {
                        ui.label(RichText::new(msg).size(12.0).color(palette.text_primary));
                        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                            if dismiss_button(ui).clicked() {
                                clear_status = true;
                            }
                        });
                    });
                });
        });
        ui.add_space(6.0);
        if clear_status {
            state.status_message = None;
        }
    }

    ScrollArea::vertical()
        .scroll_source(desktop_scroll_source())
        .auto_shrink([false, false])
        .show(ui, |ui| match state.active_tab {
            SettingsTab::General => {
                render_general_tab(
                    ui,
                    settings,
                    state,
                    palette,
                    has_accessibility,
                    &mut actions,
                );
            }
            SettingsTab::Storage => {
                render_storage_tab(
                    ui,
                    settings,
                    palette,
                    storage_bytes,
                    item_count,
                    &mut actions,
                );
            }
            SettingsTab::Appearance => {
                render_appearance_tab(ui, settings, palette, &mut actions);
            }
            SettingsTab::Ignore => {
                render_ignore_tab(ui, settings, state, palette, &mut actions);
            }
            SettingsTab::Advanced => {
                render_advanced_tab(ui, settings, collections, state, palette, &mut actions);
            }
        });

    actions
}

fn render_general_tab(
    ui: &mut Ui,
    settings: &mut AppSettings,
    state: &mut SettingsState,
    palette: &EarthPalette,
    has_accessibility: bool,
    actions: &mut Vec<SettingsAction>,
) {
    section_card(ui, palette, |ui| {
        ui.horizontal(|ui| {
            ui.label(t("settings.general.language"));
            egui::ComboBox::from_id_salt("app_language_combo")
                .selected_text(settings.language.label())
                .show_ui(ui, |ui| {
                    for lang in [AppLanguage::System, AppLanguage::ZhCn, AppLanguage::EnUs] {
                        if ui
                            .selectable_value(&mut settings.language, lang, lang.label())
                            .changed()
                        {
                            crate::i18n::set_language(settings.language);
                            actions.push(SettingsAction::SettingsChanged);
                        }
                    }
                });
        });
        ui.add_space(4.0);
        if ui
            .checkbox(
                &mut settings.launch_at_login,
                t("settings.general.launch_at_login"),
            )
            .changed()
        {
            actions.push(SettingsAction::ToggleAutostart(settings.launch_at_login));
        }
        ui.horizontal(|ui| {
            if ui
                .checkbox(
                    &mut settings.check_updates,
                    t("settings.general.check_updates"),
                )
                .changed()
            {
                actions.push(SettingsAction::SettingsChanged);
            }
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if state.checking_updates {
                    ui.label(
                        RichText::new(t("settings.general.checking_updates"))
                            .size(11.5)
                            .color(palette.text_muted),
                    );
                } else if compact_button(ui, t("settings.general.check_updates_now")).clicked() {
                    state.checking_updates = true;
                    actions.push(SettingsAction::CheckUpdatesNow);
                }
            });
        });

        if let Some(rel) = state.available_update.clone() {
            ui.add_space(6.0);
            render_update_status_row(ui, &rel, &state.update_download, palette, actions);
        }

        #[cfg(target_os = "macos")]
        {
            ui.add_space(6.0);
            if has_accessibility {
                ui.add(
                    egui::Label::new(
                        RichText::new(t("settings.general.accessibility_ok"))
                            .size(12.0)
                            .color(palette.success_text),
                    )
                    .wrap(),
                );
            } else {
                egui::Frame::NONE
                    .fill(palette.warning_bg)
                    .stroke(Stroke::new(1.0, palette.warning_border))
                    .corner_radius(CornerRadius::same(12))
                    .inner_margin(egui::Margin::symmetric(10, 8))
                    .show(ui, |ui| {
                        ui.set_min_width(ui.available_width());
                        ui.set_max_width(ui.available_width());
                        ui.add(
                            egui::Label::new(
                                RichText::new(t("settings.general.accessibility_missing"))
                                    .size(12.0)
                                    .color(palette.warning_text),
                            )
                            .wrap(),
                        );
                        ui.add_space(4.0);
                        ui.add(
                            egui::Label::new(
                                RichText::new(t("settings.general.accessibility_hint"))
                                    .size(11.0)
                                    .color(palette.text_muted),
                            )
                            .wrap(),
                        );
                        ui.add_space(6.0);
                        let auth_btn = egui::Button::new(
                            RichText::new(t("settings.general.accessibility_grant"))
                                .size(11.5)
                                .color(palette.on_accent),
                        )
                        .fill(palette.accent)
                        .stroke(Stroke::NONE)
                        .corner_radius(CornerRadius::same(255));
                        if padded_button(ui, auth_btn, vec2(12.0, 4.5)).clicked() {
                            actions.push(SettingsAction::OpenAccessibilitySettings);
                        }
                    });
            }
        }
        #[cfg(not(target_os = "macos"))]
        let _ = has_accessibility;
    });

    ui.add_space(8.0);
    section_card(ui, palette, |ui| {
        ui.label(
            RichText::new(t("settings.general.shortcuts_title"))
                .size(12.0)
                .color(palette.text_secondary),
        );
        ui.add_space(4.0);

        let default_open = AppSettings::default().hotkey_open;
        let default_pin = AppSettings::default().hotkey_pin;
        let default_del = AppSettings::default().hotkey_delete;
        let default_prev = AppSettings::default().hotkey_preview;

        let mut new_open = settings.hotkey_open.clone();
        if shortcut_row(
            ui,
            t("settings.general.shortcut_open"),
            &mut new_open,
            &default_open,
            RecordingTarget::Open,
            state,
            palette,
        ) {
            actions.push(SettingsAction::OpenHotkeyChanged(new_open));
        }
        if shortcut_row(
            ui,
            t("settings.general.shortcut_pin"),
            &mut settings.hotkey_pin,
            &default_pin,
            RecordingTarget::Pin,
            state,
            palette,
        ) {
            actions.push(SettingsAction::SettingsChanged);
        }
        if shortcut_row(
            ui,
            t("settings.general.shortcut_delete"),
            &mut settings.hotkey_delete,
            &default_del,
            RecordingTarget::Delete,
            state,
            palette,
        ) {
            actions.push(SettingsAction::SettingsChanged);
        }
        if shortcut_row(
            ui,
            t("settings.general.shortcut_preview"),
            &mut settings.hotkey_preview,
            &default_prev,
            RecordingTarget::Preview,
            state,
            palette,
        ) {
            actions.push(SettingsAction::SettingsChanged);
        }
    });

    ui.add_space(8.0);
    section_card(ui, palette, |ui| {
        ui.horizontal(|ui| {
            ui.label(t("settings.general.click_mode"));
            egui::ComboBox::from_id_salt("copy_trigger_mode_combo")
                .selected_text(settings.copy_trigger_mode.label())
                .show_ui(ui, |ui| {
                    for mode in [CopyTriggerMode::DoubleClick, CopyTriggerMode::SingleClick] {
                        if ui
                            .selectable_value(&mut settings.copy_trigger_mode, mode, mode.label())
                            .changed()
                        {
                            actions.push(SettingsAction::SettingsChanged);
                        }
                    }
                });
        });

        let click_hint = match settings.copy_trigger_mode {
            CopyTriggerMode::DoubleClick => t("settings.general.click_hint_double"),
            CopyTriggerMode::SingleClick => t("settings.general.click_hint_single"),
        };
        ui.label(
            RichText::new(click_hint)
                .size(11.0)
                .color(palette.text_muted),
        );

        ui.add_space(4.0);
        ui.horizontal(|ui| {
            ui.label(t("settings.general.search_mode"));
            egui::ComboBox::from_id_salt("search_mode_combo")
                .selected_text(settings.search_mode.label())
                .show_ui(ui, |ui| {
                    for mode in [SearchMode::Exact, SearchMode::Fuzzy, SearchMode::Regex] {
                        if ui
                            .selectable_value(&mut settings.search_mode, mode, mode.label())
                            .changed()
                        {
                            actions.push(SettingsAction::SettingsChanged);
                        }
                    }
                });
        });

        ui.add_space(4.0);
        if ui
            .checkbox(&mut settings.auto_paste, t("settings.general.auto_paste"))
            .changed()
        {
            actions.push(SettingsAction::SettingsChanged);
        }
        if ui
            .checkbox(
                &mut settings.plain_text_paste,
                t("settings.general.plain_text_paste"),
            )
            .changed()
        {
            actions.push(SettingsAction::SettingsChanged);
        }

        ui.add_space(4.0);
        #[cfg(target_os = "macos")]
        let hint = t("settings.general.paste_hint_macos");
        #[cfg(not(target_os = "macos"))]
        let hint = t("settings.general.paste_hint_other");

        ui.label(RichText::new(hint).size(11.5).color(palette.text_muted));
    });
}

fn render_update_status_row(
    ui: &mut Ui,
    rel: &ReleaseInfo,
    dl_state: &UpdateDownloadState,
    palette: &EarthPalette,
    actions: &mut Vec<SettingsAction>,
) {
    let (status_text, status_color) = match dl_state {
        UpdateDownloadState::Downloading { version } if version == &rel.version => (
            tf(
                "settings.general.update_downloading",
                &[("version", &rel.version)],
            ),
            palette.accent_text,
        ),
        UpdateDownloadState::ReadyToInstall { version, .. } if version == &rel.version => (
            tf(
                "settings.general.update_ready",
                &[("version", &rel.version)],
            ),
            palette.success_text,
        ),
        UpdateDownloadState::Failed { version, error } if version == &rel.version => (
            tf(
                "settings.general.update_failed",
                &[("version", &rel.version), ("error", error)],
            ),
            palette.warning_text,
        ),
        _ => (
            tf(
                "settings.general.update_available",
                &[("version", &rel.version)],
            ),
            palette.accent_text,
        ),
    };

    let reserve_w = match dl_state {
        UpdateDownloadState::ReadyToInstall { version, .. } if version == &rel.version => 188.0,
        UpdateDownloadState::Downloading { version } if version == &rel.version => 84.0,
        UpdateDownloadState::Failed { version, .. }
            if version == &rel.version && rel.download_url.is_some() =>
        {
            192.0
        }
        _ if rel.download_url.is_some() => 164.0,
        _ => 84.0,
    };

    egui::Frame::NONE
        .fill(palette.info_bg)
        .stroke(Stroke::new(1.0, palette.info_border))
        .corner_radius(CornerRadius::same(12))
        .inner_margin(egui::Margin::symmetric(10, 6))
        .show(ui, |ui| {
            ui.set_max_width(ui.available_width());
            ui.horizontal(|ui| {
                let text_w = (ui.available_width() - reserve_w).max(80.0);
                ui.allocate_ui_with_layout(
                    vec2(text_w, 22.0),
                    Layout::left_to_right(Align::Center),
                    |ui| {
                        ui.set_max_width(text_w);
                        ui.add(
                            egui::Label::new(
                                RichText::new(&status_text).size(11.5).color(status_color),
                            )
                            .truncate(),
                        )
                        .on_hover_text(&status_text);
                    },
                );
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if compact_button(ui, t("banner.update_open_download")).clicked() {
                        actions.push(SettingsAction::OpenUrl(rel.html_url.clone()));
                    }
                    match dl_state {
                        UpdateDownloadState::ReadyToInstall {
                            version,
                            staged_path,
                        } if version == &rel.version => {
                            let restart_btn = egui::Button::new(
                                RichText::new(t("banner.update_restart"))
                                    .size(11.0)
                                    .color(palette.on_accent),
                            )
                            .fill(palette.accent)
                            .stroke(Stroke::NONE)
                            .corner_radius(CornerRadius::same(255));
                            if padded_button(ui, restart_btn, vec2(10.0, 3.5)).clicked() {
                                actions.push(SettingsAction::ApplyUpdateAndRestart(
                                    staged_path.clone(),
                                ));
                            }
                        }
                        UpdateDownloadState::Downloading { version } if version == &rel.version => {
                        }
                        UpdateDownloadState::Failed { version, .. }
                            if version == &rel.version && rel.download_url.is_some() =>
                        {
                            if compact_button(ui, t("settings.general.update_retry")).clicked() {
                                actions.push(SettingsAction::StartUpdateDownload(rel.clone()));
                            }
                        }
                        _ if rel.download_url.is_some() => {
                            let auto_btn = egui::Button::new(
                                RichText::new(t("banner.update_auto"))
                                    .size(11.0)
                                    .color(palette.on_accent),
                            )
                            .fill(palette.accent)
                            .stroke(Stroke::NONE)
                            .corner_radius(CornerRadius::same(255));
                            if padded_button(ui, auto_btn, vec2(10.0, 3.5)).clicked() {
                                actions.push(SettingsAction::StartUpdateDownload(rel.clone()));
                            }
                        }
                        _ => {}
                    }
                });
            });
        });
}

fn shortcut_row(
    ui: &mut Ui,
    label: &str,
    value: &mut String,
    default_val: &str,
    target: RecordingTarget,
    state: &mut SettingsState,
    palette: &EarthPalette,
) -> bool {
    let mut changed = false;
    let is_recording = state.recording == Some(target);
    ui.horizontal(|ui| {
        ui.allocate_ui_with_layout(
            vec2(88.0, 26.0),
            Layout::left_to_right(Align::Center),
            |ui| {
                ui.set_min_width(88.0);
                ui.label(label);
            },
        );

        let live_mods = if is_recording {
            platform::native_recording_modifiers()
                .or_else(|| {
                    ui.ctx().input(|input| {
                        let m = input.modifiers;
                        #[cfg(target_os = "macos")]
                        let (ctrl, super_key) = (m.ctrl, m.mac_cmd || m.command);
                        #[cfg(not(target_os = "macos"))]
                        let (ctrl, super_key) = (m.ctrl || m.command, false);
                        (ctrl || m.alt || m.shift || super_key)
                            .then_some((ctrl, m.alt, m.shift, super_key))
                    })
                })
                .and_then(|(c, a, s, w)| format_live_modifiers(c, a, s, w))
        } else {
            None
        };
        let text = if is_recording {
            if let Some(mods) = live_mods {
                tf(
                    "settings.general.shortcut_recording_mods",
                    &[("mods", &mods)],
                )
            } else {
                t("settings.general.shortcut_recording").to_string()
            }
        } else {
            format_shortcut_display(value)
        };
        let fill = if is_recording {
            palette.accent_soft
        } else {
            palette.surface_bg
        };
        let stroke = if is_recording {
            Stroke::new(1.0, palette.accent)
        } else {
            Stroke::new(1.0, palette.border)
        };
        let btn = egui::Button::new(RichText::new(text).size(12.0).color(if is_recording {
            palette.accent_text
        } else {
            palette.text_primary
        }))
        .min_size(vec2(170.0, 26.0))
        .fill(fill)
        .stroke(stroke)
        .corner_radius(CornerRadius::same(12));
        let resp = ui.add(btn);
        if !state.recording_suppress_reclick && resp.clicked() {
            state.recording = Some(target);
            ui.ctx().memory_mut(|m| m.surrender_focus(resp.id));
        }

        if compact_button(ui, t("settings.general.shortcut_reset"))
            .on_hover_text(t("settings.general.shortcut_reset_tip"))
            .clicked()
        {
            *value = default_val.to_string();
            state.recording = None;
            changed = true;
        }
    });
    changed
}

fn render_storage_tab(
    ui: &mut Ui,
    settings: &mut AppSettings,
    palette: &EarthPalette,
    storage_bytes: u64,
    item_count: usize,
    actions: &mut Vec<SettingsAction>,
) {
    section_card(ui, palette, |ui| {
        ui.label(RichText::new(t("settings.storage.save_types")).strong());
        ui.add_space(4.0);
        if ui
            .checkbox(&mut settings.save_files, t("settings.storage.type_files"))
            .changed()
        {
            actions.push(SettingsAction::SettingsChanged);
        }
        if ui
            .checkbox(&mut settings.save_images, t("settings.storage.type_images"))
            .changed()
        {
            actions.push(SettingsAction::SettingsChanged);
        }
        if ui
            .checkbox(&mut settings.save_text, t("settings.storage.type_text"))
            .changed()
        {
            actions.push(SettingsAction::SettingsChanged);
        }
        ui.label(
            RichText::new(t("settings.storage.save_types_hint"))
                .size(11.5)
                .color(palette.text_muted),
        );
    });

    ui.add_space(8.0);
    section_card(ui, palette, |ui| {
        ui.horizontal(|ui| {
            ui.label(t("settings.storage.max_history"));
            if ui
                .add(
                    egui::DragValue::new(&mut settings.max_history)
                        .range(10..=2000)
                        .speed(5),
                )
                .changed()
            {
                actions.push(SettingsAction::SettingsChanged);
            }
            ui.add_space(12.0);
            let count_s = item_count.to_string();
            let size_s = format_bytes(storage_bytes);
            ui.label(
                RichText::new(tf(
                    "settings.storage.stats",
                    &[("count", &count_s), ("size", &size_s)],
                ))
                .size(12.0)
                .color(palette.text_secondary),
            );
        });

        ui.horizontal_wrapped(|ui| {
            ui.label(t("settings.storage.item_limits"));
            if ui
                .add(
                    egui::DragValue::new(&mut settings.max_text_mib)
                        .range(1..=16)
                        .prefix(t("settings.storage.limit_text_prefix"))
                        .suffix(" MiB"),
                )
                .changed()
                | ui.add(
                    egui::DragValue::new(&mut settings.max_image_mib)
                        .range(1..=64)
                        .prefix(t("settings.storage.limit_image_prefix"))
                        .suffix(" MiB"),
                )
                .changed()
                | ui.add(
                    egui::DragValue::new(&mut settings.max_image_megapixels)
                        .range(1..=64)
                        .suffix(t("settings.storage.limit_megapixels_suffix")),
                )
                .changed()
            {
                actions.push(SettingsAction::SettingsChanged);
            }
        });
        ui.add_space(6.0);
        ui.horizontal(|ui| {
            ui.label(t("settings.storage.sort_order"));
            egui::ComboBox::from_id_salt("sort_order_combo")
                .selected_text(settings.sort_order.label())
                .show_ui(ui, |ui| {
                    for order in [
                        SortOrder::LastCopiedAt,
                        SortOrder::FirstCopiedAt,
                        SortOrder::CopyCount,
                    ] {
                        if ui
                            .selectable_value(&mut settings.sort_order, order, order.label())
                            .changed()
                        {
                            actions.push(SettingsAction::SettingsChanged);
                        }
                    }
                });
        });

        ui.add_space(8.0);
        ui.horizontal_wrapped(|ui| {
            if ui.button(t("settings.storage.clear_unpinned")).clicked() {
                actions.push(SettingsAction::ClearHistory(false));
            }
            if ui
                .add(
                    egui::Button::new(
                        RichText::new(t("settings.storage.clear_all")).color(palette.danger),
                    )
                    .fill(palette.danger_bg)
                    .stroke(Stroke::new(1.0, palette.danger_border))
                    .corner_radius(CornerRadius::same(255)),
                )
                .clicked()
            {
                actions.push(SettingsAction::ClearHistory(true));
            }
            if ui.button(t("settings.storage.export_json")).clicked() {
                actions.push(SettingsAction::ExportJson);
            }
            if ui.button(t("settings.storage.import_json")).clicked() {
                actions.push(SettingsAction::ImportJson);
            }
        });
        ui.label(
            RichText::new(t("settings.storage.backup_note"))
                .size(11.5)
                .color(palette.text_muted),
        );
    });
}

fn render_appearance_tab(
    ui: &mut Ui,
    settings: &mut AppSettings,
    palette: &EarthPalette,
    actions: &mut Vec<SettingsAction>,
) {
    section_card(ui, palette, |ui| {
        ui.horizontal(|ui| {
            ui.label(t("settings.appearance.theme"));
            egui::ComboBox::from_id_salt("theme_mode_combo")
                .selected_text(settings.theme_mode.label())
                .show_ui(ui, |ui| {
                    for m in [ThemeMode::System, ThemeMode::Dark, ThemeMode::Light] {
                        if ui
                            .selectable_value(&mut settings.theme_mode, m, m.label())
                            .changed()
                        {
                            actions.push(SettingsAction::SettingsChanged);
                        }
                    }
                });
        });

        ui.horizontal(|ui| {
            ui.label(t("settings.appearance.popup_pos"));
            egui::ComboBox::from_id_salt("popup_pos_combo")
                .selected_text(settings.popup_position.label())
                .show_ui(ui, |ui| {
                    for p in [
                        PopupPosition::Cursor,
                        PopupPosition::ScreenCenter,
                        PopupPosition::WindowCenter,
                        PopupPosition::Tray,
                    ] {
                        if ui
                            .selectable_value(&mut settings.popup_position, p, p.label())
                            .changed()
                        {
                            actions.push(SettingsAction::SettingsChanged);
                        }
                    }
                });
        });

        ui.horizontal(|ui| {
            ui.label(t("settings.appearance.pin_pos"));
            egui::ComboBox::from_id_salt("pin_pos_combo")
                .selected_text(settings.pin_position.label())
                .show_ui(ui, |ui| {
                    for p in [PinPosition::Top, PinPosition::Bottom] {
                        if ui
                            .selectable_value(&mut settings.pin_position, p, p.label())
                            .changed()
                        {
                            actions.push(SettingsAction::SettingsChanged);
                        }
                    }
                });
        });

        ui.horizontal(|ui| {
            ui.label(t("settings.appearance.highlight"));
            egui::ComboBox::from_id_salt("highlight_combo")
                .selected_text(settings.match_highlight.label())
                .show_ui(ui, |ui| {
                    for h in [
                        MatchHighlight::Bold,
                        MatchHighlight::Italic,
                        MatchHighlight::Underline,
                    ] {
                        if ui
                            .selectable_value(&mut settings.match_highlight, h, h.label())
                            .changed()
                        {
                            actions.push(SettingsAction::SettingsChanged);
                        }
                    }
                });
        });
    });

    ui.add_space(8.0);
    section_card(ui, palette, |ui| {
        let mut changed = false;
        changed |= ui
            .checkbox(
                &mut settings.show_special_symbols,
                t("settings.appearance.special_symbols"),
            )
            .changed();
        changed |= ui
            .checkbox(
                &mut settings.show_tray_icon,
                t("settings.appearance.show_tray"),
            )
            .changed();
        changed |= ui
            .checkbox(
                &mut settings.show_clipboard_in_menubar,
                t("settings.appearance.menubar_preview"),
            )
            .changed();

        ui.horizontal(|ui| {
            ui.label(t("settings.appearance.search_vis"));
            egui::ComboBox::from_id_salt("search_vis_combo")
                .selected_text(settings.search_box_visibility.label())
                .show_ui(ui, |ui| {
                    for v in [
                        SearchBoxVisibility::Always,
                        SearchBoxVisibility::WhileTyping,
                    ] {
                        if ui
                            .selectable_value(&mut settings.search_box_visibility, v, v.label())
                            .changed()
                        {
                            changed = true;
                        }
                    }
                });
        });

        changed |= ui
            .checkbox(
                &mut settings.show_source_app_badge,
                t("settings.appearance.source_app"),
            )
            .changed();
        changed |= ui
            .checkbox(
                &mut settings.show_hex_color_swatch,
                t("settings.appearance.color_swatch"),
            )
            .changed();
        changed |= ui
            .checkbox(
                &mut settings.show_footer_menu,
                t("settings.appearance.footer_menu"),
            )
            .changed();

        if changed {
            actions.push(SettingsAction::SettingsChanged);
        }
    });
}

fn render_ignore_tab(
    ui: &mut Ui,
    settings: &mut AppSettings,
    state: &mut SettingsState,
    palette: &EarthPalette,
    actions: &mut Vec<SettingsAction>,
) {
    section_card(ui, palette, |ui| {
        if ui
            .checkbox(
                &mut settings.enable_privacy_masking,
                t("settings.ignore.privacy_mask"),
            )
            .changed()
        {
            actions.push(SettingsAction::SettingsChanged);
        }
        ui.label(
            RichText::new(t("settings.ignore.privacy_hint"))
                .size(11.5)
                .color(palette.text_muted),
        );
    });

    ui.add_space(8.0);
    section_card(ui, palette, |ui| {
        ui.horizontal(|ui| {
            for (sub, title) in [
                (IgnoreSubTab::Apps, t("settings.ignore.sub_apps")),
                (IgnoreSubTab::ClipboardTypes, t("settings.ignore.sub_types")),
                (IgnoreSubTab::Regexes, t("settings.ignore.sub_regexes")),
            ] {
                ui.selectable_value(&mut state.ignore_sub_tab, sub, title);
            }
        });
        ui.separator();

        match state.ignore_sub_tab {
            IgnoreSubTab::Apps => {
                if ui
                    .checkbox(
                        &mut settings.ignore_only_outside_list,
                        t("settings.ignore.whitelist_mode"),
                    )
                    .changed()
                {
                    actions.push(SettingsAction::SettingsChanged);
                }
                render_editable_string_list(
                    ui,
                    &mut settings.ignored_apps,
                    &mut state.new_ignore_app,
                    t("settings.ignore.app_placeholder"),
                    palette,
                    actions,
                );
            }
            IgnoreSubTab::ClipboardTypes => {
                ui.label(
                    RichText::new(t("settings.ignore.types_hint"))
                        .size(11.5)
                        .color(palette.text_muted),
                );
                render_editable_string_list(
                    ui,
                    &mut settings.ignored_clipboard_types,
                    &mut state.new_ignore_type,
                    t("settings.ignore.type_placeholder"),
                    palette,
                    actions,
                );
            }
            IgnoreSubTab::Regexes => {
                ui.label(
                    RichText::new(t("settings.ignore.regex_hint"))
                        .size(11.5)
                        .color(palette.text_muted),
                );
                render_editable_string_list(
                    ui,
                    &mut settings.ignored_regexes,
                    &mut state.new_ignore_regex,
                    t("settings.ignore.regex_placeholder"),
                    palette,
                    actions,
                );
            }
        }
    });
}

fn render_editable_string_list(
    ui: &mut Ui,
    list: &mut Vec<String>,
    draft: &mut String,
    placeholder: &str,
    palette: &EarthPalette,
    actions: &mut Vec<SettingsAction>,
) {
    ui.horizontal(|ui| {
        let edit = egui::TextEdit::singleline(draft)
            .hint_text(placeholder)
            .desired_width(ui.available_width() - 68.0);
        let resp = ui.add(edit);
        let add_btn =
            egui::Button::new(RichText::new(t("settings.ignore.add")).color(palette.on_accent))
                .fill(palette.accent)
                .stroke(Stroke::NONE)
                .corner_radius(CornerRadius::same(255));
        let add_clicked = ui.add(add_btn).clicked()
            || (resp.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)));
        if add_clicked {
            let trimmed = draft.trim().to_string();
            if !trimmed.is_empty() && !list.iter().any(|x| x == &trimmed) {
                list.push(trimmed);
                draft.clear();
                actions.push(SettingsAction::SettingsChanged);
            }
        }
    });

    ui.add_space(4.0);
    let mut remove_idx: Option<usize> = None;
    egui::Frame::NONE
        .fill(palette.card_bg)
        .stroke(Stroke::new(1.0, palette.border_subtle))
        .corner_radius(CornerRadius::same(12))
        .inner_margin(egui::Margin::same(8))
        .show(ui, |ui| {
            if list.is_empty() {
                ui.label(
                    RichText::new(t("settings.ignore.empty"))
                        .size(12.0)
                        .color(palette.text_muted),
                );
            } else {
                for (idx, entry) in list.iter().enumerate() {
                    ui.horizontal(|ui| {
                        ui.label(RichText::new(entry).size(12.0));
                        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                            if compact_button(ui, t("settings.ignore.remove")).clicked() {
                                remove_idx = Some(idx);
                            }
                        });
                    });
                }
            }
        });

    if let Some(idx) = remove_idx {
        list.remove(idx);
        actions.push(SettingsAction::SettingsChanged);
    }
}

fn render_advanced_tab(
    ui: &mut Ui,
    settings: &mut AppSettings,
    collections: &[Collection],
    state: &mut SettingsState,
    palette: &EarthPalette,
    actions: &mut Vec<SettingsAction>,
) {
    section_card(ui, palette, |ui| {
        if ui
            .checkbox(&mut settings.paused, t("settings.advanced.paused"))
            .changed()
        {
            actions.push(SettingsAction::SettingsChanged);
        }
        if ui
            .checkbox(
                &mut settings.ignore_next_event,
                t("settings.advanced.ignore_next"),
            )
            .changed()
        {
            actions.push(SettingsAction::SettingsChanged);
        }
        ui.separator();
        if ui
            .checkbox(
                &mut settings.clear_on_exit,
                t("settings.advanced.clear_on_exit"),
            )
            .changed()
        {
            actions.push(SettingsAction::SettingsChanged);
        }
        if ui
            .checkbox(
                &mut settings.clear_system_clipboard_too,
                t("settings.advanced.clear_system"),
            )
            .changed()
        {
            actions.push(SettingsAction::SettingsChanged);
        }
    });

    ui.add_space(8.0);
    section_card(ui, palette, |ui| {
        ui.label(RichText::new(t("settings.advanced.collections_title")).strong());
        ui.label(
            RichText::new(t("settings.advanced.collections_hint"))
                .size(11.5)
                .color(palette.text_muted),
        );
        ui.add_space(4.0);

        ui.horizontal(|ui| {
            ui.add(
                egui::TextEdit::singleline(&mut state.new_collection_name)
                    .hint_text(t("settings.advanced.collection_placeholder"))
                    .desired_width(132.0),
            );
            for hex in ["#456EFD", "#22C55E", "#F59E0B", "#EC4899", "#8B5CF6"] {
                let c = parse_hex_color32(hex).unwrap_or(palette.accent);
                let selected = state.new_collection_color == hex;
                let (rect, resp) = ui.allocate_exact_size(vec2(18.0, 18.0), egui::Sense::click());
                ui.painter().circle_filled(rect.center(), 7.0, c);
                if selected {
                    ui.painter().circle_stroke(
                        rect.center(),
                        9.0,
                        Stroke::new(1.5, palette.text_primary),
                    );
                }
                if resp.clicked() {
                    state.new_collection_color = hex.to_string();
                }
            }
            let create_btn = egui::Button::new(
                RichText::new(t("settings.advanced.create_collection")).color(palette.on_accent),
            )
            .fill(palette.accent)
            .stroke(Stroke::NONE)
            .corner_radius(CornerRadius::same(255));
            if ui.add(create_btn).clicked() && !state.new_collection_name.trim().is_empty() {
                actions.push(SettingsAction::CreateCollection {
                    name: state.new_collection_name.trim().to_string(),
                    color_hex: state.new_collection_color.clone(),
                });
                state.new_collection_name.clear();
            }
        });

        ui.add_space(6.0);
        for col in collections {
            ui.horizontal(|ui| {
                let dot_color = parse_hex_color32(&col.color_hex).unwrap_or(palette.accent);
                let (r, _) = ui.allocate_exact_size(vec2(14.0, 14.0), egui::Sense::hover());
                ui.painter().circle_filled(r.center(), 5.0, dot_color);
                if state.editing_collection_id == Some(col.id) {
                    let edit_resp = ui.add(
                        egui::TextEdit::singleline(&mut state.editing_collection_name)
                            .desired_width(170.0),
                    );
                    let submitted =
                        edit_resp.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        if compact_button(ui, t("settings.advanced.cancel")).clicked() {
                            state.editing_collection_id = None;
                            state.editing_collection_name.clear();
                        }
                        let save_btn = egui::Button::new(
                            RichText::new(t("settings.advanced.save"))
                                .size(12.0)
                                .color(palette.on_accent),
                        )
                        .fill(palette.accent)
                        .stroke(Stroke::NONE)
                        .corner_radius(CornerRadius::same(255));
                        if padded_button(ui, save_btn, vec2(10.0, 4.5)).clicked() || submitted {
                            let trimmed = state.editing_collection_name.trim().to_string();
                            if !trimmed.is_empty() {
                                actions.push(SettingsAction::RenameCollection {
                                    id: col.id,
                                    name: trimmed,
                                });
                            }
                            state.editing_collection_id = None;
                            state.editing_collection_name.clear();
                        }
                    });
                } else {
                    ui.label(RichText::new(&col.name).size(13.0));
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        let del_btn = egui::Button::new(
                            RichText::new(t("settings.advanced.delete")).color(palette.danger),
                        )
                        .fill(palette.danger_bg)
                        .stroke(Stroke::new(1.0, palette.danger_border))
                        .corner_radius(CornerRadius::same(255));
                        if padded_button(ui, del_btn, vec2(11.0, 4.5))
                            .on_hover_text(t("settings.advanced.delete_collection_tip"))
                            .clicked()
                        {
                            actions.push(SettingsAction::DeleteCollection(col.id));
                        }
                        if compact_button(ui, t("settings.advanced.rename")).clicked() {
                            state.editing_collection_id = Some(col.id);
                            state.editing_collection_name = col.name.clone();
                        }
                    });
                }
            });
        }
    });
}

fn section_card(ui: &mut Ui, palette: &EarthPalette, add_contents: impl FnOnce(&mut Ui)) {
    let card_w = (ui.available_width() - 24.0).max(160.0);
    egui::Frame::NONE
        .fill(palette.surface_bg)
        .stroke(Stroke::new(1.0, palette.border_subtle))
        .corner_radius(CornerRadius::same(16))
        .shadow(card_shadow(palette))
        .inner_margin(egui::Margin::same(12))
        .show(ui, |ui| {
            ui.set_min_width(card_w);
            ui.set_max_width(card_w);
            add_contents(ui);
        });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run_settings_frame(
        ctx: &egui::Context,
        settings: &mut AppSettings,
        state: &mut SettingsState,
        events: Vec<egui::Event>,
    ) -> Vec<SettingsAction> {
        let palette = EarthPalette::resolve(ThemeMode::Light, false);
        let raw = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::pos2(0.0, 0.0),
                vec2(560.0, 620.0),
            )),
            events,
            ..Default::default()
        };
        let mut actions = Vec::new();
        let mut out = ctx.run_ui(raw, |ui| {
            let ctx = ui.ctx().clone();
            egui::CentralPanel::default()
                .frame(egui::Frame::NONE)
                .show(ui, |ui| {
                    actions = render_settings_view(
                        ui,
                        &ctx,
                        SettingsViewData {
                            settings,
                            collections: &[],
                            palette: &palette,
                            storage_bytes: 0,
                            item_count: 0,
                            has_accessibility: true,
                        },
                        state,
                    );
                });
        });
        out.textures_delta.clear();
        actions
    }

    #[test]
    fn settings_recorder_rejects_reserved_combos_preserves_old_value_and_accepts_safe_combos() {
        let ctx = egui::Context::default();
        let mut settings = AppSettings::default();
        let mut state = SettingsState {
            active_tab: SettingsTab::General,
            recording: Some(RecordingTarget::Open),
            ..Default::default()
        };

        let previous_open = settings.hotkey_open.clone();
        let previous_pin = settings.hotkey_pin.clone();
        let previous_preview = settings.hotkey_preview.clone();

        // Clipboard shortcuts remain available for normal text editing.
        let actions = run_settings_frame(
            &ctx,
            &mut settings,
            &mut state,
            vec![egui::Event::Paste("copied text".into())],
        );
        assert_eq!(settings.hotkey_open, previous_open);
        assert_eq!(state.recording, Some(RecordingTarget::Open));
        assert!(state.hotkey_error.is_some());
        assert!(actions.is_empty());

        state.recording = Some(RecordingTarget::Pin);
        let actions = run_settings_frame(&ctx, &mut settings, &mut state, vec![egui::Event::Copy]);
        assert_eq!(settings.hotkey_pin, previous_pin);
        assert_eq!(state.recording, Some(RecordingTarget::Pin));
        assert!(state.hotkey_error.is_some());
        assert!(actions.is_empty());

        // The input-method shortcut is also rejected without replacing the old value.
        state.recording = Some(RecordingTarget::Preview);
        let actions = run_settings_frame(
            &ctx,
            &mut settings,
            &mut state,
            vec![egui::Event::Key {
                key: egui::Key::Space,
                physical_key: None,
                pressed: true,
                repeat: false,
                modifiers: egui::Modifiers::CTRL,
            }],
        );
        assert_eq!(settings.hotkey_preview, previous_preview);
        assert_eq!(state.recording, Some(RecordingTarget::Preview));
        assert!(state.hotkey_error.is_some());
        assert!(actions.is_empty());

        // An accepted global shortcut is emitted for registration before settings change.
        let safe_modifiers = egui::Modifiers {
            ctrl: true,
            shift: true,
            ..Default::default()
        };
        state.recording = Some(RecordingTarget::Open);
        let actions = run_settings_frame(
            &ctx,
            &mut settings,
            &mut state,
            vec![egui::Event::Key {
                key: egui::Key::F6,
                physical_key: None,
                pressed: true,
                repeat: false,
                modifiers: safe_modifiers,
            }],
        );
        assert_eq!(settings.hotkey_open, previous_open);
        assert!(state.hotkey_error.is_none());
        assert!(state.recording.is_none());
        assert!(matches!(
            actions.as_slice(),
            [SettingsAction::OpenHotkeyChanged(combo)] if combo == "Ctrl+Shift+F6"
        ));

        state.recording = Some(RecordingTarget::Preview);
        let actions = run_settings_frame(
            &ctx,
            &mut settings,
            &mut state,
            vec![egui::Event::Key {
                key: egui::Key::P,
                physical_key: None,
                pressed: true,
                repeat: false,
                modifiers: safe_modifiers,
            }],
        );
        assert_eq!(settings.hotkey_preview, "Ctrl+Shift+P");
        assert!(state.hotkey_error.is_none());
        assert!(state.recording.is_none());
        assert!(matches!(
            actions.as_slice(),
            [SettingsAction::SettingsChanged]
        ));

        // Escape while recording cancels capture without closing Settings.
        state.recording = Some(RecordingTarget::Delete);
        let actions = run_settings_frame(
            &ctx,
            &mut settings,
            &mut state,
            vec![egui::Event::Key {
                key: egui::Key::Escape,
                physical_key: None,
                pressed: true,
                repeat: false,
                modifiers: egui::Modifiers::NONE,
            }],
        );
        assert!(state.recording.is_none());
        assert!(actions.is_empty());
    }

    #[test]
    fn shortcut_row_focus_space_does_not_retrigger_recording_on_capture_frame() {
        let ctx = egui::Context::default();
        let palette = EarthPalette::resolve(ThemeMode::Light, false);
        let mut settings = AppSettings::default();
        let mut state = SettingsState::default();

        // Frame 1: press Tab inside a minimal panel containing shortcut_row so the shortcut button gains focus.
        let tab_raw = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::pos2(0.0, 0.0),
                vec2(480.0, 120.0),
            )),
            events: vec![egui::Event::Key {
                key: egui::Key::Tab,
                physical_key: None,
                pressed: true,
                repeat: false,
                modifiers: egui::Modifiers::NONE,
            }],
            ..Default::default()
        };
        let mut out = ctx.run_ui(tab_raw, |ui| {
            egui::CentralPanel::default()
                .frame(egui::Frame::NONE)
                .show(ui, |ui| {
                    let _ = shortcut_row(
                        ui,
                        "打开面板快捷键",
                        &mut settings.hotkey_open,
                        "Alt+V",
                        RecordingTarget::Open,
                        &mut state,
                        &palette,
                    );
                });
        });
        out.textures_delta.clear();

        // Frame 2: mimic a capture frame where recording was active at frame start (so recording_suppress_reclick is true)
        // and Ctrl+Space completes capture (setting state.recording = None) before shortcut_row runs.
        state.recording_suppress_reclick = true;
        state.recording = None;
        let space_raw = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::pos2(0.0, 0.0),
                vec2(480.0, 120.0),
            )),
            events: vec![egui::Event::Key {
                key: egui::Key::Space,
                physical_key: None,
                pressed: true,
                repeat: false,
                modifiers: egui::Modifiers::CTRL,
            }],
            ..Default::default()
        };
        let mut out = ctx.run_ui(space_raw, |ui| {
            egui::CentralPanel::default()
                .frame(egui::Frame::NONE)
                .show(ui, |ui| {
                    let _ = shortcut_row(
                        ui,
                        "打开面板快捷键",
                        &mut settings.hotkey_open,
                        "Alt+V",
                        RecordingTarget::Open,
                        &mut state,
                        &palette,
                    );
                });
        });
        out.textures_delta.clear();
        assert!(
            state.recording.is_none(),
            "focused button Space activation must not re-start recording on the frame capture finishes"
        );
    }

    #[test]
    fn update_status_row_does_not_overlap_checkbox_or_buttons_at_compact_width() {
        let ctx = egui::Context::default();
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

        let palette = EarthPalette::resolve(ThemeMode::Light, false);
        let rel = ReleaseInfo {
            version: "0.5.0".into(),
            html_url: "https://github.com/lumingbao/Earth/releases/tag/v0.5.0".into(),
            tag_name: "v0.5.0".into(),
            download_url: Some(
                "https://github.com/lumingbao/Earth/releases/download/v0.5.0/Earth-0.5.0-macOS-universal.dmg"
                    .into(),
            ),
            asset_name: Some("Earth-0.5.0-macOS-universal.dmg".into()),
            checksum_url: Some(
                "https://github.com/lumingbao/Earth/releases/download/v0.5.0/SHA256SUMS.txt".into(),
            ),
        };

        let states = [
            (
                "downloading",
                UpdateDownloadState::Downloading {
                    version: "0.5.0".into(),
                },
                "⬇ 正在下载新版本 v0.5.0",
            ),
            (
                "ready",
                UpdateDownloadState::ReadyToInstall {
                    version: "0.5.0".into(),
                    staged_path: PathBuf::from("/tmp/Earth.app"),
                },
                "✓ 新版本 v0.5.0 已就绪",
            ),
            (
                "failed",
                UpdateDownloadState::Failed {
                    version: "0.5.0".into(),
                    error: "连接超时".into(),
                },
                "⚠ v0.5.0 自动下载失败",
            ),
            ("idle", UpdateDownloadState::Idle, "🎉 发现新版本 v0.5.0"),
        ];

        fn collect_text_rects(shapes: &[egui::epaint::ClippedShape]) -> Vec<(String, egui::Rect)> {
            let mut out = Vec::new();
            fn walk(shape: &egui::epaint::Shape, out: &mut Vec<(String, egui::Rect)>) {
                match shape {
                    egui::epaint::Shape::Text(t) => {
                        out.push((t.galley.text().to_string(), t.visual_bounding_rect()));
                    }
                    egui::epaint::Shape::Vec(v) => {
                        for child in v {
                            walk(child, out);
                        }
                    }
                    _ => {}
                }
            }
            for clipped in shapes {
                walk(&clipped.shape, &mut out);
            }
            out
        }

        for (label, dl_state, status_needle) in states {
            let mut settings = AppSettings::default();
            let mut state = SettingsState {
                active_tab: SettingsTab::General,
                available_update: Some(rel.clone()),
                update_download: dl_state,
                ..Default::default()
            };
            let raw = egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::pos2(0.0, 0.0),
                    vec2(424.0, 520.0),
                )),
                ..Default::default()
            };
            let mut out = ctx.run_ui(raw, |ui| {
                let ctx = ui.ctx().clone();
                egui::CentralPanel::default()
                    .frame(egui::Frame::NONE)
                    .show(ui, |ui| {
                        let _ = render_settings_view(
                            ui,
                            &ctx,
                            SettingsViewData {
                                settings: &mut settings,
                                collections: &[],
                                palette: &palette,
                                storage_bytes: 0,
                                item_count: 0,
                                has_accessibility: true,
                            },
                            &mut state,
                        );
                    });
            });
            let rects = collect_text_rects(&out.shapes);
            out.textures_delta.clear();

            let (_, check_rect) = rects
                .iter()
                .find(|(t, _)| t.contains("启用更新提示检查"))
                .expect("checkbox text should render");
            let (_, check_btn_rect) = rects
                .iter()
                .find(|(t, _)| t == "检查更新")
                .expect("check-now button should render");
            let (_, dl_page_rect) = rects
                .iter()
                .find(|(t, _)| t == "前往下载")
                .expect("open-download button should render");
            let (_, status_rect) = rects
                .iter()
                .find(|(t, _)| t.contains(status_needle))
                .unwrap_or_else(|| {
                    panic!("status '{status_needle}' should render in state {label}")
                });

            assert!(
                check_rect.right() + 4.0 <= check_btn_rect.left(),
                "[{label}] checkbox overlapped check button: {check_rect:?} vs {check_btn_rect:?}"
            );
            assert!(
                check_rect.bottom() <= status_rect.top(),
                "[{label}] update status must render on its own row below checkbox: {check_rect:?} vs {status_rect:?}"
            );
            assert!(
                status_rect.right() + 4.0 <= dl_page_rect.left(),
                "[{label}] status text overlapped open-download button: {status_rect:?} vs {dl_page_rect:?}"
            );
            for btn_label in ["重启完成更新", "重试自动更新", "自动更新"] {
                if let Some((_, action_btn_rect)) = rects.iter().find(|(t, _)| t == btn_label) {
                    assert!(
                        status_rect.right() + 4.0 <= action_btn_rect.left(),
                        "[{label}] status text overlapped '{btn_label}': {status_rect:?} vs {action_btn_rect:?}"
                    );
                    assert!(
                        action_btn_rect.right() + 4.0 <= dl_page_rect.left(),
                        "[{label}] '{btn_label}' overlapped '前往下载': {action_btn_rect:?} vs {dl_page_rect:?}"
                    );
                }
            }
        }
    }

    #[test]
    fn english_settings_view_renders_localized_labels_without_overlap() {
        crate::i18n::with_test_language(crate::i18n::ResolvedLanguage::EnUs, || {
            let ctx = egui::Context::default();
            let palette = EarthPalette::resolve(ThemeMode::Light, false);
            palette.apply_to_ctx(&ctx);

            let rel = ReleaseInfo {
                version: "0.5.0".into(),
                html_url: "https://github.com/lumingbao/Earth/releases/tag/v0.5.0".into(),
                tag_name: "v0.5.0".into(),
                download_url: Some(
                    "https://github.com/lumingbao/Earth/releases/download/v0.5.0/Earth-0.5.0-macOS-universal.dmg"
                        .into(),
                ),
                asset_name: Some("Earth-0.5.0-macOS-universal.dmg".into()),
                checksum_url: Some(
                    "https://github.com/lumingbao/Earth/releases/download/v0.5.0/SHA256SUMS.txt"
                        .into(),
                ),
            };

            let mut settings = AppSettings::default();
            let mut state = SettingsState {
                active_tab: SettingsTab::General,
                available_update: Some(rel),
                update_download: UpdateDownloadState::Downloading {
                    version: "0.5.0".into(),
                },
                ..Default::default()
            };
            let raw = egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::pos2(0.0, 0.0),
                    vec2(424.0, 520.0),
                )),
                ..Default::default()
            };
            let mut out = ctx.run_ui(raw, |ui| {
                let ctx = ui.ctx().clone();
                egui::CentralPanel::default()
                    .frame(egui::Frame::NONE)
                    .show(ui, |ui| {
                        let _ = render_settings_view(
                            ui,
                            &ctx,
                            SettingsViewData {
                                settings: &mut settings,
                                collections: &[],
                                palette: &palette,
                                storage_bytes: 0,
                                item_count: 0,
                                has_accessibility: false,
                            },
                            &mut state,
                        );
                    });
            });
            out.textures_delta.clear();

            let mut rects = Vec::new();
            fn walk(shape: &egui::epaint::Shape, out: &mut Vec<(String, egui::Rect)>) {
                match shape {
                    egui::epaint::Shape::Text(t) => {
                        out.push((t.galley.text().to_string(), t.visual_bounding_rect()));
                    }
                    egui::epaint::Shape::Vec(v) => {
                        for child in v {
                            walk(child, out);
                        }
                    }
                    _ => {}
                }
            }
            for clipped in &out.shapes {
                walk(&clipped.shape, &mut rects);
            }

            assert!(rects.iter().any(|(t, _)| t == "Earth Preferences"));
            assert!(rects.iter().any(|(t, _)| t == "Language:"));

            let tab_labels = ["General", "Storage", "Appearance", "Privacy", "Advanced"];
            let tab_rects: Vec<egui::Rect> = tab_labels
                .iter()
                .map(|label| {
                    rects
                        .iter()
                        .find(|(t, _)| t == label)
                        .unwrap_or_else(|| panic!("tab '{label}' should render"))
                        .1
                })
                .collect();
            for pair in tab_rects.windows(2) {
                assert!(
                    pair[0].right() + 6.0 <= pair[1].left(),
                    "English settings tabs collided or were cramped: {:?} vs {:?}",
                    pair[0],
                    pair[1]
                );
            }
            assert!(
                tab_rects.last().unwrap().right() <= 424.0,
                "Last English tab overflowed viewport: {:?}",
                tab_rects.last()
            );

            let (_, check_rect) = rects
                .iter()
                .find(|(t, _)| t.contains("Check for updates"))
                .expect("English checkbox should render");
            let (_, check_btn_rect) = rects
                .iter()
                .find(|(t, _)| t == "Check Now")
                .expect("English Check Now button should render");
            let (_, dl_page_rect) = rects
                .iter()
                .find(|(t, _)| t == "Download")
                .expect("English Download button should render");
            let (_, status_rect) = rects
                .iter()
                .find(|(t, _)| t.contains("⬇ Downloading v0.5.0"))
                .expect("English downloading status should render");

            assert!(
                check_rect.right() + 4.0 <= check_btn_rect.left(),
                "English checkbox overlapped Check Now button: {check_rect:?} vs {check_btn_rect:?}"
            );
            assert!(
                check_rect.bottom() <= status_rect.top(),
                "English status must render below checkbox: {check_rect:?} vs {status_rect:?}"
            );
            assert!(
                status_rect.right() + 4.0 <= dl_page_rect.left(),
                "English status overlapped Download button: {status_rect:?} vs {dl_page_rect:?}"
            );

            #[cfg(target_os = "macos")]
            {
                let (_, warn_rect) = rects
                    .iter()
                    .find(|(t, _)| t.contains("Accessibility permission required"))
                    .expect("Accessibility warning should render on macOS");
                let (_, open_btn_rect) = rects
                    .iter()
                    .find(|(t, _)| t == "Open System Settings")
                    .expect("Open System Settings button should render on macOS");
                assert!(
                    warn_rect.bottom() <= open_btn_rect.top(),
                    "Open System Settings button should sit cleanly below warning text: {warn_rect:?} vs {open_btn_rect:?}"
                );
                assert!(
                    open_btn_rect.right() <= 390.0,
                    "Open System Settings button overflowed card bounds: {open_btn_rect:?}"
                );
            }

            let reset_xs: Vec<f32> = rects
                .iter()
                .filter(|(t, _)| t == "Reset")
                .map(|(_, r)| r.left())
                .collect();
            if reset_xs.len() >= 2 {
                let first_x = reset_xs[0];
                for x in &reset_xs[1..] {
                    assert!(
                        (x - first_x).abs() <= 1.0,
                        "Shortcut Reset buttons should align in a straight column: {reset_xs:?}"
                    );
                }
            }
        });
    }
}
