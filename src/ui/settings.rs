use crate::hotkey::{capture_egui_shortcut, format_shortcut_display};
use crate::model::{
    format_bytes, AppSettings, Collection, MatchHighlight, PinPosition, PopupPosition,
    SearchBoxVisibility, SearchMode, SortOrder, ThemeMode,
};
use crate::ui::theme::{
    compact_button, dismiss_button, padded_button, parse_hex_color32, EarthPalette,
};
use egui::{vec2, Align, CornerRadius, Layout, RichText, ScrollArea, Stroke, Ui};

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
    ClearHistory(bool),
    ExportJson,
    CreateCollection { name: String, color_hex: String },
    RenameCollection { id: i64, name: String },
    DeleteCollection(i64),
    OpenAccessibilitySettings,
}

pub struct SettingsState {
    pub active_tab: SettingsTab,
    pub ignore_sub_tab: IgnoreSubTab,
    pub recording: Option<RecordingTarget>,
    pub new_ignore_app: String,
    pub new_ignore_type: String,
    pub new_ignore_regex: String,
    pub new_collection_name: String,
    pub new_collection_color: String,
    pub editing_collection_id: Option<i64>,
    pub editing_collection_name: String,
    pub status_message: Option<(String, bool)>,
}

impl Default for SettingsState {
    fn default() -> Self {
        Self {
            active_tab: SettingsTab::General,
            ignore_sub_tab: IgnoreSubTab::Apps,
            recording: None,
            new_ignore_app: String::new(),
            new_ignore_type: String::new(),
            new_ignore_regex: String::new(),
            new_collection_name: String::new(),
            new_collection_color: "#1677FF".to_string(),
            editing_collection_id: None,
            editing_collection_name: String::new(),
            status_message: None,
        }
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
    if let Some(target) = state.recording {
        let (esc_pressed, captured) = ctx.input(|input| {
            (
                input.key_pressed(egui::Key::Escape),
                capture_egui_shortcut(input),
            )
        });
        if esc_pressed {
            state.recording = None;
        } else if let Some(combo) = captured {
            match target {
                RecordingTarget::Open => {
                    settings.hotkey_open = combo.clone();
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
        let back_btn = egui::Button::new(RichText::new("← 返回剪贴板").size(12.5))
            .fill(palette.surface_bg)
            .stroke(Stroke::new(1.0, palette.border))
            .corner_radius(CornerRadius::same(6));
        if ui.add(back_btn).clicked() {
            actions.push(SettingsAction::CloseSettings);
        }

        let drag_resp = ui.allocate_response(
            vec2((ui.available_width() - 90.0).max(40.0), 28.0),
            egui::Sense::drag(),
        );
        if drag_resp.drag_started() {
            ctx.send_viewport_cmd(egui::ViewportCommand::StartDrag);
        }
        ui.painter().text(
            drag_resp.rect.center(),
            egui::Align2::CENTER_CENTER,
            "Earth 偏好设置",
            egui::FontId::proportional(14.5),
            palette.text_primary,
        );

        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            ui.label(
                RichText::new("v0.1.0")
                    .size(11.5)
                    .color(palette.text_muted),
            );
        });
    });

    ui.add_space(6.0);

    // Ant Design Line Tabs (no background fill, primary text + 2px bottom ink bar)
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 5.0;
        let tabs = [
            (SettingsTab::General, "通用"),
            (SettingsTab::Storage, "存储"),
            (SettingsTab::Appearance, "外观"),
            (SettingsTab::Ignore, "忽略与隐私"),
            (SettingsTab::Advanced, "高级与收藏"),
        ];
        let tab_width = (ui.available_width() - 20.0) / tabs.len() as f32;
        for (tab, label) in tabs {
            let active = state.active_tab == tab;
            let (rect, resp) = ui.allocate_exact_size(vec2(tab_width, 30.0), egui::Sense::click());
            let text_color = if active {
                palette.accent
            } else if resp.hovered() {
                palette.accent_hover
            } else {
                palette.text_secondary
            };
            ui.painter().text(
                rect.center(),
                egui::Align2::CENTER_CENTER,
                label,
                egui::FontId::proportional(12.2),
                text_color,
            );
            if active {
                ui.painter().line_segment(
                    [
                        egui::pos2(rect.left() + 6.0, rect.bottom() - 1.5),
                        egui::pos2(rect.right() - 6.0, rect.bottom() - 1.5),
                    ],
                    Stroke::new(2.0, palette.accent),
                );
            }
            if resp.clicked() {
                state.active_tab = tab;
                state.recording = None;
            }
        }
    });

    ui.add_space(6.0);
    ui.separator();
    ui.add_space(6.0);

    let mut clear_status = false;
    if let Some((msg, is_err)) = state.status_message.clone() {
        let (bg, border) = if is_err {
            (palette.danger_bg, palette.danger_border)
        } else {
            (palette.success_bg, palette.success_border)
        };
        egui::Frame::NONE
            .fill(bg)
            .stroke(Stroke::new(1.0, border))
            .corner_radius(CornerRadius::same(8))
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
        ui.add_space(6.0);
    }
    if clear_status {
        state.status_message = None;
    }

    ScrollArea::vertical()
        .auto_shrink([false, false])
        .show(ui, |ui| match state.active_tab {
            SettingsTab::General => {
                render_general_tab(ui, settings, state, palette, has_accessibility, &mut actions);
            }
            SettingsTab::Storage => {
                render_storage_tab(ui, settings, palette, storage_bytes, item_count, &mut actions);
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
        if ui
            .checkbox(&mut settings.launch_at_login, "登录时启动 Earth")
            .changed()
        {
            actions.push(SettingsAction::ToggleAutostart(settings.launch_at_login));
        }
        if ui
            .checkbox(&mut settings.check_updates, "启用更新提示检查")
            .changed()
        {
            actions.push(SettingsAction::SettingsChanged);
        }

        #[cfg(target_os = "macos")]
        {
            ui.add_space(4.0);
            ui.horizontal(|ui| {
                if has_accessibility {
                    ui.label(
                        RichText::new("✓ macOS 辅助功能权限已就绪（支持自动粘贴）")
                            .size(12.0)
                            .color(palette.success_text),
                    );
                } else {
                    ui.label(
                        RichText::new("⚠ 未授予辅助功能权限，自动粘贴需要授权")
                            .size(12.0)
                            .color(palette.warning_text),
                    );
                    let auth_btn = egui::Button::new(
                        RichText::new("前往系统设置授权").color(palette.on_accent),
                    )
                    .fill(palette.accent)
                    .stroke(Stroke::NONE)
                    .corner_radius(CornerRadius::same(6));
                    if ui.add(auth_btn).clicked() {
                        actions.push(SettingsAction::OpenAccessibilitySettings);
                    }
                }
            });
            if !has_accessibility {
                ui.label(
                    RichText::new("若系统设置中已开启开关但此处仍提示未授权，是因为版本更新后签名刷新：请在系统设置「辅助功能」中选中 Earth 点击「-」移除后重新添加（或开关一次）。")
                        .size(11.0)
                        .color(palette.text_muted),
                );
            }
        }
        #[cfg(not(target_os = "macos"))]
        let _ = has_accessibility;
    });

    ui.add_space(8.0);
    section_card(ui, palette, |ui| {
        ui.label(
            RichText::new("快捷键（点击右侧按键框后直接按下新组合键）")
                .size(12.0)
                .color(palette.text_secondary),
        );
        ui.add_space(4.0);

        let default_open = AppSettings::default().hotkey_open;
        let default_pin = AppSettings::default().hotkey_pin;
        let default_del = AppSettings::default().hotkey_delete;
        let default_prev = AppSettings::default().hotkey_preview;

        if shortcut_row(
            ui,
            "打开 Earth：",
            &mut settings.hotkey_open,
            &default_open,
            RecordingTarget::Open,
            state,
            palette,
        ) {
            actions.push(SettingsAction::OpenHotkeyChanged(
                settings.hotkey_open.clone(),
            ));
        }
        if shortcut_row(
            ui,
            "固定记录：",
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
            "删除记录：",
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
            "展开速览：",
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
            ui.label("搜索匹配模式：");
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
            .checkbox(&mut settings.auto_paste, "选择后自动粘贴到原应用")
            .changed()
        {
            actions.push(SettingsAction::SettingsChanged);
        }
        if ui
            .checkbox(&mut settings.plain_text_paste, "默认以纯文本格式粘贴（去除富文本样式）")
            .changed()
        {
            actions.push(SettingsAction::SettingsChanged);
        }

        ui.add_space(4.0);
        #[cfg(target_os = "macos")]
        let hint = "• 按住 ⌥ 并点击/回车：仅复制到剪贴板\n• 按住 ⌘ 并点击/回车：复制并立即粘贴\n• 按住 ⇧⌘ 并点击/回车：复制并以纯文本匹配格式粘贴";
        #[cfg(not(target_os = "macos"))]
        let hint = "• 按住 Alt 并点击/回车：仅复制到剪贴板\n• 直接点击/回车（或 Ctrl+Enter）：复制并按默认方式粘贴\n• 按住 Ctrl+Shift 并点击/回车：复制并以纯文本匹配格式粘贴";

        ui.label(RichText::new(hint).size(11.5).color(palette.text_muted));
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
        ui.allocate_ui(vec2(86.0, 24.0), |ui| {
            ui.label(label);
        });

        let text = if is_recording {
            "请按下组合键 (Esc 取消)...".to_string()
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
        let btn = egui::Button::new(
            RichText::new(text)
                .size(12.0)
                .color(if is_recording {
                    palette.accent_text
                } else {
                    palette.text_primary
                }),
        )
        .min_size(vec2(170.0, 26.0))
        .fill(fill)
        .stroke(stroke)
        .corner_radius(CornerRadius::same(6));
        if ui.add(btn).clicked() {
            state.recording = Some(target);
        }

        if compact_button(ui, "重置")
            .on_hover_text("恢复默认快捷键")
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
        ui.label(RichText::new("保存以下剪贴板类型：").strong());
        ui.add_space(4.0);
        if ui.checkbox(&mut settings.save_files, "文件").changed() {
            actions.push(SettingsAction::SettingsChanged);
        }
        if ui.checkbox(&mut settings.save_images, "图像").changed() {
            actions.push(SettingsAction::SettingsChanged);
        }
        if ui.checkbox(&mut settings.save_text, "文本").changed() {
            actions.push(SettingsAction::SettingsChanged);
        }
        ui.label(
            RichText::new("仅记录勾选类型的复制内容（已固定和已加入收藏分组的记录不会被自动淘汰）。")
                .size(11.5)
                .color(palette.text_muted),
        );
    });

    ui.add_space(8.0);
    section_card(ui, palette, |ui| {
        ui.horizontal(|ui| {
            ui.label("历史上限数量：");
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
            ui.label(
                RichText::new(format!(
                    "当前 {item_count} 条 · 占用 {}",
                    format_bytes(storage_bytes)
                ))
                .size(12.0)
                .color(palette.text_secondary),
            );
        });

        ui.add_space(6.0);
        ui.horizontal(|ui| {
            ui.label("默认排序方式：");
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
        ui.horizontal(|ui| {
            if ui.button("清除未固定").clicked() {
                actions.push(SettingsAction::ClearHistory(false));
            }
            if ui
                .add(
                    egui::Button::new(RichText::new("清空全部（含固定）").color(palette.danger))
                        .fill(palette.surface_bg)
                        .stroke(Stroke::new(1.0, palette.danger_border))
                        .corner_radius(CornerRadius::same(6)),
                )
                .clicked()
            {
                actions.push(SettingsAction::ClearHistory(true));
            }
            if ui.button("导出 JSON...").clicked() {
                actions.push(SettingsAction::ExportJson);
            }
        });
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
            ui.label("主题外观：");
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
            ui.label("弹窗位置：");
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
            ui.label("固定位置：");
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
            ui.label("列表缩略图高度：");
            if ui
                .add(
                    egui::DragValue::new(&mut settings.image_height)
                        .range(24.0..=110.0)
                        .suffix(" px"),
                )
                .changed()
            {
                actions.push(SettingsAction::SettingsChanged);
            }
        });

        ui.horizontal(|ui| {
            ui.label("高亮搜索匹配：");
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
            .checkbox(&mut settings.show_special_symbols, "显示特殊空白符号（· ⇥ ↵）")
            .changed();
        changed |= ui
            .checkbox(&mut settings.show_tray_icon, "显示菜单栏 / 系统托盘图标")
            .changed();
        changed |= ui
            .checkbox(
                &mut settings.show_clipboard_in_menubar,
                "在菜单栏图标旁 / 托盘提示显示最新剪贴板内容",
            )
            .changed();

        ui.horizontal(|ui| {
            ui.label("搜索框显示时机：");
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
            .checkbox(&mut settings.show_source_app_badge, "显示来源应用程序标签")
            .changed();
        changed |= ui
            .checkbox(&mut settings.show_hex_color_swatch, "显示十六进制颜色色块")
            .changed();
        changed |= ui
            .checkbox(&mut settings.show_footer_menu, "显示底部操作栏")
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
                "启用敏感信息自动脱敏遮罩（API Key / JWT / Bearer Token / 私钥）",
            )
            .changed()
        {
            actions.push(SettingsAction::SettingsChanged);
        }
        ui.label(
            RichText::new("开启后，疑似密钥的内容在列表中会以 •••• 遮罩，需在右侧预览面板点击「显示明文」查看。")
                .size(11.5)
                .color(palette.text_muted),
        );
    });

    ui.add_space(8.0);
    section_card(ui, palette, |ui| {
        ui.horizontal(|ui| {
            for (sub, title) in [
                (IgnoreSubTab::Apps, "忽略应用"),
                (IgnoreSubTab::ClipboardTypes, "忽略剪贴板类型"),
                (IgnoreSubTab::Regexes, "正则表达式"),
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
                        "白名单模式：忽略列表之外的所有应用程序",
                    )
                    .changed()
                {
                    actions.push(SettingsAction::SettingsChanged);
                }
                render_editable_string_list(
                    ui,
                    &mut settings.ignored_apps,
                    &mut state.new_ignore_app,
                    "输入应用名称或 Bundle ID / 进程名（如 1Password、WeChat.exe）",
                    palette,
                    actions,
                );
            }
            IgnoreSubTab::ClipboardTypes => {
                ui.label(
                    RichText::new("匹配系统剪贴板格式标识（macOS UTI / Windows Clipboard Format）时自动跳过：")
                        .size(11.5)
                        .color(palette.text_muted),
                );
                render_editable_string_list(
                    ui,
                    &mut settings.ignored_clipboard_types,
                    &mut state.new_ignore_type,
                    "例如 org.nspasteboard.ConcealedType",
                    palette,
                    actions,
                );
            }
            IgnoreSubTab::Regexes => {
                ui.label(
                    RichText::new("若复制文本匹配下列任意正则表达式，将不录入历史记录：")
                        .size(11.5)
                        .color(palette.text_muted),
                );
                render_editable_string_list(
                    ui,
                    &mut settings.ignored_regexes,
                    &mut state.new_ignore_regex,
                    r"输入正则表达式，例如 ^sk-[A-Za-z0-9]+$",
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
        let add_btn = egui::Button::new(RichText::new("添加").color(palette.on_accent))
            .fill(palette.accent)
            .stroke(Stroke::NONE)
            .corner_radius(CornerRadius::same(6));
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
        .corner_radius(CornerRadius::same(8))
        .inner_margin(egui::Margin::same(6))
        .show(ui, |ui| {
            if list.is_empty() {
                ui.label(
                    RichText::new("暂无规则")
                        .size(12.0)
                        .color(palette.text_muted),
                );
            } else {
                for (idx, entry) in list.iter().enumerate() {
                    ui.horizontal(|ui| {
                        ui.label(RichText::new(entry).size(12.0));
                        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                            if compact_button(ui, "移除").clicked() {
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
            .checkbox(&mut settings.paused, "暂停记录新的剪贴板项目")
            .changed()
        {
            actions.push(SettingsAction::SettingsChanged);
        }
        if ui
            .checkbox(&mut settings.ignore_next_event, "仅忽略下一次复制")
            .changed()
        {
            actions.push(SettingsAction::SettingsChanged);
        }
        ui.separator();
        if ui
            .checkbox(&mut settings.clear_on_exit, "退出 Earth 时清除未固定历史记录")
            .changed()
        {
            actions.push(SettingsAction::SettingsChanged);
        }
        if ui
            .checkbox(
                &mut settings.clear_system_clipboard_too,
                "清除历史时同时清空系统剪贴板",
            )
            .changed()
        {
            actions.push(SettingsAction::SettingsChanged);
        }
    });

    ui.add_space(8.0);
    section_card(ui, palette, |ui| {
        ui.label(RichText::new("Earth Collections · 智能收藏分组").strong());
        ui.label(
            RichText::new("将常用指令、代码模板或长期素材放入收藏分组，永不随历史上限淘汰。")
                .size(11.5)
                .color(palette.text_muted),
        );
        ui.add_space(4.0);

        ui.horizontal(|ui| {
            ui.add(
                egui::TextEdit::singleline(&mut state.new_collection_name)
                    .hint_text("新分组名称（如：SQL 模板）")
                    .desired_width(132.0),
            );
            for hex in ["#1677FF", "#13C2C2", "#52C41A", "#722ED1", "#FA8C16"] {
                let c = parse_hex_color32(hex).unwrap_or(palette.accent);
                let selected = state.new_collection_color == hex;
                let (rect, resp) = ui.allocate_exact_size(vec2(18.0, 18.0), egui::Sense::click());
                ui.painter().circle_filled(rect.center(), 7.0, c);
                if selected {
                    ui.painter()
                        .circle_stroke(rect.center(), 9.0, Stroke::new(1.5, palette.text_primary));
                }
                if resp.clicked() {
                    state.new_collection_color = hex.to_string();
                }
            }
            let create_btn = egui::Button::new(
                RichText::new("创建分组").color(palette.on_accent),
            )
            .fill(palette.accent)
            .stroke(Stroke::NONE)
            .corner_radius(CornerRadius::same(6));
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
                        if compact_button(ui, "取消").clicked() {
                            state.editing_collection_id = None;
                            state.editing_collection_name.clear();
                        }
                        let save_btn = egui::Button::new(
                            RichText::new("保存").size(12.0).color(palette.on_accent),
                        )
                        .fill(palette.accent)
                        .stroke(Stroke::NONE)
                        .corner_radius(CornerRadius::same(4));
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
                        if compact_button(ui, RichText::new("删除").color(palette.danger))
                            .on_hover_text("删除分组（不会删除组内记录本身）")
                            .clicked()
                        {
                            actions.push(SettingsAction::DeleteCollection(col.id));
                        }
                        if compact_button(ui, "重命名").clicked() {
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
        .corner_radius(CornerRadius::same(8))
        .inner_margin(egui::Margin::same(12))
        .show(ui, |ui| {
            ui.set_min_width(card_w);
            ui.set_max_width(card_w);
            add_contents(ui);
        });
}
