use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::OnceLock;
use std::sync::atomic::{AtomicU8, Ordering};

const ZH_CN_JSON: &str = include_str!("../locales/zh-CN.json");
const EN_US_JSON: &str = include_str!("../locales/en-US.json");

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum AppLanguage {
    #[default]
    System,
    ZhCn,
    EnUs,
}

impl AppLanguage {
    pub fn label(&self) -> &'static str {
        match self {
            AppLanguage::System => t("lang.system"),
            AppLanguage::ZhCn => "简体中文",
            AppLanguage::EnUs => "English",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum ResolvedLanguage {
    ZhCn = 0,
    EnUs = 1,
}

impl ResolvedLanguage {
    fn from_u8(v: u8) -> Self {
        match v {
            1 => ResolvedLanguage::EnUs,
            _ => ResolvedLanguage::ZhCn,
        }
    }
}

/// Active process language. Starts as `ZhCn` until `EarthApp::new` calls `set_language(...)`
/// so headless unit tests stay deterministic regardless of the host OS locale.
static ACTIVE_LANGUAGE: AtomicU8 = AtomicU8::new(ResolvedLanguage::ZhCn as u8);

#[cfg(test)]
std::thread_local! {
    static TEST_LANG_OVERRIDE: std::cell::Cell<Option<ResolvedLanguage>> = const { std::cell::Cell::new(None) };
}

fn zh_table() -> &'static HashMap<String, String> {
    static TABLE: OnceLock<HashMap<String, String>> = OnceLock::new();
    TABLE.get_or_init(|| {
        serde_json::from_str(ZH_CN_JSON).expect("locales/zh-CN.json must be valid JSON")
    })
}

fn en_table() -> &'static HashMap<String, String> {
    static TABLE: OnceLock<HashMap<String, String>> = OnceLock::new();
    TABLE.get_or_init(|| {
        serde_json::from_str(EN_US_JSON).expect("locales/en-US.json must be valid JSON")
    })
}

pub fn set_language(lang: AppLanguage) {
    let resolved = resolve_app_language(lang);
    ACTIVE_LANGUAGE.store(resolved as u8, Ordering::Release);
}

pub fn resolve_app_language(lang: AppLanguage) -> ResolvedLanguage {
    match lang {
        AppLanguage::ZhCn => ResolvedLanguage::ZhCn,
        AppLanguage::EnUs => ResolvedLanguage::EnUs,
        AppLanguage::System => detect_system_language(),
    }
}

pub fn current_language() -> ResolvedLanguage {
    #[cfg(test)]
    if let Some(override_lang) = TEST_LANG_OVERRIDE.with(|cell| cell.get()) {
        return override_lang;
    }
    ResolvedLanguage::from_u8(ACTIVE_LANGUAGE.load(Ordering::Acquire))
}

#[cfg(test)]
pub fn with_test_language<R>(lang: ResolvedLanguage, f: impl FnOnce() -> R) -> R {
    struct Reset(Option<ResolvedLanguage>);
    impl Drop for Reset {
        fn drop(&mut self) {
            TEST_LANG_OVERRIDE.with(|cell| cell.set(self.0));
        }
    }
    let prev = TEST_LANG_OVERRIDE.with(|cell| cell.replace(Some(lang)));
    let _reset = Reset(prev);
    f()
}

/// Returns the localized string for `key` in the current active language,
/// falling back to `zh-CN` if missing and finally to `key` itself.
pub fn t(key: &'static str) -> &'static str {
    t_for(current_language(), key)
}

pub fn t_for(lang: ResolvedLanguage, key: &'static str) -> &'static str {
    lookup_in_tables(lang, key, zh_table(), en_table())
}

fn lookup_in_tables<'a>(
    lang: ResolvedLanguage,
    key: &'a str,
    zh: &'a HashMap<String, String>,
    en: &'a HashMap<String, String>,
) -> &'a str {
    if lang == ResolvedLanguage::EnUs
        && let Some(val) = en.get(key)
        && !val.is_empty()
    {
        return val.as_str();
    }
    if let Some(val) = zh.get(key)
        && !val.is_empty()
    {
        return val.as_str();
    }
    key
}

/// Formats a localized template by substituting `{name}` placeholders.
pub fn tf(key: &'static str, args: &[(&str, &str)]) -> String {
    let mut out = t(key).to_string();
    for (name, value) in args {
        let placeholder = format!("{{{name}}}");
        out = out.replace(&placeholder, value);
    }
    out
}

pub fn resolve_locale_tag(tag: &str) -> ResolvedLanguage {
    let trimmed = tag.trim();
    if trimmed.is_empty() {
        return ResolvedLanguage::ZhCn;
    }
    let primary = trimmed
        .split(['-', '_', '.', '@'])
        .next()
        .unwrap_or("")
        .to_ascii_lowercase();
    match primary.as_str() {
        "zh" => ResolvedLanguage::ZhCn,
        "c" | "posix" => ResolvedLanguage::EnUs,
        "" => ResolvedLanguage::ZhCn,
        _ => ResolvedLanguage::EnUs,
    }
}

pub fn detect_system_language() -> ResolvedLanguage {
    detect_os_locale()
        .or_else(detect_env_locale)
        .map(|tag| resolve_locale_tag(&tag))
        .unwrap_or(ResolvedLanguage::ZhCn)
}

fn detect_env_locale() -> Option<String> {
    for var in ["LC_ALL", "LC_MESSAGES", "LANG"] {
        if let Ok(val) = std::env::var(var) {
            let trimmed = val.trim();
            if !trimmed.is_empty() {
                return Some(trimmed.to_string());
            }
        }
    }
    None
}

#[cfg(target_os = "macos")]
fn detect_os_locale() -> Option<String> {
    use objc2::msg_send;
    use objc2::rc::Retained;
    use objc2::runtime::AnyClass;
    use objc2_foundation::{NSArray, NSString};

    let cls = AnyClass::get(c"NSLocale")?;
    let langs: Retained<NSArray<NSString>> = unsafe { msg_send![cls, preferredLanguages] };
    langs.firstObject().map(|s| s.to_string())
}

#[cfg(target_os = "windows")]
fn detect_os_locale() -> Option<String> {
    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn GetUserDefaultLocaleName(lp_locale_name: *mut u16, cch_locale_name: i32) -> i32;
    }

    let mut buf = [0u16; 85];
    let len = unsafe { GetUserDefaultLocaleName(buf.as_mut_ptr(), buf.len() as i32) };
    if len <= 1 {
        return None;
    }
    let slice_len = (len as usize).saturating_sub(1).min(buf.len());
    Some(String::from_utf16_lossy(&buf[..slice_len]))
}

#[cfg(not(any(target_os = "macos", target_os = "windows")))]
fn detect_os_locale() -> Option<String> {
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeSet;

    fn extract_placeholders(s: &str) -> BTreeSet<String> {
        let mut set = BTreeSet::new();
        let mut chars = s.chars().peekable();
        while let Some(c) = chars.next() {
            if c == '{' {
                let mut name = String::new();
                for ch in chars.by_ref() {
                    if ch == '}' {
                        break;
                    }
                    name.push(ch);
                }
                if !name.is_empty() {
                    set.insert(name);
                }
            }
        }
        set
    }

    #[test]
    fn locale_files_have_identical_keys_and_placeholders() {
        let zh = zh_table();
        let en = en_table();
        let zh_keys: BTreeSet<&String> = zh.keys().collect();
        let en_keys: BTreeSet<&String> = en.keys().collect();
        assert_eq!(
            zh_keys, en_keys,
            "zh-CN.json and en-US.json must contain the exact same set of keys"
        );
        for (key, zh_val) in zh {
            let en_val = en.get(key).expect("en-US key verified above");
            assert!(
                !zh_val.is_empty() && !en_val.is_empty(),
                "locale key '{key}' must not be empty"
            );
            assert_eq!(
                extract_placeholders(zh_val),
                extract_placeholders(en_val),
                "placeholder mismatch for key '{key}'"
            );
        }
    }

    #[test]
    fn missing_en_key_falls_back_to_zh_and_unknown_falls_back_to_key() {
        let zh = HashMap::from([
            ("only_zh".to_string(), "中文回退".to_string()),
            ("both".to_string(), "中文".to_string()),
        ]);
        let en = HashMap::from([("both".to_string(), "English".to_string())]);

        assert_eq!(
            lookup_in_tables(ResolvedLanguage::EnUs, "both", &zh, &en),
            "English"
        );
        assert_eq!(
            lookup_in_tables(ResolvedLanguage::EnUs, "only_zh", &zh, &en),
            "中文回退"
        );
        assert_eq!(
            lookup_in_tables(ResolvedLanguage::EnUs, "unknown.key", &zh, &en),
            "unknown.key"
        );
    }

    #[test]
    fn locale_tag_resolution_handles_chinese_english_and_others() {
        assert_eq!(resolve_locale_tag("zh-Hans-CN"), ResolvedLanguage::ZhCn);
        assert_eq!(resolve_locale_tag("zh_CN.UTF-8"), ResolvedLanguage::ZhCn);
        assert_eq!(resolve_locale_tag("zh-TW"), ResolvedLanguage::ZhCn);
        assert_eq!(resolve_locale_tag("en-US"), ResolvedLanguage::EnUs);
        assert_eq!(resolve_locale_tag("en_GB.UTF-8"), ResolvedLanguage::EnUs);
        assert_eq!(resolve_locale_tag("ja-JP"), ResolvedLanguage::EnUs);
        assert_eq!(resolve_locale_tag("C"), ResolvedLanguage::EnUs);
        assert_eq!(resolve_locale_tag(""), ResolvedLanguage::ZhCn);
    }

    #[test]
    fn test_language_guard_switches_translations_locally() {
        assert_eq!(t("settings.title"), "Earth 偏好设置");
        with_test_language(ResolvedLanguage::EnUs, || {
            assert_eq!(t("settings.title"), "Earth Preferences");
            assert_eq!(
                tf("footer.stats", &[("count", "5"), ("size", "12 KB")]),
                "5 items · 12 KB"
            );
        });
        assert_eq!(t("settings.title"), "Earth 偏好设置");
    }
}
