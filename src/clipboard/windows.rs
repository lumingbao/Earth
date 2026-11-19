use windows_sys::Win32::Foundation::{
    ERROR_SUCCESS, GetLastError, GlobalFree, HGLOBAL, HWND, SetLastError,
};
use windows_sys::Win32::System::DataExchange::{
    CloseClipboard, EmptyClipboard, EnumClipboardFormats, GetClipboardData,
    GetClipboardFormatNameW, IsClipboardFormatAvailable, OpenClipboard, RegisterClipboardFormatW,
    SetClipboardData,
};
use windows_sys::Win32::System::Memory::{
    GMEM_MOVEABLE, GlobalAlloc, GlobalLock, GlobalSize, GlobalUnlock,
};
use windows_sys::Win32::UI::WindowsAndMessaging::{CreateWindowExW, DestroyWindow, HWND_MESSAGE};

pub(super) enum ReadAmount {
    All { cap: usize },
    Dword,
}

struct ClipboardGuard;
impl ClipboardGuard {
    fn open(owner: HWND) -> Result<Self, &'static str> {
        if unsafe { OpenClipboard(owner) } == 0 {
            Err("剪贴板忙，本次未采集")
        } else {
            Ok(Self)
        }
    }
}
impl Drop for ClipboardGuard {
    fn drop(&mut self) {
        unsafe {
            CloseClipboard();
        }
    }
}

pub(super) fn available_formats() -> Option<Vec<String>> {
    let _guard = ClipboardGuard::open(std::ptr::null_mut()).ok()?;
    collect_formats(
        |previous| {
            // Zero means either normal completion or failure. Clear a stale
            // thread error before each call, including after reading a name.
            unsafe {
                SetLastError(ERROR_SUCCESS);
                let next = EnumClipboardFormats(previous);
                (next != 0 || GetLastError() == ERROR_SUCCESS).then_some(next)
            }
        },
        format_name,
    )
}

fn collect_formats(
    mut next: impl FnMut(u32) -> Option<u32>,
    mut name: impl FnMut(u32) -> Option<String>,
) -> Option<Vec<String>> {
    let mut names = Vec::new();
    let mut previous = 0;
    loop {
        let current = next(previous)?;
        if current == 0 {
            return Some(names);
        }
        // Discard the whole snapshot on any enumeration/name failure. A
        // partial list could omit a configured sensitive-format marker.
        names.push(name(current)?);
        previous = current;
    }
}

fn format_name(format: u32) -> Option<String> {
    // GetClipboardFormatNameW only supports registered formats. Preserve the
    // upstream names for predefined formats and unnamed private/GDI formats.
    let predefined = match format {
        1 => "CF_TEXT",
        2 => "CF_BITMAP",
        3 => "CF_METAFILEPICT",
        4 => "CF_SYLK",
        5 => "CF_DIF",
        6 => "CF_TIFF",
        7 => "CF_OEMTEXT",
        8 => "CF_DIB",
        9 => "CF_PALETTE",
        10 => "CF_PENDATA",
        11 => "CF_RIFF",
        12 => "CF_WAVE",
        13 => "CF_UNICODETEXT",
        14 => "CF_ENHMETAFILE",
        15 => "CF_HDROP",
        16 => "CF_LOCALE",
        17 => "CF_DIBV5",
        0x80 => "CF_OWNERDISPLAY",
        0x81 => "CF_DSPTEXT",
        0x82 => "CF_DSPBITMAP",
        0x83 => "CF_DSPMETAFILEPICT",
        0x8e => "CF_DSPENHMETAFILE",
        0xc000..=0xffff => {
            let mut buffer = vec![0u16; 256];
            loop {
                let copied = unsafe {
                    GetClipboardFormatNameW(format, buffer.as_mut_ptr(), buffer.len() as i32)
                };
                if copied <= 0 {
                    return None;
                }
                let copied = copied as usize;
                if copied < buffer.len() - 1 {
                    return String::from_utf16(&buffer[..copied]).ok();
                }
                // A full buffer may be truncated; never use a shortened name
                // for an exact-match privacy rule. Bound defensive growth.
                if buffer.len() >= 4096 {
                    return None;
                }
                buffer.resize(buffer.len() * 2, 0);
            }
        }
        _ => "unknown format",
    };
    Some(predefined.to_owned())
}

fn format_id(name: &str) -> Result<u32, &'static str> {
    let id = match name {
        "CF_DIB" => 8,
        "CF_DIBV5" => 17,
        "CF_UNICODETEXT" => 13,
        name => unsafe {
            RegisterClipboardFormatW(name.encode_utf16().chain([0]).collect::<Vec<_>>().as_ptr())
        },
    };
    if id == 0 {
        Err("无法注册剪贴板格式")
    } else {
        Ok(id)
    }
}

pub(super) fn read(name: &str, amount: ReadAmount) -> Result<Option<Vec<u8>>, &'static str> {
    let kind = format_id(name)?;
    let _guard = ClipboardGuard::open(std::ptr::null_mut())?;
    unsafe {
        if IsClipboardFormatAvailable(kind) == 0 {
            return Ok(None);
        }
        let handle = GetClipboardData(kind);
        if handle.is_null() {
            return Err("无法读取剪贴板格式");
        }
        let allocated = GlobalSize(handle);
        let length = match amount {
            ReadAmount::All { cap } if allocated > cap => return Err(super::bounds::TOO_LARGE),
            ReadAmount::All { .. } => allocated,
            // GlobalSize includes allocation padding; a DWORD is exactly the
            // first four bytes regardless of the size of its memory block.
            ReadAmount::Dword if allocated >= 4 => 4,
            ReadAmount::Dword => return Err("剪贴板标记不完整"),
        };
        if length == 0 {
            return Ok(Some(Vec::new()));
        }
        let pointer = GlobalLock(handle);
        if pointer.is_null() {
            return Err("无法读取剪贴板数据");
        }
        let value = std::slice::from_raw_parts(pointer.cast::<u8>(), length).to_vec();
        GlobalUnlock(handle);
        Ok(Some(value))
    }
}

struct OwnedMemory(HGLOBAL);
impl OwnedMemory {
    fn new(bytes: &[u8]) -> Result<Self, String> {
        let memory = Self(unsafe { GlobalAlloc(GMEM_MOVEABLE, bytes.len()) });
        if memory.0.is_null() {
            return Err("无法分配剪贴板内存".into());
        }
        let pointer = unsafe { GlobalLock(memory.0) };
        if pointer.is_null() {
            return Err("无法锁定剪贴板内存".into());
        }
        unsafe {
            std::ptr::copy_nonoverlapping(bytes.as_ptr(), pointer.cast::<u8>(), bytes.len());
            GlobalUnlock(memory.0);
        }
        Ok(memory)
    }

    fn publish(mut self, format: u32) -> Result<(), String> {
        if unsafe { SetClipboardData(format, self.0) }.is_null() {
            return Err("无法写入剪贴板格式".into());
        }
        // SetClipboardData transfers the allocation to the OS on success.
        self.0 = std::ptr::null_mut();
        Ok(())
    }
}
impl Drop for OwnedMemory {
    fn drop(&mut self) {
        if !self.0.is_null() {
            unsafe {
                GlobalFree(self.0);
            }
        }
    }
}

struct OwnerWindow(HWND);
impl OwnerWindow {
    fn new() -> Result<Self, String> {
        // EmptyClipboard needs a non-null owner to support SetClipboardData.
        // A message-only STATIC window is created and destroyed on this thread.
        let class: Vec<u16> = "STATIC".encode_utf16().chain([0]).collect();
        let window = unsafe {
            CreateWindowExW(
                0,
                class.as_ptr(),
                std::ptr::null(),
                0,
                0,
                0,
                0,
                0,
                HWND_MESSAGE,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                std::ptr::null(),
            )
        };
        if window.is_null() {
            Err("无法创建剪贴板所有者".into())
        } else {
            Ok(Self(window))
        }
    }
}
impl Drop for OwnerWindow {
    fn drop(&mut self) {
        unsafe {
            DestroyWindow(self.0);
        }
    }
}

pub(super) fn write_png_with_marker(png: &[u8], dib: &[u8], marker: &str) -> Result<(), String> {
    // Prepare every fallible allocation before replacing the user's clipboard.
    let formats = [
        (format_id(marker)?, OwnedMemory::new(&[1])?),
        (format_id("PNG")?, OwnedMemory::new(png)?),
        (format_id("CF_DIB")?, OwnedMemory::new(dib)?),
    ];
    let owner = OwnerWindow::new()?;
    let _clipboard = ClipboardGuard::open(owner.0)?;
    if unsafe { EmptyClipboard() } == 0 {
        return Err("无法清空剪贴板".into());
    }
    // Publish the marker first while the clipboard stays open, so no observer
    // can see an unmarked partial image write even when a later format fails.
    for (format, memory) in formats {
        memory.publish(format)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn enumeration_distinguishes_empty_success_from_failure() {
        assert_eq!(
            collect_formats(|_| Some(0), |_| unreachable!()),
            Some(vec![])
        );
        assert_eq!(collect_formats(|_| None, |_| unreachable!()), None);
    }

    #[test]
    fn enumeration_discards_partial_results_on_either_error() {
        assert_eq!(
            collect_formats(
                |previous| if previous == 0 { Some(13) } else { None },
                |format| Some(format.to_string()),
            ),
            None
        );
        assert_eq!(
            collect_formats(
                |previous| Some(match previous {
                    0 => 13,
                    13 => 0xc001,
                    _ => 0,
                }),
                |format| (format == 13).then(|| "CF_UNICODETEXT".into()),
            ),
            None
        );
        assert_eq!(
            collect_formats(
                |previous| Some(if previous == 0 { 13 } else { 0 }),
                |format| Some(format.to_string()),
            ),
            Some(vec!["13".into()])
        );
    }

    // These tests operate on the real Windows clipboard and are deliberately
    // opt-in; run individually on a disposable test session.
    #[test]
    #[ignore = "uses the Windows clipboard; run on an isolated Windows session"]
    fn padded_history_dword_reads_only_the_value() {
        let name = "CanIncludeInClipboardHistory";
        let owner = OwnerWindow::new().unwrap();
        {
            let _guard = ClipboardGuard::open(owner.0).unwrap();
            assert_ne!(unsafe { EmptyClipboard() }, 0);
            OwnedMemory::new(&[1, 0, 0, 0, 7, 7, 7, 7])
                .unwrap()
                .publish(format_id(name).unwrap())
                .unwrap();
        }
        assert_eq!(
            read(name, ReadAmount::Dword).unwrap(),
            Some(vec![1, 0, 0, 0])
        );
    }

    #[test]
    #[ignore = "uses the Windows clipboard; run on an isolated Windows session"]
    fn image_write_publishes_marker_png_and_compatible_dib() {
        let image = image::DynamicImage::new_rgb8(2, 2);
        let mut png = std::io::Cursor::new(Vec::new());
        image.write_to(&mut png, image::ImageFormat::Png).unwrap();
        let rgb = image.into_rgb8();
        let mut bitmap = Vec::new();
        image::codecs::bmp::BmpEncoder::new(&mut bitmap)
            .encode(&rgb, 2, 2, image::ExtendedColorType::Rgb8)
            .unwrap();
        write_png_with_marker(png.get_ref(), &bitmap[14..], super::super::OWN_FORMAT).unwrap();
        let formats = available_formats().unwrap();
        for name in [super::super::OWN_FORMAT, "PNG", "CF_DIB"] {
            assert!(formats.iter().any(|format| format == name));
        }
        let stored_png = read("PNG", ReadAmount::All { cap: 4096 }).unwrap().unwrap();
        assert!(stored_png.starts_with(png.get_ref()));
        let dib = read("CF_DIB", ReadAmount::All { cap: 4096 })
            .unwrap()
            .unwrap();
        assert_eq!(
            super::super::bounds::decode(&dib, true, 4).unwrap().width(),
            2
        );
        let marker = read(super::super::OWN_FORMAT, ReadAmount::All { cap: 64 })
            .unwrap()
            .unwrap();
        assert_eq!(marker.first(), Some(&1));
    }
}
