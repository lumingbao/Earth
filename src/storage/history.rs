use super::{ClipItem, Collection, Storage};
use rusqlite::{Connection, OpenFlags};
use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::sync::mpsc::{self, Receiver, Sender};
use std::time::{Duration, Instant};

type Stamp = (String, i64, i64, bool, Option<i64>, Option<String>);
#[derive(Default)]
pub struct Delta {
    /// The worker's first consistent snapshot replaces the provisional first page.
    pub reset: bool,
    pub items: Vec<Arc<ClipItem>>,
    pub removed: Vec<i64>,
    pub collections: Vec<Collection>,
    pub bytes: Option<u64>,
}
pub struct History {
    tx: Sender<bool>,
    pub rx: Receiver<Result<Delta, String>>,
}
impl History {
    pub fn refresh(&self, stats: bool) {
        let _ = self.tx.send(stats);
    }
    pub fn start(store: &Storage, ctx: egui::Context) -> Self {
        let base_dir = store.base_dir.clone();
        let images_dir = store.images_dir.clone();
        let thumbs_dir = store.thumbs_dir.clone();
        let (tx, requests) = mpsc::channel::<bool>();
        let (results, rx) = mpsc::channel();
        std::thread::spawn(move || {
            let conn = match Connection::open_with_flags(
                base_dir.join("earth.db"),
                OpenFlags::SQLITE_OPEN_READ_ONLY,
            ) {
                Ok(c) => c,
                Err(e) => {
                    let _ = results.send(Err(e.to_string()));
                    ctx.request_repaint();
                    return;
                }
            };
            let store = Storage {
                base_dir,
                images_dir,
                thumbs_dir,
                conn,
            };
            let mut stamps = None;
            let mut stats_at: Option<Instant> = None;
            while let Ok(mut stats) = requests.recv() {
                for pending in requests.try_iter() {
                    stats |= pending;
                }
                let result = store.history_delta(&mut stamps).map(|mut delta| {
                    if stats && stats_at.is_none_or(|t| t.elapsed() >= Duration::from_secs(5)) {
                        delta.bytes = Some(store.total_storage_bytes());
                        stats_at = Some(Instant::now());
                    }
                    delta
                });
                if results.send(result).is_err() {
                    break;
                }
                ctx.request_repaint();
            }
        });
        Self { tx, rx }
    }
}
impl Storage {
    pub fn first_page(&self) -> Result<Vec<Arc<ClipItem>>, String> {
        self.query_items(
            "NULL,NULL",
            "ORDER BY last_copied_at DESC, id DESC LIMIT 64",
            [],
        )
        .map(|items| items.into_iter().map(Arc::new).collect())
    }
    fn history_delta(&self, known: &mut Option<HashMap<i64, Stamp>>) -> Result<Delta, String> {
        let tx = self
            .conn
            .unchecked_transaction()
            .map_err(|e| e.to_string())?;
        let mut delta = Delta {
            reset: known.is_none(),
            collections: self.list_collections()?,
            ..Default::default()
        };
        let mut current = HashMap::new();
        {
            let mut stmt=self.conn.prepare("SELECT id,content_hash,last_copied_at,copy_count,pinned,collection_id,note FROM items").map_err(|e|e.to_string())?;
            let rows = stmt
                .query_map([], |r| {
                    Ok((
                        r.get::<_, i64>(0)?,
                        (
                            r.get(1)?,
                            r.get(2)?,
                            r.get(3)?,
                            r.get(4)?,
                            r.get(5)?,
                            r.get(6)?,
                        ),
                    ))
                })
                .map_err(|e| e.to_string())?;
            for row in rows {
                let (id, key) = row.map_err(|e| e.to_string())?;
                current.insert(id, key);
            }
        }
        let changed: Vec<i64> = current
            .iter()
            .filter(|(id, key)| known.as_ref().and_then(|known| known.get(id)) != Some(key))
            .map(|(id, _)| *id)
            .collect();
        // Only changed rows transfer their text; rich clipboard formats are loaded on paste.
        for batch in changed.chunks(96) {
            let ids = batch
                .iter()
                .map(i64::to_string)
                .collect::<Vec<_>>()
                .join(",");
            delta.items.extend(
                self.query_items("NULL,NULL", &format!("WHERE id IN ({ids})"), [])?
                    .into_iter()
                    .map(Arc::new),
            );
        }
        if let Some(known) = known {
            delta.removed = known
                .keys()
                .filter(|id| !current.contains_key(id))
                .copied()
                .collect();
        }
        tx.commit().map_err(|e| e.to_string())?;
        *known = Some(current);
        Ok(delta)
    }
}
impl Delta {
    pub fn apply(
        self,
        items: &mut Vec<Arc<ClipItem>>,
        cols: &mut Vec<Collection>,
        bytes: &mut u64,
    ) {
        if self.reset {
            items.clear();
        }
        let replaced: HashSet<_> = self
            .items
            .iter()
            .map(|i| i.id)
            .chain(self.removed)
            .collect();
        items.retain(|i| !replaced.contains(&i.id));
        items.extend(self.items);
        items.sort_unstable_by_key(|i| std::cmp::Reverse((i.last_copied_at, i.id)));
        *cols = self.collections;
        if let Some(size) = self.bytes {
            *bytes = size;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{CategoryFilter, ClipKind, PinPosition, SearchMode, SortOrder};
    use crate::search::filter_and_search;
    use crate::storage::{NewClipPayload, content_hash_for_text};
    use std::path::PathBuf;

    struct TestDir(PathBuf);
    impl TestDir {
        fn new(name: &str) -> Self {
            Self(std::env::temp_dir().join(format!(
                "earth-history-{name}-{}-{}",
                std::process::id(),
                chrono::Utc::now().timestamp_nanos_opt().unwrap()
            )))
        }
    }
    impl Drop for TestDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }
    fn payload(text: &str) -> NewClipPayload {
        NewClipPayload {
            content_hash: content_hash_for_text(text),
            kind: ClipKind::Text,
            text_content: Some(text.into()),
            html_content: Some(format!("<b>{text}</b>")),
            rtf_content: Some(format!("{{\\rtf1 {text}}}")),
            image_png: None,
            thumb_png: None,
            image_width: None,
            image_height: None,
            files: vec![],
            byte_size: text.len() as u64,
            source_app: None,
            source_bundle_id: None,
            sensitive_label: None,
            detected_color: None,
            is_url: false,
        }
    }

    #[test]
    fn first_snapshot_replaces_deleted_provisional_rows_and_later_deltas_share_unchanged_rows() {
        let dir = TestDir::new("snapshot");
        let store = Storage::open_in_dir(&dir.0).unwrap();
        let deleted = store
            .upsert_item(payload("delete before snapshot"), 100)
            .unwrap();
        let stable = store.upsert_item(payload("stable"), 100).unwrap();
        let edited = store.upsert_item(payload("before"), 100).unwrap();
        let mut items = store.first_page().unwrap();
        assert_eq!(items.len(), 3);
        assert!(
            items
                .iter()
                .all(|item| item.html_content.is_none() && item.rtf_content.is_none())
        );
        assert!(store.item(stable).unwrap().html_content.is_some());
        assert!(store.item(stable).unwrap().rtf_content.is_some());
        store.delete_item(deleted).unwrap();

        let mut known = None;
        let mut cols = vec![];
        let mut bytes = 99;
        let first = store.history_delta(&mut known).unwrap();
        assert!(first.reset);
        first.apply(&mut items, &mut cols, &mut bytes);
        assert_eq!(items.len(), 2);
        assert!(items.iter().all(|item| item.id != deleted));
        assert_eq!(
            bytes, 99,
            "deferred statistics must not erase the previous value"
        );
        let stable_reference = Arc::clone(items.iter().find(|item| item.id == stable).unwrap());
        let old_edited = Arc::clone(items.iter().find(|item| item.id == edited).unwrap());
        assert!(store.history_delta(&mut known).unwrap().items.is_empty());

        store.update_item_text(edited, "after!").unwrap();
        store.set_pinned(edited, true).unwrap();
        store.set_item_note(edited, Some("annotated")).unwrap();
        let collection = store.create_collection("Group", "#112233").unwrap();
        store.set_collection(edited, Some(collection)).unwrap();
        let added = store.upsert_item(payload("new item"), 100).unwrap();
        let delta = store.history_delta(&mut known).unwrap();
        assert!(!delta.reset);
        assert_eq!(delta.items.len(), 2);
        delta.apply(&mut items, &mut cols, &mut bytes);
        assert_eq!(items.len(), 3);
        assert!(Arc::ptr_eq(
            items.iter().find(|item| item.id == stable).unwrap(),
            &stable_reference
        ));
        let updated = items.iter().find(|item| item.id == edited).unwrap();
        assert!(!Arc::ptr_eq(updated, &old_edited));
        assert_eq!(updated.text_content.as_deref(), Some("after!"));
        assert!(updated.pinned);
        assert_eq!(updated.note.as_deref(), Some("annotated"));
        assert_eq!(updated.collection_id, Some(collection));
        assert!(
            cols.iter()
                .any(|col| col.id == collection && col.name == "Group")
        );
        store.delete_item(added).unwrap();
        let delta = store.history_delta(&mut known).unwrap();
        assert_eq!(delta.removed, vec![added]);
        delta.apply(&mut items, &mut cols, &mut bytes);
        assert_eq!(items.len(), 2);
    }

    #[test]
    fn first_page_is_bounded_and_worker_defers_then_throttles_storage_statistics() {
        let dir = TestDir::new("worker");
        let store = Storage::open_in_dir(&dir.0).unwrap();
        for index in 0..70 {
            store
                .upsert_item(payload(&format!("record {index}")), 100)
                .unwrap();
        }
        assert_eq!(store.first_page().unwrap().len(), 64);
        let history = History::start(&store, egui::Context::default());
        history.refresh(false);
        let first = history
            .rx
            .recv_timeout(Duration::from_secs(5))
            .unwrap()
            .unwrap();
        assert!(first.reset);
        assert_eq!(first.items.len(), 70);
        assert!(first.bytes.is_none());
        history.refresh(true);
        let stats = history
            .rx
            .recv_timeout(Duration::from_secs(5))
            .unwrap()
            .unwrap();
        assert!(stats.items.is_empty());
        assert!(stats.bytes.is_some_and(|bytes| bytes > 0));
        history.refresh(true);
        let throttled = history
            .rx
            .recv_timeout(Duration::from_secs(5))
            .unwrap()
            .unwrap();
        assert!(throttled.items.is_empty() && throttled.bytes.is_none());
    }

    /// cargo test history_5000_rows_2000_thumbnails_benchmark -- --ignored --nocapture
    #[test]
    #[ignore = "explicit synthetic history benchmark; creates 5000 rows and 2000 thumbnail files"]
    fn history_5000_rows_2000_thumbnails_benchmark() {
        let dir = TestDir::new("benchmark");
        let store = Storage::open_in_dir(&dir.0).unwrap();
        let mut png = std::io::Cursor::new(Vec::new());
        image::DynamicImage::new_rgba8(8, 8)
            .write_to(&mut png, image::ImageFormat::Png)
            .unwrap();
        let text = format!("synthetic benchmark needle {}", "x".repeat(1024));
        let rich = format!("<pre>{}</pre>", "r".repeat(4096));
        let tx = store.conn.unchecked_transaction().unwrap();
        {
            let mut insert = store.conn.prepare(
                "INSERT INTO items (content_hash,kind,text_content,html_content,rtf_content,image_path,thumb_path,image_width,image_height,byte_size,first_copied_at,last_copied_at)
                 VALUES (?1,?2,?3,?4,?4,?5,?5,8,8,?6,?7,?7)"
            ).unwrap();
            for index in 0..5000 {
                let image = index < 2000;
                let path = image.then(|| store.thumbs_dir.join(format!("synthetic-{index}.png")));
                if let Some(path) = &path {
                    std::fs::write(path, png.get_ref()).unwrap();
                }
                insert
                    .execute(rusqlite::params![
                        format!("synthetic-{index}"),
                        if image { "image" } else { "text" },
                        (!image).then_some(text.as_str()),
                        (!image).then_some(rich.as_str()),
                        path.as_ref()
                            .map(|path| path.to_string_lossy().into_owned()),
                        (if image {
                            png.get_ref().len()
                        } else {
                            text.len()
                        }) as i64,
                        index,
                    ])
                    .unwrap();
            }
        }
        tx.commit().unwrap();
        assert_eq!(std::fs::read_dir(&store.thumbs_dir).unwrap().count(), 2000);
        let first_at = Instant::now();
        let mut items = store.first_page().unwrap();
        let first_ms = first_at.elapsed().as_secs_f64() * 1000.0;
        assert_eq!(items.len(), 64);
        let mut known = None;
        let full_at = Instant::now();
        let delta = store.history_delta(&mut known).unwrap();
        delta.apply(&mut items, &mut vec![], &mut 0);
        let full_ms = full_at.elapsed().as_secs_f64() * 1000.0;
        assert_eq!(items.len(), 5000);
        assert!(
            items
                .iter()
                .all(|item| item.html_content.is_none() && item.rtf_content.is_none())
        );
        let unchanged_at = Instant::now();
        assert!(store.history_delta(&mut known).unwrap().items.is_empty());
        let unchanged_ms = unchanged_at.elapsed().as_secs_f64() * 1000.0;
        let mut query_metrics = vec![];
        for (mode, query, expected) in [
            (SearchMode::Exact, "", 5000),
            (SearchMode::Exact, "needle", 3000),
            (SearchMode::Fuzzy, "needle", 3000),
            (SearchMode::Regex, "n[e]+dle", 3000),
        ] {
            let query_at = Instant::now();
            for _ in 0..5 {
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
                assert_eq!(results.len(), expected);
                for result in results {
                    let source = &items[5000 - result.item.id as usize];
                    assert!(
                        Arc::ptr_eq(&result.item, source),
                        "query must share row payloads"
                    );
                }
            }
            query_metrics.push(format!(
                "{mode:?}/{query:?} {:.2}ms",
                query_at.elapsed().as_secs_f64() * 1000.0 / 5.0
            ));
        }
        let stats_at = Instant::now();
        assert!(store.total_storage_bytes() > 0);
        println!(
            "5000 rows / 2000 thumbnails: first64={first_ms:.2}ms full={full_ms:.2}ms unchanged={unchanged_ms:.2}ms stats={:.2}ms; {}; all results share row payloads",
            stats_at.elapsed().as_secs_f64() * 1000.0,
            query_metrics.join(", ")
        );
    }
}
