use base64::{engine::general_purpose, Engine as _};
use chrono::{DateTime, Local, NaiveDateTime, TimeZone, Utc};
use regex::Regex;
use std::sync::OnceLock;

/// Parses hex color strings like `#055745`, `#fff`, `#11223344`, or bare 6-hex tokens `055745`.
pub fn detect_hex_color(input: &str) -> Option<[u8; 4]> {
    let trimmed = input.trim();
    if trimmed.is_empty() || trimmed.len() > 16 {
        return None;
    }
    let has_hash = trimmed.starts_with('#');
    let hex = trimmed.strip_prefix('#').unwrap_or(trimmed);

    if !hex.chars().all(|c| c.is_ascii_hexdigit()) {
        return None;
    }

    match hex.len() {
        3 if has_hash => {
            let mut bytes = hex.bytes();
            let r = hex_nibble(bytes.next()?)?;
            let g = hex_nibble(bytes.next()?)?;
            let b = hex_nibble(bytes.next()?)?;
            Some([r * 17, g * 17, b * 17, 255])
        }
        6 => {
            let r = u8::from_str_radix(&hex[0..2], 16).ok()?;
            let g = u8::from_str_radix(&hex[2..4], 16).ok()?;
            let b = u8::from_str_radix(&hex[4..6], 16).ok()?;
            Some([r, g, b, 255])
        }
        8 if has_hash => {
            let r = u8::from_str_radix(&hex[0..2], 16).ok()?;
            let g = u8::from_str_radix(&hex[2..4], 16).ok()?;
            let b = u8::from_str_radix(&hex[4..6], 16).ok()?;
            let a = u8::from_str_radix(&hex[6..8], 16).ok()?;
            Some([r, g, b, a])
        }
        _ => None,
    }
}

fn hex_nibble(b: u8) -> Option<u8> {
    match b {
        b'0'..=b'9' => Some(b - b'0'),
        b'a'..=b'f' => Some(b - b'a' + 10),
        b'A'..=b'F' => Some(b - b'A' + 10),
        _ => None,
    }
}

pub fn detect_url(input: &str) -> bool {
    let trimmed = input.trim();
    if trimmed.contains('\n') || trimmed.contains(' ') {
        return false;
    }
    (trimmed.starts_with("https://") || trimmed.starts_with("http://")) && trimmed.len() > 10
}

pub fn detect_sensitive_label(input: &str) -> Option<String> {
    static RULES: OnceLock<Vec<(&'static str, Regex)>> = OnceLock::new();
    let rules = RULES.get_or_init(|| {
        vec![
            (
                "Private Key",
                Regex::new(r"-----BEGIN [A-Z0-9 ]*PRIVATE KEY-----").unwrap(),
            ),
            (
                "JWT Token",
                Regex::new(r"\beyJ[A-Za-z0-9_-]{8,}\.eyJ[A-Za-z0-9_-]{8,}\.[A-Za-z0-9_-]{8,}\b")
                    .unwrap(),
            ),
            (
                "API Key",
                Regex::new(
                    r"\b(?:sk-[A-Za-z0-9_-]{20,}|sk_(?:live|test)_[A-Za-z0-9]{16,}|gh[pousr]_[A-Za-z0-9]{20,}|github_pat_[A-Za-z0-9_]{22,}|AIza[0-9A-Za-z_-]{35}|AKIA[0-9A-Z]{16})\b",
                )
                .unwrap(),
            ),
            (
                "Bearer Token",
                Regex::new(r"(?i)\bBearer\s+[A-Za-z0-9._~+/=-]{20,}").unwrap(),
            ),
            (
                "Secret Credential",
                Regex::new(r"(?i)\b(?:api[_-]?key|client[_-]?secret|password|passwd|access[_-]?token)\s*[:=]\s*\S{8,}").unwrap(),
            ),
        ]
    });

    for (label, regex) in rules {
        if regex.is_match(input) {
            return Some((*label).to_string());
        }
    }
    None
}

pub fn mask_sensitive_preview(input: &str) -> String {
    let trimmed = input.trim();
    let chars: Vec<char> = trimmed.chars().collect();
    if chars.len() <= 10 {
        return "••••••••".to_string();
    }
    let prefix: String = chars.iter().take(4).collect();
    let suffix: String = chars[chars.len().saturating_sub(4)..].iter().collect();
    format!("{}••••••••••••{}", prefix, suffix)
}

pub fn format_display_line(input: &str, show_special_symbols: bool) -> String {
    if !show_special_symbols {
        return input
            .lines()
            .map(str::trim)
            .filter(|l| !l.is_empty())
            .collect::<Vec<_>>()
            .join(" ");
    }

    let mut out = String::with_capacity(input.len());
    for ch in input.chars() {
        match ch {
            '\n' => out.push('↵'),
            '\r' => {}
            '\t' => out.push('⇥'),
            ' ' => out.push('·'),
            _ => out.push(ch),
        }
    }
    out
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QuickActionKind {
    JsonPretty,
    JsonMinify,
    Base64Encode,
    Base64Decode,
    UrlEncode,
    UrlDecode,
    TimestampConvert,
    TrimNormalize,
}

impl QuickActionKind {
    pub fn label(&self) -> &'static str {
        match self {
            QuickActionKind::JsonPretty => "JSON 格式化",
            QuickActionKind::JsonMinify => "JSON 压缩",
            QuickActionKind::Base64Encode => "Base64 编码",
            QuickActionKind::Base64Decode => "Base64 解码",
            QuickActionKind::UrlEncode => "URL 编码",
            QuickActionKind::UrlDecode => "URL 解码",
            QuickActionKind::TimestampConvert => "时间戳转换",
            QuickActionKind::TrimNormalize => "去除空白",
        }
    }
}

pub fn apply_quick_action(action: QuickActionKind, input: &str) -> Result<String, String> {
    match action {
        QuickActionKind::JsonPretty => {
            let value: serde_json::Value =
                serde_json::from_str(input.trim()).map_err(|e| format!("非有效 JSON: {e}"))?;
            serde_json::to_string_pretty(&value).map_err(|e| e.to_string())
        }
        QuickActionKind::JsonMinify => {
            let value: serde_json::Value =
                serde_json::from_str(input.trim()).map_err(|e| format!("非有效 JSON: {e}"))?;
            serde_json::to_string(&value).map_err(|e| e.to_string())
        }
        QuickActionKind::Base64Encode => Ok(general_purpose::STANDARD.encode(input.as_bytes())),
        QuickActionKind::Base64Decode => {
            let bytes = general_purpose::STANDARD
                .decode(input.trim().as_bytes())
                .or_else(|_| general_purpose::URL_SAFE.decode(input.trim().as_bytes()))
                .map_err(|e| format!("非有效 Base64: {e}"))?;
            String::from_utf8(bytes).map_err(|_| "解码结果不是有效 UTF-8 文本".to_string())
        }
        QuickActionKind::UrlEncode => Ok(urlencoding::encode(input).into_owned()),
        QuickActionKind::UrlDecode => urlencoding::decode(input)
            .map(|cow| cow.into_owned())
            .map_err(|e| format!("URL 解码失败: {e}")),
        QuickActionKind::TimestampConvert => convert_timestamp_smart(input),
        QuickActionKind::TrimNormalize => {
            let cleaned = input
                .lines()
                .map(str::trim)
                .filter(|line| !line.is_empty())
                .collect::<Vec<_>>()
                .join("\n");
            Ok(cleaned)
        }
    }
}

pub fn convert_timestamp_smart(input: &str) -> Result<String, String> {
    let trimmed = input.trim();
    if let Ok(num) = trimmed.parse::<i64>() {
        // Distinguish seconds (10 digits ~ 1e9..1e10) vs milliseconds (13 digits ~ 1e12..1e13)
        let (secs, nanos, unit) = if num.abs() >= 1_000_000_000_000 {
            let s = num.div_euclid(1000);
            let ms = num.rem_euclid(1000) as u32;
            (s, ms * 1_000_000, "毫秒 (ms)")
        } else {
            (num, 0, "秒 (s)")
        };

        let utc_dt = DateTime::<Utc>::from_timestamp(secs, nanos)
            .ok_or_else(|| "时间戳超出有效范围".to_string())?;
        let local_dt: DateTime<Local> = utc_dt.into();
        return Ok(format!(
            "本地时间: {}\nUTC 时间: {}\n输入精度: {}",
            local_dt.format("%Y-%m-%d %H:%M:%S%.3f"),
            utc_dt.format("%Y-%m-%d %H:%M:%S%.3f UTC"),
            unit
        ));
    }

    if let Ok(dt) = DateTime::parse_from_rfc3339(trimmed) {
        return Ok(format!(
            "Unix 秒: {}\nUnix 毫秒: {}",
            dt.timestamp(),
            dt.timestamp_millis()
        ));
    }

    for fmt in ["%Y-%m-%d %H:%M:%S", "%Y-%m-%d %H:%M", "%Y/%m/%d %H:%M:%S", "%Y-%m-%d"] {
        if let Ok(naive) = NaiveDateTime::parse_from_str(trimmed, fmt)
            && let Some(local_dt) = Local.from_local_datetime(&naive).single() {
                return Ok(format!(
                    "Unix 秒: {}\nUnix 毫秒: {}",
                    local_dt.timestamp(),
                    local_dt.timestamp_millis()
                ));
            }
        if fmt == "%Y-%m-%d"
            && let Ok(date) = chrono::NaiveDate::parse_from_str(trimmed, fmt)
                && let Some(naive) = date.and_hms_opt(0, 0, 0)
                    && let Some(local_dt) = Local.from_local_datetime(&naive).single() {
                        return Ok(format!(
                            "Unix 秒: {}\nUnix 毫秒: {}",
                            local_dt.timestamp(),
                            local_dt.timestamp_millis()
                        ));
                    }
    }

    Err("请输入 Unix 时间戳（秒/毫秒）或常见日期格式（如 2026-11-13 11:30:00）".to_string())
}

pub fn available_quick_actions(input: &str) -> Vec<QuickActionKind> {
    let trimmed = input.trim();
    let mut actions = Vec::new();
    if serde_json::from_str::<serde_json::Value>(trimmed).is_ok() {
        actions.push(QuickActionKind::JsonPretty);
        actions.push(QuickActionKind::JsonMinify);
    }
    if convert_timestamp_smart(trimmed).is_ok() {
        actions.push(QuickActionKind::TimestampConvert);
    }
    actions.push(QuickActionKind::Base64Encode);
    if trimmed.len() >= 4
        && trimmed.len().is_multiple_of(4)
        && general_purpose::STANDARD.decode(trimmed.as_bytes()).is_ok()
    {
        actions.push(QuickActionKind::Base64Decode);
    }
    actions.push(QuickActionKind::UrlEncode);
    if trimmed.contains('%') {
        actions.push(QuickActionKind::UrlDecode);
    }
    actions.push(QuickActionKind::TrimNormalize);
    actions
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hex_color_detection_matches_maccy_style() {
        assert_eq!(detect_hex_color("#055745"), Some([0x05, 0x57, 0x45, 255]));
        assert_eq!(detect_hex_color("055745"), Some([0x05, 0x57, 0x45, 255]));
        assert_eq!(detect_hex_color("624765"), Some([0x62, 0x47, 0x65, 255]));
        assert_eq!(detect_hex_color("#f0a"), Some([0xff, 0x00, 0xaa, 255]));
        assert_eq!(detect_hex_color("hello!"), None);
    }

    #[test]
    fn sensitive_detection_does_not_flag_git_hashes() {
        assert_eq!(
            detect_sensitive_label("a6cb125e26f7305c8cbac1d5516a0a5a47f89611514b33075b5"),
            None
        );
        assert_eq!(
            detect_sensitive_label("sk-proj-1234567890abcdefghijklmnop"),
            Some("API Key".to_string())
        );
        assert_eq!(
            detect_sensitive_label("eyJhbGciOiJIUzI1NiJ9.eyJzdWIiOiIxMjM0NTY3OCJ9.abcdefghijkl"),
            Some("JWT Token".to_string())
        );
    }

    #[test]
    fn quick_actions_json_base64_timestamp() {
        let pretty = apply_quick_action(QuickActionKind::JsonPretty, r#"{"a":1,"b":[2,3]}"#).unwrap();
        assert!(pretty.contains('\n'));
        let minified = apply_quick_action(QuickActionKind::JsonMinify, &pretty).unwrap();
        assert_eq!(minified, r#"{"a":1,"b":[2,3]}"#);

        let encoded = apply_quick_action(QuickActionKind::Base64Encode, "Earth").unwrap();
        let decoded = apply_quick_action(QuickActionKind::Base64Decode, &encoded).unwrap();
        assert_eq!(decoded, "Earth");

        let ts = apply_quick_action(QuickActionKind::TimestampConvert, "1700000000").unwrap();
        assert!(ts.contains("2023-11-14"));
    }
}
