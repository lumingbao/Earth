use crate::model::{CategoryFilter, ClipItem, ClipKind, PinPosition, SearchMode, SortOrder};
use nucleo_matcher::pattern::{AtomKind, CaseMatching, Normalization, Pattern};
use nucleo_matcher::{Config, Matcher, Utf32Str};
use regex::RegexBuilder;
use std::borrow::Cow;
use std::collections::HashSet;
use std::sync::Arc;

#[derive(Debug, Clone)]
pub struct SearchResultItem {
    pub item: Arc<ClipItem>,
    pub score: u32,
    /// Character indices (in the item's `display_text`) matched by the current query.
    pub matched_indices: HashSet<usize>,
}

pub fn matches_filter(item: &ClipItem, filter: CategoryFilter) -> bool {
    match filter {
        CategoryFilter::All => true,
        CategoryFilter::Text => item.kind == ClipKind::Text,
        CategoryFilter::Image => item.kind == ClipKind::Image,
        CategoryFilter::Files => item.kind == ClipKind::Files,
        CategoryFilter::Pinned => item.pinned,
        CategoryFilter::Collection(id) => item.collection_id == Some(id),
    }
}

pub fn display_line(item: &ClipItem, symbols: bool) -> String {
    match item.kind {
        ClipKind::Text => crate::model::truncate_chars(
            &crate::transforms::format_display_line(
                item.text_content.as_deref().unwrap_or_default(),
                symbols,
            ),
            88,
        ),
        ClipKind::Files => item.summary_line(60),
        ClipKind::Image => item.summary_line(40),
    }
}

pub fn filter_and_search(
    items: &[Arc<ClipItem>],
    query: &str,
    filter: CategoryFilter,
    search_mode: SearchMode,
    sort_order: SortOrder,
    pin_position: PinPosition,
    symbols: bool,
) -> Result<Vec<SearchResultItem>, String> {
    let mut filtered: Vec<&Arc<ClipItem>> = items
        .iter()
        .filter(|item| matches_filter(item, filter))
        .collect();

    // Sort according to user preference & pin position
    filtered.sort_by(|a, b| {
        if filter != CategoryFilter::Pinned {
            match (a.pinned, b.pinned) {
                (true, false) => {
                    return match pin_position {
                        PinPosition::Top => std::cmp::Ordering::Less,
                        PinPosition::Bottom => std::cmp::Ordering::Greater,
                    };
                }
                (false, true) => {
                    return match pin_position {
                        PinPosition::Top => std::cmp::Ordering::Greater,
                        PinPosition::Bottom => std::cmp::Ordering::Less,
                    };
                }
                _ => {}
            }
        }
        match sort_order {
            SortOrder::LastCopiedAt => b.last_copied_at.cmp(&a.last_copied_at),
            SortOrder::FirstCopiedAt => b.first_copied_at.cmp(&a.first_copied_at),
            SortOrder::CopyCount => b
                .copy_count
                .cmp(&a.copy_count)
                .then_with(|| b.last_copied_at.cmp(&a.last_copied_at)),
        }
    });

    let trimmed_query = query.trim();
    if trimmed_query.is_empty() {
        return Ok(filtered
            .into_iter()
            .map(|item| SearchResultItem {
                item: Arc::clone(item),
                score: 0,
                matched_indices: HashSet::new(),
            })
            .collect());
    }
    if trimmed_query.len() > 4096 {
        return Err("搜索内容超过 4096 字节".into());
    }
    match search_mode {
        SearchMode::Exact => Ok(exact_search(&filtered, trimmed_query, symbols)),
        SearchMode::Fuzzy => Ok(fuzzy_search(&filtered, trimmed_query, symbols)),
        SearchMode::Regex => regex_search(&filtered, trimmed_query, symbols),
    }
}

fn searchable_text(item: &ClipItem) -> Cow<'_, str> {
    if item.kind == ClipKind::Text
        && item
            .note
            .as_deref()
            .is_none_or(|note| note.trim().is_empty())
    {
        Cow::Borrowed(item.text_content.as_deref().unwrap_or_default())
    } else {
        Cow::Owned(item.searchable_text())
    }
}

fn exact_search(items: &[&Arc<ClipItem>], query: &str, symbols: bool) -> Vec<SearchResultItem> {
    let lower_query = query.to_lowercase();
    let mut results = Vec::new();

    for item in items {
        let haystack = searchable_text(item);
        if haystack.to_lowercase().contains(&lower_query) {
            let matched = find_substring_char_indices(&display_line(item, symbols), &lower_query)
                .unwrap_or_default();
            results.push(SearchResultItem {
                item: Arc::clone(item),
                score: 100,
                matched_indices: matched,
            });
        }
    }
    results
}

fn fuzzy_search(items: &[&Arc<ClipItem>], query: &str, symbols: bool) -> Vec<SearchResultItem> {
    let mut matcher = Matcher::new(Config::DEFAULT);
    let pattern = Pattern::new(
        query,
        CaseMatching::Smart,
        Normalization::Smart,
        AtomKind::Fuzzy,
    );
    let mut utf32_buf = Vec::new();
    let mut indices_buf = Vec::new();
    let mut results = Vec::new();

    for item in items {
        let haystack = searchable_text(item);
        utf32_buf.clear();
        indices_buf.clear();
        let u32_haystack = Utf32Str::new(&haystack, &mut utf32_buf);
        if let Some(score) = pattern.score(u32_haystack, &mut matcher) {
            let label = display_line(item, symbols);
            let _ = pattern.indices(
                Utf32Str::new(&label, &mut utf32_buf),
                &mut matcher,
                &mut indices_buf,
            );
            let matched_indices = indices_buf.iter().map(|&idx| idx as usize).collect();
            results.push(SearchResultItem {
                item: Arc::clone(item),
                score,
                matched_indices,
            });
        }
    }

    results.sort_by(|a, b| {
        b.score
            .cmp(&a.score)
            .then_with(|| b.item.last_copied_at.cmp(&a.item.last_copied_at))
    });
    results
}

fn regex_search(
    items: &[&Arc<ClipItem>],
    query: &str,
    symbols: bool,
) -> Result<Vec<SearchResultItem>, String> {
    let re = RegexBuilder::new(query)
        .case_insensitive(true)
        .size_limit(1024 * 1024)
        .build()
        .map_err(|e| format!("正则语法错误：{e}"))?;
    let mut results = Vec::new();
    for item in items {
        if !re.is_match(&searchable_text(item)) {
            continue;
        }
        let label = display_line(item, symbols);
        let mut indices = HashSet::new();
        let mut last = 0;
        let mut pos = 0;
        for m in re.find_iter(&label) {
            pos += label[last..m.start()].chars().count();
            let end = pos + m.as_str().chars().count();
            indices.extend(pos..end);
            pos = end;
            last = m.end();
        }
        results.push(SearchResultItem {
            item: Arc::clone(item),
            score: 100,
            matched_indices: indices,
        });
    }
    Ok(results)
}

pub fn find_substring_char_indices(haystack: &str, lower_query: &str) -> Option<HashSet<usize>> {
    if lower_query.is_empty() {
        return Some(HashSet::new());
    }
    let hay_chars: Vec<char> = haystack.chars().collect();
    let hay_lower: Vec<char> = hay_chars.iter().flat_map(|c| c.to_lowercase()).collect();
    let needle: Vec<char> = lower_query.chars().collect();

    if needle.is_empty() || needle.len() > hay_lower.len() {
        return None;
    }

    // Fallback cleanly to byte-level lowercase match when 1-to-many lowercase expansion shifts lengths
    if hay_lower.len() != hay_chars.len() {
        let hay_l = haystack.to_lowercase();
        return hay_l.contains(lower_query).then(HashSet::new);
    }

    let mut indices = HashSet::new();
    let mut found = false;
    let mut i = 0;
    while i + needle.len() <= hay_lower.len() {
        if hay_lower[i..i + needle.len()] == needle[..] {
            found = true;
            for offset in 0..needle.len() {
                indices.insert(i + offset);
            }
            i += needle.len();
        } else {
            i += 1;
        }
    }
    found.then_some(indices)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_text_item(id: i64, text: &str, pinned: bool, ts: i64) -> Arc<ClipItem> {
        Arc::new(ClipItem {
            id,
            content_hash: format!("hash-{id}"),
            kind: ClipKind::Text,
            text_content: Some(text.to_string()),
            html_content: None,
            rtf_content: None,
            image_path: None,
            thumb_path: None,
            image_width: None,
            image_height: None,
            files: Vec::new(),
            byte_size: text.len() as u64,
            source_app: None,
            source_bundle_id: None,
            pinned,
            collection_id: None,
            first_copied_at: ts,
            last_copied_at: ts,
            copy_count: 1,
            sensitive_label: None,
            detected_color: None,
            is_url: false,
            note: None,
        })
    }

    #[test]
    fn every_search_mode_shares_immutable_payloads_with_the_history_snapshot() {
        let items = vec![sample_text_item(
            1,
            &format!("needle {}", "x".repeat(8192)),
            false,
            1,
        )];
        for mode in [SearchMode::Exact, SearchMode::Fuzzy, SearchMode::Regex] {
            for query in ["", "needle"] {
                let results = filter_and_search(
                    &items,
                    query,
                    CategoryFilter::All,
                    mode,
                    SortOrder::LastCopiedAt,
                    PinPosition::Top,
                    false,
                )
                .unwrap();
                assert_eq!(results.len(), 1);
                assert!(Arc::ptr_eq(&results[0].item, &items[0]));
                assert_eq!(
                    results[0].item.text_content.as_ref().unwrap().as_ptr(),
                    items[0].text_content.as_ref().unwrap().as_ptr()
                );
            }
        }
        assert!(matches!(searchable_text(&items[0]), Cow::Borrowed(_)));
    }

    #[test]
    fn regex_highlights_only_visible_text_and_reports_syntax_errors() {
        let mut items = [sample_text_item(1, "    一\n   两 ", false, 1)];
        Arc::make_mut(&mut items[0]).note = Some("notes".into());
        let run = |query| {
            filter_and_search(
                &items,
                query,
                CategoryFilter::All,
                SearchMode::Regex,
                SortOrder::LastCopiedAt,
                PinPosition::Top,
                false,
            )
        };
        assert_eq!(run("两").unwrap()[0].matched_indices, HashSet::from([2]));
        assert!(run("notes").unwrap()[0].matched_indices.is_empty());
        assert!(run("(").is_err());
        let big = sample_text_item(2, &"a".repeat(1_000_000), false, 2);
        assert!(
            regex_search(&[&big], ".", false).unwrap()[0]
                .matched_indices
                .len()
                <= 89
        );
    }

    #[test]
    fn sorts_pinned_according_to_pin_position() {
        let items = vec![
            sample_text_item(1, "unpinned new", false, 200),
            sample_text_item(2, "pinned old", true, 100),
        ];

        let top = filter_and_search(
            &items,
            "",
            CategoryFilter::All,
            SearchMode::Exact,
            SortOrder::LastCopiedAt,
            PinPosition::Top,
            false,
        )
        .unwrap();
        assert_eq!(top[0].item.id, 2);

        let bottom = filter_and_search(
            &items,
            "",
            CategoryFilter::All,
            SearchMode::Exact,
            SortOrder::LastCopiedAt,
            PinPosition::Bottom,
            false,
        )
        .unwrap();
        assert_eq!(bottom[0].item.id, 1);
    }

    #[test]
    fn exact_fuzzy_regex_modes_work() {
        let items = vec![
            sample_text_item(1, "getAddPrizeStatus", false, 300),
            sample_text_item(2, "Earth Clipboard Manager", false, 200),
            sample_text_item(3, "feat(competition): 扩展学科竞赛", false, 100),
        ];

        let exact = filter_and_search(
            &items,
            "prize",
            CategoryFilter::All,
            SearchMode::Exact,
            SortOrder::LastCopiedAt,
            PinPosition::Top,
            false,
        )
        .unwrap();
        assert_eq!(exact.len(), 1);
        assert_eq!(exact[0].item.id, 1);

        let fuzzy = filter_and_search(
            &items,
            "ecm",
            CategoryFilter::All,
            SearchMode::Fuzzy,
            SortOrder::LastCopiedAt,
            PinPosition::Top,
            false,
        )
        .unwrap();
        assert_eq!(fuzzy[0].item.id, 2);

        let regex = filter_and_search(
            &items,
            r"扩展.*竞赛",
            CategoryFilter::All,
            SearchMode::Regex,
            SortOrder::LastCopiedAt,
            PinPosition::Top,
            false,
        )
        .unwrap();
        assert_eq!(regex.len(), 1);
        assert_eq!(regex[0].item.id, 3);
    }
}
