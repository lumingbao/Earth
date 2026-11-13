use crate::model::{AppSettings, ClipItem, ClipKind, FileEntry};
use crate::platform;
use crate::storage::{sha256_hex, NewClipPayload};
use crate::transforms::{detect_hex_color, detect_sensitive_label, detect_url};
use clipboard_rs::common::RustImage;
use clipboard_rs::{
    Clipboard, ClipboardContent, ClipboardContext, ClipboardHandler, ClipboardWatcher,
    ClipboardWatcherContext, ContentFormat, RustImageData, WatcherShutdown,
};
use image::imageops::FilterType;
use image::{DynamicImage, ImageFormat};
use regex::Regex;
use std::io::Cursor;
use std::sync::mpsc::Sender;
use std::sync::{Arc, Mutex, RwLock};
use std::time::{Duration, Instant};

#[derive(Debug, Default)]
pub struct ClipboardSuppression {
    pub suppressed_hash: Option<String>,
    pub suppress_until: Option<Instant>,
    pub ignore_next_once: bool,
}

impl ClipboardSuppression {
    pub fn suppress_hash(&mut self, hash: String, duration: Duration) {
        self.suppressed_hash = Some(hash);
        self.suppress_until = Some(Instant::now() + duration);
    }

    pub fn should_skip(&mut self, hash: &str) -> bool {
        if self.ignore_next_once {
            self.ignore_next_once = false;
            return true;
        }
        if let (Some(expected), Some(until)) = (&self.suppressed_hash, self.suppress_until) {
            if Instant::now() <= until && expected == hash {
                self.suppressed_hash = None;
                self.suppress_until = None;
                return true;
            }
            if Instant::now() > until {
                self.suppressed_hash = None;
                self.suppress_until = None;
            }
        }
        false
    }
}

pub enum ClipboardEvent {
    NewCapture(Box<NewClipPayload>),
    ConsumedIgnoreNext,
}

struct EarthClipboardHandler {
    ctx: ClipboardContext,
    settings: Arc<RwLock<AppSettings>>,
    suppression: Arc<Mutex<ClipboardSuppression>>,
    tx: Sender<ClipboardEvent>,
    repaint_ctx: egui::Context,
}

impl ClipboardHandler for EarthClipboardHandler {
    fn on_clipboard_change(&mut self) {
        let settings = self.settings.read().map(|g| g.clone()).unwrap_or_default();

        if settings.paused {
            return;
        }

        // Check one-shot ignore flag before doing heavy reads
        {
            if let Ok(mut sup) = self.suppression.lock()
                && sup.ignore_next_once {
                    sup.ignore_next_once = false;
                    let _ = self.tx.send(ClipboardEvent::ConsumedIgnoreNext);
                    self.repaint_ctx.request_repaint();
                    return;
                }
        }

        let active_app = platform::capture_frontmost_app();
        if let Some(ref app) = active_app {
            if should_ignore_app(&app.app_name, &app.process_or_bundle, &settings) {
                return;
            }
        } else if settings.ignore_only_outside_list && !settings.ignored_apps.is_empty() {
            return;
        }

        if let Ok(formats) = self.ctx.available_formats()
            && should_ignore_formats(&formats, &settings.ignored_clipboard_types) {
                return;
            }

        let Some(payload) = capture_clipboard_payload(&self.ctx, &settings, active_app.as_ref())
        else {
            return;
        };

        if let Ok(mut sup) = self.suppression.lock()
            && sup.should_skip(&payload.content_hash) {
                return;
            }

        if self.tx.send(ClipboardEvent::NewCapture(Box::new(payload))).is_ok() {
            self.repaint_ctx.request_repaint();
        }
    }
}

pub struct ClipboardHandle {
    shutdown: Option<WatcherShutdown>,
}

impl Drop for ClipboardHandle {
    fn drop(&mut self) {
        if let Some(shutdown) = self.shutdown.take() {
            shutdown.stop();
        }
    }
}

pub fn start_clipboard_watcher(
    settings: Arc<RwLock<AppSettings>>,
    suppression: Arc<Mutex<ClipboardSuppression>>,
    tx: Sender<ClipboardEvent>,
    repaint_ctx: egui::Context,
) -> Result<ClipboardHandle, String> {
    let mut watcher = ClipboardWatcherContext::new_with_interval(Duration::from_millis(350))
        .map_err(|e| format!("初始化剪贴板监听器失败: {e}"))?;
    let ctx = ClipboardContext::new().map_err(|e| format!("打开系统剪贴板失败: {e}"))?;
    let shutdown = watcher.get_shutdown_channel();

    watcher.add_handler(EarthClipboardHandler {
        ctx,
        settings,
        suppression,
        tx,
        repaint_ctx,
    });

    std::thread::Builder::new()
        .name("earth-clipboard-watcher".to_string())
        .spawn(move || {
            watcher.start_watch();
        })
        .map_err(|e| format!("启动剪贴板监听线程失败: {e}"))?;

    Ok(ClipboardHandle {
        shutdown: Some(shutdown),
    })
}

fn should_ignore_app(app_name: &str, bundle_or_exe: &str, settings: &AppSettings) -> bool {
    if settings.ignored_apps.is_empty() {
        return false;
    }
    let name_l = app_name.to_lowercase();
    let id_l = bundle_or_exe.to_lowercase();
    let matched = settings.ignored_apps.iter().any(|rule| {
        let r = rule.trim().to_lowercase();
        !r.is_empty() && (name_l.contains(&r) || id_l.contains(&r) || r == name_l || r == id_l)
    });
    if settings.ignore_only_outside_list {
        !matched
    } else {
        matched
    }
}

fn should_ignore_formats(formats: &[String], ignored_types: &[String]) -> bool {
    if ignored_types.is_empty() {
        return false;
    }
    for fmt in formats {
        let fmt_l = fmt.to_lowercase();
        for rule in ignored_types {
            let r = rule.trim().to_lowercase();
            if !r.is_empty() && (fmt_l == r || fmt_l.contains(&r)) {
                return true;
            }
        }
    }
    false
}

fn capture_clipboard_payload(
    ctx: &ClipboardContext,
    settings: &AppSettings,
    active_app: Option<&crate::model::ActiveAppInfo>,
) -> Option<NewClipPayload> {
    let source_app = active_app
        .map(|a| a.app_name.clone())
        .filter(|s| !s.is_empty());
    let source_bundle_id = active_app
        .map(|a| a.process_or_bundle.clone())
        .filter(|s| !s.is_empty());

    // 1. Files have top priority when Finder / Explorer copies files
    if ctx.has(ContentFormat::Files)
        && let Ok(raw_files) = ctx.get_files() {
            let files: Vec<FileEntry> = raw_files
                .into_iter()
                .filter(|p| !p.trim().is_empty())
                .map(|p| normalize_file_uri(&p))
                .map(|p| FileEntry::from_path(&p))
                .collect();
            if !files.is_empty() {
                if !settings.save_files {
                    return None;
                }
                let mut paths: Vec<&str> = files.iter().map(|f| f.path.as_str()).collect();
                paths.sort_unstable();
                let hash_input = format!("files:{}", paths.join("\n"));
                let content_hash = sha256_hex(hash_input.as_bytes());
                let byte_size: u64 = files.iter().map(|f| f.size_bytes).sum();
                let summary_text = files
                    .iter()
                    .map(|f| f.path.as_str())
                    .collect::<Vec<_>>()
                    .join("\n");
                return Some(NewClipPayload {
                    content_hash,
                    kind: ClipKind::Files,
                    text_content: Some(summary_text),
                    html_content: None,
                    rtf_content: None,
                    image_png: None,
                    thumb_png: None,
                    image_width: None,
                    image_height: None,
                    files,
                    byte_size,
                    source_app,
                    source_bundle_id,
                    sensitive_label: None,
                    detected_color: None,
                    is_url: false,
                });
            }
        }

    // 2. Check text & image presence
    let has_text = ctx.has(ContentFormat::Text);
    let has_image = ctx.has(ContentFormat::Image);
    let text_opt = if has_text {
        ctx.get_text().ok().filter(|t| !t.is_empty())
    } else {
        None
    };

    // Treat as Image when there is an image and no plain text (or browser image copy whose html is solely <img>)
    let html_opt = if ctx.has(ContentFormat::Html) {
        ctx.get_html().ok().filter(|h| !h.is_empty())
    } else {
        None
    };
    let prefer_image = has_image
        && (text_opt.as_deref().map(str::trim).unwrap_or("").is_empty()
            || html_opt
                .as_deref()
                .map(|h| {
                    let hl = h.to_lowercase();
                    hl.contains("<img") && !hl.contains("<p") && !hl.contains("<div")
                })
                .unwrap_or(false));

    if prefer_image {
        if !settings.save_images {
            return None;
        }
        if let Ok(rust_img) = ctx.get_image()
            && let Ok(dyn_img) = rust_img.get_dynamic_image()
                && let Some((png_bytes, thumb_bytes, width, height)) = encode_image_and_thumb(&dyn_img)
                {
                    let mut hash_buf = Vec::with_capacity(png_bytes.len() + 6);
                    hash_buf.extend_from_slice(b"image:");
                    hash_buf.extend_from_slice(&png_bytes);
                    let content_hash = sha256_hex(&hash_buf);
                    let byte_size = png_bytes.len() as u64;
                    return Some(NewClipPayload {
                        content_hash,
                        kind: ClipKind::Image,
                        text_content: None,
                        html_content: None,
                        rtf_content: None,
                        image_png: Some(png_bytes),
                        thumb_png: Some(thumb_bytes),
                        image_width: Some(width),
                        image_height: Some(height),
                        files: Vec::new(),
                        byte_size,
                        source_app,
                        source_bundle_id,
                        sensitive_label: None,
                        detected_color: None,
                        is_url: false,
                    });
                }
    }

    // 3. Text
    if let Some(text) = text_opt {
        if !settings.save_text {
            return None;
        }
        if matches_ignored_regex(&text, &settings.ignored_regexes) {
            return None;
        }
        let rtf_opt = if ctx.has(ContentFormat::Rtf) {
            ctx.get_rich_text().ok().filter(|r| !r.is_empty())
        } else {
            None
        };

        let mut hash_buf = Vec::with_capacity(text.len() + 5);
        hash_buf.extend_from_slice(b"text:");
        hash_buf.extend_from_slice(text.as_bytes());
        let content_hash = sha256_hex(&hash_buf);

        let detected_color = detect_hex_color(&text);
        let is_url = detect_url(&text);
        let sensitive_label = if settings.enable_privacy_masking {
            detect_sensitive_label(&text)
        } else {
            None
        };
        let byte_size = text.len() as u64;

        return Some(NewClipPayload {
            content_hash,
            kind: ClipKind::Text,
            text_content: Some(text),
            html_content: html_opt,
            rtf_content: rtf_opt,
            image_png: None,
            thumb_png: None,
            image_width: None,
            image_height: None,
            files: Vec::new(),
            byte_size,
            source_app,
            source_bundle_id,
            sensitive_label,
            detected_color,
            is_url,
        });
    }

    // Fallback image check if text failed to decode
    if has_image && settings.save_images
        && let Ok(rust_img) = ctx.get_image()
            && let Ok(dyn_img) = rust_img.get_dynamic_image()
                && let Some((png_bytes, thumb_bytes, width, height)) = encode_image_and_thumb(&dyn_img)
                {
                    let mut hash_buf = Vec::with_capacity(png_bytes.len() + 6);
                    hash_buf.extend_from_slice(b"image:");
                    hash_buf.extend_from_slice(&png_bytes);
                    let content_hash = sha256_hex(&hash_buf);
                    let byte_size = png_bytes.len() as u64;
                    return Some(NewClipPayload {
                        content_hash,
                        kind: ClipKind::Image,
                        text_content: None,
                        html_content: None,
                        rtf_content: None,
                        image_png: Some(png_bytes),
                        thumb_png: Some(thumb_bytes),
                        image_width: Some(width),
                        image_height: Some(height),
                        files: Vec::new(),
                        byte_size,
                        source_app,
                        source_bundle_id,
                        sensitive_label: None,
                        detected_color: None,
                        is_url: false,
                    });
                }

    None
}

fn encode_image_and_thumb(img: &DynamicImage) -> Option<(Vec<u8>, Vec<u8>, u32, u32)> {
    let width = img.width();
    let height = img.height();
    let mut full_buf = Cursor::new(Vec::new());
    img.write_to(&mut full_buf, ImageFormat::Png).ok()?;

    let thumb = if width > 280 || height > 180 {
        img.resize(280, 180, FilterType::Triangle)
    } else {
        img.clone()
    };
    let mut thumb_buf = Cursor::new(Vec::new());
    thumb.write_to(&mut thumb_buf, ImageFormat::Png).ok()?;

    Some((full_buf.into_inner(), thumb_buf.into_inner(), width, height))
}

fn normalize_file_uri(raw: &str) -> String {
    if let Some(stripped) = raw.strip_prefix("file://") {
        urlencoding::decode(stripped)
            .map(|c| c.into_owned())
            .unwrap_or_else(|_| stripped.to_string())
    } else {
        raw.to_string()
    }
}

fn matches_ignored_regex(text: &str, patterns: &[String]) -> bool {
    for pat in patterns {
        let p = pat.trim();
        if p.is_empty() {
            continue;
        }
        if let Ok(re) = Regex::new(p)
            && re.is_match(text) {
                return true;
            }
    }
    false
}

pub fn write_item_to_clipboard(
    item: &ClipItem,
    plain_text_only: bool,
    suppression: &Arc<Mutex<ClipboardSuppression>>,
) -> Result<(), String> {
    if let Ok(mut sup) = suppression.lock() {
        sup.suppress_hash(item.content_hash.clone(), Duration::from_millis(1500));
    }

    let ctx = ClipboardContext::new().map_err(|e| format!("无法访问系统剪贴板: {e}"))?;

    match item.kind {
        ClipKind::Text => {
            let text = item.text_content.clone().unwrap_or_default();
            if plain_text_only || (item.html_content.is_none() && item.rtf_content.is_none()) {
                ctx.set_text(text)
                    .map_err(|e| format!("写入纯文本到剪贴板失败: {e}"))?;
            } else {
                let mut contents = vec![ClipboardContent::Text(text)];
                if let Some(html) = &item.html_content {
                    contents.push(ClipboardContent::Html(html.clone()));
                }
                if let Some(rtf) = &item.rtf_content {
                    contents.push(ClipboardContent::Rtf(rtf.clone()));
                }
                ctx.set(contents)
                    .map_err(|e| format!("写入富文本到剪贴板失败: {e}"))?;
            }
        }
        ClipKind::Image => {
            let path = item
                .image_path
                .as_deref()
                .ok_or_else(|| "图片文件路径缺失".to_string())?;
            let bytes = std::fs::read(path).map_err(|e| format!("读取本地图片失败: {e}"))?;
            let img = <RustImageData as clipboard_rs::common::RustImage>::from_bytes(&bytes)
                .map_err(|e| format!("解析图片失败: {e}"))?;
            ctx.set_image(img)
                .map_err(|e| format!("写入图片到剪贴板失败: {e}"))?;
        }
        ClipKind::Files => {
            let valid_paths: Vec<String> = item
                .files
                .iter()
                .filter(|f| f.exists())
                .map(|f| f.path.clone())
                .collect();
            if valid_paths.is_empty() {
                return Err("原文件已不存在或被移动，无法写入文件剪贴板".to_string());
            }
            ctx.set_files(valid_paths)
                .map_err(|e| format!("写入文件列表到剪贴板失败: {e}"))?;
        }
    }
    Ok(())
}

pub fn write_custom_text_to_clipboard(
    text: &str,
    suppression: &Arc<Mutex<ClipboardSuppression>>,
) -> Result<(), String> {
    let mut hash_buf = Vec::with_capacity(text.len() + 5);
    hash_buf.extend_from_slice(b"text:");
    hash_buf.extend_from_slice(text.as_bytes());
    let hash = sha256_hex(&hash_buf);

    if let Ok(mut sup) = suppression.lock() {
        sup.suppress_hash(hash, Duration::from_millis(1500));
    }

    let ctx = ClipboardContext::new().map_err(|e| format!("无法访问系统剪贴板: {e}"))?;
    ctx.set_text(text.to_string())
        .map_err(|e| format!("写入剪贴板失败: {e}"))?;
    Ok(())
}

pub fn clear_system_clipboard() -> Result<(), String> {
    let ctx = ClipboardContext::new().map_err(|e| format!("无法访问系统剪贴板: {e}"))?;
    ctx.clear().map_err(|e| format!("清空系统剪贴板失败: {e}"))
}
