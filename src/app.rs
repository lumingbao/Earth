use crate::clipboard::{
    clear_system_clipboard, start_clipboard_watcher, write_custom_text_to_clipboard,
    write_item_to_clipboard, ClipboardEvent, ClipboardHandle, ClipboardSuppression,
};
use crate::hotkey::GlobalHotkeyController;
use crate::model::{
    truncate_chars, ActiveAppInfo, AppSettings, ClipItem, Collection, PasteBehavior,
};
use crate::platform;
use crate::search::{filter_and_search, SearchResultItem};
use crate::storage::Storage;
use crate::tray::{EarthTray, TrayAction};
use crate::ui::popup::{render_popup_view, PopupAction, PopupState, PopupViewData};
use crate::ui::settings::{render_settings_view, SettingsAction, SettingsState, SettingsViewData};
use crate::ui::theme::EarthPalette;
use egui::{pos2, vec2, CornerRadius, Stroke, ViewportCommand};
use std::path::PathBuf;
use std::sync::mpsc::Receiver;
use std::sync::{Arc, Mutex, RwLock};
use std::time::{Duration, Instant};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ViewMode {
    Launcher,
    Settings,
}

pub struct EarthApp {
    storage: Storage,
    shared_settings: Arc<RwLock<AppSettings>>,
    settings: AppSettings,
    suppression: Arc<Mutex<ClipboardSuppression>>,
    clipboard_rx: Receiver<ClipboardEvent>,
    _clipboard_handle: Option<ClipboardHandle>,
    hotkey_ctrl: Option<GlobalHotkeyController>,
    tray: Option<EarthTray>,

    items: Vec<ClipItem>,
    collections: Vec<Collection>,
    storage_bytes: u64,
    cached_results: Vec<SearchResultItem>,
    search_dirty: bool,

    view_mode: ViewMode,
    popup_state: PopupState,
    settings_state: SettingsState,

    visible: bool,
    last_shown_at: Instant,
    was_focused_since_shown: bool,
    last_window_size: (f32, f32),
    prev_app: Option<ActiveAppInfo>,
    quitting: bool,
    dialog_open: bool,
}

impl EarthApp {
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        egui_extras::install_image_loaders(&cc.egui_ctx);
        platform::configure_macos_accessory_mode();

        let storage = Storage::open_default().expect("无法初始化 Earth 本地数据库");
        let settings = storage.load_settings();
        let items = storage.list_items().unwrap_or_default();
        let collections = storage.list_collections().unwrap_or_default();
        let storage_bytes = storage.total_storage_bytes();

        let shared_settings = Arc::new(RwLock::new(settings.clone()));
        let suppression = Arc::new(Mutex::new(ClipboardSuppression {
            ignore_next_once: settings.ignore_next_event,
            ..Default::default()
        }));

        let (clip_tx, clipboard_rx) = std::sync::mpsc::channel();
        let clipboard_handle = start_clipboard_watcher(
            Arc::clone(&shared_settings),
            Arc::clone(&suppression),
            clip_tx,
            cc.egui_ctx.clone(),
        )
        .ok();

        let (hotkey_ctrl, hotkey_warn) =
            match GlobalHotkeyController::new(&settings.hotkey_open, cc.egui_ctx.clone()) {
                Ok((ctrl, warn)) => (Some(ctrl), warn),
                Err(err) => (None, Some(err)),
            };

        let tray = EarthTray::new(
            cc.egui_ctx.clone(),
            settings.show_tray_icon,
            settings.paused,
            settings.ignore_next_event,
        )
        .ok();

        let mut popup_state = PopupState::default();
        if let Some(warn) = hotkey_warn {
            popup_state.status_banner = Some((warn, true));
        }

        let initial_size = (470.0, 550.0);
        let prev_app = platform::capture_frontmost_app();
        let (x, y) = platform::compute_popup_origin(
            settings.popup_position,
            initial_size,
            prev_app.as_ref(),
            None,
            cc.egui_ctx.pixels_per_point(),
        );
        cc.egui_ctx
            .send_viewport_cmd(ViewportCommand::OuterPosition(pos2(x, y)));
        cc.egui_ctx
            .send_viewport_cmd(ViewportCommand::InnerSize(vec2(initial_size.0, initial_size.1)));
        cc.egui_ctx.send_viewport_cmd(ViewportCommand::Focus);
        platform::activate_self_window();

        let mut app = Self {
            storage,
            shared_settings,
            settings,
            suppression,
            clipboard_rx,
            _clipboard_handle: clipboard_handle,
            hotkey_ctrl,
            tray,
            items,
            collections,
            storage_bytes,
            cached_results: Vec::new(),
            search_dirty: true,
            view_mode: ViewMode::Launcher,
            popup_state,
            settings_state: SettingsState::default(),
            visible: true,
            last_shown_at: Instant::now(),
            was_focused_since_shown: false,
            last_window_size: initial_size,
            prev_app,
            quitting: false,
            dialog_open: false,
        };
        app.sync_tray();
        app.recompute_search();
        app
    }

    fn reload_items(&mut self) {
        self.items = self.storage.list_items().unwrap_or_default();
        self.collections = self.storage.list_collections().unwrap_or_default();
        self.storage_bytes = self.storage.total_storage_bytes();
        self.search_dirty = true;
        self.sync_tray();
    }

    fn persist_settings(&mut self) {
        if let Ok(mut guard) = self.shared_settings.write() {
            *guard = self.settings.clone();
        }
        if let Ok(mut sup) = self.suppression.lock() {
            sup.ignore_next_once = self.settings.ignore_next_event;
        }
        let _ = self.storage.save_settings(&self.settings);
        let _ = self.storage.enforce_history_limit(self.settings.max_history);
        self.reload_items();
    }

    fn sync_tray(&mut self) {
        let preview = if self.settings.show_clipboard_in_menubar {
            self.items.first().map(|it| truncate_chars(&it.summary_line(24), 22))
        } else {
            None
        };
        if let Some(tray) = &mut self.tray {
            tray.sync_state(
                self.settings.show_tray_icon,
                self.settings.paused,
                self.settings.ignore_next_event,
                preview.as_deref(),
            );
        }
    }

    fn desired_window_size(&self) -> (f32, f32) {
        match self.view_mode {
            ViewMode::Launcher => {
                if self.popup_state.preview_open {
                    (860.0, 560.0)
                } else {
                    (470.0, 550.0)
                }
            }
            ViewMode::Settings => (680.0, 570.0),
        }
    }

    fn sync_window_size(&mut self, ctx: &egui::Context) {
        let target = self.desired_window_size();
        if (target.0 - self.last_window_size.0).abs() > 1.0
            || (target.1 - self.last_window_size.1).abs() > 1.0
        {
            self.last_window_size = target;
            ctx.send_viewport_cmd(ViewportCommand::InnerSize(vec2(target.0, target.1)));
        }
    }

    fn show_popup(&mut self, ctx: &egui::Context, mode: ViewMode, tray_pos: Option<(f32, f32)>) {
        if let Some(active) = platform::capture_frontmost_app() {
            self.prev_app = Some(active);
        }
        self.view_mode = mode;
        if mode == ViewMode::Launcher {
            self.popup_state.request_search_focus = true;
            self.popup_state.selection_changed_at = Instant::now();
            self.popup_state.preview_manually_toggled = false;
            self.popup_state.preview_open = false;
            self.popup_state.active_transform = None;
        }

        let target_size = self.desired_window_size();
        self.last_window_size = target_size;
        let resolved_tray = tray_pos.or_else(|| self.tray.as_ref().and_then(|t| t.last_tray_pos()));
        let (x, y) = platform::compute_popup_origin(
            self.settings.popup_position,
            target_size,
            self.prev_app.as_ref(),
            resolved_tray,
            ctx.pixels_per_point(),
        );

        ctx.send_viewport_cmd(ViewportCommand::InnerSize(vec2(target_size.0, target_size.1)));
        ctx.send_viewport_cmd(ViewportCommand::OuterPosition(pos2(x, y)));
        ctx.send_viewport_cmd(ViewportCommand::Visible(true));
        ctx.send_viewport_cmd(ViewportCommand::Focus);
        platform::activate_self_window();

        self.visible = true;
        self.last_shown_at = Instant::now();
        self.was_focused_since_shown = false;
        ctx.request_repaint();
    }

    fn hide_popup(&mut self, ctx: &egui::Context, restore_focus: bool) {
        self.visible = false;
        self.popup_state.show_about_modal = false;
        self.settings_state.recording = None;
        ctx.send_viewport_cmd(ViewportCommand::Visible(false));
        if restore_focus {
            platform::hide_self_app();
            if let Some(ref prev) = self.prev_app {
                platform::restore_frontmost_app(prev);
            }
        }
    }

    fn recompute_search(&mut self) {
        self.cached_results = filter_and_search(
            &self.items,
            &self.popup_state.search_query,
            self.popup_state.filter,
            self.settings.search_mode,
            self.settings.sort_order,
            self.settings.pin_position,
        );
        self.search_dirty = false;
    }

    fn handle_paste_item(&mut self, ctx: &egui::Context, id: i64, behavior: PasteBehavior) {
        let Some(item) = self.items.iter().find(|it| it.id == id).cloned() else {
            return;
        };

        let (do_paste, plain_only) = match behavior {
            PasteBehavior::Default => (self.settings.auto_paste, self.settings.plain_text_paste),
            PasteBehavior::CopyOnly => (false, self.settings.plain_text_paste),
            PasteBehavior::CopyAndPaste => (true, false),
            PasteBehavior::CopyAndPastePlainText => (true, true),
        };

        if let Err(err) = write_item_to_clipboard(&item, plain_only, &self.suppression) {
            self.popup_state.status_banner = Some((err, true));
            return;
        }

        let _ = self.storage.mark_copied(id);
        self.reload_items();

        if do_paste {
            if !platform::has_accessibility_permission() {
                self.popup_state.status_banner = Some((
                    "已复制到剪贴板，但自动粘贴需要 macOS「辅助功能」权限（请按 ⌘V 粘贴）"
                        .to_string(),
                    true,
                ));
                return;
            }
            self.hide_popup(ctx, true);
            std::thread::spawn(|| {
                if let Err(err) = platform::simulate_paste() {
                    eprintln!("自动粘贴失败: {err}");
                }
            });
        } else {
            self.hide_popup(ctx, true);
        }
    }

    fn handle_paste_transformed(&mut self, ctx: &egui::Context, text: String, paste_now: bool) {
        if let Err(err) = write_custom_text_to_clipboard(&text, &self.suppression) {
            self.popup_state.status_banner = Some((err, true));
            return;
        }
        if paste_now {
            if !platform::has_accessibility_permission() {
                self.popup_state.status_banner = Some((
                    "已复制加工结果，请授权限后自动粘贴或直接 ⌘V".to_string(),
                    true,
                ));
                return;
            }
            self.hide_popup(ctx, true);
            std::thread::spawn(|| {
                if let Err(err) = platform::simulate_paste() {
                    eprintln!("自动粘贴失败: {err}");
                }
            });
        } else {
            self.popup_state.status_banner = Some(("✓ 已复制加工结果到剪贴板".to_string(), false));
        }
    }

    fn request_quit(&mut self, ctx: &egui::Context) {
        if self.settings.clear_on_exit {
            let _ = self.storage.clear_history(false);
            if self.settings.clear_system_clipboard_too {
                let _ = clear_system_clipboard();
            }
        }
        self.quitting = true;
        ctx.send_viewport_cmd(ViewportCommand::Close);
    }

    fn process_background_events(&mut self, ctx: &egui::Context) {
        let mut captured_any = false;
        while let Ok(ev) = self.clipboard_rx.try_recv() {
            match ev {
                ClipboardEvent::NewCapture(payload) => {
                    let _ = self
                        .storage
                        .upsert_item(*payload, self.settings.max_history);
                    captured_any = true;
                }
                ClipboardEvent::ConsumedIgnoreNext => {
                    self.settings.ignore_next_event = false;
                    let _ = self.storage.save_settings(&self.settings);
                    if let Ok(mut guard) = self.shared_settings.write() {
                        guard.ignore_next_event = false;
                    }
                    self.sync_tray();
                }
            }
        }
        if captured_any {
            self.reload_items();
        }

        if let Some(ctrl) = &self.hotkey_ctrl
            && ctrl.poll_open_triggered() {
                if self.visible && self.last_shown_at.elapsed() > Duration::from_millis(250) {
                    self.hide_popup(ctx, true);
                } else {
                    self.show_popup(ctx, ViewMode::Launcher, None);
                }
            }

        let dpi = ctx.pixels_per_point();
        let tray_actions = self
            .tray
            .as_mut()
            .map(|t| t.poll_actions(dpi))
            .unwrap_or_default();

        for action in tray_actions {
            match action {
                TrayAction::TogglePopup(pos) => {
                    if self.visible && self.last_shown_at.elapsed() > Duration::from_millis(250) {
                        self.hide_popup(ctx, true);
                    } else {
                        self.show_popup(ctx, ViewMode::Launcher, pos);
                    }
                }
                TrayAction::TogglePause => {
                    self.settings.paused = !self.settings.paused;
                    self.persist_settings();
                }
                TrayAction::IgnoreNextEvent => {
                    self.settings.ignore_next_event = !self.settings.ignore_next_event;
                    self.persist_settings();
                }
                TrayAction::OpenSettings => {
                    self.show_popup(ctx, ViewMode::Settings, None);
                }
                TrayAction::ClearHistory => {
                    let _ = self.storage.clear_history(false);
                    if self.settings.clear_system_clipboard_too {
                        let _ = clear_system_clipboard();
                    }
                    self.reload_items();
                }
                TrayAction::Quit => {
                    self.request_quit(ctx);
                }
            }
        }
    }
}

impl eframe::App for EarthApp {
    fn logic(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.process_background_events(ctx);

        // Convert OS close button into hide unless user explicitly clicked Quit
        if ctx.input(|i| i.viewport().close_requested()) && !self.quitting {
            ctx.send_viewport_cmd(ViewportCommand::CancelClose);
            self.hide_popup(ctx, true);
        }

        // Track focus & hide cleanly on blur when clicking another app
        if self.visible {
            let focused = ctx.input(|i| i.focused);
            if focused {
                self.was_focused_since_shown = true;
            } else if self.was_focused_since_shown
                && self.view_mode == ViewMode::Launcher
                && !self.popup_state.show_about_modal
                && !self.dialog_open
                && self.last_shown_at.elapsed() > Duration::from_millis(320)
            {
                self.hide_popup(ctx, false);
            }
        }
    }

    fn clear_color(&self, _visuals: &egui::Visuals) -> [f32; 4] {
        [0.0, 0.0, 0.0, 0.0]
    }

    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        if !self.visible {
            return;
        }

        let system_dark = ctx
            .system_theme()
            .map(|t| t == egui::Theme::Dark)
            .unwrap_or_else(|| ctx.theme() == egui::Theme::Dark);
        let palette = EarthPalette::resolve(self.settings.theme_mode, system_dark);
        palette.apply_to_ctx(&ctx);

        let has_accessibility = platform::has_accessibility_permission();

        egui::CentralPanel::default()
            .frame(egui::Frame::NONE)
            .show(ui, |ui| {
                egui::Frame::NONE
                    .fill(palette.window_bg)
                    .stroke(Stroke::new(1.0, palette.border))
                    .corner_radius(CornerRadius::same(14))
                    .inner_margin(egui::Margin::same(12))
                    .show(ui, |ui| {
                        ui.set_width(ui.available_width());
                        ui.set_height(ui.available_height());

                        match self.view_mode {
                            ViewMode::Launcher => {
                                let prev_query = self.popup_state.search_query.clone();
                                let prev_filter = self.popup_state.filter;
                                if self.search_dirty {
                                    self.recompute_search();
                                }
                                let results = self.cached_results.clone();
                                let collections = self.collections.clone();
                                let total_items = self.items.len();

                                let actions = render_popup_view(
                                    ui,
                                    &ctx,
                                    PopupViewData {
                                        settings: &mut self.settings,
                                        results: &results,
                                        collections: &collections,
                                        palette: &palette,
                                        storage_bytes: self.storage_bytes,
                                        total_items,
                                        has_accessibility,
                                    },
                                    &mut self.popup_state,
                                );

                                if self.popup_state.search_query != prev_query
                                    || self.popup_state.filter != prev_filter
                                {
                                    self.search_dirty = true;
                                }

                                for action in actions {
                                    match action {
                                        PopupAction::HidePopup => {
                                            self.hide_popup(&ctx, true);
                                        }
                                        PopupAction::PasteItem { id, behavior } => {
                                            self.handle_paste_item(&ctx, id, behavior);
                                        }
                                        PopupAction::CopyTransformedText(text) => {
                                            self.handle_paste_transformed(&ctx, text, false);
                                        }
                                        PopupAction::PasteTransformedText(text) => {
                                            self.handle_paste_transformed(&ctx, text, true);
                                        }
                                        PopupAction::TogglePin(id) => {
                                            if let Some(it) = self.items.iter().find(|x| x.id == id) {
                                                let _ = self.storage.set_pinned(id, !it.pinned);
                                                self.reload_items();
                                            }
                                        }
                                        PopupAction::SetCollection {
                                            item_id,
                                            collection_id,
                                        } => {
                                            let _ = self
                                                .storage
                                                .set_collection(item_id, collection_id);
                                            self.reload_items();
                                        }
                                        PopupAction::DeleteItem(id) => {
                                            let _ = self.storage.delete_item(id);
                                            self.reload_items();
                                        }
                                        PopupAction::ClearUnpinned => {
                                            let _ = self.storage.clear_history(false);
                                            if self.settings.clear_system_clipboard_too {
                                                let _ = clear_system_clipboard();
                                            }
                                            self.reload_items();
                                        }
                                        PopupAction::OpenSettings => {
                                            self.view_mode = ViewMode::Settings;
                                        }
                                        PopupAction::TogglePause => {
                                            self.settings.paused = !self.settings.paused;
                                            self.persist_settings();
                                        }
                                        PopupAction::ToggleIgnoreNext => {
                                            self.settings.ignore_next_event =
                                                !self.settings.ignore_next_event;
                                            self.persist_settings();
                                        }
                                        PopupAction::SettingsChanged => {
                                            self.persist_settings();
                                        }
                                        PopupAction::SaveImageAs(src_path) => {
                                            self.dialog_open = true;
                                            if let Some(dest) = rfd::FileDialog::new()
                                                .set_file_name("earth_image.png")
                                                .add_filter("PNG Image", &["png"])
                                                .save_file()
                                            {
                                                if let Err(e) = std::fs::copy(&src_path, &dest) {
                                                    self.popup_state.status_banner =
                                                        Some((format!("保存图片失败: {e}"), true));
                                                } else {
                                                    self.popup_state.status_banner = Some((
                                                        format!("✓ 已保存到 {}", dest.display()),
                                                        false,
                                                    ));
                                                }
                                            }
                                            self.dialog_open = false;
                                        }
                                        PopupAction::RevealFile(path) => {
                                            platform::reveal_in_file_manager(&path);
                                        }
                                        PopupAction::OpenUrl(url) => {
                                            platform::open_url(&url);
                                        }
                                        PopupAction::OpenAccessibilitySettings => {
                                            platform::open_accessibility_settings();
                                        }
                                        PopupAction::Quit => {
                                            self.request_quit(&ctx);
                                        }
                                    }
                                }
                            }
                            ViewMode::Settings => {
                                let collections = self.collections.clone();
                                let item_count = self.items.len();
                                let actions = render_settings_view(
                                    ui,
                                    &ctx,
                                    SettingsViewData {
                                        settings: &mut self.settings,
                                        collections: &collections,
                                        palette: &palette,
                                        storage_bytes: self.storage_bytes,
                                        item_count,
                                        has_accessibility,
                                    },
                                    &mut self.settings_state,
                                );

                                for action in actions {
                                    match action {
                                        SettingsAction::CloseSettings => {
                                            self.view_mode = ViewMode::Launcher;
                                            self.popup_state.request_search_focus = true;
                                        }
                                        SettingsAction::SettingsChanged => {
                                            self.persist_settings();
                                        }
                                        SettingsAction::OpenHotkeyChanged(new_combo) => {
                                            if let Some(ctrl) = &mut self.hotkey_ctrl {
                                                match ctrl.set_open_shortcut(&new_combo) {
                                                    Ok(()) => {
                                                        self.settings_state.status_message = Some((
                                                            "✓ 全局唤醒快捷键已生效".to_string(),
                                                            false,
                                                        ));
                                                    }
                                                    Err(e) => {
                                                        self.settings_state.status_message =
                                                            Some((e, true));
                                                    }
                                                }
                                            }
                                            self.persist_settings();
                                        }
                                        SettingsAction::ToggleAutostart(enabled) => {
                                            match platform::sync_autostart(enabled) {
                                                Ok(()) => {
                                                    self.settings_state.status_message = Some((
                                                        if enabled {
                                                            "✓ 已开启登录自启"
                                                        } else {
                                                            "✓ 已关闭登录自启"
                                                        }
                                                        .to_string(),
                                                        false,
                                                    ));
                                                }
                                                Err(e) => {
                                                    self.settings.launch_at_login = !enabled;
                                                    self.settings_state.status_message =
                                                        Some((e, true));
                                                }
                                            }
                                            self.persist_settings();
                                        }
                                        SettingsAction::ClearHistory(include_pinned) => {
                                            let _ = self.storage.clear_history(include_pinned);
                                            if self.settings.clear_system_clipboard_too {
                                                let _ = clear_system_clipboard();
                                            }
                                            self.reload_items();
                                            self.settings_state.status_message = Some((
                                                "✓ 历史记录已清理".to_string(),
                                                false,
                                            ));
                                        }
                                        SettingsAction::ExportJson => {
                                            self.dialog_open = true;
                                            if let Some(dest) = rfd::FileDialog::new()
                                                .set_file_name("earth_clipboard_backup.json")
                                                .add_filter("JSON", &["json"])
                                                .save_file()
                                            {
                                                let res = serde_json::to_string_pretty(&self.items)
                                                    .map_err(|e| e.to_string())
                                                    .and_then(|json| {
                                                        std::fs::write(&dest, json)
                                                            .map_err(|e| e.to_string())
                                                    });
                                                match res {
                                                    Ok(()) => {
                                                        self.settings_state.status_message = Some((
                                                            format!(
                                                                "✓ 已导出至 {}",
                                                                PathBuf::from(&dest).display()
                                                            ),
                                                            false,
                                                        ));
                                                    }
                                                    Err(e) => {
                                                        self.settings_state.status_message =
                                                            Some((format!("导出失败: {e}"), true));
                                                    }
                                                }
                                            }
                                            self.dialog_open = false;
                                        }
                                        SettingsAction::CreateCollection { name, color_hex } => {
                                            match self.storage.create_collection(&name, &color_hex)
                                            {
                                                Ok(_) => {
                                                    self.reload_items();
                                                    self.settings_state.status_message = Some((
                                                        format!("✓ 已创建收藏分组「{name}」"),
                                                        false,
                                                    ));
                                                }
                                                Err(e) => {
                                                    self.settings_state.status_message =
                                                        Some((e, true));
                                                }
                                            }
                                        }
                                        SettingsAction::RenameCollection { id, name } => {
                                            match self.storage.rename_collection(id, &name) {
                                                Ok(()) => {
                                                    self.reload_items();
                                                    self.settings_state.status_message = Some((
                                                        format!("✓ 已重命名分组为「{name}」"),
                                                        false,
                                                    ));
                                                }
                                                Err(e) => {
                                                    self.settings_state.status_message =
                                                        Some((e, true));
                                                }
                                            }
                                        }
                                        SettingsAction::DeleteCollection(id) => {
                                            let _ = self.storage.delete_collection(id);
                                            if self.popup_state.filter
                                                == crate::model::CategoryFilter::Collection(id)
                                            {
                                                self.popup_state.filter =
                                                    crate::model::CategoryFilter::All;
                                            }
                                            self.reload_items();
                                        }
                                        SettingsAction::OpenAccessibilitySettings => {
                                            platform::open_accessibility_settings();
                                        }
                                    }
                                }
                            }
                        }
                    });
            });

        self.sync_window_size(&ctx);
    }
}
