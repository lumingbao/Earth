mod bounds;
mod events;
mod formats;
#[cfg(windows)]
mod windows;
use crate::i18n::{t, tf};
use crate::model::{AppSettings, ClipItem, ClipKind, FileEntry};
use crate::platform;
use crate::storage::{NewClipPayload, content_hash_for_text, sha256_prefixed_hex};
use crate::transforms::{detect_color, detect_sensitive_label, detect_url};
use clipboard_rs::{
    Clipboard, ClipboardContent, ClipboardContext, ClipboardHandler, ClipboardWatcher,
    ClipboardWatcherContext, ContentFormat, WatcherShutdown,
};
use events::ClipboardEventSender;
pub use events::{ClipboardEvents, channel as clipboard_event_channel};
use formats::{FormatRules, source_allowed_compiled};
use image::imageops::FilterType;
use image::{DynamicImage, ImageFormat};
use regex::{Regex, RegexBuilder};
use std::io::Cursor;
use std::sync::{Arc, Mutex, RwLock};
use std::time::Duration;

pub struct CompiledCaptureConfig {
    pub settings: AppSettings,
    format_rules: FormatRules,
    ignored_apps_lower: Vec<String>,
    ignored_regexes: Vec<Regex>,
}

impl CompiledCaptureConfig {
    pub fn from_settings(settings: AppSettings) -> Self {
        let format_rules = FormatRules::new(&settings.ignored_clipboard_types);
        let ignored_apps_lower = settings
            .ignored_apps
            .iter()
            .map(|r| r.trim().to_lowercase())
            .filter(|r| !r.is_empty())
            .collect();
        let ignored_regexes = settings
            .ignored_regexes
            .iter()
            .map(|p| p.trim())
            .filter(|p| !p.is_empty())
            .filter_map(|p| RegexBuilder::new(p).size_limit(1024 * 1024).build().ok())
            .collect();
        Self {
            settings,
            format_rules,
            ignored_apps_lower,
            ignored_regexes,
        }
    }
}

pub type SharedCaptureConfig = Arc<RwLock<Arc<CompiledCaptureConfig>>>;

pub fn validate_ignore_regex(pattern: &str) -> Result<(), String> {
    RegexBuilder::new(pattern)
        .size_limit(1024 * 1024)
        .build()
        .map(|_| ())
        .map_err(|e| {
            tf(
                "settings.ignore.regex_invalid",
                &[("error", &e.to_string())],
            )
        })
}

#[derive(Debug, Default)]
pub struct ClipboardSuppression {
    pub own_write: Option<(i64, String)>,
    pub ignore_next_once: bool,
    pub ignore_generation: u64,
}

impl ClipboardSuppression {
    fn consume_ignore(&mut self, generation: u64) -> bool {
        self.ignore_generation == generation && std::mem::take(&mut self.ignore_next_once)
    }
    fn is_own(&self, sequence: i64, hash: &str) -> bool {
        self.own_write
            .as_ref()
            .is_some_and(|(seq, content)| *seq == sequence && content == hash)
    }
}

pub enum ClipboardEvent {
    NewCapture(Box<NewClipPayload>),
    ConsumedIgnoreNext(u64),
    Skipped(&'static str),
    WatcherStopped,
}

struct EarthClipboardHandler {
    ctx: ClipboardContext,
    config: SharedCaptureConfig,
    work: std::sync::mpsc::SyncSender<(Candidate, u64)>,
    suppression: Arc<Mutex<ClipboardSuppression>>,
    tx: ClipboardEventSender,
    repaint_ctx: egui::Context,
}

impl ClipboardHandler for EarthClipboardHandler {
    fn on_clipboard_change(&mut self) {
        let Ok(config) = self.config.read().map(|g| Arc::clone(&g)) else {
            return;
        };
        let Ok(generation) = self.suppression.lock().map(|s| s.ignore_generation) else {
            return;
        };

        let settings = &config.settings;
        if settings.paused {
            return;
        }

        let sequence = bounds::sequence();
        let active_app = platform::capture_frontmost_app_identity();
        if !source_allowed_compiled(
            active_app.as_ref(),
            &config.ignored_apps_lower,
            settings.ignore_only_outside_list,
        ) {
            return;
        }
        let rules = &config.format_rules;
        let Some(formats) = bounds::available_formats(&self.ctx) else {
            return;
        };
        if formats.iter().any(|f| f == OWN_FORMAT) {
            return;
        }
        if !rules.permits_event(Some(&formats), |name| bounds::dword(name).ok().flatten()) {
            return;
        }
        let payload = match capture_clipboard_payload(&self.ctx, &config, active_app.as_ref()) {
            Ok(Some(p)) => p,
            Err(message) => {
                let _ = self.tx.send(ClipboardEvent::Skipped(message));
                self.repaint_ctx.request_repaint();
                return;
            }
            _ => return,
        };
        {
            let Ok(gate) = self.suppression.lock() else {
                return;
            };
            if sequence != bounds::sequence()
                || matches!(&payload, Candidate::Ready(p) if gate.is_own(sequence, &p.content_hash))
            {
                return;
            }
        }
        if self.work.try_send((payload, generation)).is_err() {
            let _ = self
                .tx
                .send(ClipboardEvent::Skipped(t("clipboard.skip_queue_full")));
            self.repaint_ctx.request_repaint();
        }
    }
}

pub struct ClipboardHandle {
    shutdown: Option<WatcherShutdown>,
}

struct WatcherExitNotification {
    tx: ClipboardEventSender,
    repaint_ctx: egui::Context,
}

impl Drop for WatcherExitNotification {
    fn drop(&mut self) {
        let _ = self.tx.send(ClipboardEvent::WatcherStopped);
        self.repaint_ctx.request_repaint();
    }
}

impl Drop for ClipboardHandle {
    fn drop(&mut self) {
        if let Some(shutdown) = self.shutdown.take() {
            shutdown.stop();
        }
    }
}

pub fn start_clipboard_watcher(
    config: SharedCaptureConfig,
    suppression: Arc<Mutex<ClipboardSuppression>>,
    tx: ClipboardEventSender,
    repaint_ctx: egui::Context,
) -> Result<ClipboardHandle, String> {
    let mut watcher = ClipboardWatcherContext::new_with_interval(Duration::from_millis(100))
        .map_err(|e| format!("初始化剪贴板监听器失败: {e}"))?;
    let ctx = ClipboardContext::new().map_err(|e| format!("打开系统剪贴板失败: {e}"))?;
    let shutdown = watcher.get_shutdown_channel();
    let exit_notification = WatcherExitNotification {
        tx: tx.clone(),
        repaint_ctx: repaint_ctx.clone(),
    };

    let (work, jobs) = std::sync::mpsc::sync_channel::<(Candidate, u64)>(3);
    let sink = tx.clone();
    let repaint = repaint_ctx.clone();
    let gate = suppression.clone();
    std::thread::Builder::new()
        .name("earth-capture-encoder".into())
        .spawn(move || {
            let _exit = WatcherExitNotification {
                tx: sink.clone(),
                repaint_ctx: repaint.clone(),
            };
            for (candidate, generation) in jobs {
                let event = match candidate.finish() {
                    Err(message) => ClipboardEvent::Skipped(message),
                    Ok(payload) => {
                        let Ok(mut sup) = gate.lock() else {
                            break;
                        };
                        if sup.consume_ignore(generation) {
                            ClipboardEvent::ConsumedIgnoreNext(generation)
                        } else {
                            ClipboardEvent::NewCapture(Box::new(payload))
                        }
                    }
                };
                if sink.send(event).is_err() {
                    break;
                }
                repaint.request_repaint();
            }
        })
        .map_err(|e| format!("启动采集编码线程失败：{e}"))?;
    watcher.add_handler(EarthClipboardHandler {
        work,
        ctx,
        config,
        suppression,
        tx,
        repaint_ctx,
    });

    std::thread::Builder::new()
        .name("earth-clipboard-watcher".to_string())
        .spawn(move || {
            let _exit_notification = exit_notification;
            watcher.start_watch();
        })
        .map_err(|e| format!("启动剪贴板监听线程失败: {e}"))?;

    Ok(ClipboardHandle {
        shutdown: Some(shutdown),
    })
}

#[cfg(test)]
fn should_ignore_app(app_name: &str, bundle_or_exe: &str, settings: &AppSettings) -> bool {
    let lowered: Vec<String> = settings
        .ignored_apps
        .iter()
        .map(|r| r.trim().to_lowercase())
        .filter(|r| !r.is_empty())
        .collect();
    should_ignore_app_compiled(
        app_name,
        bundle_or_exe,
        &lowered,
        settings.ignore_only_outside_list,
    )
}

pub(super) fn should_ignore_app_compiled(
    app_name: &str,
    bundle_or_exe: &str,
    ignored_apps_lower: &[String],
    ignore_only_outside_list: bool,
) -> bool {
    if ignored_apps_lower.is_empty() {
        return false;
    }
    let name_l = app_name.to_lowercase();
    let id_l = bundle_or_exe.to_lowercase();
    let matched = ignored_apps_lower
        .iter()
        .any(|r| name_l.contains(r) || id_l.contains(r) || r == &name_l || r == &id_l);
    if ignore_only_outside_list {
        !matched
    } else {
        matched
    }
}

const OWN_FORMAT: &str = "io.earth.clipboard.self-write";
enum Candidate {
    Ready(Box<NewClipPayload>),
    Image {
        bytes: Vec<u8>,
        is_png: bool,
        dib: bool,
        pixels: u64,
        cap: usize,
        source_app: Option<String>,
        source_bundle_id: Option<String>,
    },
}
impl Candidate {
    fn finish(self) -> Result<NewClipPayload, &'static str> {
        match self {
            Self::Ready(p) => Ok(*p),
            Self::Image {
                bytes,
                is_png,
                dib,
                pixels,
                cap,
                source_app,
                source_bundle_id,
            } => {
                let image = bounds::decode(&bytes, dib, pixels)?;
                let (png, thumb, w, h) = if is_png && bytes.len() <= cap {
                    let (thumb, w, h) = encode_thumb_only(&image)?;
                    drop(image);
                    (bytes, thumb, w, h)
                } else {
                    drop(bytes);
                    encode_image_and_thumb(image, cap)?
                };
                Ok(NewClipPayload {
                    content_hash: image_hash(&png),
                    byte_size: png.len() as u64,
                    kind: ClipKind::Image,
                    text_content: None,
                    html_content: None,
                    rtf_content: None,
                    image_png: Some(png),
                    thumb_png: Some(thumb),
                    image_width: Some(w),
                    image_height: Some(h),
                    files: Vec::new(),
                    source_app,
                    source_bundle_id,
                    sensitive_label: None,
                    detected_color: None,
                    is_url: false,
                })
            }
        }
    }
}

fn capture_clipboard_payload(
    ctx: &ClipboardContext,
    config: &CompiledCaptureConfig,
    active_app: Option<&crate::model::ActiveAppInfo>,
) -> Result<Option<Candidate>, &'static str> {
    let settings = &config.settings;
    let rules = &config.format_rules;
    let source_app = active_app
        .map(|a| a.app_name.clone())
        .filter(|s| !s.is_empty());
    let source_bundle_id = active_app
        .map(|a| a.process_or_bundle.clone())
        .filter(|s| !s.is_empty());
    if settings.save_files
        && rules.allows("files")
        && ctx.has(ContentFormat::Files)
        && let Ok(raw_files) = ctx.get_files()
    {
        let files: Vec<FileEntry> = raw_files
            .into_iter()
            .filter(|p| !p.trim().is_empty())
            .map(|p| normalize_file_uri(&p))
            .map(|p| FileEntry::from_path(&p))
            .collect();
        if !files.is_empty() {
            let mut paths: Vec<&str> = files.iter().map(|f| f.path.as_str()).collect();
            paths.sort_unstable();
            let content_hash = sha256_prefixed_hex(b"files:", paths.join("\n").as_bytes());
            let byte_size: u64 = files.iter().map(|f| f.size_bytes).sum();
            let summary_text = files
                .iter()
                .map(|f| f.path.as_str())
                .collect::<Vec<_>>()
                .join("\n");
            return Ok(Some(Candidate::Ready(Box::new(NewClipPayload {
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
            }))));
        }
    }

    let text_cap = settings.max_text_mib.clamp(1, 16) * 1024 * 1024;
    let image_cap = settings.max_image_mib.clamp(1, 64) * 1024 * 1024;
    let want_image = settings.save_images && rules.allows("image") && ctx.has(ContentFormat::Image);
    let check_regex = !config.ignored_regexes.is_empty();
    let text = if ((rules.allows("text") && settings.save_text) || check_regex)
        && ctx.has(ContentFormat::Text)
    {
        #[cfg(windows)]
        let text = bounds::bytes("CF_UNICODETEXT", text_cap * 2 + 2)?
            .as_deref()
            .map(bounds::decode_windows_text);
        #[cfg(not(windows))]
        let text = bounds::bytes("public.utf8-plain-text", text_cap)?
            .and_then(|raw| String::from_utf8(raw).ok());
        if check_regex && text.is_none() {
            return Ok(None);
        }
        text.filter(|s| !s.is_empty())
    } else {
        None
    };
    if text.as_ref().is_some_and(|t| t.len() > text_cap) {
        return Err(bounds::too_large());
    }
    if text
        .as_deref()
        .is_some_and(|t| matches_compiled_regexes(t, &config.ignored_regexes))
    {
        return Ok(None);
    }
    let text = text.filter(|_| rules.allows("text") && settings.save_text);
    let read_rich = |kind, native, format, remaining| -> Option<String> {
        if !settings.save_text || !rules.allows(kind) || !ctx.has(format) {
            return None;
        }
        let raw = bounds::bytes(native, remaining).ok()??;
        let s = String::from_utf8(raw).ok()?;
        #[cfg(windows)]
        if kind == "html" {
            return Some(
                s.split_once("<!--StartFragment-->")
                    .and_then(|(_, s)| s.split_once("<!--EndFragment-->"))
                    .map_or_else(
                        || s.trim_end_matches('\0').to_string(),
                        |(s, _)| s.to_owned(),
                    ),
            );
        }
        Some(s.trim_end_matches('\0').to_string()).filter(|s| !s.is_empty())
    };
    let remaining = text_cap - text.as_ref().map_or(0, String::len);
    let html = read_rich(
        "html",
        if cfg!(windows) {
            "HTML Format"
        } else {
            "public.html"
        },
        ContentFormat::Html,
        remaining,
    );
    let prefer_image = prefer_image(
        want_image,
        settings.save_text,
        text.as_deref(),
        html.as_deref(),
    );
    let get_image = || -> Result<Option<Candidate>, &'static str> {
        let names: &[&str] = if cfg!(windows) {
            &["PNG", "CF_DIBV5", "CF_DIB"]
        } else {
            &["public.png", "public.tiff"]
        };
        for name in names {
            if let Some(bytes) = bounds::bytes(name, image_cap)? {
                return Ok(Some(Candidate::Image {
                    bytes,
                    is_png: matches!(*name, "PNG" | "public.png"),
                    dib: name.starts_with("CF_DIB"),
                    pixels: settings.max_image_megapixels.clamp(1, 64) * 1_000_000,
                    cap: image_cap,
                    source_app: source_app.clone(),
                    source_bundle_id: source_bundle_id.clone(),
                }));
            }
        }
        Ok(None)
    };
    if prefer_image && let Some(p) = get_image()? {
        return Ok(Some(p));
    }
    if settings.save_text
        && let Some(text) = text
    {
        let rtf = read_rich(
            "rtf",
            if cfg!(windows) {
                "Rich Text Format"
            } else {
                "public.rtf"
            },
            ContentFormat::Rtf,
            remaining - html.as_ref().map_or(0, String::len),
        );
        let p = NewClipPayload {
            content_hash: content_hash_for_text(&text),
            byte_size: text.len() as u64,
            kind: ClipKind::Text,
            detected_color: detect_color(&text),
            is_url: detect_url(&text),
            sensitive_label: detect_sensitive_label(&text),
            text_content: Some(text),
            html_content: html,
            rtf_content: rtf,
            image_png: None,
            thumb_png: None,
            image_width: None,
            image_height: None,
            files: Vec::new(),
            source_app,
            source_bundle_id,
        };
        return Ok(Some(Candidate::Ready(Box::new(p))));
    }
    if want_image && !prefer_image {
        get_image()
    } else {
        Ok(None)
    }
}

fn image_hash(bytes: &[u8]) -> String {
    sha256_prefixed_hex(b"image:", bytes)
}

fn prefer_image(available: bool, save_text: bool, text: Option<&str>, html: Option<&str>) -> bool {
    available
        && (!save_text
            || text.is_none_or(|t| t.trim().is_empty() || detect_url(t))
            || html.is_some_and(|h| {
                let h = h.trim_start().as_bytes();
                h.get(..4).is_some_and(|s| s.eq_ignore_ascii_case(b"<img"))
                    && h.get(4)
                        .is_some_and(|b| b.is_ascii_whitespace() || matches!(b, b'/' | b'>'))
            }))
}

fn encode_thumb_only(img: &DynamicImage) -> Result<(Vec<u8>, u32, u32), &'static str> {
    let (w, h) = (img.width(), img.height());
    let thumb = img.resize(w.min(280), h.min(180), FilterType::Triangle);
    let mut small = Cursor::new(Vec::new());
    thumb
        .write_to(&mut small, ImageFormat::Png)
        .map_err(|_| t("clipboard.skip_thumb_encode"))?;
    Ok((small.into_inner(), w, h))
}

type EncodedImage = (Vec<u8>, Vec<u8>, u32, u32);
fn encode_image_and_thumb(img: DynamicImage, cap: usize) -> Result<EncodedImage, &'static str> {
    use image::ImageEncoder;
    use std::io::{self, Write};
    struct Limited {
        buf: Vec<u8>,
        cap: usize,
    }
    impl Write for Limited {
        fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
            if bytes.len() > self.cap - self.buf.len() {
                return Err(io::Error::other(bounds::too_large()));
            }
            self.buf.extend_from_slice(bytes);
            Ok(bytes.len())
        }
        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }
    let (thumb_png, w, h) = encode_thumb_only(&img)?;
    let rgba = img.into_rgba8();
    let mut output = Limited {
        buf: Vec::new(),
        cap,
    };
    image::codecs::png::PngEncoder::new(&mut output)
        .write_image(&rgba, w, h, image::ExtendedColorType::Rgba8)
        .map_err(|_| bounds::too_large())?;
    Ok((output.buf, thumb_png, w, h))
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

fn matches_compiled_regexes(text: &str, regexes: &[Regex]) -> bool {
    regexes.iter().any(|re| re.is_match(text))
}

pub fn write_item_to_clipboard(
    item: &ClipItem,
    plain_text_only: bool,
    suppression: &Arc<Mutex<ClipboardSuppression>>,
) -> Result<Option<String>, String> {
    let ctx = ClipboardContext::new().map_err(|e| format!("无法访问系统剪贴板: {e}"))?;
    let mut gate = suppression.lock().map_err(|_| "剪贴板监听状态不可用")?;
    let mut hash = item.content_hash.clone();
    let mut warning = None;
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
                .ok_or_else(|| t("clipboard.err_image_missing").to_string())?;
            let bytes = std::fs::read(path).map_err(|e| format!("读取本地图片失败: {e}"))?;
            if hash.is_empty() {
                hash = image_hash(&bytes);
            }
            write_managed_png_raw(&bytes)?;
        }
        ClipKind::Files => {
            let valid_paths: Vec<String> = item
                .files
                .iter()
                .filter(|f| f.exists())
                .map(|f| f.path.clone())
                .collect();
            if valid_paths.is_empty() {
                return Err(t("clipboard.err_files_missing").to_string());
            }
            if valid_paths.len() < item.files.len() {
                let copied = valid_paths.len().to_string();
                let missing = (item.files.len() - valid_paths.len()).to_string();
                warning = Some(tf(
                    "app.copied_partial_files",
                    &[("copied", &copied), ("missing", &missing)],
                ));
            }
            let mut ordered: Vec<_> = valid_paths.iter().map(String::as_str).collect();
            ordered.sort_unstable();
            hash = sha256_prefixed_hex(b"files:", ordered.join("\n").as_bytes());
            ctx.set_files(valid_paths)
                .map_err(|e| format!("写入文件列表到剪贴板失败: {e}"))?;
        }
    }
    gate.own_write = Some((bounds::sequence(), hash));
    Ok(warning)
}

pub fn write_custom_text_to_clipboard(
    text: &str,
    suppression: &Arc<Mutex<ClipboardSuppression>>,
) -> Result<(), String> {
    let hash = content_hash_for_text(text);

    let ctx = ClipboardContext::new().map_err(|e| format!("无法访问系统剪贴板: {e}"))?;
    let mut gate = suppression.lock().map_err(|_| "剪贴板监听状态不可用")?;
    ctx.set_text(text.to_string())
        .map_err(|e| format!("写入剪贴板失败: {e}"))?;
    gate.own_write = Some((bounds::sequence(), hash));
    Ok(())
}

pub fn write_managed_png(
    png_bytes: &[u8],
    suppression: &Arc<Mutex<ClipboardSuppression>>,
) -> Result<(), String> {
    let hash = image_hash(png_bytes);
    let mut gate = suppression.lock().map_err(|_| "剪贴板监听状态不可用")?;
    write_managed_png_raw(png_bytes)?;
    gate.own_write = Some((bounds::sequence(), hash));
    Ok(())
}

fn write_managed_png_raw(png_bytes: &[u8]) -> Result<(), String> {
    #[cfg(windows)]
    {
        let image = bounds::decode(png_bytes, false, 64_000_000).map_err(str::to_string)?;
        let rgb = image.into_rgb8();
        let mut bitmap = Vec::new();
        image::codecs::bmp::BmpEncoder::new(&mut bitmap)
            .encode(
                &rgb,
                rgb.width(),
                rgb.height(),
                image::ExtendedColorType::Rgb8,
            )
            .map_err(|e| format!("转换位图失败: {e}"))?;
        // CF_DIB starts at BITMAPINFOHEADER, without the 14-byte BMP file header.
        windows::write_png_with_marker(png_bytes, &bitmap[14..], OWN_FORMAT)
            .map_err(|e| format!("写入图片到剪贴板失败: {e}"))
    }
    #[cfg(not(windows))]
    {
        let ctx = ClipboardContext::new().map_err(|e| format!("无法访问系统剪贴板: {e}"))?;
        ctx.set(vec![
            ClipboardContent::Other("public.png".into(), png_bytes.to_vec()),
            ClipboardContent::Other(OWN_FORMAT.into(), vec![1]),
        ])
        .map_err(|e| format!("写入图片到剪贴板失败: {e}"))
    }
}

pub fn clear_system_clipboard() -> Result<(), String> {
    let ctx = ClipboardContext::new().map_err(|e| format!("无法访问系统剪贴板: {e}"))?;
    ctx.clear().map_err(|e| format!("清空系统剪贴板失败: {e}"))
}

#[cfg(test)]
mod capture_tests {
    use super::*;

    #[test]
    fn mixed_copy_uses_enabled_representations() {
        let url = Some("https://example.com/image.png");
        assert!(prefer_image(true, true, url, None));
        assert!(!prefer_image(false, true, url, Some("<img>")));
        assert!(prefer_image(true, false, Some("caption"), None));
        assert!(prefer_image(
            true,
            true,
            Some("caption"),
            Some("  <IMG src='one'>")
        ));
        assert!(!prefer_image(
            true,
            true,
            Some("caption"),
            Some("<imgwidget>text")
        ));
        assert!(!prefer_image(true, true, Some("caption"), None));
    }

    #[test]
    fn external_copy_with_identical_content_can_consume_ignore_once() {
        let mut suppression = ClipboardSuppression {
            own_write: Some((16, "same".into())),
            ignore_next_once: true,
            ignore_generation: 3,
        };
        assert!(suppression.is_own(16, "same"));
        assert!(!suppression.is_own(17, "same"));
        assert!(!suppression.consume_ignore(2));
        assert!(suppression.consume_ignore(3));
        assert!(!suppression.consume_ignore(3));
    }

    #[test]
    fn encoded_image_cannot_exceed_requested_byte_budget() {
        let img = DynamicImage::new_rgba8(8, 6);
        assert!(encode_image_and_thumb(img.clone(), 12).is_err());
        assert!(encode_image_and_thumb(img, 2048).is_ok());
    }

    #[test]
    fn png_candidate_skips_full_reencode_and_compiled_config_validates_regexes() {
        let mut raw_png = Cursor::new(Vec::new());
        DynamicImage::from(image::RgbaImage::from_pixel(
            12,
            10,
            image::Rgba([10, 132, 255, 255]),
        ))
        .write_to(&mut raw_png, ImageFormat::Png)
        .unwrap();
        let png_bytes = raw_png.into_inner();

        let payload = Candidate::Image {
            bytes: png_bytes.clone(),
            is_png: true,
            dib: false,
            pixels: 1_000_000,
            cap: 65_536,
            source_app: Some("Preview".into()),
            source_bundle_id: None,
        }
        .finish()
        .unwrap();

        assert_eq!(
            payload.image_png.as_deref(),
            Some(png_bytes.as_slice()),
            "native PNG clipboard bytes within cap must pass through without re-encoding"
        );
        assert!(payload.thumb_png.is_some());
        assert_eq!(
            (payload.image_width, payload.image_height),
            (Some(12), Some(10))
        );

        assert!(validate_ignore_regex(r"^sk-[A-Za-z0-9]{16,}$").is_ok());
        assert!(validate_ignore_regex("([unclosed").is_err());

        let cfg = CompiledCaptureConfig::from_settings(AppSettings {
            ignored_regexes: vec![r"^SECRET_\d+$".into()],
            ..Default::default()
        });
        assert!(matches_compiled_regexes(
            "SECRET_12345",
            &cfg.ignored_regexes
        ));
        assert!(!matches_compiled_regexes(
            "PUBLIC_12345",
            &cfg.ignored_regexes
        ));
    }
}
