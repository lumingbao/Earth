use egui::{Color32, ColorImage, Context, TextureHandle, TextureOptions};
use std::collections::VecDeque;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, TryRecvError};

const MAX_PREVIEW_SIDE: u32 = 2048;
const MAX_IMAGE_BYTES: u64 = 32 * 1024 * 1024;
const MAX_DECODE_BYTES: u64 = 128 * 1024 * 1024;
const MAX_THUMBNAILS: usize = 64;

struct PendingPreview {
    key: String,
    cancel: Arc<AtomicBool>,
    receiver: Receiver<Result<ColorImage, String>>,
}

/// One decoder job and one GPU texture, even while selection changes rapidly.
#[derive(Default)]
pub struct ImagePreview {
    desired: Option<String>,
    pending: Option<PendingPreview>,
    texture: Option<TextureHandle>,
    error: Option<String>,
}

impl ImagePreview {
    pub fn clear(&mut self) {
        self.desired = None;
        self.texture = None;
        self.error = None;
        if let Some(pending) = &self.pending {
            pending.cancel.store(true, Ordering::Relaxed);
        }
    }

    pub fn texture(&mut self, ctx: &Context, path: &str) -> Result<Option<TextureHandle>, String> {
        if self.desired.as_deref() != Some(path) {
            self.clear();
            self.desired = Some(path.to_string());
        }
        if let Some(pending) = self.pending.take() {
            match pending.receiver.try_recv() {
                Ok(result) if pending.key == path && !pending.cancel.load(Ordering::Relaxed) => {
                    match result {
                        Ok(pixels) => {
                            self.texture = Some(ctx.load_texture(
                                format!("earth-preview:{path}"),
                                pixels,
                                TextureOptions::LINEAR,
                            ))
                        }
                        Err(error) => self.error = Some(error),
                    }
                }
                Err(TryRecvError::Empty) => self.pending = Some(pending),
                Err(TryRecvError::Disconnected)
                    if !pending.cancel.load(Ordering::Relaxed) && pending.key == path =>
                {
                    self.error = Some("图片预览后台任务已停止".into());
                }
                _ => {}
            }
        }
        if let Some(error) = &self.error {
            return Err(error.clone());
        }
        if self.texture.is_none() && self.pending.is_none() {
            let (tx, receiver) = mpsc::channel();
            let key = path.to_string();
            let task_path = key.clone();
            let cancel = Arc::new(AtomicBool::new(false));
            let worker_cancel = Arc::clone(&cancel);
            let repaint = ctx.clone();
            let max_side = ctx
                .input(|input| input.max_texture_side as u32)
                .clamp(1, MAX_PREVIEW_SIDE);
            std::thread::Builder::new()
                .name("earth-image-preview".into())
                .spawn(move || {
                    let result = decode_preview(&task_path, max_side);
                    if !worker_cancel.load(Ordering::Relaxed) {
                        let _ = tx.send(result);
                    }
                    drop(tx);
                    repaint.request_repaint();
                })
                .map_err(|e| format!("无法启动图片预览任务: {e}"))?;
            self.pending = Some(PendingPreview {
                key,
                cancel,
                receiver,
            });
        }
        Ok(self.texture.clone())
    }
}

impl Drop for ImagePreview {
    fn drop(&mut self) {
        self.clear();
    }
}

fn decode_preview(path: &str, max_side: u32) -> Result<ColorImage, String> {
    let file = std::fs::File::open(path).map_err(|e| format!("读取图片失败: {e}"))?;
    if file.metadata().map_err(|e| e.to_string())?.len() > MAX_IMAGE_BYTES {
        return Err("图片超过 32 MiB 预览上限，可另存后在图片应用中打开".into());
    }
    let mut reader = image::ImageReader::new(std::io::BufReader::new(file))
        .with_guessed_format()
        .map_err(|e| e.to_string())?;
    let mut limits = image::Limits::default();
    limits.max_alloc = Some(MAX_DECODE_BYTES);
    limits.max_image_width = Some(16_384);
    limits.max_image_height = Some(16_384);
    reader.limits(limits);
    let decoded = reader
        .decode()
        .map_err(|e| format!("图片无法安全预览: {e}"))?;
    let preview = decoded.thumbnail(max_side, max_side).to_rgba8();
    Ok(ColorImage::from_rgba_unmultiplied(
        [preview.width() as usize, preview.height() as usize],
        preview.as_raw(),
    ))
}

#[derive(Default)]
pub struct ThumbnailCache {
    context: Option<Context>,
    uris: VecDeque<String>,
}

impl ThumbnailCache {
    pub fn track(&mut self, ctx: &Context, uri: &str) {
        self.context = Some(ctx.clone());
        self.uris.retain(|old| old != uri);
        self.uris.push_back(uri.to_string());
        while self.uris.len() > MAX_THUMBNAILS {
            if let Some(old) = self.uris.pop_front() {
                ctx.forget_image(&old);
            }
        }
    }

    pub fn clear(&mut self) {
        if let Some(ctx) = self.context.take() {
            for uri in self.uris.drain(..) {
                ctx.forget_image(&uri);
            }
        }
    }
}

impl Drop for ThumbnailCache {
    fn drop(&mut self) {
        self.clear();
    }
}

pub fn qr_pixels(width: usize, modules: &[bool]) -> Result<ColorImage, String> {
    if width == 0 || width > 177 || modules.len() != width * width {
        return Err("二维码矩阵无效".into());
    }
    let side = width + 8;
    let mut pixels = vec![Color32::WHITE; side * side];
    for y in 0..width {
        for x in 0..width {
            if modules[y * width + x] {
                pixels[(y + 4) * side + x + 4] = Color32::from_rgb(5, 9, 20);
            }
        }
    }
    Ok(ColorImage::new([side, side], pixels))
}

pub fn encode_qr_png(width: usize, modules: &[bool]) -> Result<Vec<u8>, String> {
    let pixels = qr_pixels(width, modules)?;
    let raw: Vec<u8> = pixels
        .pixels
        .iter()
        .flat_map(|color| color.to_array())
        .collect();
    let image = image::RgbaImage::from_raw(pixels.width() as u32, pixels.height() as u32, raw)
        .ok_or("二维码图像无效")?;
    let scaled = image::imageops::resize(
        &image,
        image.width() * 6,
        image.height() * 6,
        image::imageops::FilterType::Nearest,
    );
    let mut output = std::io::Cursor::new(Vec::new());
    image::DynamicImage::ImageRgba8(scaled)
        .write_to(&mut output, image::ImageFormat::Png)
        .map_err(|e| format!("编码二维码失败: {e}"))?;
    Ok(output.into_inner())
}

type QrResult = Result<(bool, Vec<u8>), String>;

#[derive(Default)]
pub struct QrExport {
    receiver: Option<Receiver<QrResult>>,
}

impl QrExport {
    pub fn is_busy(&self) -> bool {
        self.receiver.is_some()
    }

    pub fn start(
        &mut self,
        ctx: &Context,
        width: usize,
        modules: Vec<bool>,
        save: bool,
    ) -> Result<(), String> {
        if self.is_busy() {
            return Ok(());
        }
        let (tx, receiver) = mpsc::channel();
        let repaint = ctx.clone();
        std::thread::Builder::new()
            .name("earth-qr-export".into())
            .spawn(move || {
                let result = encode_qr_png(width, &modules).map(|bytes| (save, bytes));
                let _ = tx.send(result);
                repaint.request_repaint();
            })
            .map_err(|e| format!("无法启动二维码导出: {e}"))?;
        self.receiver = Some(receiver);
        Ok(())
    }

    pub fn poll(&mut self) -> Option<QrResult> {
        let receiver = self.receiver.take()?;
        match receiver.try_recv() {
            Ok(result) => Some(result),
            Err(TryRecvError::Empty) => {
                self.receiver = Some(receiver);
                None
            }
            Err(TryRecvError::Disconnected) => Some(Err("二维码导出任务已停止".into())),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn qr_raster_has_quiet_zone_and_exports_crisp_png() {
        let pixels = qr_pixels(2, &[true, false, false, true]).unwrap();
        assert_eq!(pixels.size, [10, 10]);
        assert_eq!(pixels.pixels[0], Color32::WHITE);
        assert_ne!(pixels.pixels[44], Color32::WHITE);
        let png = encode_qr_png(2, &[true, false, false, true]).unwrap();
        let decoded = image::load_from_memory(&png).unwrap();
        assert_eq!((decoded.width(), decoded.height()), (60, 60));
        assert!(qr_pixels(177, &[true]).is_err());
    }

    #[test]
    fn thumbnail_cache_is_bounded_and_can_be_released() {
        let ctx = Context::default();
        let mut cache = ThumbnailCache::default();
        for index in 0..100 {
            cache.track(&ctx, &format!("file:///thumbnail-{index}.png"));
        }
        assert_eq!(cache.uris.len(), MAX_THUMBNAILS);
        cache.clear();
        assert!(cache.uris.is_empty());
    }

    #[test]
    fn decoder_downscales_before_upload_and_rejects_oversized_files() {
        let dir = std::env::temp_dir().join(format!("earth-preview-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        // Keep spaces and URL-reserved chars valid across Windows and POSIX filesystems (`?` is illegal on Windows).
        let path = dir.join("preview #1%.png");
        image::RgbaImage::from_pixel(4096, 16, image::Rgba([10, 20, 30, 255]))
            .save(&path)
            .unwrap();
        let decoded = decode_preview(path.to_str().unwrap(), 2048).unwrap();
        assert_eq!(decoded.size, [2048, 8]);
        let large = dir.join("too-large.png");
        std::fs::File::create(&large)
            .unwrap()
            .set_len(MAX_IMAGE_BYTES + 1)
            .unwrap();
        assert!(
            decode_preview(large.to_str().unwrap(), 2048)
                .unwrap_err()
                .contains("32 MiB")
        );
        std::fs::remove_dir_all(dir).unwrap();
    }
}
