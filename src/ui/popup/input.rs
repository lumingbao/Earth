use super::*;

pub(super) const NAV_HOLD_INITIAL_DELAY_SECS: f64 = 0.20;
pub(super) const NAV_HOLD_RAMP_SECS: f32 = 0.90;
pub(super) const NAV_HOLD_START_INTERVAL_MS: f32 = 76.0;
pub(super) const NAV_HOLD_MIN_INTERVAL_MS: f32 = 16.0;

/// Returns the repeat interval while holding Up/Down.
/// Returns `None` during the initial tap window so slow taps step strictly 1 item at a time,
/// then smoothly accelerates from ~13 items/sec up to ~62 items/sec as the key stays held.
pub(super) fn nav_repeat_interval(hold_elapsed: Duration) -> Option<Duration> {
    let elapsed_secs = hold_elapsed.as_secs_f64();
    if elapsed_secs + 1e-6 < NAV_HOLD_INITIAL_DELAY_SECS {
        return None;
    }
    let t = (((elapsed_secs - NAV_HOLD_INITIAL_DELAY_SECS) as f32) / NAV_HOLD_RAMP_SECS)
        .clamp(0.0, 1.0);
    let ratio = NAV_HOLD_MIN_INTERVAL_MS / NAV_HOLD_START_INTERVAL_MS;
    let interval_ms = NAV_HOLD_START_INTERVAL_MS * ratio.powf(t);
    Some(Duration::from_secs_f32(interval_ms / 1000.0))
}

pub(super) fn handle_popup_keyboard(
    ctx: &egui::Context,
    settings: &mut AppSettings,
    results: &[SearchResultItem],
    collections: &[Collection],
    state: &mut PopupState,
    actions: &mut Vec<PopupAction>,
) {
    if state.show_about_modal {
        state.nav_hold = None;
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

    // Dismiss the visible transient layer before changing search or closing the popup.
    if ctx.input_mut(|i| i.consume_key(Modifiers::NONE, Key::Escape)) {
        state.nav_hold = None;
        if state.status_banner.take().is_some() {
            return;
        } else if editing_workbench {
            state.inline_edit_text = None;
            state.inline_edit_note = None;
        } else if !state.search_query.is_empty() {
            state.search_query.clear();
            state.request_search_focus = true;
            state.reset_list_navigation();
        } else if !state.window_pinned {
            actions.push(PopupAction::HidePopup);
        }
        return;
    }

    // While editing text/note inside the workbench, let the editor handle arrows/Enter/Tab
    if editing_workbench {
        state.nav_hold = None;
        return;
    }

    // Tab / Shift+Tab and Left / Right arrows cycle through category & collection filter capsules
    // with wrap-around from rightmost to leftmost and vice versa.
    let allow_arrow_filter_switch = state.search_query.is_empty() || !state.search_focused;
    let shift_tab = ctx.input_mut(|i| i.consume_key(Modifiers::SHIFT, Key::Tab));
    let plain_tab = !shift_tab && ctx.input_mut(|i| i.consume_key(Modifiers::NONE, Key::Tab));
    let arrow_left = allow_arrow_filter_switch
        && ctx.input_mut(|i| i.consume_key(Modifiers::NONE, Key::ArrowLeft));
    let arrow_right = allow_arrow_filter_switch
        && !arrow_left
        && ctx.input_mut(|i| i.consume_key(Modifiers::NONE, Key::ArrowRight));
    if shift_tab || plain_tab || arrow_left || arrow_right {
        let backward = shift_tab || arrow_left;
        state.filter = cycle_category_filter(state.filter, collections, backward);
        state.scroll_to_filter = true;
        state.active_transform = None;
        state.reset_list_navigation();
        ctx.request_repaint();
        return;
    }

    // Navigation: Up / Down (slow taps step 1-by-1; holding accelerates smoothly)
    if results.is_empty() {
        state.nav_hold = None;
    } else {
        let mut cur_idx = state
            .selected_id
            .and_then(|id| results.iter().position(|r| r.item.id == id))
            .unwrap_or(0);

        let (now_secs, focused, mods_none, down_held, up_held, fresh_dir) = ctx.input_mut(|i| {
            let focused = i.focused;
            let mods_none = i.modifiers.is_none();
            let mut fresh_dir: i32 = 0;

            // Consume both initial and OS-repeat plain ArrowUp / ArrowDown press events so
            // TextEdit never steals them and slow OS repeat ticks never fight our hold curve.
            i.events.retain(|ev| {
                if let egui::Event::Key {
                    key,
                    pressed: true,
                    repeat,
                    modifiers,
                    ..
                } = ev
                    && modifiers.is_none()
                    && matches!(key, Key::ArrowDown | Key::ArrowUp)
                {
                    if !*repeat {
                        fresh_dir = if *key == Key::ArrowDown { 1 } else { -1 };
                    }
                    return false;
                }
                true
            });

            (
                i.time,
                focused,
                mods_none,
                i.key_down(Key::ArrowDown),
                i.key_down(Key::ArrowUp),
                fresh_dir,
            )
        });

        let mut step_delta: i32 = 0;
        let mut keep_hold_repaint = false;

        if fresh_dir != 0 {
            step_delta += fresh_dir;
            let still_held = focused
                && mods_none
                && ((fresh_dir > 0 && down_held) || (fresh_dir < 0 && up_held));
            if still_held {
                state.nav_hold = Some(VerticalNavHold {
                    dir: fresh_dir,
                    pressed_at: now_secs,
                    next_step_at: now_secs + NAV_HOLD_INITIAL_DELAY_SECS,
                });
                keep_hold_repaint = true;
            } else {
                state.nav_hold = None;
            }
        } else if let Some(mut hold) = state.nav_hold {
            let active_held =
                focused && mods_none && ((hold.dir > 0 && down_held) || (hold.dir < 0 && up_held));
            if !active_held {
                state.nav_hold = None;
            } else {
                if now_secs < hold.pressed_at {
                    hold.pressed_at = now_secs;
                    hold.next_step_at = now_secs + NAV_HOLD_INITIAL_DELAY_SECS;
                }
                let mut steps_this_frame = 0;
                while now_secs + 1e-6 >= hold.next_step_at && steps_this_frame < 3 {
                    let hold_elapsed =
                        Duration::from_secs_f64((now_secs - hold.pressed_at).max(0.0));
                    let Some(interval) = nav_repeat_interval(hold_elapsed) else {
                        break;
                    };
                    step_delta += hold.dir;
                    hold.next_step_at += interval.as_secs_f64();
                    steps_this_frame += 1;
                }
                if hold.next_step_at < now_secs {
                    hold.next_step_at = now_secs;
                }
                state.nav_hold = Some(hold);
                keep_hold_repaint = true;
            }
        }

        if step_delta != 0 {
            let target_idx = if step_delta > 0 {
                cur_idx
                    .saturating_add(step_delta as usize)
                    .min(results.len() - 1)
            } else {
                cur_idx.saturating_sub(step_delta.unsigned_abs() as usize)
            };
            if target_idx != cur_idx {
                cur_idx = target_idx;
                state.select_item(Some(results[cur_idx].item.id));
                state.selection_changed_at = Instant::now();
                state.scroll_to_selection = true;
                state.active_transform = None;
                ctx.request_repaint();
            }
        }

        if keep_hold_repaint {
            let at_boundary = state.nav_hold.is_some_and(|h| {
                (h.dir > 0 && cur_idx + 1 >= results.len()) || (h.dir < 0 && cur_idx == 0)
            });
            if !at_boundary {
                ctx.request_repaint();
            }
        }

        if ctx.input_mut(|i| i.consume_key(Modifiers::NONE, Key::PageDown)) {
            state.nav_hold = None;
            let next = (cur_idx + 8).min(results.len() - 1);
            state.select_item(Some(results[next].item.id));
            state.selection_changed_at = Instant::now();
            state.scroll_to_selection = true;
            state.active_transform = None;
            ctx.request_repaint();
        } else if ctx.input_mut(|i| i.consume_key(Modifiers::NONE, Key::PageUp)) {
            state.nav_hold = None;
            let prev = cur_idx.saturating_sub(8);
            state.select_item(Some(results[prev].item.id));
            state.selection_changed_at = Instant::now();
            state.scroll_to_selection = true;
            state.active_transform = None;
            ctx.request_repaint();
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
        && let Some(id) = state.selected_id
    {
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

        if triggered && let Some(item) = results.get(idx) {
            actions.push(PopupAction::PasteItem {
                id: item.item.id,
                behavior: PasteBehavior::Default,
            });
            return;
        }
    }

    // Pin, Delete, Preview, Settings, Clear, Quit shortcuts
    let allow_item_shortcuts =
        !state.search_focused && !state.request_search_focus && !ctx.egui_wants_keyboard_input();
    let (pin_hit, del_hit, preview_hit, settings_hit, clear_hit, quit_hit) = ctx.input_mut(|i| {
        let pin = allow_item_shortcuts && consume_egui_shortcut(i, &settings.hotkey_pin);
        let del = allow_item_shortcuts && consume_egui_shortcut(i, &settings.hotkey_delete);
        let prev = allow_item_shortcuts && consume_egui_shortcut(i, &settings.hotkey_preview);
        #[cfg(target_os = "macos")]
        let (prefs_key, clear_key, quit_key) = ("Super+Comma", "Super+Alt+Backspace", "Super+Q");
        #[cfg(not(target_os = "macos"))]
        let (prefs_key, clear_key, quit_key) = ("Ctrl+Comma", "Ctrl+Alt+Backspace", "Ctrl+Q");

        let prefs = consume_egui_shortcut(i, prefs_key);
        let clear = allow_item_shortcuts && consume_egui_shortcut(i, clear_key);
        let quit = consume_egui_shortcut(i, quit_key);
        (pin, del, prev, prefs, clear, quit)
    });

    if pin_hit && let Some(id) = state.selected_id {
        actions.push(PopupAction::TogglePin(id));
    }
    if del_hit && let Some(id) = state.selected_id {
        actions.push(PopupAction::DeleteItem(id));
    }
    if preview_hit {
        state.toggle_preview();
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

pub(super) fn is_masked(item: &ClipItem, privacy_masking: bool, state: &PopupState) -> bool {
    privacy_masking && item.is_sensitive() && !state.revealed_sensitive_ids.contains(&item.id)
}

pub(super) fn cycle_category_filter(
    current: CategoryFilter,
    collections: &[Collection],
    backward: bool,
) -> CategoryFilter {
    let mut filters = Vec::with_capacity(5 + collections.len());
    filters.extend_from_slice(&[
        CategoryFilter::All,
        CategoryFilter::Text,
        CategoryFilter::Image,
        CategoryFilter::Files,
        CategoryFilter::Pinned,
    ]);
    for col in collections {
        filters.push(CategoryFilter::Collection(col.id));
    }
    let cur = filters.iter().position(|&f| f == current).unwrap_or(0);
    let next = if backward {
        if cur == 0 { filters.len() - 1 } else { cur - 1 }
    } else {
        (cur + 1) % filters.len()
    };
    filters[next]
}

pub(super) fn behavior_from_modifiers(mods: Modifiers) -> PasteBehavior {
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
