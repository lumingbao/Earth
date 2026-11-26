use crate::i18n::{AppLanguage, t, tf};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ClipKind {
    Text,
    Image,
    Files,
}

impl ClipKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            ClipKind::Text => "text",
            ClipKind::Image => "image",
            ClipKind::Files => "files",
        }
    }

    pub fn from_db_str(s: &str) -> Self {
        match s {
            "image" => ClipKind::Image,
            "files" => ClipKind::Files,
            _ => ClipKind::Text,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FileEntry {
    pub path: String,
    pub name: String,
    pub is_dir: bool,
    pub size_bytes: u64,
}

impl FileEntry {
    pub fn from_path(path_str: &str) -> Self {
        let path = PathBuf::from(path_str);
        let name = path
            .file_name()
            .and_then(|n| n.to_str())
            .map(ToOwned::to_owned)
            .unwrap_or_else(|| path_str.to_string());
        let meta = std::fs::metadata(&path).ok();
        let is_dir = meta.as_ref().map(|m| m.is_dir()).unwrap_or(false);
        let size_bytes = meta
            .as_ref()
            .filter(|m| !m.is_dir())
            .map(|m| m.len())
            .unwrap_or(0);
        Self {
            path: path_str.to_string(),
            name,
            is_dir,
            size_bytes,
        }
    }

    pub fn exists(&self) -> bool {
        PathBuf::from(&self.path).exists()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClipItem {
    pub id: i64,
    pub content_hash: String,
    pub kind: ClipKind,
    pub text_content: Option<String>,
    pub html_content: Option<String>,
    pub rtf_content: Option<String>,
    pub image_path: Option<String>,
    pub thumb_path: Option<String>,
    pub image_width: Option<u32>,
    pub image_height: Option<u32>,
    pub files: Vec<FileEntry>,
    pub byte_size: u64,
    pub source_app: Option<String>,
    pub source_bundle_id: Option<String>,
    pub pinned: bool,
    pub collection_id: Option<i64>,
    pub first_copied_at: i64,
    pub last_copied_at: i64,
    pub copy_count: u64,
    pub sensitive_label: Option<String>,
    pub detected_color: Option<[u8; 4]>,
    pub is_url: bool,
    #[serde(default)]
    pub note: Option<String>,
    /// Set on lightweight resident history snapshots when `text_content` was truncated in SQL.
    #[serde(skip)]
    pub text_truncated: bool,
    /// Indicates whether the stored SQLite row has HTML or RTF rich-text representations.
    #[serde(skip)]
    pub has_rich_text: bool,
}

impl ClipItem {
    pub fn searchable_text(&self) -> String {
        self.searchable_text_with(self.text_content.as_deref())
    }

    pub fn searchable_text_with(&self, text_override: Option<&str>) -> String {
        let mut base = match self.kind {
            ClipKind::Text => text_override.unwrap_or_default().to_string(),
            ClipKind::Files => {
                let mut out = String::new();
                for (i, f) in self.files.iter().enumerate() {
                    if i > 0 {
                        out.push('\n');
                    }
                    out.push_str(&f.name);
                    out.push(' ');
                    out.push_str(&f.path);
                }
                out
            }
            ClipKind::Image => {
                let dims = match (self.image_width, self.image_height) {
                    (Some(w), Some(h)) => format!("{}x{}", w, h),
                    _ => String::new(),
                };
                format!(
                    "image 图片 {} {}",
                    dims,
                    self.source_app.as_deref().unwrap_or("")
                )
            }
        };
        if let Some(note) = self
            .note
            .as_deref()
            .map(str::trim)
            .filter(|n| !n.is_empty())
        {
            base.push('\n');
            base.push_str(note);
        }
        base
    }

    pub fn is_sensitive(&self) -> bool {
        self.sensitive_label.is_some()
    }

    pub fn summary_line(&self, max_chars: usize) -> String {
        match self.kind {
            ClipKind::Text => {
                let raw = self.text_content.as_deref().unwrap_or("");
                truncate_chars(raw.trim(), max_chars)
            }
            ClipKind::Image => {
                let dims = match (self.image_width, self.image_height) {
                    (Some(w), Some(h)) => format!("{}×{}", w, h),
                    _ => "Image".to_string(),
                };
                let size = format_bytes(self.byte_size);
                tf("model.image_summary", &[("dims", &dims), ("size", &size)])
            }
            ClipKind::Files => {
                if self.files.is_empty() {
                    t("model.file_fallback").to_string()
                } else if self.files.len() == 1 {
                    truncate_chars(&self.files[0].name, max_chars)
                } else {
                    let name = truncate_chars(&self.files[0].name, max_chars.saturating_sub(12));
                    let count = self.files.len().to_string();
                    tf("model.files_multi", &[("name", &name), ("count", &count)])
                }
            }
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Collection {
    pub id: i64,
    pub name: String,
    pub color_hex: String,
    pub created_at: i64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CategoryFilter {
    All,
    Text,
    Image,
    Files,
    Pinned,
    Collection(i64),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SearchMode {
    Exact,
    Fuzzy,
    Regex,
}

impl SearchMode {
    pub fn label(&self) -> &'static str {
        match self {
            SearchMode::Exact => t("model.search_mode.exact"),
            SearchMode::Fuzzy => t("model.search_mode.fuzzy"),
            SearchMode::Regex => t("model.search_mode.regex"),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum CopyTriggerMode {
    #[default]
    DoubleClick,
    SingleClick,
}

impl CopyTriggerMode {
    pub fn label(&self) -> &'static str {
        match self {
            CopyTriggerMode::DoubleClick => t("model.copy_trigger.double_click"),
            CopyTriggerMode::SingleClick => t("model.copy_trigger.single_click"),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SortOrder {
    LastCopiedAt,
    FirstCopiedAt,
    CopyCount,
}

impl SortOrder {
    pub fn label(&self) -> &'static str {
        match self {
            SortOrder::LastCopiedAt => t("model.sort_order.last_copied"),
            SortOrder::FirstCopiedAt => t("model.sort_order.first_copied"),
            SortOrder::CopyCount => t("model.sort_order.copy_count"),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum PopupPosition {
    Cursor,
    ScreenCenter,
    WindowCenter,
    Tray,
}

impl PopupPosition {
    pub fn label(&self) -> &'static str {
        match self {
            PopupPosition::Cursor => t("model.popup_pos.cursor"),
            PopupPosition::ScreenCenter => t("model.popup_pos.screen_center"),
            PopupPosition::WindowCenter => t("model.popup_pos.window_center"),
            PopupPosition::Tray => t("model.popup_pos.tray"),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum PinPosition {
    Top,
    Bottom,
}

impl PinPosition {
    pub fn label(&self) -> &'static str {
        match self {
            PinPosition::Top => t("model.pin_pos.top"),
            PinPosition::Bottom => t("model.pin_pos.bottom"),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum MatchHighlight {
    Bold,
    Italic,
    Underline,
}

impl MatchHighlight {
    pub fn label(&self) -> &'static str {
        match self {
            MatchHighlight::Bold => t("model.highlight.bold"),
            MatchHighlight::Italic => t("model.highlight.italic"),
            MatchHighlight::Underline => t("model.highlight.underline"),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SearchBoxVisibility {
    Always,
    WhileTyping,
}

impl SearchBoxVisibility {
    pub fn label(&self) -> &'static str {
        match self {
            SearchBoxVisibility::Always => t("model.search_vis.always"),
            SearchBoxVisibility::WhileTyping => t("model.search_vis.while_typing"),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ThemeMode {
    System,
    Dark,
    Light,
}

impl ThemeMode {
    pub fn label(&self) -> &'static str {
        match self {
            ThemeMode::System => t("model.theme.system"),
            ThemeMode::Dark => t("model.theme.dark"),
            ThemeMode::Light => t("model.theme.light"),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PasteBehavior {
    /// Follow settings (auto paste if enabled, plain text if enabled).
    Default,
    /// Only copy to clipboard, do not paste.
    CopyOnly,
    /// Copy and paste immediately with original formatting.
    CopyAndPaste,
    /// Copy and paste immediately as plain text.
    CopyAndPastePlainText,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct AppSettings {
    // General
    pub language: AppLanguage,
    pub launch_at_login: bool,
    pub check_updates: bool,
    pub hotkey_open: String,
    pub hotkey_pin: String,
    pub hotkey_delete: String,
    pub hotkey_preview: String,
    pub search_mode: SearchMode,
    pub copy_trigger_mode: CopyTriggerMode,
    pub auto_paste: bool,
    pub plain_text_paste: bool,

    // Storage
    pub save_files: bool,
    pub save_images: bool,
    pub save_text: bool,
    pub max_history: usize,
    pub max_text_mib: usize,
    pub max_image_mib: usize,
    pub max_image_megapixels: u64,
    pub sort_order: SortOrder,

    // Appearance
    pub theme_mode: ThemeMode,
    pub popup_position: PopupPosition,
    pub pin_position: PinPosition,
    pub match_highlight: MatchHighlight,
    pub show_special_symbols: bool,
    pub show_tray_icon: bool,
    pub show_clipboard_in_menubar: bool,
    pub search_box_visibility: SearchBoxVisibility,
    pub show_source_app_badge: bool,
    pub show_hex_color_swatch: bool,
    pub show_footer_menu: bool,

    // Ignore & Privacy
    pub ignored_apps: Vec<String>,
    pub ignore_only_outside_list: bool,
    pub ignored_clipboard_types: Vec<String>,
    pub ignored_regexes: Vec<String>,
    pub enable_privacy_masking: bool,

    // Advanced
    pub paused: bool,
    pub ignore_next_event: bool,
    pub clear_on_exit: bool,
    pub clear_system_clipboard_too: bool,
}

impl Default for AppSettings {
    fn default() -> Self {
        #[cfg(target_os = "macos")]
        let (hotkey_open, hotkey_delete) =
            ("Shift+Super+C".to_string(), "Alt+Backspace".to_string());
        #[cfg(not(target_os = "macos"))]
        let (hotkey_open, hotkey_delete) = ("Shift+Alt+V".to_string(), "Alt+Delete".to_string());
        #[cfg(target_os = "macos")]
        let hotkey_preview = "Shift+Super+P".to_string();
        #[cfg(not(target_os = "macos"))]
        let hotkey_preview = "Ctrl+Shift+P".to_string();

        Self {
            language: AppLanguage::System,
            launch_at_login: false,
            check_updates: true,
            hotkey_open,
            hotkey_pin: "Alt+P".to_string(),
            hotkey_delete,
            hotkey_preview,
            search_mode: SearchMode::Fuzzy,
            copy_trigger_mode: CopyTriggerMode::DoubleClick,
            auto_paste: true,
            plain_text_paste: false,

            save_files: true,
            save_images: true,
            save_text: true,
            max_history: 200,
            max_text_mib: 2,
            max_image_mib: 20,
            max_image_megapixels: 40,
            sort_order: SortOrder::LastCopiedAt,

            theme_mode: ThemeMode::System,
            popup_position: PopupPosition::Cursor,
            pin_position: PinPosition::Bottom,
            match_highlight: MatchHighlight::Bold,
            show_special_symbols: true,
            show_tray_icon: true,
            show_clipboard_in_menubar: false,
            search_box_visibility: SearchBoxVisibility::Always,
            show_source_app_badge: true,
            show_hex_color_swatch: true,
            show_footer_menu: true,

            ignored_apps: vec![
                "com.agilebits.onepassword7".to_string(),
                "com.1password.1password".to_string(),
                "com.bitwarden.desktop".to_string(),
                "com.apple.keychainaccess".to_string(),
                "1Password.exe".to_string(),
                "Bitwarden.exe".to_string(),
                "KeePassXC.exe".to_string(),
            ],
            ignore_only_outside_list: false,
            ignored_clipboard_types: vec![
                "org.nspasteboard.ConcealedType".to_string(),
                "org.nspasteboard.TransientType".to_string(),
                "org.nspasteboard.AutoGeneratedType".to_string(),
                "ExcludeClipboardContentFromMonitorProcessing".to_string(),
                "CanIncludeInClipboardHistory".to_string(),
            ],
            ignored_regexes: Vec::new(),
            enable_privacy_masking: true,

            paused: false,
            ignore_next_event: false,
            clear_on_exit: false,
            clear_system_clipboard_too: false,
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct ActiveAppInfo {
    pub app_name: String,
    pub process_or_bundle: String,
    #[cfg_attr(not(target_os = "macos"), allow(dead_code))]
    pub pid: u64,
    pub window_rect: Option<(f32, f32, f32, f32)>,
    #[cfg_attr(not(target_os = "windows"), allow(dead_code))]
    pub window_handle: Option<isize>,
}

pub fn truncate_chars(text: &str, max_chars: usize) -> String {
    if max_chars == 0 {
        return String::new();
    }
    let mut out = String::new();
    for (count, ch) in text.chars().enumerate() {
        if count >= max_chars {
            out.push('…');
            return out;
        }
        out.push(ch);
    }
    out
}

pub fn format_bytes(bytes: u64) -> String {
    const KB: f64 = 1024.0;
    const MB: f64 = KB * 1024.0;
    const GB: f64 = MB * 1024.0;
    let b = bytes as f64;
    if b >= GB {
        format!("{:.2} GB", b / GB)
    } else if b >= MB {
        format!("{:.1} MB", b / MB)
    } else if b >= KB {
        format!("{:.1} KB", b / KB)
    } else {
        format!("{} B", bytes)
    }
}

#[cfg(test)]
mod settings_tests {
    use super::*;

    #[test]
    fn obsolete_settings_are_ignored_without_losing_supported_values() {
        let settings: AppSettings = serde_json::from_str(
            r#"{"auto_open_preview":true,"preview_delay_ms":50,"image_height":80,"show_app_name_in_search":false,"max_history":321}"#,
        ).unwrap();
        assert_eq!(settings.max_history, 321);
        let serialized = serde_json::to_value(settings).unwrap();
        for field in [
            "auto_open_preview",
            "preview_delay_ms",
            "image_height",
            "show_app_name_in_search",
        ] {
            assert!(
                serialized.get(field).is_none(),
                "obsolete field retained: {field}"
            );
        }
    }
}
