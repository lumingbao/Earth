//! Synchronous native HTTPS, used only from the updater's worker threads.
//! macOS ships libcurl; Windows ships WinHTTP. No external executable is needed.
use std::io::Write;

pub(super) fn check_status(status: u32) -> Result<(), String> {
    match status {
        200..=299 => Ok(()),
        404 => Err("未找到发布信息或安装包，请前往 GitHub Releases 页面查看".into()),
        403 | 429 => Err("更新服务拒绝请求或已达到访问频率限制，请稍后重试".into()),
        code => Err(format!("更新服务返回 HTTP {code}，请稍后重试")),
    }
}

pub(super) fn download_to(url: &str, writer: &mut impl Write, limit: u64) -> Result<(), String> {
    if !url.starts_with("https://") || url.bytes().any(|b| b.is_ascii_control()) {
        return Err("更新服务地址必须使用 HTTPS".into());
    }
    imp::download(url, writer, limit)
}

#[cfg(target_os = "macos")]
mod imp {
    use super::*;
    use std::ffi::{CString, c_char, c_int, c_long, c_void};
    use std::sync::OnceLock;

    #[link(name = "curl")]
    unsafe extern "C" {
        fn curl_global_init(flags: c_long) -> c_int;
        fn curl_easy_init() -> *mut c_void;
        fn curl_easy_setopt(curl: *mut c_void, option: c_int, ...) -> c_int;
        fn curl_easy_perform(curl: *mut c_void) -> c_int;
        fn curl_easy_getinfo(curl: *mut c_void, info: c_int, ...) -> c_int;
        fn curl_easy_cleanup(curl: *mut c_void);
    }
    struct Handle(*mut c_void);
    impl Drop for Handle {
        fn drop(&mut self) {
            unsafe {
                curl_easy_cleanup(self.0);
            }
        }
    }
    struct Sink<'a> {
        writer: &'a mut dyn Write,
        remaining: u64,
        error: Option<String>,
    }

    unsafe extern "C" fn write_data(
        data: *mut c_char,
        size: usize,
        count: usize,
        context: *mut c_void,
    ) -> usize {
        let Some(len) = size.checked_mul(count) else {
            return 0;
        };
        // libcurl may deliver an empty body with a null data pointer.
        if len == 0 {
            return 0;
        }
        let sink = unsafe { &mut *context.cast::<Sink<'_>>() };
        if len as u64 > sink.remaining {
            sink.error = Some("更新响应超出允许大小，已拒绝下载".into());
            return 0;
        }
        let bytes = unsafe { std::slice::from_raw_parts(data.cast::<u8>(), len) };
        // Never unwind through a C callback.
        match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            sink.writer.write_all(bytes)
        })) {
            Ok(Ok(())) => {
                sink.remaining -= len as u64;
                len
            }
            Ok(Err(error)) => {
                sink.error = Some(format!("保存更新数据失败: {error}"));
                0
            }
            Err(_) => {
                sink.error = Some("保存更新数据失败".into());
                0
            }
        }
    }

    pub(super) fn download(url: &str, writer: &mut impl Write, limit: u64) -> Result<(), String> {
        static INIT: OnceLock<c_int> = OnceLock::new();
        if *INIT.get_or_init(|| unsafe { curl_global_init(3) }) != 0 {
            return Err("初始化系统 HTTPS 客户端失败".into());
        }
        let handle = Handle(unsafe { curl_easy_init() });
        if handle.0.is_null() {
            return Err("创建系统 HTTPS 请求失败".into());
        }
        let url = CString::new(url).map_err(|_| "无效的更新服务地址")?;
        let ua = CString::new(format!("Earth/{}", super::super::CURRENT_VERSION)).unwrap();
        let mut sink = Sink {
            writer,
            remaining: limit,
            error: None,
        };
        // libcurl options are stable ABI constants from curl/curl.h. Verify every
        // setter, including TLS and redirect restrictions, before making a request.
        let options = unsafe {
            [
                curl_easy_setopt(handle.0, 10002, url.as_ptr()), // CURLOPT_URL
                curl_easy_setopt(handle.0, 10018, ua.as_ptr()),  // CURLOPT_USERAGENT
                curl_easy_setopt(
                    handle.0,
                    10001,
                    (&mut sink as *mut Sink<'_>).cast::<c_void>(),
                ),
                curl_easy_setopt(
                    handle.0,
                    20011,
                    write_data
                        as unsafe extern "C" fn(*mut c_char, usize, usize, *mut c_void) -> usize,
                ),
                curl_easy_setopt(handle.0, 13, 180 as c_long), // TIMEOUT
                curl_easy_setopt(handle.0, 78, 15 as c_long),  // CONNECTTIMEOUT
                curl_easy_setopt(handle.0, 99, 1 as c_long),   // NOSIGNAL for worker threads
                curl_easy_setopt(handle.0, 52, 1 as c_long),   // FOLLOWLOCATION
                curl_easy_setopt(handle.0, 68, 5 as c_long),   // MAXREDIRS
                curl_easy_setopt(handle.0, 181, 2 as c_long),  // PROTOCOLS: HTTPS only
                curl_easy_setopt(handle.0, 182, 2 as c_long),  // REDIR_PROTOCOLS: HTTPS only
                curl_easy_setopt(handle.0, 64, 1 as c_long),   // SSL_VERIFYPEER
                curl_easy_setopt(handle.0, 81, 2 as c_long),   // SSL_VERIFYHOST
            ]
        };
        if options.iter().any(|&status| status != 0) {
            return Err("无法启用 HTTPS 安全校验，已中止请求".into());
        }
        let result = unsafe { curl_easy_perform(handle.0) };
        if let Some(error) = sink.error {
            return Err(error);
        }
        if result != 0 {
            return Err(format!(
                "无法连接更新服务，请检查网络或代理设置（HTTPS 错误 {result}）"
            ));
        }
        let mut status: c_long = 0;
        if unsafe { curl_easy_getinfo(handle.0, 0x200002, &mut status as *mut c_long) } != 0 {
            return Err("无法读取更新服务响应状态".into());
        }
        check_status(status as u32)
    }
}

#[cfg(target_os = "windows")]
mod imp {
    use super::*;
    use std::ffi::c_void;
    use std::ptr::{null, null_mut};
    use std::time::{Duration, Instant};
    use windows_sys::Win32::Networking::WinHttp::*;
    struct Handle(*mut c_void);
    impl Handle {
        fn checked(value: *mut c_void) -> Result<Self, String> {
            if value.is_null() {
                Err(network_error())
            } else {
                Ok(Self(value))
            }
        }
    }
    impl Drop for Handle {
        fn drop(&mut self) {
            unsafe {
                WinHttpCloseHandle(self.0);
            }
        }
    }
    fn wide(value: &str) -> Vec<u16> {
        value.encode_utf16().chain(Some(0)).collect()
    }
    fn network_error() -> String {
        format!(
            "无法连接更新服务，请检查网络或代理设置: {}",
            std::io::Error::last_os_error()
        )
    }
    fn option(handle: &Handle, key: u32, value: u32) -> Result<(), String> {
        if unsafe { WinHttpSetOption(handle.0, key, (&value as *const u32).cast(), 4) } == 0 {
            return Err(network_error());
        }
        Ok(())
    }
    pub(super) fn download(url: &str, writer: &mut impl Write, limit: u64) -> Result<(), String> {
        let (host, path) = url
            .strip_prefix("https://")
            .and_then(|url| url.split_once('/'))
            .ok_or("无效 HTTPS 地址")?;
        if host.is_empty()
            || !host
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'-'))
        {
            return Err("无效 HTTPS 主机名".into());
        }
        let ua = wide(&format!("Earth/{}", super::super::CURRENT_VERSION));
        let session = Handle::checked(unsafe {
            WinHttpOpen(
                ua.as_ptr(),
                WINHTTP_ACCESS_TYPE_AUTOMATIC_PROXY,
                null(),
                null(),
                0,
            )
        })?;
        if unsafe { WinHttpSetTimeouts(session.0, 15000, 15000, 30000, 30000) } == 0 {
            return Err(network_error());
        }
        option(
            &session,
            WINHTTP_OPTION_SECURE_PROTOCOLS,
            WINHTTP_FLAG_SECURE_PROTOCOL_TLS1_2,
        )?;
        let host = wide(host);
        let connection =
            Handle::checked(unsafe { WinHttpConnect(session.0, host.as_ptr(), 443, 0) })?;
        let path = wide(&format!("/{path}"));
        let method = wide("GET");
        let request = Handle::checked(unsafe {
            WinHttpOpenRequest(
                connection.0,
                method.as_ptr(),
                path.as_ptr(),
                null(),
                null(),
                null(),
                WINHTTP_FLAG_SECURE,
            )
        })?;
        option(
            &request,
            WINHTTP_OPTION_REDIRECT_POLICY,
            WINHTTP_OPTION_REDIRECT_POLICY_DISALLOW_HTTPS_TO_HTTP,
        )?;
        option(&request, WINHTTP_OPTION_MAX_HTTP_AUTOMATIC_REDIRECTS, 5)?;
        option(
            &request,
            WINHTTP_OPTION_AUTOLOGON_POLICY,
            WINHTTP_AUTOLOGON_SECURITY_LEVEL_HIGH,
        )?;
        if unsafe { WinHttpSendRequest(request.0, null(), 0, null(), 0, 0, 0) } == 0
            || unsafe { WinHttpReceiveResponse(request.0, null_mut()) } == 0
        {
            return Err(network_error());
        }
        let mut status = 0u32;
        let mut size = 4u32;
        if unsafe {
            WinHttpQueryHeaders(
                request.0,
                WINHTTP_QUERY_STATUS_CODE | WINHTTP_QUERY_FLAG_NUMBER,
                null(),
                (&mut status as *mut u32).cast(),
                &mut size,
                null_mut(),
            )
        } == 0
        {
            return Err(network_error());
        }
        check_status(status)?;
        let mut remaining = limit;
        let started = Instant::now();
        let mut buffer = [0u8; 65536];
        loop {
            if started.elapsed() > Duration::from_secs(180) {
                return Err("下载更新超时，请重试".into());
            }
            let mut len = 0u32;
            if unsafe {
                WinHttpReadData(
                    request.0,
                    buffer.as_mut_ptr().cast(),
                    buffer.len() as u32,
                    &mut len,
                )
            } == 0
            {
                return Err(network_error());
            }
            if len == 0 {
                break;
            }
            if u64::from(len) > remaining {
                return Err("更新响应超出允许大小，已拒绝下载".into());
            }
            writer
                .write_all(&buffer[..len as usize])
                .map_err(|e| format!("保存更新失败: {e}"))?;
            remaining -= u64::from(len);
        }
        Ok(())
    }
}

#[cfg(not(any(target_os = "macos", target_os = "windows")))]
mod imp {
    use super::*;
    pub(super) fn download(
        _url: &str,
        _writer: &mut impl Write,
        _limit: u64,
    ) -> Result<(), String> {
        Err("当前平台请通过发布页面手动检查更新".into())
    }
}
