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
}

impl ClipItem {
    pub fn searchable_text(&self) -> String {
        match self.kind {
            ClipKind::Text => self.text_content.clone().unwrap_or_default(),
            ClipKind::Files => self
                .files
                .iter()
                .map(|f| format!("{} {}", f.name, f.path))
                .collect::<Vec<_>>()
                .join("\n"),
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
        }
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
                format!("图片 · {} · {}", dims, format_bytes(self.byte_size))
            }
            ClipKind::Files => {
                if self.files.is_empty() {
                    "文件".to_string()
                } else if self.files.len() == 1 {
                    truncate_chars(&self.files[0].name, max_chars)
                } else {
                    format!(
                        "{} 等 {} 个文件",
                        truncate_chars(&self.files[0].name, max_chars.saturating_sub(12)),
                        self.files.len()
                    )
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
    Color,
    Link,
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
            SearchMode::Exact => "精确",
            SearchMode::Fuzzy => "模糊",
            SearchMode::Regex => "正则",
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
            SortOrder::LastCopiedAt => "上次复制时间",
            SortOrder::FirstCopiedAt => "首次复制时间",
            SortOrder::CopyCount => "复制次数",
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
            PopupPosition::Cursor => "光标",
            PopupPosition::ScreenCenter => "屏幕中心",
            PopupPosition::WindowCenter => "窗口中心",
            PopupPosition::Tray => "菜单栏 / 托盘",
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
            PinPosition::Top => "顶部",
            PinPosition::Bottom => "底部",
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
            MatchHighlight::Bold => "粗体",
            MatchHighlight::Italic => "斜体",
            MatchHighlight::Underline => "下划线",
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
            SearchBoxVisibility::Always => "始终",
            SearchBoxVisibility::WhileTyping => "输入时",
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
            ThemeMode::System => "跟随系统（灰白）",
            ThemeMode::Dark => "Earth 石墨深灰",
            ThemeMode::Light => "Earth 灰白",
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
    pub launch_at_login: bool,
    pub check_updates: bool,
    pub hotkey_open: String,
    pub hotkey_pin: String,
    pub hotkey_delete: String,
    pub hotkey_preview: String,
    pub search_mode: SearchMode,
    pub auto_paste: bool,
    pub plain_text_paste: bool,

    // Storage
    pub save_files: bool,
    pub save_images: bool,
    pub save_text: bool,
    pub max_history: usize,
    pub sort_order: SortOrder,

    // Appearance
    pub theme_mode: ThemeMode,
    pub popup_position: PopupPosition,
    pub pin_position: PinPosition,
    pub image_height: f32,
    pub auto_open_preview: bool,
    pub preview_delay_ms: u64,
    pub match_highlight: MatchHighlight,
    pub show_special_symbols: bool,
    pub show_tray_icon: bool,
    pub show_clipboard_in_menubar: bool,
    pub search_box_visibility: SearchBoxVisibility,
    pub show_app_name_in_search: bool,
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
        let (hotkey_open, hotkey_delete) = ("Shift+Super+C".to_string(), "Alt+Backspace".to_string());
        #[cfg(not(target_os = "macos"))]
        let (hotkey_open, hotkey_delete) = ("Alt+V".to_string(), "Alt+Delete".to_string());

        Self {
            launch_at_login: false,
            check_updates: true,
            hotkey_open,
            hotkey_pin: "Alt+P".to_string(),
            hotkey_delete,
            hotkey_preview: "Ctrl+Space".to_string(),
            search_mode: SearchMode::Fuzzy,
            auto_paste: true,
            plain_text_paste: false,

            save_files: true,
            save_images: true,
            save_text: true,
            max_history: 200,
            sort_order: SortOrder::LastCopiedAt,

            theme_mode: ThemeMode::Light,
            popup_position: PopupPosition::Cursor,
            pin_position: PinPosition::Bottom,
            image_height: 40.0,
            auto_open_preview: true,
            preview_delay_ms: 1200,
            match_highlight: MatchHighlight::Bold,
            show_special_symbols: true,
            show_tray_icon: true,
            show_clipboard_in_menubar: false,
            search_box_visibility: SearchBoxVisibility::Always,
            show_app_name_in_search: true,
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
