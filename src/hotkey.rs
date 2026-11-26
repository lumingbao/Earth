use crate::i18n::{t, tf};
use egui::{InputState, Key, Modifiers};
use global_hotkey::hotkey::HotKey;
use global_hotkey::{GlobalHotKeyEvent, GlobalHotKeyManager, HotKeyState};
use std::str::FromStr;
use std::sync::mpsc::{self, Receiver};

pub struct GlobalHotkeyController {
    manager: GlobalHotKeyManager,
    active_open_hotkey: Option<HotKey>,
    desired_open_shortcut: String,
    suspended: bool,
    rx: Receiver<GlobalHotKeyEvent>,
}

impl GlobalHotkeyController {
    pub fn new(
        initial_open_shortcut: &str,
        repaint_ctx: egui::Context,
    ) -> Result<(Self, Option<String>), String> {
        let manager =
            GlobalHotKeyManager::new().map_err(|e| format!("初始化全局快捷键管理器失败: {e}"))?;
        let (tx, rx) = mpsc::channel();
        GlobalHotKeyEvent::set_event_handler(Some(move |ev| {
            let _ = tx.send(ev);
            repaint_ctx.request_repaint();
        }));
        let mut ctrl = Self {
            manager,
            active_open_hotkey: None,
            desired_open_shortcut: initial_open_shortcut.trim().to_string(),
            suspended: false,
            rx,
        };
        let warn = ctrl
            .set_open_shortcut(initial_open_shortcut)
            .err()
            .map(|error| {
                let fallback = crate::model::AppSettings::default().hotkey_open;
                if ctrl.set_open_shortcut(&fallback).is_ok() {
                    let disp = format_shortcut_display(&fallback);
                    tf(
                        "hotkey.warn_fallback_restored",
                        &[("error", &error), ("fallback", &disp)],
                    )
                } else {
                    error
                }
            });
        Ok((ctrl, warn))
    }

    pub fn set_open_shortcut(&mut self, shortcut: &str) -> Result<(), String> {
        validate_shortcut(shortcut, true)?;
        if self.suspended {
            return Err(t("hotkey.err_recording_active").to_string());
        }
        let previous = self.desired_open_shortcut.clone();
        self.desired_open_shortcut = shortcut.trim().to_string();
        if let Err(error) = self.apply_open_shortcut() {
            self.desired_open_shortcut = previous;
            return Err(error);
        }
        Ok(())
    }

    pub fn set_suspended(&mut self, suspended: bool) -> Result<(), String> {
        if self.suspended == suspended {
            return Ok(());
        }
        while self.rx.try_recv().is_ok() {}
        if suspended && let Some(prev) = self.active_open_hotkey {
            self.manager
                .unregister(prev)
                .map_err(|e| format!("暂停全局快捷键失败: {e}"))?;
            self.active_open_hotkey = None;
        }
        self.suspended = suspended;
        if !suspended {
            self.apply_open_shortcut()?;
        }
        Ok(())
    }

    pub fn is_available(&self) -> bool {
        !self.suspended && self.active_open_hotkey.is_some()
    }

    pub fn open_shortcut(&self) -> &str {
        &self.desired_open_shortcut
    }

    fn apply_open_shortcut(&mut self) -> Result<(), String> {
        let trimmed = self.desired_open_shortcut.trim();
        validate_shortcut(trimmed, true)?;
        if trimmed.is_empty() {
            if let Some(prev) = self.active_open_hotkey.take() {
                let _ = self.manager.unregister(prev);
            }
            return Ok(());
        }

        let hotkey = HotKey::from_str(trimmed)
            .map_err(|e| format!("无法解析全局快捷键 \"{trimmed}\": {e}"))?;

        replace_registered_hotkey(
            &mut self.active_open_hotkey,
            hotkey,
            |next| {
                self.manager.register(next).map_err(|e| {
                    tf(
                        "hotkey.err_register_conflict",
                        &[("shortcut", trimmed), ("error", &e.to_string())],
                    )
                })
            },
            |old| {
                self.manager
                    .unregister(old)
                    .map_err(|e| format!("注销旧快捷键失败: {e}"))
            },
        )
    }

    pub fn poll_open_triggered(&self) -> bool {
        if self.suspended {
            while self.rx.try_recv().is_ok() {}
            return false;
        }
        let Some(target) = self.active_open_hotkey else {
            return false;
        };
        let mut triggered = false;
        while let Ok(ev) = self.rx.try_recv() {
            if ev.id == target.id() && ev.state == HotKeyState::Pressed {
                triggered = true;
            }
        }
        triggered
    }
}

fn replace_registered_hotkey(
    active: &mut Option<HotKey>,
    next: HotKey,
    mut register: impl FnMut(HotKey) -> Result<(), String>,
    mut unregister: impl FnMut(HotKey) -> Result<(), String>,
) -> Result<(), String> {
    if *active == Some(next) {
        return Ok(());
    }
    // Register first: a conflict must never remove the working application entry.
    register(next)?;
    if let Some(previous) = *active
        && let Err(error) = unregister(previous)
    {
        let rollback = unregister(next);
        return Err(match rollback {
            Ok(()) => error,
            Err(rollback) => format!("{error}；回滚新快捷键也失败: {rollback}"),
        });
    }
    *active = Some(next);
    Ok(())
}

/// Shared validation for native and egui recorders, and for global registration.
pub fn validate_shortcut(shortcut: &str, global: bool) -> Result<(), String> {
    use global_hotkey::hotkey::{Code, Modifiers as HotkeyMods};
    let hotkey = HotKey::from_str(shortcut)
        .map_err(|e| tf("hotkey.err_parse", &[("error", &e.to_string())]))?;
    let mods = hotkey.mods;
    let key = hotkey.key;
    let ctrl = HotkeyMods::CONTROL;
    let cmd = HotkeyMods::META;
    let alt = HotkeyMods::ALT;
    let shift = HotkeyMods::SHIFT;
    if mods.is_empty() || mods == shift {
        return Err(t("hotkey.err_needs_mod").to_string());
    }
    let reserved = (key == Code::Space && (mods == ctrl || mods == cmd))
        || (key == Code::Tab && (mods == alt || mods == cmd))
        || (key == Code::F4 && mods == alt)
        || (key == Code::Escape && mods == (alt | cmd))
        || (key == Code::Delete && mods == (ctrl | alt))
        || (key == Code::KeyV && mods == cmd && !cfg!(target_os = "macos"));
    let editing = matches!(
        key,
        Code::KeyC | Code::KeyV | Code::KeyX | Code::KeyA | Code::KeyZ | Code::KeyQ | Code::KeyW
    ) && (mods == ctrl || mods == cmd);
    let menu_accelerator = global && mods == alt && !cfg!(target_os = "macos");
    if reserved || editing || menu_accelerator {
        return Err(t("hotkey.err_reserved").to_string());
    }
    Ok(())
}

pub fn format_shortcut_display(shortcut: &str) -> String {
    let trimmed = shortcut.trim();
    if trimmed.is_empty() {
        return t("hotkey.unset").to_string();
    }
    let parts: Vec<&str> = trimmed
        .split('+')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .collect();
    if parts.is_empty() {
        return t("hotkey.unset").to_string();
    }

    #[cfg(target_os = "macos")]
    {
        let mut out = String::new();
        for (idx, part) in parts.iter().enumerate() {
            let is_last = idx + 1 == parts.len();
            if !is_last {
                match part.to_ascii_lowercase().as_str() {
                    "ctrl" | "control" => out.push('⌃'),
                    "alt" | "option" => out.push('⌥'),
                    "shift" => out.push('⇧'),
                    "super" | "meta" | "cmd" | "command" => out.push('⌘'),
                    other => {
                        out.push_str(other);
                        out.push('+');
                    }
                }
            } else {
                let key_disp = match part.to_ascii_lowercase().as_str() {
                    "space" => t("hotkey.space").to_string(),
                    "backspace" => "⌫".to_string(),
                    "delete" => "⌦".to_string(),
                    "enter" | "return" => "↩".to_string(),
                    "escape" | "esc" => "⎋".to_string(),
                    "arrowup" | "up" => "↑".to_string(),
                    "arrowdown" | "down" => "↓".to_string(),
                    "arrowleft" | "left" => "←".to_string(),
                    "arrowright" | "right" => "→".to_string(),
                    "comma" => ",".to_string(),
                    "period" => ".".to_string(),
                    "slash" => "/".to_string(),
                    "semicolon" => ";".to_string(),
                    "minus" => "-".to_string(),
                    "equal" => "=".to_string(),
                    "numpadadd" => "+".to_string(),
                    "backquote" => "`".to_string(),
                    "bracketleft" => "[".to_string(),
                    "bracketright" => "]".to_string(),
                    "backslash" => "\\".to_string(),
                    "quote" => "'".to_string(),
                    _ => part.to_uppercase(),
                };
                out.push_str(&key_disp);
            }
        }
        out
    }

    #[cfg(not(target_os = "macos"))]
    {
        parts
            .iter()
            .map(|p| match p.to_ascii_lowercase().as_str() {
                "super" | "meta" | "cmd" | "command" => "Win".to_string(),
                "ctrl" | "control" => "Ctrl".to_string(),
                "alt" | "option" => "Alt".to_string(),
                "shift" => "Shift".to_string(),
                "backspace" => "Backspace".to_string(),
                "delete" => "Del".to_string(),
                "space" => "Space".to_string(),
                "arrowup" | "up" => "↑".to_string(),
                "arrowdown" | "down" => "↓".to_string(),
                "arrowleft" | "left" => "←".to_string(),
                "arrowright" | "right" => "→".to_string(),
                "comma" => ",".to_string(),
                "period" => ".".to_string(),
                "slash" => "/".to_string(),
                "semicolon" => ";".to_string(),
                "minus" => "-".to_string(),
                "equal" => "=".to_string(),
                "numpadadd" => "+".to_string(),
                "backquote" => "`".to_string(),
                "bracketleft" => "[".to_string(),
                "bracketright" => "]".to_string(),
                "backslash" => "\\".to_string(),
                "quote" => "'".to_string(),
                _ => p.to_uppercase(),
            })
            .collect::<Vec<_>>()
            .join("+")
    }
}

pub fn parse_shortcut_to_egui(shortcut: &str) -> Option<(Modifiers, Key)> {
    let parts: Vec<&str> = shortcut
        .split('+')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .collect();
    if parts.is_empty() {
        return None;
    }

    let mut mods = Modifiers::NONE;
    for part in &parts[..parts.len() - 1] {
        match part.to_ascii_lowercase().as_str() {
            "ctrl" | "control" => mods.ctrl = true,
            "alt" | "option" => mods.alt = true,
            "shift" => mods.shift = true,
            "super" | "meta" | "cmd" | "command" => {
                #[cfg(target_os = "macos")]
                {
                    mods.mac_cmd = true;
                    mods.command = true;
                }
                #[cfg(not(target_os = "macos"))]
                {
                    // Treat Super/Win if explicitly specified; standard Ctrl uses ctrl above
                    mods.command = true;
                }
            }
            _ => return None,
        }
    }

    let key = parse_egui_key(parts[parts.len() - 1])?;
    Some((mods, key))
}

pub fn consume_egui_shortcut(input: &mut InputState, shortcut: &str) -> bool {
    let Some((target_mods, target_key)) = parse_shortcut_to_egui(shortcut) else {
        return false;
    };
    let mut triggered = false;
    input.events.retain(|event| {
        let egui::Event::Key {
            key,
            pressed: true,
            repeat,
            modifiers,
            ..
        } = event
        else {
            return true;
        };
        #[cfg(target_os = "macos")]
        let super_matches = (modifiers.mac_cmd || modifiers.command)
            == (target_mods.mac_cmd || target_mods.command);
        #[cfg(not(target_os = "macos"))]
        let super_matches = !target_mods.command;
        let matches = *key == target_key
            && modifiers.ctrl == target_mods.ctrl
            && modifiers.alt == target_mods.alt
            && modifiers.shift == target_mods.shift
            && super_matches;
        if matches {
            triggered |= !repeat;
        }
        !matches
    });
    triggered
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ShortcutCapture {
    Combo(String),
    NeedsModifier,
}

pub fn assemble_shortcut_combo(
    ctrl: bool,
    alt: bool,
    shift: bool,
    super_key: bool,
    key_token: &str,
) -> Option<String> {
    let mut parts: Vec<&str> = Vec::with_capacity(5);
    if ctrl {
        parts.push("Ctrl");
    }
    if alt {
        parts.push("Alt");
    }
    if shift {
        parts.push("Shift");
    }
    if super_key {
        parts.push("Super");
    }
    if parts.is_empty() {
        return None;
    }
    parts.push(key_token);
    Some(parts.join("+"))
}

pub fn format_live_modifiers(
    ctrl: bool,
    alt: bool,
    shift: bool,
    super_key: bool,
) -> Option<String> {
    if !ctrl && !alt && !shift && !super_key {
        return None;
    }
    #[cfg(target_os = "macos")]
    {
        let mut out = String::new();
        if ctrl {
            out.push('⌃');
        }
        if alt {
            out.push('⌥');
        }
        if shift {
            out.push('⇧');
        }
        if super_key {
            out.push('⌘');
        }
        Some(out)
    }
    #[cfg(not(target_os = "macos"))]
    {
        let mut parts: Vec<&str> = Vec::with_capacity(4);
        if ctrl {
            parts.push("Ctrl");
        }
        if alt {
            parts.push("Alt");
        }
        if shift {
            parts.push("Shift");
        }
        if super_key {
            parts.push("Win");
        }
        Some(parts.join("+"))
    }
}

/// Captures a new shortcut string when the user presses a modifier+key combo inside recorder mode.
pub fn capture_egui_shortcut(input: &InputState) -> Option<ShortcutCapture> {
    for ev in &input.events {
        match ev {
            egui::Event::Key {
                key,
                physical_key,
                pressed: true,
                modifiers,
                ..
            } => {
                if *key == Key::Escape {
                    return None;
                }
                let Some(key_name) =
                    egui_key_to_token(*key).or_else(|| physical_key.and_then(egui_key_to_token))
                else {
                    continue;
                };
                #[cfg(target_os = "macos")]
                let (ctrl, super_key) = (modifiers.ctrl, modifiers.mac_cmd || modifiers.command);
                #[cfg(not(target_os = "macos"))]
                let (ctrl, super_key) = (modifiers.ctrl || modifiers.command, false);
                return Some(
                    assemble_shortcut_combo(
                        ctrl,
                        modifiers.alt,
                        modifiers.shift,
                        super_key,
                        &key_name,
                    )
                    .map(ShortcutCapture::Combo)
                    .unwrap_or(ShortcutCapture::NeedsModifier),
                );
            }
            egui::Event::Cut => {
                // egui-winit intercepts Ctrl/Cmd+X and Shift+Delete (Windows) as Event::Cut
                if !cfg!(target_os = "macos") && input.modifiers.shift && !input.modifiers.ctrl {
                    return Some(ShortcutCapture::Combo("Shift+Delete".to_string()));
                }
                let combo = if cfg!(target_os = "macos") {
                    "Super+X"
                } else {
                    "Ctrl+X"
                };
                return Some(ShortcutCapture::Combo(combo.to_string()));
            }
            egui::Event::Copy => {
                // egui-winit intercepts Ctrl/Cmd+C and Ctrl+Insert (Windows) as Event::Copy
                let combo = if cfg!(target_os = "macos") {
                    "Super+C"
                } else {
                    "Ctrl+C"
                };
                return Some(ShortcutCapture::Combo(combo.to_string()));
            }
            egui::Event::Paste(_) | egui::Event::PasteImage(_) => {
                // egui-winit intercepts Ctrl/Cmd+V and Shift+Insert (Windows) as Event::Paste
                if !cfg!(target_os = "macos") && input.modifiers.shift && !input.modifiers.ctrl {
                    return Some(ShortcutCapture::Combo("Shift+Insert".to_string()));
                }
                let combo = if cfg!(target_os = "macos") {
                    "Super+V"
                } else {
                    "Ctrl+V"
                };
                return Some(ShortcutCapture::Combo(combo.to_string()));
            }
            _ => {}
        }
    }
    None
}

#[cfg(any(target_os = "windows", test))]
pub fn win32_vk_to_token(vk: u16) -> Option<&'static str> {
    const LETTERS: [&str; 26] = [
        "A", "B", "C", "D", "E", "F", "G", "H", "I", "J", "K", "L", "M", "N", "O", "P", "Q", "R",
        "S", "T", "U", "V", "W", "X", "Y", "Z",
    ];
    const DIGITS: [&str; 10] = ["0", "1", "2", "3", "4", "5", "6", "7", "8", "9"];
    const FKEYS: [&str; 24] = [
        "F1", "F2", "F3", "F4", "F5", "F6", "F7", "F8", "F9", "F10", "F11", "F12", "F13", "F14",
        "F15", "F16", "F17", "F18", "F19", "F20", "F21", "F22", "F23", "F24",
    ];

    match vk {
        0x41..=0x5A => Some(LETTERS[(vk - 0x41) as usize]),
        0x30..=0x39 => Some(DIGITS[(vk - 0x30) as usize]),
        0x70..=0x87 => Some(FKEYS[(vk - 0x70) as usize]),
        0x20 => Some("Space"),
        0x08 => Some("Backspace"),
        0x2E => Some("Delete"),
        0x0D => Some("Enter"),
        0x09 => Some("Tab"),
        0x2D => Some("Insert"),
        0x24 => Some("Home"),
        0x23 => Some("End"),
        0x21 => Some("PageUp"),
        0x22 => Some("PageDown"),
        0x26 => Some("ArrowUp"),
        0x28 => Some("ArrowDown"),
        0x25 => Some("ArrowLeft"),
        0x27 => Some("ArrowRight"),
        0xBC => Some("Comma"),
        0xBE => Some("Period"),
        0xBF => Some("Slash"),
        0xBA => Some("Semicolon"),
        0xBD | 0x6D => Some("Minus"),
        0xBB => Some("Equal"),
        0x6B => Some("NumpadAdd"),
        0xC0 => Some("Backquote"),
        0xDB => Some("BracketLeft"),
        0xDC => Some("Backslash"),
        0xDD => Some("BracketRight"),
        0xDE => Some("Quote"),
        _ => None,
    }
}

fn parse_egui_key(raw: &str) -> Option<Key> {
    match raw.to_ascii_lowercase().as_str() {
        "a" | "keya" => Some(Key::A),
        "b" | "keyb" => Some(Key::B),
        "c" | "keyc" => Some(Key::C),
        "d" | "keyd" => Some(Key::D),
        "e" | "keye" => Some(Key::E),
        "f" | "keyf" => Some(Key::F),
        "g" | "keyg" => Some(Key::G),
        "h" | "keyh" => Some(Key::H),
        "i" | "keyi" => Some(Key::I),
        "j" | "keyj" => Some(Key::J),
        "k" | "keyk" => Some(Key::K),
        "l" | "keyl" => Some(Key::L),
        "m" | "keym" => Some(Key::M),
        "n" | "keyn" => Some(Key::N),
        "o" | "keyo" => Some(Key::O),
        "p" | "keyp" => Some(Key::P),
        "q" | "keyq" => Some(Key::Q),
        "r" | "keyr" => Some(Key::R),
        "s" | "keys" => Some(Key::S),
        "t" | "keyt" => Some(Key::T),
        "u" | "keyu" => Some(Key::U),
        "v" | "keyv" => Some(Key::V),
        "w" | "keyw" => Some(Key::W),
        "x" | "keyx" => Some(Key::X),
        "y" | "keyy" => Some(Key::Y),
        "z" | "keyz" => Some(Key::Z),
        "0" | "digit0" => Some(Key::Num0),
        "1" | "digit1" => Some(Key::Num1),
        "2" | "digit2" => Some(Key::Num2),
        "3" | "digit3" => Some(Key::Num3),
        "4" | "digit4" => Some(Key::Num4),
        "5" | "digit5" => Some(Key::Num5),
        "6" | "digit6" => Some(Key::Num6),
        "7" | "digit7" => Some(Key::Num7),
        "8" | "digit8" => Some(Key::Num8),
        "9" | "digit9" => Some(Key::Num9),
        "space" => Some(Key::Space),
        "backspace" => Some(Key::Backspace),
        "delete" => Some(Key::Delete),
        "enter" | "return" => Some(Key::Enter),
        "tab" => Some(Key::Tab),
        "insert" => Some(Key::Insert),
        "home" => Some(Key::Home),
        "end" => Some(Key::End),
        "pageup" => Some(Key::PageUp),
        "pagedown" => Some(Key::PageDown),
        "arrowup" | "up" => Some(Key::ArrowUp),
        "arrowdown" | "down" => Some(Key::ArrowDown),
        "arrowleft" | "left" => Some(Key::ArrowLeft),
        "arrowright" | "right" => Some(Key::ArrowRight),
        "comma" | "," => Some(Key::Comma),
        "period" | "." => Some(Key::Period),
        "slash" | "/" => Some(Key::Slash),
        "semicolon" | ";" => Some(Key::Semicolon),
        "minus" | "-" => Some(Key::Minus),
        "equal" | "=" => Some(Key::Equals),
        "numpadadd" | "+" => Some(Key::Plus),
        "backquote" | "backtick" | "`" => Some(Key::Backtick),
        "bracketleft" | "openbracket" | "[" => Some(Key::OpenBracket),
        "bracketright" | "closebracket" | "]" => Some(Key::CloseBracket),
        "backslash" | "\\" => Some(Key::Backslash),
        "quote" | "'" => Some(Key::Quote),
        "f1" => Some(Key::F1),
        "f2" => Some(Key::F2),
        "f3" => Some(Key::F3),
        "f4" => Some(Key::F4),
        "f5" => Some(Key::F5),
        "f6" => Some(Key::F6),
        "f7" => Some(Key::F7),
        "f8" => Some(Key::F8),
        "f9" => Some(Key::F9),
        "f10" => Some(Key::F10),
        "f11" => Some(Key::F11),
        "f12" => Some(Key::F12),
        "f13" => Some(Key::F13),
        "f14" => Some(Key::F14),
        "f15" => Some(Key::F15),
        "f16" => Some(Key::F16),
        "f17" => Some(Key::F17),
        "f18" => Some(Key::F18),
        "f19" => Some(Key::F19),
        "f20" => Some(Key::F20),
        "f21" => Some(Key::F21),
        "f22" => Some(Key::F22),
        "f23" => Some(Key::F23),
        "f24" => Some(Key::F24),
        _ => None,
    }
}

fn egui_key_to_token(key: Key) -> Option<String> {
    let s = match key {
        Key::A => "A",
        Key::B => "B",
        Key::C => "C",
        Key::D => "D",
        Key::E => "E",
        Key::F => "F",
        Key::G => "G",
        Key::H => "H",
        Key::I => "I",
        Key::J => "J",
        Key::K => "K",
        Key::L => "L",
        Key::M => "M",
        Key::N => "N",
        Key::O => "O",
        Key::P => "P",
        Key::Q => "Q",
        Key::R => "R",
        Key::S => "S",
        Key::T => "T",
        Key::U => "U",
        Key::V => "V",
        Key::W => "W",
        Key::X => "X",
        Key::Y => "Y",
        Key::Z => "Z",
        Key::Num0 => "0",
        Key::Num1 => "1",
        Key::Num2 => "2",
        Key::Num3 => "3",
        Key::Num4 => "4",
        Key::Num5 => "5",
        Key::Num6 => "6",
        Key::Num7 => "7",
        Key::Num8 => "8",
        Key::Num9 => "9",
        Key::Space => "Space",
        Key::Backspace => "Backspace",
        Key::Delete => "Delete",
        Key::Enter => "Enter",
        Key::Tab => "Tab",
        Key::Insert => "Insert",
        Key::Home => "Home",
        Key::End => "End",
        Key::PageUp => "PageUp",
        Key::PageDown => "PageDown",
        Key::ArrowUp => "ArrowUp",
        Key::ArrowDown => "ArrowDown",
        Key::ArrowLeft => "ArrowLeft",
        Key::ArrowRight => "ArrowRight",
        Key::Comma => "Comma",
        Key::Period => "Period",
        Key::Slash => "Slash",
        Key::Semicolon => "Semicolon",
        Key::Minus => "Minus",
        Key::Equals => "Equal",
        Key::Plus => "NumpadAdd",
        Key::Backtick => "Backquote",
        Key::OpenBracket => "BracketLeft",
        Key::CloseBracket => "BracketRight",
        Key::Backslash => "Backslash",
        Key::Quote => "Quote",
        Key::F1 => "F1",
        Key::F2 => "F2",
        Key::F3 => "F3",
        Key::F4 => "F4",
        Key::F5 => "F5",
        Key::F6 => "F6",
        Key::F7 => "F7",
        Key::F8 => "F8",
        Key::F9 => "F9",
        Key::F10 => "F10",
        Key::F11 => "F11",
        Key::F12 => "F12",
        Key::F13 => "F13",
        Key::F14 => "F14",
        Key::F15 => "F15",
        Key::F16 => "F16",
        Key::F17 => "F17",
        Key::F18 => "F18",
        Key::F19 => "F19",
        Key::F20 => "F20",
        Key::F21 => "F21",
        Key::F22 => "F22",
        Key::F23 => "F23",
        Key::F24 => "F24",
        _ => return None,
    };
    Some(s.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn failed_hotkey_replacement_preserves_working_registration() {
        use std::cell::RefCell;
        use std::collections::HashSet;
        let old = HotKey::from_str("Ctrl+Shift+K").unwrap();
        let new = HotKey::from_str("Ctrl+Shift+J").unwrap();
        let mut active = Some(old);
        let registered = RefCell::new(HashSet::from([old]));
        let result = replace_registered_hotkey(
            &mut active,
            new,
            |_| Err("occupied".into()),
            |key| {
                registered.borrow_mut().remove(&key);
                Ok(())
            },
        );
        assert!(result.is_err());
        assert_eq!(active, Some(old));
        assert_eq!(*registered.borrow(), HashSet::from([old]));

        replace_registered_hotkey(
            &mut active,
            new,
            |key| {
                registered.borrow_mut().insert(key);
                Ok(())
            },
            |key| {
                registered.borrow_mut().remove(&key);
                Ok(())
            },
        )
        .unwrap();
        assert_eq!(active, Some(new));
        assert_eq!(*registered.borrow(), HashSet::from([new]));
    }

    #[test]
    fn shortcut_validation_blocks_reserved_aliases_but_accepts_new_defaults() {
        for key in [
            "Ctrl+Space",
            "Command+Space",
            "Alt+Tab",
            "Alt+F4",
            "Ctrl+C",
            "Cmd+V",
            "Super+Q",
        ] {
            assert!(validate_shortcut(key, true).is_err(), "accepted {key}");
            assert!(validate_shortcut(key, false).is_err(), "accepted {key}");
        }
        let settings = crate::model::AppSettings::default();
        assert!(validate_shortcut(&settings.hotkey_open, true).is_ok());
        for key in [
            &settings.hotkey_pin,
            &settings.hotkey_delete,
            &settings.hotkey_preview,
        ] {
            assert!(validate_shortcut(key, false).is_ok(), "rejected {key}");
        }
    }

    #[test]
    fn item_shortcut_consumes_only_matching_events_and_ignores_repeat() {
        let mut input = InputState::default();
        let event = |repeat| egui::Event::Key {
            key: Key::Backspace,
            physical_key: None,
            pressed: true,
            repeat,
            modifiers: Modifiers::ALT,
        };
        input.events = vec![event(false), event(true), egui::Event::Text("keep".into())];
        assert!(consume_egui_shortcut(&mut input, "Alt+Backspace"));
        assert_eq!(input.events, vec![egui::Event::Text("keep".into())]);
        input.events = vec![event(true)];
        assert!(!consume_egui_shortcut(&mut input, "Alt+Backspace"));
        assert!(input.events.is_empty());
    }

    #[test]
    fn parses_default_shortcuts_for_global_hotkey_and_egui() {
        for s in [
            "Shift+Super+C",
            "Alt+V",
            "Alt+P",
            "Alt+Backspace",
            "Alt+Delete",
            "Ctrl+Space",
            "Ctrl+V",
            "Ctrl+C",
            "Ctrl+X",
            "Ctrl+ArrowUp",
            "Alt+BracketLeft",
        ] {
            assert!(HotKey::from_str(s).is_ok(), "HotKey failed for {s}");
            assert!(parse_shortcut_to_egui(s).is_some(), "egui failed for {s}");
        }
    }

    #[test]
    fn capture_egui_shortcut_handles_clipboard_events_and_modifiers() {
        let mut input = InputState::default();
        input.events.push(egui::Event::Paste("hello".to_string()));
        assert_eq!(
            capture_egui_shortcut(&input),
            Some(ShortcutCapture::Combo(if cfg!(target_os = "macos") {
                "Super+V".to_string()
            } else {
                "Ctrl+V".to_string()
            }))
        );

        let mut bare_key = InputState::default();
        bare_key.events.push(egui::Event::Key {
            key: Key::V,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: Modifiers::NONE,
        });
        assert_eq!(
            capture_egui_shortcut(&bare_key),
            Some(ShortcutCapture::NeedsModifier)
        );
    }

    #[test]
    fn win32_vk_tokens_parse_for_global_hotkey_and_egui() {
        for vk in [
            0x41u16, 0x56, 0x30, 0x39, 0x70, 0x7B, 0x20, 0x08, 0x2E, 0x0D, 0x09, 0x2D, 0x24, 0x23,
            0x21, 0x22, 0x26, 0x28, 0x25, 0x27, 0xBC, 0xBE, 0xBF, 0xBA, 0xBD, 0xBB, 0xC0, 0xDB,
            0xDC, 0xDD, 0xDE, 0x6B,
        ] {
            let tok = win32_vk_to_token(vk).expect("vk token must map");
            let combo = format!("Ctrl+Alt+{tok}");
            assert!(HotKey::from_str(&combo).is_ok(), "HotKey rejected {combo}");
            assert!(
                parse_shortcut_to_egui(&combo).is_some(),
                "egui rejected {combo}"
            );
        }
    }
}
