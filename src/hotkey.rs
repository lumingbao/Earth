use egui::{InputState, Key, Modifiers};
use global_hotkey::hotkey::HotKey;
use global_hotkey::{GlobalHotKeyEvent, GlobalHotKeyManager, HotKeyState};
use std::str::FromStr;
use std::sync::mpsc::{self, Receiver};

pub struct GlobalHotkeyController {
    manager: GlobalHotKeyManager,
    active_open_hotkey: Option<HotKey>,
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
            rx,
        };
        let warn = ctrl.set_open_shortcut(initial_open_shortcut).err();
        Ok((ctrl, warn))
    }

    pub fn set_open_shortcut(&mut self, shortcut: &str) -> Result<(), String> {
        let trimmed = shortcut.trim();
        if trimmed.is_empty() {
            if let Some(prev) = self.active_open_hotkey.take() {
                let _ = self.manager.unregister(prev);
            }
            return Ok(());
        }

        let hotkey = HotKey::from_str(trimmed)
            .map_err(|e| format!("无法解析全局快捷键 \"{trimmed}\": {e}"))?;

        if self.active_open_hotkey == Some(hotkey) {
            return Ok(());
        }

        if let Some(prev) = self.active_open_hotkey.take() {
            let _ = self.manager.unregister(prev);
        }

        self.manager
            .register(hotkey)
            .map_err(|e| format!("注册全局快捷键 \"{trimmed}\" 失败（可能被系统或其他应用占用）: {e}"))?;
        self.active_open_hotkey = Some(hotkey);
        Ok(())
    }

    pub fn poll_open_triggered(&self) -> bool {
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

pub fn format_shortcut_display(shortcut: &str) -> String {
    let trimmed = shortcut.trim();
    if trimmed.is_empty() {
        return "未设置".to_string();
    }
    let parts: Vec<&str> = trimmed.split('+').map(str::trim).filter(|s| !s.is_empty()).collect();
    if parts.is_empty() {
        return "未设置".to_string();
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
                    "space" => "空格".to_string(),
                    "backspace" => "⌫".to_string(),
                    "delete" => "⌦".to_string(),
                    "enter" | "return" => "↩".to_string(),
                    "escape" | "esc" => "⎋".to_string(),
                    "arrowup" | "up" => "↑".to_string(),
                    "arrowdown" | "down" => "↓".to_string(),
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

pub fn matches_egui_shortcut(input: &InputState, shortcut: &str) -> bool {
    let Some((target_mods, target_key)) = parse_shortcut_to_egui(shortcut) else {
        return false;
    };
    if !input.key_pressed(target_key) {
        return false;
    }

    #[cfg(target_os = "macos")]
    let actual_super = input.modifiers.mac_cmd || input.modifiers.command;
    #[cfg(target_os = "macos")]
    let target_super = target_mods.mac_cmd || target_mods.command;
    #[cfg(not(target_os = "macos"))]
    let actual_super = false;
    #[cfg(not(target_os = "macos"))]
    let target_super = target_mods.command;

    input.modifiers.ctrl == target_mods.ctrl
        && input.modifiers.alt == target_mods.alt
        && input.modifiers.shift == target_mods.shift
        && actual_super == target_super
}

/// Captures a new shortcut string when the user presses a modifier+key combo inside recorder mode.
pub fn capture_egui_shortcut(input: &InputState) -> Option<String> {
    for ev in &input.events {
        if let egui::Event::Key {
            key,
            pressed: true,
            modifiers,
            ..
        } = ev
        {
            if *key == Key::Escape {
                return None;
            }
            let key_name = egui_key_to_token(*key)?;
            let mut parts: Vec<String> = Vec::new();
            if modifiers.ctrl {
                parts.push("Ctrl".to_string());
            }
            if modifiers.alt {
                parts.push("Alt".to_string());
            }
            if modifiers.shift {
                parts.push("Shift".to_string());
            }
            #[cfg(target_os = "macos")]
            if modifiers.mac_cmd || modifiers.command {
                parts.push("Super".to_string());
            }
            #[cfg(not(target_os = "macos"))]
            if modifiers.command && !modifiers.ctrl {
                parts.push("Ctrl".to_string());
            }
            // Require at least one modifier for safety so typing normal letters doesn't bind globally
            if parts.is_empty() {
                return None;
            }
            parts.push(key_name);
            return Some(parts.join("+"));
        }
    }
    None
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
        "comma" | "," => Some(Key::Comma),
        "period" | "." => Some(Key::Period),
        "slash" | "/" => Some(Key::Slash),
        "semicolon" | ";" => Some(Key::Semicolon),
        "minus" | "-" => Some(Key::Minus),
        "equal" | "=" => Some(Key::Equals),
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
        Key::Comma => "Comma",
        Key::Period => "Period",
        Key::Slash => "Slash",
        Key::Semicolon => "Semicolon",
        Key::Minus => "Minus",
        Key::Equals => "Equal",
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
        _ => return None,
    };
    Some(s.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_default_shortcuts_for_global_hotkey_and_egui() {
        for s in ["Shift+Super+C", "Alt+V", "Alt+P", "Alt+Backspace", "Alt+Delete", "Ctrl+Space"] {
            assert!(HotKey::from_str(s).is_ok(), "HotKey failed for {s}");
            assert!(parse_shortcut_to_egui(s).is_some(), "egui failed for {s}");
        }
    }
}
