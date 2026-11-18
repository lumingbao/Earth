use crate::clipboard::{
    clear_system_clipboard, start_clipboard_watcher, write_custom_png_to_clipboard,
    write_custom_text_to_clipboard, write_item_to_clipboard, ClipboardEvent, ClipboardHandle,
    ClipboardSuppression,
};
use crate::hotkey::GlobalHotkeyController;
use crate::model::{
    truncate_chars, ActiveAppInfo, AppSettings, ClipItem, Collection, PasteBehavior, ThemeMode,
};
use crate::platform;
use crate::search::{filter_and_search, SearchResultItem};
use crate::storage::Storage;
use crate::tray::{EarthTray, TrayAction};
use crate::ui::popup::{
    render_popup_view, PopupAction, PopupState, PopupViewData, WorkbenchSide,
};
use crate::ui::settings::{render_settings_view, SettingsAction, SettingsState, SettingsViewData};
use crate::ui::theme::{install_crisp_fonts, EarthPalette};
use crate::updater::{
    apply_staged_update_and_restart, spawn_update_check, spawn_update_download, UpdateCheckOutcome,
    UpdateDownloadState, UpdaterEvent,
};
use egui::{pos2, vec2, CornerRadius, Stroke, ViewportCommand};
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

pub struct EarthApp {
    storage: Storage,
    shared_settings: Arc<RwLock<AppSettings>>,
    settings: AppSettings,
    suppression: Arc<Mutex<ClipboardSuppression>>,
    clipboard_rx: Receiver<ClipboardEvent>,
    _clipboard_handle: Option<ClipboardHandle>,
    update_tx: Sender<UpdaterEvent>,
    update_rx: Receiver<UpdaterEvent>,
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
    pub fn new(cc: &eframe::CreationContext<'_>, start_hidden: bool) -> Self {
        egui_extras::install_image_loaders(&cc.egui_ctx);
        install_crisp_fonts(&cc.egui_ctx);
        platform::configure_macos_accessory_mode();
        platform::configure_main_window(cc);

        let storage = Storage::open_default().expect("无法初始化 Earth 本地数据库");
        let settings = storage.load_settings();
        if settings.launch_at_login {
            let _ = platform::refresh_autostart_if_enabled();
        }
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

        let (update_tx, update_rx) = std::sync::mpsc::channel();
        if settings.check_updates {
            spawn_update_check(false, update_tx.clone(), cc.egui_ctx.clone());
        }

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
            popup_state.set_status_banner(warn, true);
        }

        let initial_size = (COMPACT_WIDTH, WINDOW_HEIGHT);
        let prev_app = if start_hidden {
            None
        } else {
            platform::capture_frontmost_app()
        };
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
        if start_hidden {
            cc.egui_ctx
                .send_viewport_cmd(ViewportCommand::Visible(false));
        } else {
            cc.egui_ctx.send_viewport_cmd(ViewportCommand::Focus);
            platform::activate_self_window();
        }

        let cached_palette = EarthPalette::resolve(settings.theme_mode, false);
        let cached_accessibility = platform::has_accessibility_permission();
        let mut app = Self {
            storage,
            shared_settings,
            settings,
            suppression,
            clipboard_rx,
            _clipboard_handle: clipboard_handle,
            update_tx,
            update_rx,
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
        app
    }

    fn reload_items(&mut self) {
        self.items = self.storage.list_items().unwrap_or_default();
        self.collections = self.storage.list_collections().unwrap_or_default();
        self.storage_bytes = self.storage.total_storage_bytes();
        self.popup_state.preview_cache = None;
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
        let unpinned_count = self
            .items
            .iter()
            .filter(|it| !it.pinned && it.collection_id.is_none())
            .count();
        if unpinned_count > self.settings.max_history {
            let _ = self.storage.enforce_history_limit(self.settings.max_history);
            self.reload_items();
        } else {
            self.search_dirty = true;
            self.sync_tray();
        }
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
                    ctx.send_viewport_cmd(ViewportCommand::OuterPosition(pos2(
                        target_x, target_y,
                    )));
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
                egui::viewport::WindowLevel::AlwaysOnTop,
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
                self.popup_state.set_status_banner(
                    "已复制到剪贴板，但自动粘贴需要 macOS「辅助功能」权限（请按 ⌘V 粘贴）",
                    true,
                );
                ctx.request_repaint();
                return;
            }
            if self.popup_state.window_pinned {
                if let Some(ref prev) = self.prev_app {
                    platform::restore_frontmost_app(prev);
                }
                self.apply_window_pinned_state(ctx);
                self.popup_state.set_status_banner(pinned_paste_msg, false);
                ctx.request_repaint();
            } else {
                self.hide_popup(ctx, true);
            }
            std::thread::spawn(|| {
                if let Err(err) = platform::simulate_paste() {
                    eprintln!("自动粘贴失败: {err}");
                }
            });
        } else if self.popup_state.window_pinned {
            self.popup_state.set_status_banner(pinned_copy_msg, false);
            ctx.request_repaint();
        } else {
            self.hide_popup(ctx, true);
        }
    }

    fn show_popup(&mut self, ctx: &egui::Context, mode: ViewMode, tray_pos: Option<(f32, f32)>) {
        if let Some(active) = platform::capture_frontmost_app() {
            self.prev_app = Some(active);
        }
        if !self.visible {
            self.popup_state.window_pinned = false;
        }
        self.view_mode = mode;
        self.workbench_anim = None;
        self.workbench_collapse_pending = false;
        self.workbench_os_expanded = false;
        platform::set_window_resize_anchor_right(false);
        self.popup_state.workbench_side = WorkbenchSide::Right;
        self.popup_state.preview_reveal = None;
        if mode == ViewMode::Launcher {
            self.popup_state.request_search_focus = true;
            self.popup_state.ime_composing = false;
            self.popup_state.selection_changed_at = Instant::now();
            self.popup_state.preview_manually_toggled = false;
            self.popup_state.preview_open = false;
            self.popup_state.active_transform = None;
            self.popup_state.inline_edit_text = None;
            self.popup_state.inline_edit_note = None;
            self.popup_state.qr_preview_id = None;
            self.popup_state.reset_list_navigation();
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
        );
        self.last_window_pos = (x, y);

        ctx.send_viewport_cmd(ViewportCommand::InnerSize(vec2(target_size.0, target_size.1)));
        ctx.send_viewport_cmd(ViewportCommand::OuterPosition(pos2(x, y)));
        ctx.send_viewport_cmd(ViewportCommand::Visible(true));
        ctx.send_viewport_cmd(ViewportCommand::Focus);
        platform::activate_self_window();
        self.apply_window_pinned_state(ctx);

        self.visible = true;
        self.last_shown_at = Instant::now();
        self.was_focused_since_shown = false;
        self.last_known_focused = false;
        ctx.request_repaint();
    }

    fn hide_popup(&mut self, ctx: &egui::Context, restore_focus: bool) {
        self.visible = false;
        self.popup_state.show_about_modal = false;
        self.settings_state.recording = None;
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
            self.popup_state.set_status_banner(err, true);
            ctx.request_repaint();
            return;
        }

        let _ = self.storage.mark_copied(id);
        self.reload_items();

        self.complete_copy_or_paste(ctx, do_paste, "✓ 已复制到剪贴板", "✓ 已粘贴到当前目标窗口");
    }

    fn handle_paste_transformed(&mut self, ctx: &egui::Context, text: String, paste_now: bool) {
        if let Err(err) = write_custom_text_to_clipboard(&text, &self.suppression) {
            self.popup_state.set_status_banner(err, true);
            ctx.request_repaint();
            return;
        }
        if paste_now {
            self.complete_copy_or_paste(
                ctx,
                true,
                "✓ 已复制加工结果到剪贴板",
                "✓ 已粘贴加工结果到目标窗口",
            );
        } else {
            self.popup_state
                .set_status_banner("✓ 已复制加工结果到剪贴板", false);
            ctx.request_repaint();
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
                                        format!(
                                            "🎉 发现新版本 v{}，正在后台自动下载更新…",
                                            info.version
                                        ),
                                        false,
                                    );
                                }
                            } else if manual {
                                self.settings_state.set_status_message(
                                    format!(
                                        "🎉 发现新版本 v{}，可点击「重启完成更新」或「前往下载」",
                                        info.version
                                    ),
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
                                    format!("✓ 当前已是最新版本 (v{ver})"),
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
                            format!("✓ 新版本 v{version} 已下载就绪，点击「重启完成更新」即可生效"),
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

        // Track focus: hide on blur normally, or keep floating above other apps while pinned.
        if self.visible {
            let focused = ctx.input(|i| i.focused);
            if focused {
                self.was_focused_since_shown = true;
            } else if self.popup_state.window_pinned {
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

        let system_dark = ctx
            .system_theme()
            .map(|t| t == egui::Theme::Dark)
            .unwrap_or_else(|| ctx.theme() == egui::Theme::Dark);
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
                ui.painter().rect_filled(
                    outer_rect,
                    CornerRadius::same(18),
                    palette.window_bg,
                );
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
                                let total_items = self.items.len();

                                let actions = render_popup_view(
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
                                    },
                                    &mut self.popup_state,
                                );

                                if self.popup_state.search_query != prev_query
                                    || self.popup_state.filter != prev_filter
                                {
                                    self.popup_state.reset_list_navigation();
                                    self.search_dirty = true;
                                    ctx.request_repaint();
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
                                        PopupAction::SetItemNote { item_id, note } => {
                                            let _ = self
                                                .storage
                                                .set_item_note(item_id, note.as_deref());
                                            self.reload_items();
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
                                                            "✓ 已保存对记录内容的修改",
                                                            false,
                                                        );
                                                        ctx.request_repaint();
                                                    }
                                                }
                                                Err(err) => {
                                                    self.popup_state.set_status_banner(err, true);
                                                    ctx.request_repaint();
                                                }
                                            }
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
                                                        format!("保存图片失败: {e}"),
                                                        true,
                                                    );
                                                } else {
                                                    self.popup_state.set_status_banner(
                                                        format!("✓ 已保存到 {}", dest.display()),
                                                        false,
                                                    );
                                                }
                                                ctx.request_repaint();
                                            }
                                            self.dialog_open = false;
                                        }
                                        PopupAction::CopyQrImage(png_bytes) => {
                                            match write_custom_png_to_clipboard(&png_bytes) {
                                                Ok(()) => {
                                                    self.popup_state.set_status_banner(
                                                        "✓ 已复制二维码图片到剪贴板",
                                                        false,
                                                    );
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
                                                        format!("保存二维码失败: {e}"),
                                                        true,
                                                    );
                                                } else {
                                                    self.popup_state.set_status_banner(
                                                        format!("✓ 二维码已保存到 {}", dest.display()),
                                                        false,
                                                    );
                                                }
                                                ctx.request_repaint();
                                            }
                                            self.dialog_open = false;
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

                                for action in actions {
                                    match action {
                                        SettingsAction::CloseSettings => {
                                            self.view_mode = ViewMode::Launcher;
                                            self.popup_state.request_search_focus = true;
                                            ctx.request_repaint();
                                        }
                                        SettingsAction::SettingsChanged => {
                                            self.persist_settings();
                                        }
                                        SettingsAction::OpenHotkeyChanged(new_combo) => {
                                            if let Some(ctrl) = &mut self.hotkey_ctrl {
                                                match ctrl.set_open_shortcut(&new_combo) {
                                                    Ok(()) => {
                                                        self.settings_state.set_status_message(
                                                            "✓ 全局唤醒快捷键已生效",
                                                            false,
                                                        );
                                                    }
                                                    Err(e) => {
                                                        self.settings_state
                                                            .set_status_message(e, true);
                                                    }
                                                }
                                            }
                                            self.persist_settings();
                                        }
                                        SettingsAction::ToggleAutostart(enabled) => {
                                            match platform::sync_autostart(enabled) {
                                                Ok(()) => {
                                                    self.settings_state.set_status_message(
                                                        if enabled {
                                                            "✓ 已开启登录自启"
                                                        } else {
                                                            "✓ 已关闭登录自启"
                                                        },
                                                        false,
                                                    );
                                                }
                                                Err(e) => {
                                                    self.settings.launch_at_login = !enabled;
                                                    self.settings_state
                                                        .set_status_message(e, true);
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
                                            self.settings_state
                                                .set_status_message("✓ 历史记录已清理", false);
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
                                                        self.settings_state.set_status_message(
                                                            format!(
                                                                "✓ 已导出至 {}",
                                                                PathBuf::from(&dest).display()
                                                            ),
                                                            false,
                                                        );
                                                    }
                                                    Err(e) => {
                                                        self.settings_state.set_status_message(
                                                            format!("导出失败: {e}"),
                                                            true,
                                                        );
                                                    }
                                                }
                                            }
                                            self.dialog_open = false;
                                        }
                                        SettingsAction::ImportJson => {
                                            self.dialog_open = true;
                                            if let Some(src) = rfd::FileDialog::new()
                                                .add_filter("JSON", &["json"])
                                                .pick_file()
                                            {
                                                let res = std::fs::read_to_string(&src)
                                                    .map_err(|e| format!("读取文件失败: {e}"))
                                                    .and_then(|raw| {
                                                        serde_json::from_str::<Vec<ClipItem>>(&raw)
                                                            .map_err(|e| {
                                                                format!("解析 JSON 失败: {e}")
                                                            })
                                                    })
                                                    .and_then(|parsed| {
                                                        self.storage.import_items(
                                                            &parsed,
                                                            self.settings.max_history,
                                                        )
                                                    });
                                                match res {
                                                    Ok(count) => {
                                                        self.reload_items();
                                                        self.settings_state.set_status_message(
                                                            format!(
                                                                "✓ 导入完成，新增 {count} 条记录（重复记录已自动跳过）"
                                                            ),
                                                            false,
                                                        );
                                                    }
                                                    Err(e) => {
                                                        self.settings_state
                                                            .set_status_message(e, true);
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
                                                    self.settings_state.set_status_message(
                                                        format!("✓ 已创建收藏分组「{name}」"),
                                                        false,
                                                    );
                                                }
                                                Err(e) => {
                                                    self.settings_state
                                                        .set_status_message(e, true);
                                                }
                                            }
                                        }
                                        SettingsAction::RenameCollection { id, name } => {
                                            match self.storage.rename_collection(id, &name) {
                                                Ok(()) => {
                                                    self.reload_items();
                                                    self.settings_state.set_status_message(
                                                        format!("✓ 已重命名分组为「{name}」"),
                                                        false,
                                                    );
                                                }
                                                Err(e) => {
                                                    self.settings_state
                                                        .set_status_message(e, true);
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
                    });
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
        let local_x = (target_screen_x - ox)
            .clamp(0.0, (panel_rect.width() - COMPACT_WIDTH).max(0.0));
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
        (WorkbenchSide::Right, (win_x.clamp(mon_left, max_x), clamped_y))
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
    use egui::{pos2, vec2, Modifiers, ViewportCommand, ViewportId};

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
                screen_rect: Some(egui::Rect::from_min_size(pos2(0.0, 0.0), vec2(424.0, 520.0))),
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
        assert!(started_blank, "dragging empty space should start window drag");
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
        assert!(!started_click, "normal button click must not trigger StartDrag");
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
        assert!(started_btn_drag, "dragging across a button should start window drag");
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
        let rect_before_move =
            compute_animated_card_rect(panel_760, WorkbenchSide::Left, 0.0, Some((1480.0, Some(1144.0))));
        assert!((rect_before_move.min.x - 336.0).abs() < 0.5);
        assert!((rect_before_move.width() - 424.0).abs() < 0.5);

        // 2. If OS OuterPosition jumped to 1480 before WindowEvent::Resized shrunk panel_rect:
        //    card pins to local [0..424] so screen position stays [1480..1904] without jumping right.
        let rect_after_move =
            compute_animated_card_rect(panel_760, WorkbenchSide::Left, 0.0, Some((1480.0, Some(1480.0))));
        assert!((rect_after_move.min.x - 0.0).abs() < 0.5);
        assert!((rect_after_move.width() - 424.0).abs() < 0.5);
    }

    #[test]
    fn pinned_window_never_auto_hides_on_blur() {
        use super::{should_auto_hide_on_blur, FocusBlurCheck, ViewMode};
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
        use super::{WorkbenchAnimation, WORKBENCH_OPEN_DURATION};
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
        assert!(r_hitch > r_first && r_hitch < 0.70, "hitch frame reveal={r_hitch}");
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
}
