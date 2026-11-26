use crate::clipboard::{
    ClipboardEvent, ClipboardHandle, ClipboardSuppression, CompiledCaptureConfig,
    SharedCaptureConfig, clear_system_clipboard, start_clipboard_watcher,
    write_custom_text_to_clipboard, write_item_to_clipboard, write_managed_png,
};
use crate::hotkey::GlobalHotkeyController;
use crate::i18n::{t, tf};
use crate::model::{
    ActiveAppInfo, AppSettings, CategoryFilter, ClipItem, Collection, PasteBehavior, ThemeMode,
    truncate_chars,
};
use crate::platform;
use crate::search::{SearchResultItem, filter_and_search, filter_and_search_with};
use crate::storage::{ImportSummary, Storage};
use crate::tray::{EarthTray, TrayAction};
use crate::ui::popup::{PopupAction, PopupState, PopupViewData, WorkbenchSide, render_popup_view};
use crate::ui::settings::{
    RecordingTarget, SettingsAction, SettingsState, SettingsViewData, render_settings_view,
};
use crate::ui::theme::{EarthPalette, install_crisp_fonts};
use crate::updater::{
    UpdateCheckOutcome, UpdateDownloadState, UpdaterEvent, apply_staged_update_and_restart,
    spawn_update_check, spawn_update_download,
};
use egui::{CornerRadius, Stroke, ViewportCommand, pos2, vec2};
use std::path::PathBuf;
use std::sync::mpsc::{Receiver, Sender};
use std::sync::{Arc, Mutex, RwLock};
use std::time::{Duration, Instant};

const COMPACT_WIDTH: f32 = 424.0;
const EXPANDED_WIDTH: f32 = 760.0;
const WINDOW_HEIGHT: f32 = 520.0;
const WORKBENCH_OPEN_DURATION: Duration = Duration::from_millis(190);
const WORKBENCH_CLOSE_DURATION: Duration = Duration::from_millis(150);

#[derive(Debug, Clone, Copy)]
struct WorkbenchAnimation {
    from: f32,
    to: f32,
    elapsed: f32,
    duration: f32,
    waiting_for_viewport: bool,
    last_tick: Option<Instant>,
}

impl WorkbenchAnimation {
    fn new(from: f32, to: f32, duration: Duration, waiting_for_viewport: bool) -> Self {
        Self {
            from,
            to,
            elapsed: 0.0,
            duration: duration.as_secs_f32().max(0.001),
            waiting_for_viewport,
            last_tick: None,
        }
    }

    /// Advances the workbench animation using frame-paced clamped delta time so an OS
    /// resize/swapchain reallocation hitch never skips the opening frames.
    /// Returns `(reveal, finished)`.
    fn step(&mut self, now: Instant, predicted_dt: f32, viewport_ready: bool) -> (f32, bool) {
        if self.waiting_for_viewport {
            if !viewport_ready {
                self.last_tick = None;
                return (self.from, false);
            }
            self.waiting_for_viewport = false;
        }

        let dt = match self.last_tick {
            Some(prev) => now
                .saturating_duration_since(prev)
                .as_secs_f32()
                .clamp(0.001, 1.0 / 40.0),
            None => predicted_dt.clamp(1.0 / 120.0, 1.0 / 60.0),
        };
        self.last_tick = Some(now);
        self.elapsed = (self.elapsed + dt).min(self.duration);

        let raw_t = (self.elapsed / self.duration).clamp(0.0, 1.0);
        let eased = 1.0 - (1.0 - raw_t).powi(3);
        let reveal = (self.from + (self.to - self.from) * eased).clamp(0.0, 1.0);
        (reveal, raw_t >= 1.0)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ViewMode {
    Launcher,
    Settings,
}

enum BackupEvent {
    Exported(Result<PathBuf, String>),
    Imported(Result<ImportSummary, String>),
}

pub struct EarthApp {
    storage: Storage,
    instance: crate::instance::InstanceGuard,
    paste_tx: Sender<Result<String, String>>,
    paste_rx: Receiver<Result<String, String>>,
    backup_tx: Sender<BackupEvent>,
    backup_rx: Receiver<BackupEvent>,
    shared_capture_config: SharedCaptureConfig,
    settings: AppSettings,
    suppression: Arc<Mutex<ClipboardSuppression>>,
    clipboard_rx: crate::clipboard::ClipboardEvents,
    _clipboard_handle: Option<ClipboardHandle>,
    update_tx: Sender<UpdaterEvent>,
    update_rx: Receiver<UpdaterEvent>,
    hotkey_ctrl: Option<GlobalHotkeyController>,
    tray: Option<EarthTray>,

    history: crate::storage::history::History,
    items: Vec<Arc<ClipItem>>,
    collections: Vec<Collection>,
    storage_bytes: u64,
    cached_results: Vec<SearchResultItem>,
    search_dirty: bool,
    awaiting_newest_selection: bool,

    view_mode: ViewMode,
    popup_state: PopupState,
    settings_state: SettingsState,
    pending_error: Option<String>,

    visible: bool,
    last_shown_at: Instant,
    was_focused_since_shown: bool,
    last_known_focused: bool,
    last_frontmost_poll: Instant,
    last_window_size: (f32, f32),
    last_window_pos: (f32, f32),
    workbench_os_expanded: bool,
    workbench_collapse_pending: bool,
    workbench_anim: Option<WorkbenchAnimation>,
    prev_app: Option<ActiveAppInfo>,
    quitting: bool,
    dialog_open: bool,
    last_applied_theme: Option<(ThemeMode, bool, u32)>,
    cached_palette: EarthPalette,
    cached_accessibility: bool,
    last_accessibility_check: Instant,
}

impl EarthApp {
    pub fn new(
        cc: &eframe::CreationContext<'_>,
        start_hidden: bool,
        storage: Storage,
        mut instance_lock: crate::instance::InstanceGuard,
    ) -> Self {
        egui_extras::install_image_loaders(&cc.egui_ctx);
        install_crisp_fonts(&cc.egui_ctx);
        platform::configure_macos_accessory_mode();
        platform::configure_main_window(cc);
        platform::install_shortcut_repaint_ctx(&cc.egui_ctx);
        platform::install_system_theme_observer(&cc.egui_ctx);

        let (mut settings, settings_warning) = storage.load_settings_with_warning();
        crate::i18n::set_language(settings.language);
        if settings.hotkey_preview == "Ctrl+Space" {
            settings.hotkey_preview = AppSettings::default().hotkey_preview;
        }
        let mut startup_errors: Vec<String> = settings_warning.into_iter().collect();
        if let Err(error) = instance_lock.attach_context(cc.egui_ctx.clone()) {
            startup_errors.push(error);
        }
        let (paste_tx, paste_rx) = std::sync::mpsc::channel();
        let (backup_tx, backup_rx) = std::sync::mpsc::channel();
        if settings.launch_at_login
            && let Err(error) = platform::refresh_autostart_if_enabled()
        {
            startup_errors.push(error);
        }
        let items = storage.first_page().unwrap_or_else(|err| {
            startup_errors.push(err);
            Vec::new()
        });
        let history = crate::storage::history::History::start(&storage, cc.egui_ctx.clone());
        history.refresh(false);
        let collections = storage.list_collections().unwrap_or_else(|err| {
            startup_errors.push(err);
            Vec::new()
        });
        let storage_bytes = 0;

        let shared_capture_config: SharedCaptureConfig = Arc::new(RwLock::new(Arc::new(
            CompiledCaptureConfig::from_settings(settings.clone()),
        )));
        let suppression = Arc::new(Mutex::new(ClipboardSuppression {
            ignore_next_once: settings.ignore_next_event,
            ..Default::default()
        }));

        let (clip_tx, clipboard_rx) = crate::clipboard::clipboard_event_channel();
        let clipboard_handle = match start_clipboard_watcher(
            Arc::clone(&shared_capture_config),
            Arc::clone(&suppression),
            clip_tx,
            cc.egui_ctx.clone(),
        ) {
            Ok(handle) => Some(handle),
            Err(error) => {
                startup_errors.push(error);
                None
            }
        };

        let (update_tx, update_rx) = std::sync::mpsc::channel();
        if settings.check_updates {
            spawn_update_check(false, update_tx.clone(), cc.egui_ctx.clone());
        }

        let (hotkey_ctrl, hotkey_warn) =
            match GlobalHotkeyController::new(&settings.hotkey_open, cc.egui_ctx.clone()) {
                Ok((ctrl, warn)) => (Some(ctrl), warn),
                Err(err) => (None, Some(err)),
            };
        if let Some(error) = hotkey_warn {
            startup_errors.push(error);
        }
        if let Some(ctrl) = &hotkey_ctrl
            && ctrl.is_available()
        {
            settings.hotkey_open = ctrl.open_shortcut().to_string();
        } else {
            // A hidden tray is unsafe when the other entry point did not register.
            settings.show_tray_icon = true;
        }
        if let Ok(mut shared) = shared_capture_config.write() {
            *shared = Arc::new(CompiledCaptureConfig::from_settings(settings.clone()));
        }
        if let Err(error) = storage.save_settings(&settings) {
            startup_errors.push(error);
        }

        let tray = match EarthTray::new(
            cc.egui_ctx.clone(),
            settings.show_tray_icon,
            settings.paused,
            settings.ignore_next_event,
        ) {
            Ok(tray) => Some(tray),
            Err(error) => {
                startup_errors.push(error);
                None
            }
        };

        let mut popup_state = PopupState::default();
        let service_error = (!startup_errors.is_empty()).then(|| startup_errors.join("\n"));
        popup_state.service_error = service_error.clone();
        let settings_state = SettingsState {
            service_error,
            ..Default::default()
        };
        let start_hidden = start_hidden && startup_errors.is_empty();

        let initial_size = (COMPACT_WIDTH, WINDOW_HEIGHT);
        let prev_app = if start_hidden {
            None
        } else {
            cc.egui_ctx
                .send_viewport_cmd(ViewportCommand::Visible(true));
            platform::capture_frontmost_app()
        };
        let (x, y) = platform::compute_popup_origin(
            settings.popup_position,
            initial_size,
            prev_app.as_ref(),
            None,
            cc.egui_ctx.pixels_per_point(),
            cc.egui_ctx.zoom_factor(),
        );
        cc.egui_ctx
            .send_viewport_cmd(ViewportCommand::OuterPosition(pos2(x, y)));
        cc.egui_ctx
            .send_viewport_cmd(ViewportCommand::InnerSize(vec2(
                initial_size.0,
                initial_size.1,
            )));
        if start_hidden {
            cc.egui_ctx
                .send_viewport_cmd(ViewportCommand::Visible(false));
        } else {
            cc.egui_ctx.send_viewport_cmd(ViewportCommand::Focus);
            platform::activate_self_window();
        }

        let initial_system_dark = platform::system_is_dark()
            .or_else(|| cc.egui_ctx.system_theme().map(|t| t == egui::Theme::Dark))
            .unwrap_or(false);
        let cached_palette = EarthPalette::resolve(settings.theme_mode, initial_system_dark);
        let cached_accessibility = platform::has_accessibility_permission();
        let mut app = Self {
            storage,
            instance: instance_lock,
            paste_tx,
            paste_rx,
            backup_tx,
            backup_rx,
            shared_capture_config,
            settings,
            suppression,
            clipboard_rx,
            _clipboard_handle: clipboard_handle,
            update_tx,
            update_rx,
            hotkey_ctrl,
            tray,
            history,
            items,
            collections,
            storage_bytes,
            cached_results: Vec::new(),
            search_dirty: true,
            awaiting_newest_selection: true,
            view_mode: ViewMode::Launcher,
            popup_state,
            settings_state,
            pending_error: None,
            visible: !start_hidden,
            last_shown_at: Instant::now(),
            was_focused_since_shown: false,
            last_known_focused: false,
            last_frontmost_poll: Instant::now(),
            last_window_size: initial_size,
            last_window_pos: (x, y),
            workbench_os_expanded: false,
            workbench_collapse_pending: false,
            workbench_anim: None,
            prev_app,
            quitting: false,
            dialog_open: false,
            last_applied_theme: None,
            cached_palette,
            cached_accessibility,
            last_accessibility_check: Instant::now(),
        };
        app.sync_tray();
        app.recompute_search();
        app.select_newest_result();
        app
    }

    fn select_newest_result(&mut self) {
        if self.search_dirty {
            self.recompute_search();
        }
        let newest = newest_result_id(&self.cached_results);
        if self.popup_state.selected_id != newest {
            self.popup_state.clear_sensitive_context();
            self.popup_state.selected_id = newest;
        }
        self.popup_state.selection_changed_at = Instant::now();
        self.popup_state.reset_list_navigation();
        self.popup_state.list_scroll_y = 0.0;
        self.popup_state.scroll_anim_target = Some(0.0);
        self.popup_state.scroll_to_selection = true;
    }

    fn reload_items(&mut self) {
        self.popup_state.release_images();
        self.history.refresh(self.view_mode == ViewMode::Settings);
    }

    fn persist_settings(&mut self) {
        if !self.settings.show_tray_icon
            && !self
                .hotkey_ctrl
                .as_ref()
                .is_some_and(|ctrl| ctrl.is_available())
        {
            self.settings.show_tray_icon = true;
            self.record_service_error(t("app.hotkey_unavailable_keep_tray"));
        }
        let (privacy_changed, language_changed) = self
            .shared_capture_config
            .read()
            .map(|previous| {
                (
                    previous.settings.enable_privacy_masking
                        != self.settings.enable_privacy_masking,
                    previous.settings.language != self.settings.language,
                )
            })
            .unwrap_or((false, false));
        if privacy_changed {
            self.popup_state.clear_sensitive_context();
        }
        if language_changed {
            crate::i18n::set_language(self.settings.language);
            self.popup_state.preview_cache = None;
            self.popup_state.active_transform = None;
            if let Some(tray) = &mut self.tray {
                tray.refresh_language();
            }
        }
        if let Ok(mut guard) = self.shared_capture_config.write() {
            if guard.settings.ignore_next_event != self.settings.ignore_next_event
                && let Ok(mut sup) = self.suppression.lock()
            {
                sup.ignore_next_once = self.settings.ignore_next_event;
                sup.ignore_generation = sup.ignore_generation.wrapping_add(1);
            }
            *guard = Arc::new(CompiledCaptureConfig::from_settings(self.settings.clone()));
        }
        if let Err(err) = self.storage.save_settings(&self.settings) {
            self.report_error(tf("app.settings_save_failed", &[("error", &err)]));
        }
        let unpinned_count = self
            .items
            .iter()
            .filter(|it| !it.pinned && it.collection_id.is_none())
            .count();
        if unpinned_count > self.settings.max_history {
            if let Err(err) = self
                .storage
                .enforce_history_limit(self.settings.max_history)
            {
                self.report_error(err);
            }
            self.reload_items();
        } else {
            self.search_dirty = true;
            self.sync_tray();
        }
    }

    fn sync_tray(&mut self) {
        let preview = if self.settings.show_clipboard_in_menubar {
            self.items.first().map(|it| {
                if self.settings.enable_privacy_masking && it.is_sensitive() {
                    "••••".to_owned()
                } else {
                    truncate_chars(&it.summary_line(24), 22)
                }
            })
        } else {
            None
        };
        if let Some(tray) = &mut self.tray {
            let result = tray.sync_state(
                self.settings.show_tray_icon,
                self.settings.paused,
                self.settings.ignore_next_event,
                preview.as_deref(),
            );
            if let Err(error) = result {
                self.record_service_error(&error);
            }
        }
    }

    fn record_service_error(&mut self, error: &str) {
        let message = self
            .popup_state
            .service_error
            .get_or_insert_with(String::new);
        if !message.contains(error) {
            if !message.is_empty() {
                message.push('\n');
            }
            message.push_str(error);
        }
        self.settings_state.service_error = self.popup_state.service_error.clone();
    }

    /// Do not let a background operation's error toast expire while Earth is hidden.
    fn report_error(&mut self, message: String) {
        if !self.visible {
            self.pending_error = Some(message);
        } else if self.view_mode == ViewMode::Settings {
            self.settings_state.set_status_message(message, true);
        } else {
            self.popup_state.set_status_banner(message, true);
        }
    }

    fn finish_history_write(&mut self, result: Result<(), String>) -> bool {
        self.reload_items();
        if let Err(err) = result {
            self.report_error(err);
            false
        } else {
            true
        }
    }

    fn clear_history(&mut self, include_pinned: bool) -> Result<(), String> {
        let result = self.storage.clear_history(include_pinned);
        self.reload_items();
        result?;
        if self.settings.clear_system_clipboard_too {
            clear_system_clipboard()
                .map_err(|err| tf("app.clear_system_clipboard_failed", &[("error", &err)]))?;
        }
        Ok(())
    }

    fn advance_workbench_animation(&mut self, ctx: &egui::Context, panel_rect: egui::Rect) -> bool {
        if self.workbench_collapse_pending {
            if panel_rect.width() <= COMPACT_WIDTH + 4.0 {
                self.workbench_collapse_pending = false;
                self.workbench_os_expanded = false;
                self.popup_state.workbench_side = WorkbenchSide::Right;
                self.popup_state.preview_reveal = None;
                return true;
            }
            self.popup_state.preview_reveal = Some(0.0);
            ctx.request_repaint();
            return false;
        }

        if self.workbench_os_expanded
            && !self.popup_state.preview_open
            && self.workbench_anim.is_none()
            && panel_rect.width() <= COMPACT_WIDTH + 4.0
        {
            self.workbench_os_expanded = false;
            self.popup_state.workbench_side = WorkbenchSide::Right;
            self.popup_state.preview_reveal = None;
            return true;
        }

        let Some(ref mut anim) = self.workbench_anim else {
            return false;
        };

        let viewport_ready = anim.to <= 0.5 || panel_rect.width() >= EXPANDED_WIDTH - 4.0;
        let predicted_dt = ctx.input(|i| i.predicted_dt);
        let (reveal, finished) = anim.step(Instant::now(), predicted_dt, viewport_ready);
        self.popup_state.preview_reveal = Some(reveal);

        if finished {
            let finished_to = anim.to;
            self.workbench_anim = None;
            if finished_to <= 0.01 {
                self.popup_state.preview_reveal = Some(0.0);
                self.collapse_workbench_os_viewport(ctx, panel_rect);
                return !self.workbench_collapse_pending;
            } else {
                self.popup_state.preview_reveal = None;
                return true;
            }
        }

        ctx.request_repaint();
        false
    }

    fn animated_card_rect(&self, panel_rect: egui::Rect, outer_min_x: Option<f32>) -> egui::Rect {
        if self.view_mode != ViewMode::Launcher || !self.workbench_os_expanded {
            return panel_rect;
        }
        let reveal = match self.popup_state.preview_reveal {
            Some(r) => r.clamp(0.0, 1.0),
            None if self.popup_state.preview_open => 1.0,
            None => 0.0,
        };
        let pending_target = self
            .workbench_collapse_pending
            .then_some((self.last_window_pos.0, outer_min_x));
        compute_animated_card_rect(
            panel_rect,
            self.popup_state.workbench_side,
            reveal,
            pending_target,
        )
    }

    fn collapse_workbench_os_viewport(&mut self, ctx: &egui::Context, panel_rect: egui::Rect) {
        if self.workbench_collapse_pending && panel_rect.width() > COMPACT_WIDTH + 4.0 {
            return;
        }
        if !self.workbench_os_expanded && (self.last_window_size.0 - COMPACT_WIDTH).abs() <= 1.0 {
            self.workbench_collapse_pending = false;
            return;
        }
        let side = self.popup_state.workbench_side;
        platform::set_window_resize_anchor_right(side == WorkbenchSide::Left);
        let (cur_x, cur_y) = self.last_window_pos;
        let dpi = ctx.pixels_per_point();
        let mon = platform::monitor_bounds_for_point(
            cur_x + EXPANDED_WIDTH * 0.5,
            cur_y + WINDOW_HEIGHT * 0.5,
            dpi,
        );
        let (target_x, target_y) = plan_workbench_collapse(
            (cur_x, cur_y),
            side,
            COMPACT_WIDTH,
            EXPANDED_WIDTH,
            WINDOW_HEIGHT,
            mon,
        );
        self.last_window_size = (COMPACT_WIDTH, WINDOW_HEIGHT);
        self.last_window_pos = (target_x, target_y);
        ctx.send_viewport_cmd(ViewportCommand::InnerSize(vec2(
            COMPACT_WIDTH,
            WINDOW_HEIGHT,
        )));
        if (target_x - cur_x).abs() > 0.5 || (target_y - cur_y).abs() > 0.5 {
            ctx.send_viewport_cmd(ViewportCommand::OuterPosition(pos2(target_x, target_y)));
        }
        if panel_rect.width() <= COMPACT_WIDTH + 4.0 {
            self.workbench_collapse_pending = false;
            self.workbench_os_expanded = false;
            self.popup_state.workbench_side = WorkbenchSide::Right;
            self.popup_state.preview_reveal = None;
        } else {
            self.workbench_collapse_pending = true;
        }
        ctx.request_repaint();
    }

    fn sync_window_size(&mut self, ctx: &egui::Context, panel_rect: egui::Rect) {
        if self.workbench_collapse_pending {
            ctx.request_repaint();
            return;
        }
        if self.view_mode != ViewMode::Launcher {
            self.workbench_anim = None;
            if self.workbench_os_expanded || (self.last_window_size.0 - COMPACT_WIDTH).abs() > 1.0 {
                self.collapse_workbench_os_viewport(ctx, panel_rect);
            }
            return;
        }

        let want_open = self.popup_state.preview_open;
        let opening_or_open = self
            .workbench_anim
            .map(|a| a.to > 0.5)
            .unwrap_or(self.workbench_os_expanded);

        if want_open && !opening_or_open {
            let cur_reveal = self
                .popup_state
                .preview_reveal
                .unwrap_or(if self.workbench_os_expanded { 1.0 } else { 0.0 });
            let already_expanded =
                self.workbench_os_expanded && panel_rect.width() >= EXPANDED_WIDTH - 4.0;
            if !self.workbench_os_expanded {
                let (cur_x, cur_y) = self.last_window_pos;
                let dpi = ctx.pixels_per_point();
                let mon = platform::monitor_bounds_for_point(
                    cur_x + COMPACT_WIDTH * 0.5,
                    cur_y + WINDOW_HEIGHT * 0.5,
                    dpi,
                );
                let (side, (target_x, target_y)) = plan_workbench_expansion(
                    (cur_x, cur_y),
                    (COMPACT_WIDTH, WINDOW_HEIGHT),
                    EXPANDED_WIDTH,
                    mon,
                );
                platform::set_window_resize_anchor_right(side == WorkbenchSide::Left);
                self.popup_state.workbench_side = side;
                self.workbench_os_expanded = true;
                self.last_window_size = (EXPANDED_WIDTH, WINDOW_HEIGHT);
                self.last_window_pos = (target_x, target_y);
                if (target_x - cur_x).abs() > 0.5 || (target_y - cur_y).abs() > 0.5 {
                    ctx.send_viewport_cmd(ViewportCommand::OuterPosition(pos2(target_x, target_y)));
                }
                ctx.send_viewport_cmd(ViewportCommand::InnerSize(vec2(
                    EXPANDED_WIDTH,
                    WINDOW_HEIGHT,
                )));
            }
            let ratio = (1.0 - cur_reveal).clamp(0.2, 1.0);
            self.workbench_anim = Some(WorkbenchAnimation::new(
                cur_reveal,
                1.0,
                WORKBENCH_OPEN_DURATION.mul_f32(ratio),
                !already_expanded,
            ));
            self.popup_state.preview_reveal = Some(cur_reveal);
            ctx.request_repaint();
        } else if !want_open
            && self.workbench_os_expanded
            && !self.workbench_anim.is_some_and(|a| a.to <= 0.5)
        {
            platform::set_window_resize_anchor_right(
                self.popup_state.workbench_side == WorkbenchSide::Left,
            );
            let cur_reveal = self.popup_state.preview_reveal.unwrap_or(1.0);
            let ratio = cur_reveal.clamp(0.2, 1.0);
            self.workbench_anim = Some(WorkbenchAnimation::new(
                cur_reveal,
                0.0,
                WORKBENCH_CLOSE_DURATION.mul_f32(ratio),
                false,
            ));
            self.popup_state.preview_reveal = Some(cur_reveal);
            ctx.request_repaint();
        }
    }

    fn apply_window_pinned_state(&self, ctx: &egui::Context) {
        #[cfg(not(target_os = "macos"))]
        {
            ctx.send_viewport_cmd(ViewportCommand::WindowLevel(
                if self.popup_state.window_pinned {
                    egui::viewport::WindowLevel::AlwaysOnTop
                } else {
                    egui::viewport::WindowLevel::Normal
                },
            ));
        }
        #[cfg(target_os = "macos")]
        let _ = ctx;
        platform::set_window_pinned(self.popup_state.window_pinned);
    }

    fn complete_copy_or_paste(
        &mut self,
        ctx: &egui::Context,
        do_paste: bool,
        pinned_copy_msg: &str,
        pinned_paste_msg: &str,
    ) {
        if do_paste {
            if !platform::has_accessibility_permission() {
                self.popup_state
                    .set_status_banner(t("app.accessibility_paste_hint"), true);
                ctx.request_repaint();
                return;
            }
            let Some(target) = self.prev_app.clone() else {
                self.popup_state
                    .set_status_banner(t("app.no_target_window"), true);
                return;
            };
            if !self.popup_state.window_pinned {
                self.hide_popup(ctx, false);
                platform::hide_self_app();
            }
            if !platform::restore_frontmost_app(&target) {
                self.report_error(t("app.restore_target_failed").to_owned());
                return;
            }
            let tx = self.paste_tx.clone();
            let ctx = ctx.clone();
            let message = pinned_paste_msg.to_owned();
            std::thread::spawn(move || {
                let result = platform::paste_into(&target).map(|()| message);
                let _ = tx.send(result);
                ctx.request_repaint();
            });
        } else if self.popup_state.window_pinned {
            self.popup_state.set_status_banner(pinned_copy_msg, false);
            ctx.request_repaint();
        } else {
            self.hide_popup(ctx, true);
        }
    }

    fn show_popup(
        &mut self,
        ctx: &egui::Context,
        mode: ViewMode,
        tray_pos: Option<platform::TrayPosition>,
    ) {
        let was_visible = self.visible;
        if !was_visible {
            self.prev_app = platform::capture_frontmost_app();
        } else if let Some(active) = platform::capture_frontmost_app() {
            self.prev_app = Some(active);
        }
        if !was_visible {
            self.popup_state.window_pinned = false;
        }
        self.view_mode = mode;
        if mode == ViewMode::Settings {
            self.history.refresh(true);
        }
        self.workbench_anim = None;
        self.workbench_collapse_pending = false;
        self.workbench_os_expanded = false;
        platform::set_window_resize_anchor_right(false);
        self.popup_state.workbench_side = WorkbenchSide::Right;
        self.popup_state.preview_reveal = None;
        if mode == ViewMode::Launcher {
            prepare_launcher_reopen(was_visible, &mut self.popup_state, &mut self.search_dirty);
            self.select_newest_result();
            self.awaiting_newest_selection = true;
        }

        let target_size = (COMPACT_WIDTH, WINDOW_HEIGHT);
        self.last_window_size = target_size;
        let resolved_tray = tray_pos.or_else(|| self.tray.as_ref().and_then(|t| t.last_tray_pos()));
        let (x, y) = platform::compute_popup_origin(
            self.settings.popup_position,
            target_size,
            self.prev_app.as_ref(),
            resolved_tray,
            ctx.pixels_per_point(),
            ctx.zoom_factor(),
        );
        self.last_window_pos = (x, y);

        ctx.send_viewport_cmd(ViewportCommand::InnerSize(vec2(
            target_size.0,
            target_size.1,
        )));
        ctx.send_viewport_cmd(ViewportCommand::OuterPosition(pos2(x, y)));
        ctx.send_viewport_cmd(ViewportCommand::Visible(true));
        ctx.send_viewport_cmd(ViewportCommand::Focus);
        platform::activate_self_window();
        self.apply_window_pinned_state(ctx);

        self.visible = true;
        if let Some(message) = self.pending_error.take() {
            self.report_error(message);
        }
        self.last_shown_at = Instant::now();
        self.was_focused_since_shown = false;
        self.last_known_focused = false;
        ctx.request_repaint();
    }

    fn hide_popup(&mut self, ctx: &egui::Context, restore_focus: bool) {
        self.popup_state.clear_sensitive_context();
        self.popup_state.inline_edit_note = None;
        self.popup_state.release_images();
        self.settings_state.recording = None;
        self.sync_shortcut_recording_guards();
        if !has_reopen_entry(
            self.hotkey_ctrl
                .as_ref()
                .is_some_and(|ctrl| ctrl.is_available()),
            self.tray.as_ref().is_some_and(|tray| tray.is_visible()),
        ) {
            self.record_service_error(t("app.no_entry_point"));
            ctx.send_viewport_cmd(ViewportCommand::Visible(true));
            return;
        }
        self.visible = false;
        self.awaiting_newest_selection = false;
        if self.cached_results.capacity() > 128 {
            self.cached_results.shrink_to(64);
        }
        self.popup_state.show_about_modal = false;
        self.workbench_anim = None;
        self.workbench_collapse_pending = false;
        self.workbench_os_expanded = false;
        platform::set_window_resize_anchor_right(false);
        self.popup_state.window_pinned = false;
        self.apply_window_pinned_state(ctx);
        self.popup_state.preview_open = false;
        self.popup_state.preview_reveal = None;
        self.popup_state.workbench_side = WorkbenchSide::Right;
        self.popup_state.status_banner = None;
        self.popup_state.reset_list_navigation();
        ctx.send_viewport_cmd(ViewportCommand::Visible(false));
        if restore_focus {
            platform::hide_self_app();
            if let Some(ref prev) = self.prev_app {
                platform::restore_frontmost_app(prev);
            }
        }
    }

    fn sync_shortcut_recording_guards(&mut self) {
        let recording_target = (self.visible && self.view_mode == ViewMode::Settings)
            .then_some(self.settings_state.recording)
            .flatten();
        platform::set_shortcut_recording(
            recording_target.is_some(),
            recording_target == Some(RecordingTarget::Open),
        );
        if let Some(ctrl) = &mut self.hotkey_ctrl
            && let Err(error) = ctrl.set_suspended(recording_target.is_some())
        {
            self.settings_state.hotkey_error = Some(error.clone());
            self.popup_state.service_error = Some(error);
        }
    }

    fn recompute_search(&mut self) {
        let query = &self.popup_state.search_query;
        let filter = self.popup_state.filter;
        let mode = self.settings.search_mode;
        let sort_order = self.settings.sort_order;
        let pin_pos = self.settings.pin_position;
        let symbols = self.settings.show_special_symbols;
        let items = &self.items;

        let search_res = if query.trim().is_empty() || !items.iter().any(|it| it.text_truncated) {
            filter_and_search(items, query, filter, mode, sort_order, pin_pos, symbols)
        } else {
            self.storage.with_full_text_lookup(|lookup| {
                filter_and_search_with(
                    items, query, filter, mode, sort_order, pin_pos, symbols, lookup,
                )
            })
        };

        self.cached_results = match search_res {
            Ok(results) => results,
            Err(err) => {
                self.popup_state.set_status_banner(err, true);
                Vec::new()
            }
        };
        self.search_dirty = false;
    }

    fn handle_paste_item(&mut self, ctx: &egui::Context, id: i64, behavior: PasteBehavior) {
        let item = match self.storage.item(id) {
            Ok(it) => it,
            Err(err) => {
                self.popup_state.set_status_banner(err, true);
                return;
            }
        };

        let (do_paste, plain_only) = match behavior {
            PasteBehavior::Default => (self.settings.auto_paste, self.settings.plain_text_paste),
            PasteBehavior::CopyOnly => (false, self.settings.plain_text_paste),
            PasteBehavior::CopyAndPaste => (true, false),
            PasteBehavior::CopyAndPastePlainText => (true, true),
        };

        let partial_warning = match write_item_to_clipboard(&item, plain_only, &self.suppression) {
            Ok(warning) => warning,
            Err(err) => {
                self.popup_state.set_status_banner(err, true);
                ctx.request_repaint();
                return;
            }
        };

        let marked = self.storage.mark_copied(id);
        self.reload_items();

        self.complete_copy_or_paste(ctx, do_paste, t("app.copied"), t("app.pasted"));
        if let Some(warning) = partial_warning {
            self.report_error(warning);
        }
        if let Err(err) = marked {
            self.report_error(tf("app.copied_history_unsaved", &[("error", &err)]));
        }
    }

    fn handle_paste_transformed(&mut self, ctx: &egui::Context, text: String, paste_now: bool) {
        if let Err(err) = write_custom_text_to_clipboard(&text, &self.suppression) {
            self.popup_state.set_status_banner(err, true);
            ctx.request_repaint();
            return;
        }
        if paste_now {
            self.complete_copy_or_paste(ctx, true, t("app.qa_copied"), t("app.qa_pasted"));
        } else {
            self.popup_state
                .set_status_banner(t("app.qa_copied"), false);
            ctx.request_repaint();
        }
    }

    fn request_quit(&mut self, ctx: &egui::Context) {
        if self.settings.clear_on_exit
            && let Err(err) = self.clear_history(false)
        {
            rfd::MessageDialog::new()
                .set_title(t("app.quit_cleanup_title"))
                .set_description(err)
                .set_level(rfd::MessageLevel::Error)
                .show();
        }
        self.quitting = true;
        ctx.send_viewport_cmd(ViewportCommand::Close);
    }

    fn finish_qr_export(&mut self, ctx: &egui::Context, action: PopupAction) {
        match action {
            PopupAction::CopyQrImage(png_bytes) => {
                match write_managed_png(&png_bytes, &self.suppression) {
                    Ok(()) => {
                        self.popup_state
                            .set_status_banner(t("app.qr_copied"), false);
                    }
                    Err(e) => {
                        self.popup_state.set_status_banner(e, true);
                    }
                }
                ctx.request_repaint();
            }
            PopupAction::SaveQrImageAs(png_bytes) => {
                self.dialog_open = true;
                if let Some(dest) = rfd::FileDialog::new()
                    .set_file_name("earth_qrcode.png")
                    .add_filter("PNG Image", &["png"])
                    .save_file()
                {
                    if let Err(e) = std::fs::write(&dest, &png_bytes) {
                        self.popup_state.set_status_banner(
                            tf("app.qr_save_failed", &[("error", &e.to_string())]),
                            true,
                        );
                    } else {
                        let path = dest.display().to_string();
                        self.popup_state
                            .set_status_banner(tf("app.qr_saved", &[("path", &path)]), false);
                    }
                    ctx.request_repaint();
                }
                self.dialog_open = false;
            }
            _ => {}
        }
    }

    fn process_background_events(&mut self, ctx: &egui::Context) {
        if let Some(action) = self.popup_state.poll_qr_export() {
            self.finish_qr_export(ctx, action);
        }
        while let Ok(result) = self.paste_rx.try_recv() {
            match result {
                Ok(message) if self.visible => self.popup_state.set_status_banner(message, false),
                Err(err) => self.report_error(tf("app.paste_manual_hint", &[("error", &err)])),
                _ => (),
            }
        }
        while let Ok(ev) = self.backup_rx.try_recv() {
            self.settings_state.backup_busy = false;
            match ev {
                BackupEvent::Exported(Ok(dest)) => {
                    let path = dest.display().to_string();
                    self.settings_state
                        .set_status_message(tf("app.export_ok", &[("path", &path)]), false);
                }
                BackupEvent::Exported(Err(e)) => {
                    self.report_error(tf("app.export_failed", &[("error", &e)]));
                }
                BackupEvent::Imported(Ok(summary)) => {
                    self.reload_items();
                    let image_warning = if summary.missing_images > 0 {
                        let count = summary.missing_images.to_string();
                        tf("app.import_missing_images", &[("count", &count)])
                    } else if summary.legacy_format {
                        t("app.import_legacy_note").to_string()
                    } else {
                        String::new()
                    };
                    let imported = summary.imported.to_string();
                    let skipped = summary.skipped_duplicates.to_string();
                    self.settings_state.set_status_message(
                        tf(
                            "app.import_ok",
                            &[
                                ("imported", &imported),
                                ("skipped", &skipped),
                                ("warning", &image_warning),
                            ],
                        ),
                        summary.missing_images > 0,
                    );
                }
                BackupEvent::Imported(Err(e)) => {
                    self.report_error(e);
                }
            }
        }
        while let Ok(result) = self.history.rx.try_recv() {
            match result {
                Ok(delta) => {
                    let had_changed_items = !delta.items.is_empty();
                    delta.apply(
                        &mut self.items,
                        &mut self.collections,
                        &mut self.storage_bytes,
                    );
                    self.search_dirty = true;
                    self.sync_tray();
                    if had_changed_items && (!self.visible || self.awaiting_newest_selection) {
                        self.select_newest_result();
                    }
                }
                Err(err) => self.report_error(tf("app.history_read_failed", &[("error", &err)])),
            }
        }
        let mut captured_any = false;
        while let Ok(ev) = self.clipboard_rx.try_recv() {
            match ev {
                ClipboardEvent::NewCapture(payload) => {
                    match self
                        .storage
                        .upsert_item(*payload, self.settings.max_history)
                    {
                        Ok(_) => {
                            captured_any = true;
                            if !self.visible {
                                self.awaiting_newest_selection = true;
                            }
                        }
                        Err(err) => {
                            self.report_error(tf("app.history_save_failed", &[("error", &err)]))
                        }
                    }
                }
                ClipboardEvent::Skipped(reason) => self.report_error(reason.to_owned()),
                ClipboardEvent::ConsumedIgnoreNext(generation) => {
                    if self
                        .suppression
                        .lock()
                        .is_ok_and(|s| s.ignore_generation != generation)
                    {
                        continue;
                    }
                    self.popup_state
                        .set_status_banner(t("app.ignored_once"), false);
                    self.settings.ignore_next_event = false;
                    if let Err(err) = self.storage.save_settings(&self.settings) {
                        self.report_error(tf("app.settings_save_failed", &[("error", &err)]));
                    }
                    if let Ok(mut guard) = self.shared_capture_config.write() {
                        *guard =
                            Arc::new(CompiledCaptureConfig::from_settings(self.settings.clone()));
                    }
                    self.sync_tray();
                }
                ClipboardEvent::WatcherStopped => {
                    self.record_service_error(t("app.watcher_stopped"));
                    if !self.visible && !self.quitting {
                        self.show_popup(ctx, ViewMode::Settings, None);
                    }
                }
            }
        }
        if captured_any {
            self.reload_items();
        }

        while let Ok(ev) = self.update_rx.try_recv() {
            match ev {
                UpdaterEvent::CheckFinished { manual, outcome } => {
                    self.settings_state.checking_updates = false;
                    match outcome {
                        UpdateCheckOutcome::UpdateAvailable(info) => {
                            let already_staged_or_downloading = matches!(
                                &self.popup_state.update_download,
                                UpdateDownloadState::Downloading { version }
                                | UpdateDownloadState::ReadyToInstall { version, .. }
                                if version == &info.version
                            );

                            if info.download_url.is_some() && !already_staged_or_downloading {
                                let dl_state = UpdateDownloadState::Downloading {
                                    version: info.version.clone(),
                                };
                                self.popup_state.update_download = dl_state.clone();
                                self.settings_state.update_download = dl_state;
                                spawn_update_download(
                                    info.clone(),
                                    self.update_tx.clone(),
                                    ctx.clone(),
                                );
                                if manual {
                                    self.settings_state.set_status_message(
                                        tf(
                                            "app.update_found_downloading",
                                            &[("version", &info.version)],
                                        ),
                                        false,
                                    );
                                }
                            } else if manual {
                                self.settings_state.set_status_message(
                                    tf("app.update_found_manual", &[("version", &info.version)]),
                                    false,
                                );
                            }
                            self.popup_state.available_update = Some(info.clone());
                            self.settings_state.available_update = Some(info);
                        }
                        UpdateCheckOutcome::UpToDate(ver) => {
                            self.popup_state.available_update = None;
                            self.settings_state.available_update = None;
                            self.popup_state.update_download = UpdateDownloadState::Idle;
                            self.settings_state.update_download = UpdateDownloadState::Idle;
                            if manual {
                                self.settings_state.set_status_message(
                                    tf("app.update_up_to_date", &[("version", &ver)]),
                                    false,
                                );
                            }
                        }
                        UpdateCheckOutcome::Error(err) => {
                            if manual {
                                self.settings_state.set_status_message(err, true);
                            }
                        }
                    }
                }
                UpdaterEvent::DownloadFinished { version, result } => match result {
                    Ok(staged_path) => {
                        let ready = UpdateDownloadState::ReadyToInstall {
                            version: version.clone(),
                            staged_path,
                        };
                        self.popup_state.update_download = ready.clone();
                        self.settings_state.update_download = ready;
                        self.settings_state.set_status_message(
                            tf("app.update_ready_toast", &[("version", &version)]),
                            false,
                        );
                    }
                    Err(err) => {
                        let failed = UpdateDownloadState::Failed {
                            version: version.clone(),
                            error: err.clone(),
                        };
                        self.popup_state.update_download = failed.clone();
                        self.settings_state.update_download = failed;
                        self.settings_state.set_status_message(err, true);
                    }
                },
            }
        }

        if self.settings_state.recording.is_none()
            && let Some(ctrl) = &self.hotkey_ctrl
            && ctrl.poll_open_triggered()
        {
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
                    if let Err(err) = self.clear_history(false) {
                        self.report_error(err);
                    }
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
        if self.instance.take_activation() && !self.quitting {
            if self.visible {
                ctx.send_viewport_cmd(ViewportCommand::Focus);
                platform::activate_self_window();
            } else {
                self.show_popup(ctx, self.view_mode, None);
            }
        }
        self.process_background_events(ctx);

        // Convert OS close button into hide unless user explicitly clicked Quit
        if ctx.input(|i| i.viewport().close_requested()) && !self.quitting {
            ctx.send_viewport_cmd(ViewportCommand::CancelClose);
            self.hide_popup(ctx, true);
        }

        // Track focus: hide on blur normally, or keep floating above other apps while pinned.
        if self.visible {
            let focused = ctx.input(|i| i.focused);
            if focused {
                self.was_focused_since_shown = true;
            } else {
                if self.was_focused_since_shown && self.settings_state.recording.is_some() {
                    self.settings_state.recording = None;
                    self.sync_shortcut_recording_guards();
                }
                if self.popup_state.window_pinned {
                    if self.last_known_focused {
                        self.apply_window_pinned_state(ctx);
                    }
                    if self.last_frontmost_poll.elapsed() > Duration::from_millis(150) {
                        self.last_frontmost_poll = Instant::now();
                        if let Some(active) = platform::capture_frontmost_app_identity() {
                            self.prev_app = Some(active);
                        }
                    }
                } else if should_auto_hide_on_blur(&FocusBlurCheck {
                    visible: self.visible,
                    focused,
                    was_focused_since_shown: self.was_focused_since_shown,
                    view_mode: self.view_mode,
                    show_about_modal: self.popup_state.show_about_modal,
                    dialog_open: self.dialog_open,
                    window_pinned: self.popup_state.window_pinned,
                    since_shown: self.last_shown_at.elapsed(),
                }) {
                    self.hide_popup(ctx, false);
                }
            }
            self.last_known_focused = focused;
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

        let system_dark = platform::system_is_dark()
            .or_else(|| ctx.system_theme().map(|t| t == egui::Theme::Dark))
            .unwrap_or(false);
        let ppp_key = (ctx.pixels_per_point() * 100.0).round() as u32;
        let theme_key = (self.settings.theme_mode, system_dark, ppp_key);
        if self.last_applied_theme != Some(theme_key) {
            self.cached_palette = EarthPalette::resolve(self.settings.theme_mode, system_dark);
            self.cached_palette.apply_to_ctx(&ctx);
            self.last_applied_theme = Some(theme_key);
        }
        let palette = self.cached_palette;

        if self.workbench_anim.is_none()
            && self.last_accessibility_check.elapsed() > Duration::from_secs(2)
        {
            self.cached_accessibility = platform::has_accessibility_permission();
            self.last_accessibility_check = Instant::now();
        }
        let has_accessibility = self.cached_accessibility;
        let panel_rect = ctx.content_rect();
        let outer_min_x = ctx.input(|i| i.viewport().outer_rect).map(|outer| {
            if self.workbench_anim.is_none() && !self.workbench_collapse_pending {
                self.last_window_pos = (outer.min.x, outer.min.y);
            }
            outer.min.x
        });
        let shadow_dirty = self.advance_workbench_animation(&ctx, panel_rect);
        let card_rect = self.animated_card_rect(panel_rect, outer_min_x);

        egui::CentralPanel::default()
            .frame(egui::Frame::NONE)
            .show(ui, |ui| {
                let outer_rect = card_rect.shrink(1.0);
                handle_background_window_drag(ui, &ctx, outer_rect, "earth_window_drag_bg");
                ui.painter()
                    .rect_filled(outer_rect, CornerRadius::same(18), palette.window_bg);
                ui.painter().rect_stroke(
                    outer_rect,
                    CornerRadius::same(18),
                    Stroke::new(1.0, palette.border),
                    egui::StrokeKind::Inside,
                );

                let content_rect = outer_rect.shrink(12.0);
                ui.scope_builder(
                    egui::UiBuilder::new()
                        .max_rect(content_rect)
                        .layout(egui::Layout::top_down(egui::Align::Min)),
                    |ui| {
                        ui.set_clip_rect(outer_rect.shrink(2.0));
                        ui.set_min_size(content_rect.size());
                        ui.set_max_size(content_rect.size());

                        match self.view_mode {
                            ViewMode::Launcher => {
                                let prev_query = self.popup_state.search_query.clone();
                                let prev_filter = self.popup_state.filter;
                                if self.search_dirty {
                                    self.recompute_search();
                                }
                                let prev_selected = self.popup_state.selected_id;
                                let total_items = self.items.len();

                                let actions = {
                                    let storage = &self.storage;
                                    let load_full = |id: i64| storage.item(id).ok();
                                    render_popup_view(
                                        ui,
                                        &ctx,
                                        PopupViewData {
                                            settings: &mut self.settings,
                                            results: &self.cached_results,
                                            collections: &self.collections,
                                            palette: &palette,
                                            storage_bytes: self.storage_bytes,
                                            total_items,
                                            has_accessibility,
                                            load_full_item: Some(&load_full),
                                        },
                                        &mut self.popup_state,
                                    )
                                };

                                if self.popup_state.search_query != prev_query
                                    || self.popup_state.filter != prev_filter
                                {
                                    self.awaiting_newest_selection = false;
                                    self.popup_state.reset_list_navigation();
                                    self.search_dirty = true;
                                    ctx.request_repaint();
                                } else if self.popup_state.selected_id != prev_selected {
                                    self.awaiting_newest_selection = false;
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
                                            if let Some(it) = self.items.iter().find(|x| x.id == id)
                                            {
                                                self.finish_history_write(
                                                    self.storage.set_pinned(id, !it.pinned),
                                                );
                                            }
                                        }
                                        PopupAction::SetCollection {
                                            item_id,
                                            collection_id,
                                        } => {
                                            self.finish_history_write(
                                                self.storage.set_collection(item_id, collection_id),
                                            );
                                        }
                                        PopupAction::SetItemNote { item_id, note } => {
                                            if !self.finish_history_write(
                                                self.storage
                                                    .set_item_note(item_id, note.as_deref()),
                                            ) {
                                                self.popup_state.inline_edit_note =
                                                    Some((item_id, note.unwrap_or_default()));
                                            }
                                        }
                                        PopupAction::UpdateItemText {
                                            item_id,
                                            new_text,
                                            paste_after,
                                        } => {
                                            match self.storage.update_item_text(item_id, &new_text)
                                            {
                                                Ok(()) => {
                                                    self.reload_items();
                                                    if paste_after {
                                                        self.handle_paste_item(
                                                            &ctx,
                                                            item_id,
                                                            PasteBehavior::Default,
                                                        );
                                                    } else {
                                                        self.popup_state.set_status_banner(
                                                            t("app.item_edit_saved"),
                                                            false,
                                                        );
                                                        ctx.request_repaint();
                                                    }
                                                }
                                                Err(err) => {
                                                    self.popup_state.inline_edit_text =
                                                        Some((item_id, new_text));
                                                    self.popup_state.set_status_banner(err, true);
                                                    ctx.request_repaint();
                                                }
                                            }
                                        }
                                        PopupAction::DeleteItem(id) => {
                                            self.finish_history_write(self.storage.delete_item(id));
                                        }
                                        PopupAction::ClearUnpinned => {
                                            if let Err(err) = self.clear_history(false) {
                                                self.report_error(err);
                                            }
                                        }
                                        PopupAction::OpenSettings => {
                                            self.view_mode = ViewMode::Settings;
                                            self.history.refresh(true);
                                            ctx.request_repaint();
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
                                        PopupAction::ToggleWindowPin => {
                                            self.apply_window_pinned_state(&ctx);
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
                                                    self.popup_state.set_status_banner(
                                                        tf(
                                                            "app.image_save_failed",
                                                            &[("error", &e.to_string())],
                                                        ),
                                                        true,
                                                    );
                                                } else {
                                                    let path = dest.display().to_string();
                                                    self.popup_state.set_status_banner(
                                                        tf("app.image_saved", &[("path", &path)]),
                                                        false,
                                                    );
                                                }
                                                ctx.request_repaint();
                                            }
                                            self.dialog_open = false;
                                        }
                                        action @ (PopupAction::CopyQrImage(_)
                                        | PopupAction::SaveQrImageAs(_)) => {
                                            self.finish_qr_export(&ctx, action);
                                        }
                                        PopupAction::RevealFile(path) => {
                                            platform::reveal_in_file_manager(&path);
                                        }
                                        PopupAction::OpenUrl(url) => {
                                            platform::open_url(&url);
                                        }
                                        PopupAction::StartUpdateDownload(info) => {
                                            let dl = UpdateDownloadState::Downloading {
                                                version: info.version.clone(),
                                            };
                                            self.popup_state.update_download = dl.clone();
                                            self.settings_state.update_download = dl;
                                            spawn_update_download(
                                                info,
                                                self.update_tx.clone(),
                                                ctx.clone(),
                                            );
                                        }
                                        PopupAction::ApplyUpdateAndRestart(staged_path) => {
                                            match apply_staged_update_and_restart(&staged_path) {
                                                Ok(()) => {
                                                    self.request_quit(&ctx);
                                                }
                                                Err(err) => {
                                                    self.popup_state.set_status_banner(err, true);
                                                    ctx.request_repaint();
                                                }
                                            }
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
                                let item_count = self.items.len();
                                let actions = render_settings_view(
                                    ui,
                                    &ctx,
                                    SettingsViewData {
                                        settings: &mut self.settings,
                                        collections: &self.collections,
                                        palette: &palette,
                                        storage_bytes: self.storage_bytes,
                                        item_count,
                                        has_accessibility,
                                    },
                                    &mut self.settings_state,
                                );
                                self.sync_shortcut_recording_guards();

                                for action in actions {
                                    match action {
                                        SettingsAction::CloseSettings => {
                                            self.view_mode = ViewMode::Launcher;
                                            self.popup_state.request_search_focus = true;
                                            self.sync_shortcut_recording_guards();
                                            ctx.request_repaint();
                                        }
                                        SettingsAction::SettingsChanged => {
                                            self.persist_settings();
                                        }
                                        SettingsAction::OpenHotkeyChanged(new_combo) => {
                                            let result = match &mut self.hotkey_ctrl {
                                                Some(ctrl) => ctrl.set_open_shortcut(&new_combo),
                                                None => {
                                                    Err(t("app.hotkey_service_unavailable")
                                                        .to_string())
                                                }
                                            };
                                            match result {
                                                Ok(()) => {
                                                    self.settings.hotkey_open = new_combo;
                                                    self.settings_state.hotkey_error = None;
                                                    self.settings_state.set_status_message(
                                                        t("app.hotkey_applied"),
                                                        false,
                                                    );
                                                    self.persist_settings();
                                                }
                                                Err(e) => {
                                                    self.settings_state.hotkey_error = Some(e);
                                                }
                                            }
                                        }
                                        SettingsAction::ToggleAutostart(enabled) => {
                                            match platform::sync_autostart(enabled) {
                                                Ok(()) => {
                                                    self.settings_state.set_status_message(
                                                        if enabled {
                                                            t("app.autostart_enabled")
                                                        } else {
                                                            t("app.autostart_disabled")
                                                        },
                                                        false,
                                                    );
                                                }
                                                Err(e) => {
                                                    self.settings.launch_at_login = !enabled;
                                                    self.settings_state.set_status_message(e, true);
                                                }
                                            }
                                            self.persist_settings();
                                        }
                                        SettingsAction::ClearHistory(include_pinned) => {
                                            match self.clear_history(include_pinned) {
                                                Ok(()) => self.settings_state.set_status_message(
                                                    t("app.history_cleared"),
                                                    false,
                                                ),
                                                Err(err) => self.report_error(err),
                                            }
                                        }
                                        SettingsAction::ExportJson => {
                                            if !self.settings_state.backup_busy {
                                                self.dialog_open = true;
                                                let picked = rfd::FileDialog::new()
                                                    .set_file_name("earth_clipboard_backup.json")
                                                    .add_filter("JSON", &["json"])
                                                    .save_file();
                                                self.dialog_open = false;
                                                if let Some(dest) = picked {
                                                    self.settings_state.backup_busy = true;
                                                    self.settings_state.set_status_message(
                                                        t("app.backup_exporting"),
                                                        false,
                                                    );
                                                    let base_dir =
                                                        self.storage.base_dir().to_path_buf();
                                                    let tx = self.backup_tx.clone();
                                                    let repaint = ctx.clone();
                                                    let _ = std::thread::Builder::new()
                                                        .name("earth-backup-export".into())
                                                        .spawn(move || {
                                                            let res =
                                                                Storage::open_existing_in_dir(
                                                                    &base_dir,
                                                                )
                                                                .and_then(|s| {
                                                                    s.export_backup(&dest)
                                                                })
                                                                .map(|()| dest);
                                                            let _ =
                                                                tx.send(BackupEvent::Exported(res));
                                                            repaint.request_repaint();
                                                        });
                                                }
                                            }
                                        }
                                        SettingsAction::ImportJson => {
                                            if !self.settings_state.backup_busy {
                                                self.dialog_open = true;
                                                let picked = rfd::FileDialog::new()
                                                    .add_filter("JSON", &["json"])
                                                    .pick_file();
                                                self.dialog_open = false;
                                                if let Some(src) = picked {
                                                    self.settings_state.backup_busy = true;
                                                    self.settings_state.set_status_message(
                                                        t("app.backup_importing"),
                                                        false,
                                                    );
                                                    let base_dir =
                                                        self.storage.base_dir().to_path_buf();
                                                    let max_history = self.settings.max_history;
                                                    let tx = self.backup_tx.clone();
                                                    let repaint = ctx.clone();
                                                    let _ = std::thread::Builder::new()
                                                        .name("earth-backup-import".into())
                                                        .spawn(move || {
                                                            let res =
                                                                Storage::open_existing_in_dir(
                                                                    &base_dir,
                                                                )
                                                                .and_then(|s| {
                                                                    s.import_backup(
                                                                        &src,
                                                                        max_history,
                                                                    )
                                                                });
                                                            let _ =
                                                                tx.send(BackupEvent::Imported(res));
                                                            repaint.request_repaint();
                                                        });
                                                }
                                            }
                                        }
                                        SettingsAction::CreateCollection { name, color_hex } => {
                                            match self.storage.create_collection(&name, &color_hex)
                                            {
                                                Ok(_) => {
                                                    self.reload_items();
                                                    self.settings_state.set_status_message(
                                                        tf(
                                                            "app.collection_created",
                                                            &[("name", &name)],
                                                        ),
                                                        false,
                                                    );
                                                }
                                                Err(e) => {
                                                    self.settings_state.set_status_message(e, true);
                                                }
                                            }
                                        }
                                        SettingsAction::RenameCollection { id, name } => {
                                            match self.storage.rename_collection(id, &name) {
                                                Ok(()) => {
                                                    self.reload_items();
                                                    self.settings_state.set_status_message(
                                                        tf(
                                                            "app.collection_renamed",
                                                            &[("name", &name)],
                                                        ),
                                                        false,
                                                    );
                                                }
                                                Err(e) => {
                                                    self.settings_state.set_status_message(e, true);
                                                }
                                            }
                                        }
                                        SettingsAction::DeleteCollection(id) => {
                                            if self.finish_history_write(
                                                self.storage.delete_collection(id),
                                            ) && self.popup_state.filter
                                                == crate::model::CategoryFilter::Collection(id)
                                            {
                                                self.popup_state.filter =
                                                    crate::model::CategoryFilter::All;
                                            }
                                        }
                                        SettingsAction::OpenAccessibilitySettings => {
                                            platform::open_accessibility_settings();
                                        }
                                        SettingsAction::CheckUpdatesNow => {
                                            spawn_update_check(
                                                true,
                                                self.update_tx.clone(),
                                                ctx.clone(),
                                            );
                                        }
                                        SettingsAction::StartUpdateDownload(info) => {
                                            let dl = UpdateDownloadState::Downloading {
                                                version: info.version.clone(),
                                            };
                                            self.popup_state.update_download = dl.clone();
                                            self.settings_state.update_download = dl;
                                            spawn_update_download(
                                                info,
                                                self.update_tx.clone(),
                                                ctx.clone(),
                                            );
                                        }
                                        SettingsAction::ApplyUpdateAndRestart(staged_path) => {
                                            match apply_staged_update_and_restart(&staged_path) {
                                                Ok(()) => {
                                                    self.request_quit(&ctx);
                                                }
                                                Err(err) => {
                                                    self.settings_state
                                                        .set_status_message(err, true);
                                                }
                                            }
                                        }
                                        SettingsAction::OpenUrl(url) => {
                                            platform::open_url(&url);
                                        }
                                    }
                                }
                                if self
                                    .settings_state
                                    .status_message
                                    .as_ref()
                                    .is_some_and(|t| t.shown_at.is_none())
                                {
                                    ctx.request_repaint();
                                }
                            }
                        }
                    },
                );
            });

        self.sync_window_size(&ctx, panel_rect);
        if shadow_dirty {
            platform::invalidate_window_shadow();
        }
    }
}

pub(crate) struct FocusBlurCheck {
    pub visible: bool,
    pub focused: bool,
    pub was_focused_since_shown: bool,
    pub view_mode: ViewMode,
    pub show_about_modal: bool,
    pub dialog_open: bool,
    pub window_pinned: bool,
    pub since_shown: Duration,
}

fn has_reopen_entry(hotkey_available: bool, tray_visible: bool) -> bool {
    hotkey_available || tray_visible
}

fn prepare_launcher_reopen(was_visible: bool, state: &mut PopupState, search_dirty: &mut bool) {
    if !was_visible && (!state.search_query.is_empty() || state.filter != CategoryFilter::All) {
        state.search_query.clear();
        state.filter = CategoryFilter::All;
        *search_dirty = true;
    }
    state.request_search_focus = true;
    state.ime_composing = false;
    state.preview_open = false;
    state.active_transform = None;
    state.inline_edit_text = None;
    state.inline_edit_note = None;
    state.qr_preview_id = None;
}

fn newest_result_id(results: &[SearchResultItem]) -> Option<i64> {
    results
        .iter()
        .max_by(|a, b| {
            a.item
                .last_copied_at
                .cmp(&b.item.last_copied_at)
                .then_with(|| a.item.id.cmp(&b.item.id))
        })
        .map(|r| r.item.id)
}

pub(crate) fn should_auto_hide_on_blur(s: &FocusBlurCheck) -> bool {
    s.visible
        && !s.focused
        && s.was_focused_since_shown
        && s.view_mode == ViewMode::Launcher
        && !s.show_about_modal
        && !s.dialog_open
        && !s.window_pinned
        && s.since_shown > Duration::from_millis(320)
}

pub(crate) fn compute_animated_card_rect(
    panel_rect: egui::Rect,
    side: WorkbenchSide,
    reveal: f32,
    collapse_pending_target_and_outer_x: Option<(f32, Option<f32>)>,
) -> egui::Rect {
    if let Some((target_screen_x, outer_min_x)) = collapse_pending_target_and_outer_x
        && panel_rect.width() > COMPACT_WIDTH + 4.0
        && let Some(ox) = outer_min_x
    {
        // Keep the compact card anchored at its screen X even if OS OuterPosition moves
        // before WindowEvent::Resized shrinks panel_rect.
        let local_x =
            (target_screen_x - ox).clamp(0.0, (panel_rect.width() - COMPACT_WIDTH).max(0.0));
        return egui::Rect::from_min_size(
            pos2(panel_rect.min.x + local_x, panel_rect.min.y),
            vec2(COMPACT_WIDTH, panel_rect.height()),
        );
    }

    let reveal = reveal.clamp(0.0, 1.0);
    let target_w = (COMPACT_WIDTH + (EXPANDED_WIDTH - COMPACT_WIDTH) * reveal)
        .round()
        .clamp(COMPACT_WIDTH, panel_rect.width().max(COMPACT_WIDTH));
    if (panel_rect.width() - target_w).abs() <= 1.0 {
        return panel_rect;
    }
    match side {
        WorkbenchSide::Right => {
            egui::Rect::from_min_size(panel_rect.min, vec2(target_w, panel_rect.height()))
        }
        WorkbenchSide::Left => egui::Rect::from_min_max(
            pos2(
                (panel_rect.max.x - target_w).max(panel_rect.min.x),
                panel_rect.min.y,
            ),
            panel_rect.max,
        ),
    }
}

/// Decides whether the workbench should expand to the right or flip to the left so the
/// expanded window stays completely inside the monitor containing the window.
pub(crate) fn plan_workbench_expansion(
    compact_origin: (f32, f32),
    compact_size: (f32, f32),
    expanded_width: f32,
    monitor: platform::ScreenRect,
) -> (WorkbenchSide, (f32, f32)) {
    const EDGE_MARGIN: f32 = 4.0;
    let (win_x, win_y) = compact_origin;
    let (compact_w, win_h) = compact_size;
    let win_right = win_x + compact_w;
    let mon_left = monitor.x + EDGE_MARGIN;
    let mon_right = (monitor.x + monitor.width - EDGE_MARGIN).max(mon_left);
    let mon_top = monitor.y + EDGE_MARGIN;
    let mon_bottom = (monitor.y + monitor.height - EDGE_MARGIN).max(mon_top);

    let max_y = (mon_bottom - win_h).max(mon_top);
    let clamped_y = win_y.clamp(mon_top, max_y);

    let right_fits = win_x >= mon_left && win_x + expanded_width <= mon_right;
    let right_space = mon_right - win_x;
    let left_space = win_right - mon_left;

    if right_fits {
        (WorkbenchSide::Right, (win_x, clamped_y))
    } else if left_space >= expanded_width || left_space > right_space {
        let anchored_right = win_right.min(mon_right);
        let target_x = (anchored_right - expanded_width).max(mon_left);
        (WorkbenchSide::Left, (target_x, clamped_y))
    } else {
        let max_x = (mon_right - expanded_width).max(mon_left);
        (
            WorkbenchSide::Right,
            (win_x.clamp(mon_left, max_x), clamped_y),
        )
    }
}

/// Computes the compact window origin after collapsing the workbench while keeping the list pane anchored.
pub(crate) fn plan_workbench_collapse(
    current_origin: (f32, f32),
    side: WorkbenchSide,
    compact_width: f32,
    expanded_width: f32,
    window_height: f32,
    monitor: platform::ScreenRect,
) -> (f32, f32) {
    const EDGE_MARGIN: f32 = 4.0;
    let (cur_x, cur_y) = current_origin;
    let mon_left = monitor.x + EDGE_MARGIN;
    let mon_right = (monitor.x + monitor.width - EDGE_MARGIN).max(mon_left);
    let mon_top = monitor.y + EDGE_MARGIN;
    let mon_bottom = (monitor.y + monitor.height - EDGE_MARGIN).max(mon_top);

    let target_x = match side {
        WorkbenchSide::Right => cur_x,
        WorkbenchSide::Left => cur_x + (expanded_width - compact_width),
    };
    let max_x = (mon_right - compact_width).max(mon_left);
    let max_y = (mon_bottom - window_height).max(mon_top);
    (target_x.clamp(mon_left, max_x), cur_y.clamp(mon_top, max_y))
}

/// Registers a lowest-priority drag surface behind UI content so dragging anywhere outside
/// widgets that genuinely consume drag (such as text inputs, scrollbars, and numeric steppers)
/// moves the borderless window cleanly without breaking clicks.
pub(crate) fn handle_background_window_drag(
    ui: &mut egui::Ui,
    ctx: &egui::Context,
    rect: egui::Rect,
    id_salt: &str,
) {
    let resp = ui.interact(
        rect,
        ui.scope_id().with(id_salt),
        egui::Sense::click_and_drag(),
    );
    if resp.drag_started() {
        ctx.send_viewport_cmd(ViewportCommand::StartDrag);
        // Clear egui's active drag lock because Cocoa/OS takes over the pointer loop.
        ctx.stop_dragging();
    }
}

#[cfg(test)]
mod tests {
    use super::handle_background_window_drag;
    use egui::{Modifiers, ViewportCommand, ViewportId, pos2, vec2};

    #[test]
    fn hiding_requires_at_least_one_working_entry() {
        assert!(!super::has_reopen_entry(false, false));
        assert!(super::has_reopen_entry(false, true));
        assert!(super::has_reopen_entry(true, false));
        assert!(super::has_reopen_entry(true, true));
    }

    fn run_drag_sequence(
        ctx: &egui::Context,
        base_time: &mut f64,
        start: egui::Pos2,
        end: egui::Pos2,
        release: bool,
        mut build_ui: impl FnMut(&mut egui::Ui),
    ) -> bool {
        *base_time += 1.0;
        let mut start_drag_emitted = false;
        let mut steps = vec![
            (0.0, vec![egui::Event::PointerMoved(start)]),
            (
                0.02,
                vec![egui::Event::PointerButton {
                    pos: start,
                    button: egui::PointerButton::Primary,
                    pressed: true,
                    modifiers: Modifiers::NONE,
                }],
            ),
            (0.04, vec![egui::Event::PointerMoved(end)]),
        ];
        if release {
            steps.push((
                0.06,
                vec![egui::Event::PointerButton {
                    pos: end,
                    button: egui::PointerButton::Primary,
                    pressed: false,
                    modifiers: Modifiers::NONE,
                }],
            ));
        }

        for (t, events) in steps {
            let raw = egui::RawInput {
                time: Some(*base_time + t),
                screen_rect: Some(egui::Rect::from_min_size(
                    pos2(0.0, 0.0),
                    vec2(424.0, 520.0),
                )),
                events,
                ..Default::default()
            };
            let mut out = ctx.run_ui(raw, |ui| {
                #[allow(deprecated)]
                egui::CentralPanel::default()
                    .frame(egui::Frame::NONE)
                    .show_inside(ui, |ui| {
                        let rect = ctx.content_rect();
                        handle_background_window_drag(ui, ctx, rect, "test_drag_bg");
                        build_ui(ui);
                    });
            });
            if let Some(vp) = out.viewport_output.get(&ViewportId::ROOT)
                && vp
                    .commands
                    .iter()
                    .any(|cmd| matches!(cmd, ViewportCommand::StartDrag))
            {
                start_drag_emitted = true;
            }
            out.textures_delta.clear();
        }
        start_drag_emitted
    }

    #[test]
    fn background_drag_moves_window_from_empty_and_clickable_areas_without_breaking_clicks() {
        let ctx = egui::Context::default();
        let mut text = "hello world".to_string();
        let mut clicked_count = 0usize;
        let mut base_time = 10.0;

        // 1. Dragging blank background starts window drag.
        let started_blank = run_drag_sequence(
            &ctx,
            &mut base_time,
            pos2(300.0, 400.0),
            pos2(320.0, 415.0),
            true,
            |ui| {
                ui.add(egui::TextEdit::singleline(&mut text).desired_width(120.0));
                if ui.button("Action").clicked() {
                    clicked_count += 1;
                }
            },
        );
        assert!(
            started_blank,
            "dragging empty space should start window drag"
        );
        assert_eq!(clicked_count, 0);

        // 2. Normal click on a button activates it without starting window drag.
        let started_click = run_drag_sequence(
            &ctx,
            &mut base_time,
            pos2(30.0, 42.0),
            pos2(31.0, 42.0),
            true,
            |ui| {
                ui.add(egui::TextEdit::singleline(&mut text).desired_width(120.0));
                if ui.button("Action").clicked() {
                    clicked_count += 1;
                }
            },
        );
        assert!(
            !started_click,
            "normal button click must not trigger StartDrag"
        );
        assert_eq!(clicked_count, 1);

        // 3. Pressing and dragging (> 6px) starting on a button drags the window and cancels the click.
        let started_btn_drag = run_drag_sequence(
            &ctx,
            &mut base_time,
            pos2(30.0, 42.0),
            pos2(55.0, 42.0),
            false,
            |ui| {
                ui.add(egui::TextEdit::singleline(&mut text).desired_width(120.0));
                if ui.button("Action").clicked() {
                    clicked_count += 1;
                }
            },
        );
        assert!(
            started_btn_drag,
            "dragging across a button should start window drag"
        );
        assert_eq!(clicked_count, 1);

        // 4. Dragging inside a TextEdit selects text rather than dragging the window.
        let ctx_text = egui::Context::default();
        let mut text_time = 10.0;
        let started_text_edit = run_drag_sequence(
            &ctx_text,
            &mut text_time,
            pos2(20.0, 14.0),
            pos2(80.0, 14.0),
            false,
            |ui| {
                ui.add(egui::TextEdit::singleline(&mut text).desired_width(120.0));
            },
        );
        assert!(
            !started_text_edit,
            "dragging inside TextEdit must keep text selection instead of dragging window"
        );
    }

    #[test]
    fn workbench_expansion_flips_left_near_right_edge_and_collapses_back_cleanly() {
        use super::{plan_workbench_collapse, plan_workbench_expansion};
        use crate::platform::ScreenRect;
        use crate::ui::popup::WorkbenchSide;

        let monitor = ScreenRect {
            x: 0.0,
            y: 25.0,
            width: 1920.0,
            height: 1055.0,
        };

        // 1. Plenty of space on the right -> opens rightward from original origin.
        let (side, pos) = plan_workbench_expansion((400.0, 120.0), (424.0, 520.0), 760.0, monitor);
        assert_eq!(side, WorkbenchSide::Right);
        assert_eq!(pos, (400.0, 120.0));

        // 2. Window near the right screen edge -> flips leftward and anchors right edge.
        let compact_x = 1480.0; // right = 1904; 1480 + 760 = 2240 > 1916
        let (side_r, pos_r) =
            plan_workbench_expansion((compact_x, 120.0), (424.0, 520.0), 760.0, monitor);
        assert_eq!(side_r, WorkbenchSide::Left);
        assert_eq!(pos_r.0, 1144.0);
        assert_eq!(pos_r.0 + 760.0, compact_x + 424.0);
        assert!(pos_r.0 + 760.0 <= monitor.x + monitor.width - 4.0);

        // Collapsing back restores exact compact right-side origin.
        let collapsed = plan_workbench_collapse(pos_r, side_r, 424.0, 760.0, 520.0, monitor);
        assert_eq!(collapsed, (compact_x, 120.0));

        // 3. Secondary monitor on the left (negative coordinates) near its right edge.
        let left_monitor = ScreenRect {
            x: -1920.0,
            y: 0.0,
            width: 1920.0,
            height: 1080.0,
        };
        let (side_neg, pos_neg) =
            plan_workbench_expansion((-440.0, 80.0), (424.0, 520.0), 760.0, left_monitor);
        assert_eq!(side_neg, WorkbenchSide::Left);
        assert_eq!(pos_neg.0, -776.0);
        assert!(pos_neg.0 + 760.0 <= left_monitor.x + left_monitor.width - 4.0);
    }

    #[test]
    fn pending_left_collapse_keeps_card_screen_x_even_if_outer_pos_updates_before_resize() {
        use super::compute_animated_card_rect;
        use crate::ui::popup::WorkbenchSide;

        let panel_760 = egui::Rect::from_min_size(pos2(0.0, 0.0), vec2(760.0, 520.0));

        // 1. OS viewport still at expanded left origin (1144), target compact X = 1480:
        //    card stays at local [336..760], which maps to screen [1480..1904].
        let rect_before_move = compute_animated_card_rect(
            panel_760,
            WorkbenchSide::Left,
            0.0,
            Some((1480.0, Some(1144.0))),
        );
        assert!((rect_before_move.min.x - 336.0).abs() < 0.5);
        assert!((rect_before_move.width() - 424.0).abs() < 0.5);

        // 2. If OS OuterPosition jumped to 1480 before WindowEvent::Resized shrunk panel_rect:
        //    card pins to local [0..424] so screen position stays [1480..1904] without jumping right.
        let rect_after_move = compute_animated_card_rect(
            panel_760,
            WorkbenchSide::Left,
            0.0,
            Some((1480.0, Some(1480.0))),
        );
        assert!((rect_after_move.min.x - 0.0).abs() < 0.5);
        assert!((rect_after_move.width() - 424.0).abs() < 0.5);
    }

    #[test]
    fn pinned_window_never_auto_hides_on_blur() {
        use super::{FocusBlurCheck, ViewMode, should_auto_hide_on_blur};
        use std::time::Duration;

        // Unpinned: clicking another window hides Earth after grace period.
        assert!(should_auto_hide_on_blur(&FocusBlurCheck {
            visible: true,
            focused: false,
            was_focused_since_shown: true,
            view_mode: ViewMode::Launcher,
            show_about_modal: false,
            dialog_open: false,
            window_pinned: false,
            since_shown: Duration::from_millis(500),
        }));

        // Pinned: clicking another window must keep Earth open.
        assert!(!should_auto_hide_on_blur(&FocusBlurCheck {
            visible: true,
            focused: false,
            was_focused_since_shown: true,
            view_mode: ViewMode::Launcher,
            show_about_modal: false,
            dialog_open: false,
            window_pinned: true,
            since_shown: Duration::from_millis(5000),
        }));
    }

    #[test]
    fn workbench_animation_clamps_os_resize_stall_and_never_skips_opening_frames() {
        use super::{WORKBENCH_OPEN_DURATION, WorkbenchAnimation};
        use std::time::{Duration, Instant};

        let t0 = Instant::now();
        let mut anim = WorkbenchAnimation::new(0.0, 1.0, WORKBENCH_OPEN_DURATION, true);

        // Before the OS viewport finishes growing to expanded width, reveal stays at 0.0
        let (r_wait, done_wait) = anim.step(t0 + Duration::from_millis(30), 1.0 / 60.0, false);
        assert_eq!(r_wait, 0.0);
        assert!(!done_wait);

        // Even if Cocoa took 85ms to reallocate the backing surface, the first ready frame
        // starts cleanly with one vsync step instead of jumping past 70%+ of the slide.
        let t_ready = t0 + Duration::from_millis(115);
        let (r_first, done_first) = anim.step(t_ready, 1.0 / 60.0, true);
        assert!(
            r_first > 0.05 && r_first < 0.35,
            "first visible frame should start smoothly, got reveal={r_first}"
        );
        assert!(!done_first);

        // Even if a single background hiccup stalls a mid-animation frame by 90ms, clamped dt
        // prevents teleporting straight to completion.
        let (r_hitch, done_hitch) =
            anim.step(t_ready + Duration::from_millis(90), 1.0 / 60.0, true);
        assert!(
            r_hitch > r_first && r_hitch < 0.70,
            "hitch frame reveal={r_hitch}"
        );
        assert!(!done_hitch);

        // Subsequent normal 60Hz frames glide smoothly to completion (1.0)
        let mut t = t_ready + Duration::from_millis(90);
        let mut finished = false;
        let mut last_r = r_hitch;
        for _ in 0..20 {
            t += Duration::from_millis(16);
            let (r, done) = anim.step(t, 1.0 / 60.0, true);
            assert!(r >= last_r, "reveal must advance monotonically");
            last_r = r;
            if done {
                finished = true;
                break;
            }
        }
        assert!(finished);
        assert_eq!(last_r, 1.0);
    }

    #[test]
    fn newest_result_id_selects_latest_copy_even_with_pinned_items_at_top() {
        use super::newest_result_id;
        use crate::model::{ClipItem, ClipKind};
        use crate::search::{MatchMask, SearchResultItem};
        use std::sync::Arc;

        assert_eq!(newest_result_id(&[]), None);

        let make_item = |id: i64, last_copied_at: i64, pinned: bool| SearchResultItem {
            item: Arc::new(ClipItem {
                id,
                kind: ClipKind::Text,
                content_hash: format!("h{id}"),
                text_content: Some(format!("item {id}")),
                html_content: None,
                rtf_content: None,
                image_path: None,
                thumb_path: None,
                image_width: None,
                image_height: None,
                files: Vec::new(),
                byte_size: 8,
                source_app: None,
                source_bundle_id: None,
                first_copied_at: last_copied_at,
                last_copied_at,
                copy_count: 1,
                pinned,
                collection_id: None,
                note: None,
                text_truncated: false,
                has_rich_text: false,
                detected_color: None,
                is_url: false,
                sensitive_label: None,
            }),
            score: 0,
            matched_indices: MatchMask::default(),
            display_text: None,
        };

        // Pinned old item sits at index 0, newest unpinned copy sits below it.
        let results = vec![
            make_item(1, 1000, true),
            make_item(2, 2000, true),
            make_item(3, 5000, false),
            make_item(4, 3000, false),
        ];
        assert_eq!(newest_result_id(&results), Some(3));

        // When a pinned item itself was re-copied most recently, select that pinned item.
        let recopied_pinned = vec![
            make_item(1, 9000, true),
            make_item(3, 5000, false),
            make_item(4, 3000, false),
        ];
        assert_eq!(newest_result_id(&recopied_pinned), Some(1));
    }

    #[test]
    fn reopening_launcher_from_hidden_resets_stale_search_and_filter() {
        use super::prepare_launcher_reopen;
        use crate::model::CategoryFilter;
        use crate::ui::popup::PopupState;

        let mut state = PopupState {
            search_query: "stale query".into(),
            filter: CategoryFilter::Image,
            preview_open: true,
            inline_edit_text: Some((1, "draft".into())),
            inline_edit_note: Some((1, "note".into())),
            ..Default::default()
        };
        let mut dirty = false;

        // While already visible, switching back to Launcher keeps active query/filter.
        prepare_launcher_reopen(true, &mut state, &mut dirty);
        assert_eq!(state.search_query, "stale query");
        assert_eq!(state.filter, CategoryFilter::Image);
        assert!(!dirty);
        assert!(!state.preview_open);

        // Reopening from hidden resets stale query and filter so the newest item is always visible.
        prepare_launcher_reopen(false, &mut state, &mut dirty);
        assert!(state.search_query.is_empty());
        assert_eq!(state.filter, CategoryFilter::All);
        assert!(dirty);
        assert!(state.inline_edit_text.is_none());
        assert!(state.inline_edit_note.is_none());
    }
}
