use crate::storage::sha256_hex;
use base64::{engine::general_purpose, Engine as _};
use chrono::{DateTime, Local, NaiveDateTime, TimeZone, Utc};
use regex::Regex;
use std::collections::HashSet;
use std::sync::OnceLock;

/// Parses hex color strings such as `#055745`, `#fff`, `#f0a8`, `#11223344`,
/// `0xFF5733`, `0xFF573380`, or bare 6-hex tokens `055745`.
pub fn detect_hex_color(input: &str) -> Option<[u8; 4]> {
    let trimmed = input.trim();
    if trimmed.is_empty() || trimmed.len() > 16 {
        return None;
    }
    let (has_prefix, hex) = if let Some(rest) = trimmed.strip_prefix('#') {
        (true, rest)
    } else if let Some(rest) = trimmed
        .strip_prefix("0x")
        .or_else(|| trimmed.strip_prefix("0X"))
    {
        (true, rest)
    } else {
        (false, trimmed)
    };

    if !hex.chars().all(|c| c.is_ascii_hexdigit()) {
        return None;
    }

    match hex.len() {
        3 if trimmed.starts_with('#') => {
            let mut bytes = hex.bytes();
            let r = hex_nibble(bytes.next()?)?;
            let g = hex_nibble(bytes.next()?)?;
            let b = hex_nibble(bytes.next()?)?;
            Some([r * 17, g * 17, b * 17, 255])
        }
        4 if trimmed.starts_with('#') => {
            let mut bytes = hex.bytes();
            let r = hex_nibble(bytes.next()?)?;
            let g = hex_nibble(bytes.next()?)?;
            let b = hex_nibble(bytes.next()?)?;
            let a = hex_nibble(bytes.next()?)?;
            Some([r * 17, g * 17, b * 17, a * 17])
        }
        6 => {
            let r = u8::from_str_radix(&hex[0..2], 16).ok()?;
            let g = u8::from_str_radix(&hex[2..4], 16).ok()?;
            let b = u8::from_str_radix(&hex[4..6], 16).ok()?;
            Some([r, g, b, 255])
        }
        8 if has_prefix => {
            let r = u8::from_str_radix(&hex[0..2], 16).ok()?;
            let g = u8::from_str_radix(&hex[2..4], 16).ok()?;
            let b = u8::from_str_radix(&hex[4..6], 16).ok()?;
            let a = u8::from_str_radix(&hex[6..8], 16).ok()?;
            Some([r, g, b, a])
        }
        _ => None,
    }
}

/// Parses RGB / RGBA color strings:
/// - Functional CSS syntax: `rgb(255, 128, 0)`, `rgba(255, 128, 0, 0.5)`, `rgb(100%, 50%, 0%)`,
///   CSS Color 4 space/slash syntax `rgb(255 128 0 / 50%)`, and full-width punctuation variants.
/// - Bare RGB tuples commonly copied from design tools: `255, 128, 0`, `(255, 128, 0)`,
///   `255, 128, 0, 0.5`, `(255, 128, 0, 50%)`.
pub fn detect_rgb_color(input: &str) -> Option<[u8; 4]> {
    let trimmed = input.trim();
    if trimmed.is_empty() || trimmed.len() > 64 || trimmed.contains('\n') || trimmed.contains('\r') {
        return None;
    }

    if let Some(rgba) = parse_functional_rgb(trimmed) {
        return Some(rgba);
    }
    parse_bare_rgb_tuple(trimmed)
}

/// Unified color detector supporting HEX (`#RGB`, `#RGBA`, `#RRGGBB`, `#RRGGBBAA`, `0xRRGGBB`,
/// bare `RRGGBB`), RGB / RGBA (CSS functional + CSS Color 4 space/slash + bare tuples), optional
/// trailing CSS semicolons, quoted explicit colors, and single-value CSS color declarations
/// (e.g. `color: #0A84FF;` or `--accent: rgb(10, 132, 255);`).
pub fn detect_color(input: &str) -> Option<[u8; 4]> {
    let trimmed = input.trim();
    if trimmed.is_empty() || trimmed.len() > 96 || trimmed.contains('\n') || trimmed.contains('\r') {
        return None;
    }

    let cleaned = trimmed.strip_suffix(';').unwrap_or(trimmed).trim();
    if let Some(rgba) = detect_hex_color(cleaned).or_else(|| detect_rgb_color(cleaned)) {
        return Some(rgba);
    }

    // Quoted explicit color literal, e.g. `"#0A84FF"` or `'rgb(10, 132, 255)'`
    if let Some(inner) = strip_matching_quotes(cleaned)
        && has_explicit_color_prefix(inner)
        && let Some(rgba) = detect_hex_color(inner).or_else(|| detect_rgb_color(inner))
    {
        return Some(rgba);
    }

    // Single CSS declaration like `color: #0A84FF;` or `--primary: rgba(10, 132, 255, 0.8);`
    if let Some((prop, raw_val)) = cleaned.split_once(':') {
        let prop = prop.trim();
        let val = raw_val.trim().strip_suffix(';').unwrap_or(raw_val.trim()).trim();
        let val = strip_matching_quotes(val).unwrap_or(val);
        let valid_prop = !prop.is_empty()
            && prop.len() <= 32
            && prop
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_');
        if valid_prop && has_explicit_color_prefix(val) {
            return detect_hex_color(val).or_else(|| detect_rgb_color(val));
        }
    }

    None
}

fn strip_matching_quotes(s: &str) -> Option<&str> {
    if s.len() >= 2
        && ((s.starts_with('"') && s.ends_with('"')) || (s.starts_with('\'') && s.ends_with('\'')))
    {
        Some(s[1..s.len() - 1].trim())
    } else {
        None
    }
}

fn has_explicit_color_prefix(s: &str) -> bool {
    let bytes = s.as_bytes();
    if s.starts_with('#') || s.starts_with("0x") || s.starts_with("0X") {
        return true;
    }
    bytes.len() >= 4 && bytes[..3].eq_ignore_ascii_case(b"rgb")
}

fn parse_functional_rgb(s: &str) -> Option<[u8; 4]> {
    let bytes = s.as_bytes();
    let (is_rgba, rest) = if bytes.len() >= 6 && bytes[..4].eq_ignore_ascii_case(b"rgba") {
        (true, &s[4..])
    } else if bytes.len() >= 5 && bytes[..3].eq_ignore_ascii_case(b"rgb") {
        (false, &s[3..])
    } else {
        return None;
    };

    let rest = rest.trim_start();
    let inner = rest
        .strip_prefix('(')
        .and_then(|v| v.strip_suffix(')'))
        .or_else(|| rest.strip_prefix('（').and_then(|v| v.strip_suffix('）')))?
        .trim();

    if inner.is_empty() {
        return None;
    }

    let has_comma = inner.contains(',') || inner.contains('，');
    let has_slash = inner.contains('/');

    if has_comma && has_slash {
        return None;
    }

    if has_comma {
        let parts: Vec<&str> = inner
            .split([',', '，'])
            .map(str::trim)
            .collect();
        match parts.as_slice() {
            [r, g, b] => Some([
                parse_rgb_channel(r)?,
                parse_rgb_channel(g)?,
                parse_rgb_channel(b)?,
                255,
            ]),
            [r, g, b, a] => Some([
                parse_rgb_channel(r)?,
                parse_rgb_channel(g)?,
                parse_rgb_channel(b)?,
                parse_alpha_channel(a, true)?,
            ]),
            _ => None,
        }
    } else if has_slash {
        let mut slash_parts = inner.split('/');
        let rgb_part = slash_parts.next()?.trim();
        let alpha_part = slash_parts.next()?.trim();
        if slash_parts.next().is_some() {
            return None;
        }
        let channels: Vec<&str> = rgb_part.split_whitespace().collect();
        if channels.len() != 3 {
            return None;
        }
        Some([
            parse_rgb_channel(channels[0])?,
            parse_rgb_channel(channels[1])?,
            parse_rgb_channel(channels[2])?,
            parse_alpha_channel(alpha_part, true)?,
        ])
    } else {
        let channels: Vec<&str> = inner.split_whitespace().collect();
        match channels.as_slice() {
            [r, g, b] => Some([
                parse_rgb_channel(r)?,
                parse_rgb_channel(g)?,
                parse_rgb_channel(b)?,
                255,
            ]),
            [r, g, b, a] if is_rgba => Some([
                parse_rgb_channel(r)?,
                parse_rgb_channel(g)?,
                parse_rgb_channel(b)?,
                parse_alpha_channel(a, true)?,
            ]),
            _ => None,
        }
    }
}

fn parse_bare_rgb_tuple(s: &str) -> Option<[u8; 4]> {
    let inner = s
        .strip_prefix('(')
        .and_then(|v| v.strip_suffix(')'))
        .or_else(|| s.strip_prefix('（').and_then(|v| v.strip_suffix('）')))
        .unwrap_or(s)
        .trim();

    if !inner.contains(',') && !inner.contains('，') {
        return None;
    }
    let parts: Vec<&str> = inner.split([',', '，']).map(str::trim).collect();
    match parts.as_slice() {
        [r, g, b] => Some([
            parse_strict_u8_channel(r)?,
            parse_strict_u8_channel(g)?,
            parse_strict_u8_channel(b)?,
            255,
        ]),
        // For bare 4-tuples require explicit fractional/percentage alpha (e.g. `0.5`, `.5`, `50%`)
        // so ordinary 4-column integer CSV rows (`1, 2, 3, 4`) are never misidentified.
        [r, g, b, a] => Some([
            parse_strict_u8_channel(r)?,
            parse_strict_u8_channel(g)?,
            parse_strict_u8_channel(b)?,
            parse_alpha_channel(a, false)?,
        ]),
        _ => None,
    }
}

fn parse_strict_u8_channel(s: &str) -> Option<u8> {
    if s.is_empty() || s.len() > 3 || !s.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    s.parse::<u8>().ok()
}

fn parse_unsigned_decimal(s: &str) -> Option<f32> {
    if s.is_empty() || s.len() > 10 {
        return None;
    }
    let mut seen_dot = false;
    let mut seen_digit = false;
    for b in s.bytes() {
        if b.is_ascii_digit() {
            seen_digit = true;
        } else if b == b'.' && !seen_dot {
            seen_dot = true;
        } else {
            return None;
        }
    }
    if !seen_digit {
        return None;
    }
    s.parse::<f32>().ok().filter(|v| v.is_finite())
}

fn parse_rgb_channel(s: &str) -> Option<u8> {
    let s = s.trim();
    if let Some(pct_str) = s.strip_suffix('%') {
        let pct = parse_unsigned_decimal(pct_str)?;
        if (0.0..=100.0).contains(&pct) {
            Some((pct * 2.55).round().clamp(0.0, 255.0) as u8)
        } else {
            None
        }
    } else {
        let val = parse_unsigned_decimal(s)?;
        if (0.0..=255.0).contains(&val) {
            Some(val.round().clamp(0.0, 255.0) as u8)
        } else {
            None
        }
    }
}

fn parse_alpha_channel(s: &str, allow_integer: bool) -> Option<u8> {
    let s = s.trim();
    if let Some(pct_str) = s.strip_suffix('%') {
        let pct = parse_unsigned_decimal(pct_str)?;
        if (0.0..=100.0).contains(&pct) {
            return Some((pct * 2.55).round().clamp(0.0, 255.0) as u8);
        }
        return None;
    }
    if s.contains('.') {
        let val = parse_unsigned_decimal(s)?;
        if (0.0..=1.0).contains(&val) {
            return Some((val * 255.0).round().clamp(0.0, 255.0) as u8);
        }
        return None;
    }
    if !allow_integer {
        return None;
    }
    let int_val = parse_strict_u8_channel(s)?;
    match int_val {
        0 => Some(0),
        1 => Some(255),
        v => Some(v),
    }
}

pub fn format_color_hex(rgba: [u8; 4]) -> String {
    if rgba[3] == 255 {
        format!("#{:02X}{:02X}{:02X}", rgba[0], rgba[1], rgba[2])
    } else {
        format!(
            "#{:02X}{:02X}{:02X}{:02X}",
            rgba[0], rgba[1], rgba[2], rgba[3]
        )
    }
}

pub fn format_alpha_decimal(alpha: u8) -> String {
    if alpha == 0 {
        return "0".to_string();
    }
    if alpha == 255 {
        return "1".to_string();
    }
    let formatted = format!("{:.2}", alpha as f32 / 255.0);
    formatted
        .trim_end_matches('0')
        .trim_end_matches('.')
        .to_string()
}

pub fn format_color_rgb(rgba: [u8; 4]) -> String {
    if rgba[3] == 255 {
        format!("rgb({}, {}, {})", rgba[0], rgba[1], rgba[2])
    } else {
        format!(
            "rgba({}, {}, {}, {})",
            rgba[0],
            rgba[1],
            rgba[2],
            format_alpha_decimal(rgba[3])
        )
    }
}

pub fn rgba_to_hsl(r: u8, g: u8, b: u8) -> (u16, u8, u8) {
    let rf = r as f32 / 255.0;
    let gf = g as f32 / 255.0;
    let bf = b as f32 / 255.0;
    let max = rf.max(gf).max(bf);
    let min = rf.min(gf).min(bf);
    let delta = max - min;
    let l = (max + min) * 0.5;

    let (h, s) = if delta.abs() < 1e-6 {
        (0.0, 0.0)
    } else {
        let s = delta / (1.0 - (2.0 * l - 1.0).abs());
        let mut h = if (max - rf).abs() < 1e-6 {
            ((gf - bf) / delta) % 6.0
        } else if (max - gf).abs() < 1e-6 {
            (bf - rf) / delta + 2.0
        } else {
            (rf - gf) / delta + 4.0
        };
        h *= 60.0;
        if h < 0.0 {
            h += 360.0;
        }
        (h, s)
    };

    let h_deg = (h.round() as i32).rem_euclid(360) as u16;
    let s_pct = (s * 100.0).round().clamp(0.0, 100.0) as u8;
    let l_pct = (l * 100.0).round().clamp(0.0, 100.0) as u8;
    (h_deg, s_pct, l_pct)
}

pub fn format_color_hsl(rgba: [u8; 4]) -> String {
    let (h, s, l) = rgba_to_hsl(rgba[0], rgba[1], rgba[2]);
    if rgba[3] == 255 {
        format!("hsl({h}, {s}%, {l}%)")
    } else {
        format!(
            "hsla({h}, {s}%, {l}%, {})",
            format_alpha_decimal(rgba[3])
        )
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
    let mut head = trimmed.chars();
    let prefix: String = head.by_ref().take(4).collect();
    if head.take(7).count() < 7 {
        return "••••••••".to_string();
    }
    let suffix_rev: Vec<char> = trimmed.chars().rev().take(4).collect();
    let suffix: String = suffix_rev.into_iter().rev().collect();
    format!("{}••••••••••••{}", prefix, suffix)
}

pub fn format_display_line(input: &str, show_special_symbols: bool) -> String {
    const MAX_PREVIEW_CHARS: usize = 120;
    let mut out = String::with_capacity(MAX_PREVIEW_CHARS.min(input.len()));
    let mut count = 0usize;

    if !show_special_symbols {
        for line in input.lines().map(str::trim).filter(|l| !l.is_empty()) {
            if !out.is_empty() {
                out.push(' ');
                count += 1;
                if count >= MAX_PREVIEW_CHARS {
                    break;
                }
            }
            for ch in line.chars() {
                out.push(ch);
                count += 1;
                if count >= MAX_PREVIEW_CHARS {
                    return out;
                }
            }
        }
        return out;
    }

    for ch in input.chars() {
        match ch {
            '\n' => {
                out.push('↵');
                count += 1;
            }
            '\r' => {}
            '\t' => {
                out.push('⇥');
                count += 1;
            }
            ' ' => {
                out.push('·');
                count += 1;
            }
            _ => {
                out.push(ch);
                count += 1;
            }
        }
        if count >= MAX_PREVIEW_CHARS {
            break;
        }
    }
    out
}

/// Computes `(line_count, char_count, word_count)` for the workbench metadata bar.
pub fn text_stats(input: &str) -> (usize, usize, usize) {
    let lines = input.lines().count().max(usize::from(!input.is_empty()));
    let chars = input.chars().count();
    let words = input.split_whitespace().count();
    (lines, chars, words)
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
    LinesDedupe,
    LinesSort,
    CaseSnake,
    CaseCamel,
    CaseUpperSnake,
    UnescapeString,
    Sha256Hash,
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
            QuickActionKind::LinesDedupe => "多行去重",
            QuickActionKind::LinesSort => "多行排序",
            QuickActionKind::CaseSnake => "snake_case",
            QuickActionKind::CaseCamel => "camelCase",
            QuickActionKind::CaseUpperSnake => "UPPER_CASE",
            QuickActionKind::UnescapeString => "反转义",
            QuickActionKind::Sha256Hash => "SHA-256",
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
        QuickActionKind::LinesDedupe => {
            let mut seen = HashSet::new();
            let mut out = Vec::new();
            for line in input.lines() {
                if seen.insert(line) {
                    out.push(line);
                }
            }
            Ok(out.join("\n"))
        }
        QuickActionKind::LinesSort => {
            let mut lines: Vec<&str> = input.lines().collect();
            lines.sort_unstable();
            Ok(lines.join("\n"))
        }
        QuickActionKind::CaseSnake => Ok(split_identifier_words(input).join("_")),
        QuickActionKind::CaseCamel => {
            let words = split_identifier_words(input);
            let mut out = String::new();
            for (idx, w) in words.iter().enumerate() {
                if idx == 0 {
                    out.push_str(w);
                } else {
                    let mut chars = w.chars();
                    if let Some(first) = chars.next() {
                        out.extend(first.to_uppercase());
                        out.push_str(chars.as_str());
                    }
                }
            }
            Ok(out)
        }
        QuickActionKind::CaseUpperSnake => Ok(split_identifier_words(input)
            .into_iter()
            .map(|w| w.to_uppercase())
            .collect::<Vec<_>>()
            .join("_")),
        QuickActionKind::UnescapeString => Ok(unescape_text(input)),
        QuickActionKind::Sha256Hash => Ok(sha256_hex(input.as_bytes())),
    }
}

fn split_identifier_words(input: &str) -> Vec<String> {
    let mut words = Vec::new();
    let mut cur = String::new();
    let mut prev_is_lower = false;

    for ch in input.trim().chars() {
        if ch.is_ascii_alphanumeric() {
            if ch.is_ascii_uppercase() && prev_is_lower && !cur.is_empty() {
                words.push(cur.to_ascii_lowercase());
                cur.clear();
            }
            cur.push(ch);
            prev_is_lower = ch.is_ascii_lowercase();
        } else {
            if !cur.is_empty() {
                words.push(cur.to_ascii_lowercase());
                cur.clear();
            }
            prev_is_lower = false;
        }
    }
    if !cur.is_empty() {
        words.push(cur.to_ascii_lowercase());
    }
    words
}

fn unescape_text(input: &str) -> String {
    let trimmed = input.trim();
    let inner = trimmed
        .strip_prefix('"')
        .and_then(|s| s.strip_suffix('"'))
        .unwrap_or(trimmed);
    let mut out = String::with_capacity(inner.len());
    let mut chars = inner.chars();
    while let Some(c) = chars.next() {
        if c == '\\' {
            match chars.next() {
                Some('n') => out.push('\n'),
                Some('r') => out.push('\r'),
                Some('t') => out.push('\t'),
                Some('"') => out.push('"'),
                Some('\'') => out.push('\''),
                Some('\\') => out.push('\\'),
                Some(other) => {
                    out.push('\\');
                    out.push(other);
                }
                None => out.push('\\'),
            }
        } else {
            out.push(c);
        }
    }
    out
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
    let mut actions = Vec::with_capacity(6);
    if trimmed.len() <= 131_072
        && ((trimmed.starts_with('{') && trimmed.ends_with('}'))
            || (trimmed.starts_with('[') && trimmed.ends_with(']')))
        && serde_json::from_str::<serde::de::IgnoredAny>(trimmed).is_ok()
    {
        actions.push(QuickActionKind::JsonPretty);
        actions.push(QuickActionKind::JsonMinify);
    }
    if trimmed.len() <= 64 && convert_timestamp_smart(trimmed).is_ok() {
        actions.push(QuickActionKind::TimestampConvert);
    }
    actions.push(QuickActionKind::Base64Encode);
    if trimmed.len() >= 4
        && trimmed.len() <= 65_536
        && trimmed.len().is_multiple_of(4)
        && trimmed
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'+' | b'/' | b'='))
        && general_purpose::STANDARD.decode(trimmed.as_bytes()).is_ok()
    {
        actions.push(QuickActionKind::Base64Decode);
    }
    actions.push(QuickActionKind::UrlEncode);
    if trimmed.contains('%') {
        actions.push(QuickActionKind::UrlDecode);
    }
    if trimmed.contains("\\n") || trimmed.contains("\\\"") || trimmed.contains("\\\\") {
        actions.push(QuickActionKind::UnescapeString);
    }
    if !trimmed.contains('\n')
        && trimmed.len() >= 2
        && trimmed.len() <= 96
        && trimmed.chars().any(|c| c.is_ascii_alphabetic())
        && trimmed
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | ' '))
    {
        actions.push(QuickActionKind::CaseSnake);
        actions.push(QuickActionKind::CaseCamel);
        actions.push(QuickActionKind::CaseUpperSnake);
    }
    if trimmed.lines().count() >= 2 {
        actions.push(QuickActionKind::LinesDedupe);
        actions.push(QuickActionKind::LinesSort);
    }
    actions.push(QuickActionKind::TrimNormalize);
    actions.push(QuickActionKind::Sha256Hash);
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
    fn detect_color_supports_hex_and_rgb_variants_without_false_positives() {
        // Hex formats
        assert_eq!(detect_color("#f0a"), Some([0xff, 0x00, 0xaa, 255]));
        assert_eq!(detect_color("#f0a8"), Some([0xff, 0x00, 0xaa, 0x88]));
        assert_eq!(detect_color("#0A84FF"), Some([10, 132, 255, 255]));
        assert_eq!(detect_color("#0A84FF80"), Some([10, 132, 255, 128]));
        assert_eq!(detect_color("0x0A84FF"), Some([10, 132, 255, 255]));
        assert_eq!(detect_color("0X0A84FF80"), Some([10, 132, 255, 128]));
        assert_eq!(detect_color("055745"), Some([0x05, 0x57, 0x45, 255]));
        assert_eq!(detect_color("#0A84FF;"), Some([10, 132, 255, 255]));
        assert_eq!(detect_color("\"#0A84FF\""), Some([10, 132, 255, 255]));
        assert_eq!(detect_color("color: #0A84FF;"), Some([10, 132, 255, 255]));

        // RGB / RGBA functional formats (CSS 3 & CSS Color 4)
        assert_eq!(detect_color("rgb(10, 132, 255)"), Some([10, 132, 255, 255]));
        assert_eq!(detect_color("RGB(255,128,0);"), Some([255, 128, 0, 255]));
        assert_eq!(
            detect_color("rgba(10, 132, 255, 0.5)"),
            Some([10, 132, 255, 128])
        );
        assert_eq!(
            detect_color("rgba(10, 132, 255, .25)"),
            Some([10, 132, 255, 64])
        );
        assert_eq!(
            detect_color("rgba(10, 132, 255, 50%)"),
            Some([10, 132, 255, 128])
        );
        assert_eq!(
            detect_color("rgba(10, 132, 255, 128)"),
            Some([10, 132, 255, 128])
        );
        assert_eq!(
            detect_color("rgb(255 128 0)"),
            Some([255, 128, 0, 255])
        );
        assert_eq!(
            detect_color("rgb(255 128 0 / 50%)"),
            Some([255, 128, 0, 128])
        );
        assert_eq!(
            detect_color("rgb(100%, 50%, 0%)"),
            Some([255, 128, 0, 255])
        );
        assert_eq!(
            detect_color("rgb（10，132，255）"),
            Some([10, 132, 255, 255])
        );
        assert_eq!(
            detect_color("--accent: rgb(10, 132, 255);"),
            Some([10, 132, 255, 255])
        );

        // Bare RGB tuples from design tools / Digital Color Meter
        assert_eq!(detect_color("255, 128, 0"), Some([255, 128, 0, 255]));
        assert_eq!(detect_color("(10, 132, 255)"), Some([10, 132, 255, 255]));
        assert_eq!(detect_color("10, 132, 255, 0.5"), Some([10, 132, 255, 128]));
        assert_eq!(detect_color("(10, 132, 255, 50%)"), Some([10, 132, 255, 128]));

        // Non-colors must NOT match
        assert_eq!(detect_color("hello!"), None);
        assert_eq!(detect_color("192.168.1.1"), None);
        assert_eq!(detect_color("2026-11-18"), None);
        assert_eq!(detect_color("\"123456\""), None);
        assert_eq!(detect_color("port: 123456"), None);
        assert_eq!(detect_color("1, 2, 3, 4"), None);
        assert_eq!(detect_color("rgb(300, 0, 0)"), None);
        assert_eq!(detect_color("rgba(10, 20, 30, 1.5)"), None);
        assert_eq!(detect_color("rgb(-10, 20, 30)"), None);
        assert_eq!(detect_color("10 20 30"), None);
    }

    #[test]
    fn color_format_helpers_emit_valid_hex_rgb_and_hsl() {
        let blue = [10, 132, 255, 255];
        assert_eq!(format_color_hex(blue), "#0A84FF");
        assert_eq!(format_color_rgb(blue), "rgb(10, 132, 255)");
        assert_eq!(format_color_hsl(blue), "hsl(210, 100%, 52%)");

        let translucent = [255, 128, 0, 128];
        assert_eq!(format_color_hex(translucent), "#FF800080");
        assert_eq!(format_color_rgb(translucent), "rgba(255, 128, 0, 0.5)");
        assert_eq!(format_color_hsl(translucent), "hsla(30, 100%, 50%, 0.5)");
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

        assert_eq!(
            apply_quick_action(QuickActionKind::CaseSnake, "copyTriggerMode").unwrap(),
            "copy_trigger_mode"
        );
        assert_eq!(
            apply_quick_action(QuickActionKind::CaseCamel, "copy_trigger_mode").unwrap(),
            "copyTriggerMode"
        );
        assert_eq!(
            apply_quick_action(QuickActionKind::CaseUpperSnake, "copyTriggerMode").unwrap(),
            "COPY_TRIGGER_MODE"
        );
        assert_eq!(
            apply_quick_action(QuickActionKind::LinesDedupe, "b\na\nb\na").unwrap(),
            "b\na"
        );
        assert_eq!(
            apply_quick_action(QuickActionKind::LinesSort, "c\na\nb").unwrap(),
            "a\nb\nc"
        );
        assert_eq!(
            apply_quick_action(QuickActionKind::UnescapeString, r#""hello\nworld""#).unwrap(),
            "hello\nworld"
        );
        assert_eq!(
            apply_quick_action(QuickActionKind::Sha256Hash, "Earth")
                .unwrap()
                .len(),
            64
        );
        assert_eq!(text_stats("hello world\nsecond line"), (2, 23, 4));
    }
}
