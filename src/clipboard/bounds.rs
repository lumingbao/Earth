use image::{DynamicImage, ImageDecoder, ImageReader, Limits};
use std::io::Cursor;

pub(super) const TOO_LARGE: &str = "剪贴板内容超过存储设置中的单条上限，已跳过";

#[cfg(windows)]
pub(super) fn available_formats(_ctx: &clipboard_rs::ClipboardContext) -> Option<Vec<String>> {
    super::windows::available_formats()
}

#[cfg(not(windows))]
pub(super) fn available_formats(ctx: &clipboard_rs::ClipboardContext) -> Option<Vec<String>> {
    use clipboard_rs::Clipboard;
    ctx.available_formats().ok()
}

#[cfg(target_os = "macos")]
pub(super) fn sequence() -> i64 {
    objc2_app_kit::NSPasteboard::generalPasteboard().changeCount() as i64
}
#[cfg(windows)]
pub(super) fn sequence() -> i64 {
    unsafe { windows_sys::Win32::System::DataExchange::GetClipboardSequenceNumber() as i64 }
}
#[cfg(not(any(windows, target_os = "macos")))]
pub(super) fn sequence() -> i64 {
    0
}

#[cfg(target_os = "macos")]
pub(super) fn bytes(name: &str, cap: usize) -> Result<Option<Vec<u8>>, &'static str> {
    use objc2_app_kit::NSPasteboard;
    use objc2_foundation::NSString;
    objc2::rc::autoreleasepool(|_| {
        let pb = NSPasteboard::generalPasteboard();
        let Some(data) = pb.dataForType(&NSString::from_str(name)) else {
            return Ok(None);
        };
        if data.length() > cap {
            return Err(TOO_LARGE);
        }
        Ok(Some(data.to_vec()))
    })
}
#[cfg(windows)]
pub(super) fn bytes(name: &str, cap: usize) -> Result<Option<Vec<u8>>, &'static str> {
    super::windows::read(name, super::windows::ReadAmount::All { cap })
}
#[cfg(not(any(windows, target_os = "macos")))]
pub(super) fn bytes(_: &str, _: usize) -> Result<Option<Vec<u8>>, &'static str> {
    Ok(None)
}

pub(super) fn dword(name: &str) -> Result<Option<u32>, &'static str> {
    #[cfg(windows)]
    let raw = super::windows::read(name, super::windows::ReadAmount::Dword)?;
    #[cfg(not(windows))]
    let raw = bytes(name, 4)?;
    Ok(raw.as_deref().and_then(parse_dword))
}

fn parse_dword(bytes: &[u8]) -> Option<u32> {
    Some(u32::from_le_bytes(bytes.get(..4)?.try_into().ok()?))
}

#[cfg(any(windows, test))]
pub(super) fn decode_windows_text(bytes: &[u8]) -> String {
    let (pairs, _) = bytes.as_chunks::<2>();
    let units = pairs
        .iter()
        .copied()
        .map(u16::from_le_bytes)
        .take_while(|&unit| unit != 0);
    char::decode_utf16(units)
        .map(|c| c.unwrap_or(char::REPLACEMENT_CHARACTER))
        .collect()
}

pub(super) fn decode(
    bytes: &[u8],
    dib: bool,
    max_pixels: u64,
) -> Result<DynamicImage, &'static str> {
    let mut limits = Limits::default();
    limits.max_image_width = Some(32768);
    limits.max_image_height = Some(32768);
    limits.max_alloc = Some(max_pixels * 8);
    fn checked(
        mut decoder: impl ImageDecoder,
        limits: Limits,
        max_pixels: u64,
    ) -> Result<DynamicImage, &'static str> {
        let (w, h) = decoder.dimensions();
        if u64::from(w) * u64::from(h) > max_pixels || decoder.total_bytes() > max_pixels * 8 {
            return Err(TOO_LARGE);
        }
        decoder.set_limits(limits).map_err(|_| TOO_LARGE)?;
        DynamicImage::from_decoder(decoder).map_err(|_| "无法解码剪贴板图片")
    }
    if dib {
        let decoder = image::codecs::bmp::BmpDecoder::new_without_file_header(Cursor::new(bytes))
            .map_err(|_| "无法读取 DIB 图片")?;
        checked(decoder, limits, max_pixels)
    } else {
        let mut reader = ImageReader::new(Cursor::new(bytes))
            .with_guessed_format()
            .map_err(|_| "无法识别图片格式")?;
        reader.limits(limits.clone());
        let decoder = reader
            .into_decoder()
            .map_err(|_| "无法读取图片头部或尺寸超限")?;
        checked(decoder, limits, max_pixels)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn windows_unicode_text_respects_utf16_boundaries_and_terminator() {
        let raw = [0x60, 0x4f, 0x3c, 0xd8, 0x0d, 0xdf, 0, 0, b'X', 0];
        assert_eq!(decode_windows_text(&raw), "你🌍");
        assert_eq!(decode_windows_text(&[0x00, 0xd8, b'A', 0, 0xff]), "�A");
    }

    #[test]
    fn dword_reads_value_without_requiring_exact_allocation_size() {
        assert_eq!(parse_dword(&[1, 0, 0, 0, 0, 0, 0, 0]), Some(1));
        assert_eq!(parse_dword(&[0, 0, 0, 0, 7, 7, 7, 7]), Some(0));
        assert_eq!(parse_dword(&[1, 0, 0]), None);
    }

    #[test]
    fn pixel_limit_is_enforced_before_decoding_a_complete_image() {
        let mut encoded = Cursor::new(Vec::new());
        DynamicImage::new_rgba8(8, 6)
            .write_to(&mut encoded, image::ImageFormat::Png)
            .unwrap();
        assert!(decode(encoded.get_ref(), false, 47).is_err());
        assert_eq!(decode(encoded.get_ref(), false, 48).unwrap().width(), 8);
    }

    #[test]
    fn dib_limit_and_pixels_match_the_png_path() {
        let image = DynamicImage::new_rgb8(8, 6).into_rgb8();
        let mut encoded = Vec::new();
        image::codecs::bmp::BmpEncoder::new(&mut encoded)
            .encode(&image, 8, 6, image::ExtendedColorType::Rgb8)
            .unwrap();
        assert!(decode(&encoded[14..], true, 47).is_err());
        assert_eq!(decode(&encoded[14..], true, 48).unwrap().height(), 6);
    }
}
