use crate::model::{AppSettings, ClipItem, ClipKind, Collection, FileEntry};
use chrono::Utc;
use directories::ProjectDirs;
use rusqlite::{params, Connection, OptionalExtension};
use sha2::{Digest, Sha256};
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone)]
pub struct NewClipPayload {
    pub content_hash: String,
    pub kind: ClipKind,
    pub text_content: Option<String>,
    pub html_content: Option<String>,
    pub rtf_content: Option<String>,
    pub image_png: Option<Vec<u8>>,
    pub thumb_png: Option<Vec<u8>>,
    pub image_width: Option<u32>,
    pub image_height: Option<u32>,
    pub files: Vec<FileEntry>,
    pub byte_size: u64,
    pub source_app: Option<String>,
    pub source_bundle_id: Option<String>,
    pub sensitive_label: Option<String>,
    pub detected_color: Option<[u8; 4]>,
    pub is_url: bool,
}

pub struct Storage {
    base_dir: PathBuf,
    images_dir: PathBuf,
    thumbs_dir: PathBuf,
    conn: Connection,
}

impl Storage {
    pub fn open_default() -> Result<Self, String> {
        let proj = ProjectDirs::from("io", "earth", "Earth")
            .ok_or_else(|| "无法解析系统应用数据目录".to_string())?;
        Self::open_in_dir(proj.data_dir())
    }

    pub fn open_in_dir(base_dir: &Path) -> Result<Self, String> {
        fs::create_dir_all(base_dir).map_err(|e| format!("创建数据目录失败: {e}"))?;
        let images_dir = base_dir.join("images");
        let thumbs_dir = base_dir.join("thumbs");
        fs::create_dir_all(&images_dir).map_err(|e| format!("创建图片目录失败: {e}"))?;
        fs::create_dir_all(&thumbs_dir).map_err(|e| format!("创建缩略图目录失败: {e}"))?;

        let db_path = base_dir.join("earth.db");
        let conn = Connection::open(&db_path).map_err(|e| format!("打开数据库失败: {e}"))?;
        conn.execute_batch(
            "PRAGMA journal_mode = WAL;
             PRAGMA synchronous = NORMAL;
             PRAGMA foreign_keys = ON;",
        )
        .map_err(|e| format!("配置 SQLite PRAGMA 失败: {e}"))?;

        let storage = Self {
            base_dir: base_dir.to_path_buf(),
            images_dir,
            thumbs_dir,
            conn,
        };
        storage.init_schema()?;
        Ok(storage)
    }

    fn init_schema(&self) -> Result<(), String> {
        self.conn
            .execute_batch(
                "CREATE TABLE IF NOT EXISTS collections (
                    id INTEGER PRIMARY KEY AUTOINCREMENT,
                    name TEXT NOT NULL UNIQUE,
                    color_hex TEXT NOT NULL,
                    created_at INTEGER NOT NULL
                );
                CREATE TABLE IF NOT EXISTS items (
                    id INTEGER PRIMARY KEY AUTOINCREMENT,
                    content_hash TEXT NOT NULL UNIQUE,
                    kind TEXT NOT NULL,
                    text_content TEXT,
                    html_content TEXT,
                    rtf_content TEXT,
                    image_path TEXT,
                    thumb_path TEXT,
                    image_width INTEGER,
                    image_height INTEGER,
                    files_json TEXT NOT NULL DEFAULT '[]',
                    byte_size INTEGER NOT NULL DEFAULT 0,
                    source_app TEXT,
                    source_bundle_id TEXT,
                    pinned INTEGER NOT NULL DEFAULT 0,
                    collection_id INTEGER REFERENCES collections(id) ON DELETE SET NULL,
                    first_copied_at INTEGER NOT NULL,
                    last_copied_at INTEGER NOT NULL,
                    copy_count INTEGER NOT NULL DEFAULT 1,
                    sensitive_label TEXT,
                    detected_color TEXT,
                    is_url INTEGER NOT NULL DEFAULT 0
                );
                CREATE INDEX IF NOT EXISTS idx_items_last_copied ON items(last_copied_at DESC);
                CREATE INDEX IF NOT EXISTS idx_items_pinned ON items(pinned);",
            )
            .map_err(|e| format!("初始化数据库结构失败: {e}"))?;

        // Seed starter collections only once on empty table
        let existing_count: i64 = self
            .conn
            .query_row("SELECT COUNT(*) FROM collections", [], |r| r.get(0))
            .unwrap_or(0);
        if existing_count == 0 {
            let now = Utc::now().timestamp_millis();
            let _ = self.conn.execute(
                "INSERT OR IGNORE INTO collections (name, color_hex, created_at) VALUES (?1, ?2, ?3)",
                params!["常用指令", "#456EFD", now],
            );
            let _ = self.conn.execute(
                "INSERT OR IGNORE INTO collections (name, color_hex, created_at) VALUES (?1, ?2, ?3)",
                params!["代码片段", "#22C55E", now],
            );
        } else {
            // Upgrade untouched legacy starter colors to GOSTOCK Electric Royal Blue / Emerald
            let _ = self.conn.execute(
                "UPDATE collections SET color_hex = '#456EFD' WHERE name = '常用指令' AND color_hex IN ('#52525B', '#1677FF')",
                [],
            );
            let _ = self.conn.execute(
                "UPDATE collections SET color_hex = '#22C55E' WHERE name = '代码片段' AND color_hex IN ('#71717A', '#13C2C2')",
                [],
            );
        }
        Ok(())
    }

    pub fn settings_path(&self) -> PathBuf {
        self.base_dir.join("settings.json")
    }

    pub fn load_settings(&self) -> AppSettings {
        let path = self.settings_path();
        if let Ok(raw) = fs::read_to_string(&path)
            && let Ok(parsed) = serde_json::from_str::<AppSettings>(&raw) {
                return parsed;
            }
        AppSettings::default()
    }

    pub fn save_settings(&self, settings: &AppSettings) -> Result<(), String> {
        let json = serde_json::to_string_pretty(settings)
            .map_err(|e| format!("序列化设置失败: {e}"))?;
        fs::write(self.settings_path(), json).map_err(|e| format!("写入设置失败: {e}"))
    }

    pub fn upsert_item(
        &self,
        payload: NewClipPayload,
        max_history: usize,
    ) -> Result<i64, String> {
        let now = Utc::now().timestamp_millis();

        let existing_id: Option<i64> = self
            .conn
            .query_row(
                "SELECT id FROM items WHERE content_hash = ?1",
                params![&payload.content_hash],
                |row| row.get(0),
            )
            .optional()
            .map_err(|e| format!("查询重复记录失败: {e}"))?;

        if let Some(id) = existing_id {
            self.conn
                .execute(
                    "UPDATE items
                     SET last_copied_at = ?1,
                         copy_count = copy_count + 1,
                         source_app = COALESCE(?2, source_app),
                         source_bundle_id = COALESCE(?3, source_bundle_id),
                         html_content = COALESCE(?4, html_content),
                         rtf_content = COALESCE(?5, rtf_content)
                     WHERE id = ?6",
                    params![
                        now,
                        payload.source_app,
                        payload.source_bundle_id,
                        payload.html_content,
                        payload.rtf_content,
                        id
                    ],
                )
                .map_err(|e| format!("更新剪贴板记录失败: {e}"))?;
            return Ok(id);
        }

        let (image_path, thumb_path) = if payload.kind == ClipKind::Image {
            let file_stem = &payload.content_hash[..16.min(payload.content_hash.len())];
            let img_file = self.images_dir.join(format!("{file_stem}.png"));
            let thumb_file = self.thumbs_dir.join(format!("{file_stem}_thumb.png"));

            if let Some(bytes) = &payload.image_png {
                fs::write(&img_file, bytes).map_err(|e| format!("保存图片失败: {e}"))?;
            }
            if let Some(bytes) = &payload.thumb_png {
                fs::write(&thumb_file, bytes).map_err(|e| format!("保存缩略图失败: {e}"))?;
            }
            (
                payload.image_png.as_ref().map(|_| img_file.to_string_lossy().into_owned()),
                payload.thumb_png.as_ref().map(|_| thumb_file.to_string_lossy().into_owned()),
            )
        } else {
            (None, None)
        };

        let files_json =
            serde_json::to_string(&payload.files).unwrap_or_else(|_| "[]".to_string());
        let color_str = payload
            .detected_color
            .map(|c| format!("{},{},{},{}", c[0], c[1], c[2], c[3]));

        self.conn
            .execute(
                "INSERT INTO items (
                    content_hash, kind, text_content, html_content, rtf_content,
                    image_path, thumb_path, image_width, image_height,
                    files_json, byte_size, source_app, source_bundle_id,
                    pinned, collection_id, first_copied_at, last_copied_at,
                    copy_count, sensitive_label, detected_color, is_url
                ) VALUES (
                    ?1, ?2, ?3, ?4, ?5,
                    ?6, ?7, ?8, ?9,
                    ?10, ?11, ?12, ?13,
                    0, NULL, ?14, ?14,
                    1, ?15, ?16, ?17
                )",
                params![
                    payload.content_hash,
                    payload.kind.as_str(),
                    payload.text_content,
                    payload.html_content,
                    payload.rtf_content,
                    image_path,
                    thumb_path,
                    payload.image_width.map(|v| v as i64),
                    payload.image_height.map(|v| v as i64),
                    files_json,
                    payload.byte_size as i64,
                    payload.source_app,
                    payload.source_bundle_id,
                    now,
                    payload.sensitive_label,
                    color_str,
                    payload.is_url as i64,
                ],
            )
            .map_err(|e| format!("写入新剪贴板记录失败: {e}"))?;

        let new_id = self.conn.last_insert_rowid();
        self.enforce_history_limit(max_history)?;
        Ok(new_id)
    }

    pub fn enforce_history_limit(&self, max_history: usize) -> Result<(), String> {
        let limit = max_history.max(1) as i64;
        let mut stmt = self
            .conn
            .prepare(
                "SELECT id, image_path, thumb_path
                 FROM items
                 WHERE pinned = 0 AND collection_id IS NULL
                 ORDER BY last_copied_at DESC
                 LIMIT -1 OFFSET ?1",
            )
            .map_err(|e| format!("查询超限历史失败: {e}"))?;

        let rows = stmt
            .query_map(params![limit], |row| {
                Ok((
                    row.get::<_, i64>(0)?,
                    row.get::<_, Option<String>>(1)?,
                    row.get::<_, Option<String>>(2)?,
                ))
            })
            .map_err(|e| format!("读取超限历史失败: {e}"))?;

        let mut to_delete = Vec::new();
        for tuple in rows.flatten() {
            to_delete.push(tuple);
        }
        drop(stmt);

        for (id, img, thumb) in to_delete {
            self.conn
                .execute("DELETE FROM items WHERE id = ?1", params![id])
                .map_err(|e| format!("清理超限历史失败: {e}"))?;
            remove_file_if_exists(img.as_deref());
            remove_file_if_exists(thumb.as_deref());
        }
        Ok(())
    }

    pub fn list_items(&self) -> Result<Vec<ClipItem>, String> {
        let mut stmt = self
            .conn
            .prepare(
                "SELECT id, content_hash, kind, text_content, html_content, rtf_content,
                        image_path, thumb_path, image_width, image_height,
                        files_json, byte_size, source_app, source_bundle_id,
                        pinned, collection_id, first_copied_at, last_copied_at,
                        copy_count, sensitive_label, detected_color, is_url
                 FROM items
                 ORDER BY last_copied_at DESC",
            )
            .map_err(|e| format!("查询记录失败: {e}"))?;

        let rows = stmt
            .query_map([], |row| {
                let kind_str: String = row.get(2)?;
                let files_json: String = row.get(10)?;
                let files: Vec<FileEntry> =
                    serde_json::from_str(&files_json).unwrap_or_default();
                let color_str: Option<String> = row.get(20)?;
                let detected_color = color_str.as_deref().and_then(parse_rgba_csv);

                Ok(ClipItem {
                    id: row.get(0)?,
                    content_hash: row.get(1)?,
                    kind: ClipKind::from_db_str(&kind_str),
                    text_content: row.get(3)?,
                    html_content: row.get(4)?,
                    rtf_content: row.get(5)?,
                    image_path: row.get(6)?,
                    thumb_path: row.get(7)?,
                    image_width: row.get::<_, Option<i64>>(8)?.map(|v| v as u32),
                    image_height: row.get::<_, Option<i64>>(9)?.map(|v| v as u32),
                    files,
                    byte_size: row.get::<_, i64>(11)?.max(0) as u64,
                    source_app: row.get(12)?,
                    source_bundle_id: row.get(13)?,
                    pinned: row.get::<_, i64>(14)? != 0,
                    collection_id: row.get(15)?,
                    first_copied_at: row.get(16)?,
                    last_copied_at: row.get(17)?,
                    copy_count: row.get::<_, i64>(18)?.max(1) as u64,
                    sensitive_label: row.get(19)?,
                    detected_color,
                    is_url: row.get::<_, i64>(21)? != 0,
                })
            })
            .map_err(|e| format!("读取记录失败: {e}"))?;

        let mut items = Vec::new();
        for item in rows {
            items.push(item.map_err(|e| format!("解析记录行失败: {e}"))?);
        }
        Ok(items)
    }

    pub fn list_collections(&self) -> Result<Vec<Collection>, String> {
        let mut stmt = self
            .conn
            .prepare("SELECT id, name, color_hex, created_at FROM collections ORDER BY id ASC")
            .map_err(|e| format!("查询收藏集失败: {e}"))?;

        let rows = stmt
            .query_map([], |row| {
                Ok(Collection {
                    id: row.get(0)?,
                    name: row.get(1)?,
                    color_hex: row.get(2)?,
                    created_at: row.get(3)?,
                })
            })
            .map_err(|e| format!("读取收藏集失败: {e}"))?;

        let mut out = Vec::new();
        for col in rows {
            out.push(col.map_err(|e| format!("解析收藏集失败: {e}"))?);
        }
        Ok(out)
    }

    pub fn create_collection(&self, name: &str, color_hex: &str) -> Result<i64, String> {
        let trimmed = name.trim();
        if trimmed.is_empty() {
            return Err("收藏分组名称不能为空".to_string());
        }
        let now = Utc::now().timestamp_millis();
        self.conn
            .execute(
                "INSERT INTO collections (name, color_hex, created_at) VALUES (?1, ?2, ?3)",
                params![trimmed, color_hex, now],
            )
            .map_err(|e| format!("创建收藏分组失败（可能名称已存在）: {e}"))?;
        Ok(self.conn.last_insert_rowid())
    }

    pub fn rename_collection(&self, id: i64, new_name: &str) -> Result<(), String> {
        let trimmed = new_name.trim();
        if trimmed.is_empty() {
            return Err("名称不能为空".to_string());
        }
        self.conn
            .execute(
                "UPDATE collections SET name = ?1 WHERE id = ?2",
                params![trimmed, id],
            )
            .map_err(|e| format!("重命名失败: {e}"))?;
        Ok(())
    }

    pub fn delete_collection(&self, id: i64) -> Result<(), String> {
        self.conn
            .execute("DELETE FROM collections WHERE id = ?1", params![id])
            .map_err(|e| format!("删除收藏分组失败: {e}"))?;
        Ok(())
    }

    pub fn set_pinned(&self, id: i64, pinned: bool) -> Result<(), String> {
        self.conn
            .execute(
                "UPDATE items SET pinned = ?1 WHERE id = ?2",
                params![pinned as i64, id],
            )
            .map_err(|e| format!("更新固定状态失败: {e}"))?;
        Ok(())
    }

    pub fn set_collection(&self, id: i64, collection_id: Option<i64>) -> Result<(), String> {
        self.conn
            .execute(
                "UPDATE items SET collection_id = ?1 WHERE id = ?2",
                params![collection_id, id],
            )
            .map_err(|e| format!("更新收藏分组失败: {e}"))?;
        Ok(())
    }

    pub fn mark_copied(&self, id: i64) -> Result<(), String> {
        let now = Utc::now().timestamp_millis();
        self.conn
            .execute(
                "UPDATE items SET last_copied_at = ?1, copy_count = copy_count + 1 WHERE id = ?2",
                params![now, id],
            )
            .map_err(|e| format!("更新复制时间失败: {e}"))?;
        Ok(())
    }

    pub fn delete_item(&self, id: i64) -> Result<(), String> {
        let paths: Option<(Option<String>, Option<String>)> = self
            .conn
            .query_row(
                "SELECT image_path, thumb_path FROM items WHERE id = ?1",
                params![id],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()
            .map_err(|e| format!("查询删除项失败: {e}"))?;

        self.conn
            .execute("DELETE FROM items WHERE id = ?1", params![id])
            .map_err(|e| format!("删除记录失败: {e}"))?;

        if let Some((img, thumb)) = paths {
            remove_file_if_exists(img.as_deref());
            remove_file_if_exists(thumb.as_deref());
        }
        Ok(())
    }

    pub fn clear_history(&self, include_pinned_and_collections: bool) -> Result<(), String> {
        let sql_select = if include_pinned_and_collections {
            "SELECT image_path, thumb_path FROM items"
        } else {
            "SELECT image_path, thumb_path FROM items WHERE pinned = 0 AND collection_id IS NULL"
        };

        let mut stmt = self
            .conn
            .prepare(sql_select)
            .map_err(|e| format!("查询清理图片失败: {e}"))?;
        let rows = stmt
            .query_map([], |r| Ok((r.get::<_, Option<String>>(0)?, r.get::<_, Option<String>>(1)?)))
            .map_err(|e| format!("读取清理图片失败: {e}"))?;
        let mut to_remove = Vec::new();
        for pair in rows.flatten() {
            to_remove.push(pair);
        }
        drop(stmt);

        if include_pinned_and_collections {
            self.conn
                .execute("DELETE FROM items", [])
                .map_err(|e| format!("清空所有历史失败: {e}"))?;
        } else {
            self.conn
                .execute(
                    "DELETE FROM items WHERE pinned = 0 AND collection_id IS NULL",
                    [],
                )
                .map_err(|e| format!("清空历史失败: {e}"))?;
        }

        for (img, thumb) in to_remove {
            remove_file_if_exists(img.as_deref());
            remove_file_if_exists(thumb.as_deref());
        }
        Ok(())
    }

    pub fn total_storage_bytes(&self) -> u64 {
        let mut total = 0u64;
        for name in ["earth.db", "earth.db-wal", "earth.db-shm", "settings.json"] {
            if let Ok(meta) = fs::metadata(self.base_dir.join(name)) {
                total += meta.len();
            }
        }
        total += dir_size(&self.images_dir);
        total += dir_size(&self.thumbs_dir);
        total
    }
}

pub fn sha256_hex(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    let digest = hasher.finalize();
    let mut s = String::with_capacity(digest.len() * 2);
    for b in digest {
        use std::fmt::Write as _;
        let _ = write!(&mut s, "{b:02x}");
    }
    s
}

fn parse_rgba_csv(s: &str) -> Option<[u8; 4]> {
    let parts: Vec<u8> = s
        .split(',')
        .filter_map(|p| p.trim().parse::<u8>().ok())
        .collect();
    if parts.len() == 4 {
        Some([parts[0], parts[1], parts[2], parts[3]])
    } else {
        None
    }
}

fn remove_file_if_exists(path: Option<&str>) {
    if let Some(p) = path {
        let _ = fs::remove_file(p);
    }
}

fn dir_size(path: &Path) -> u64 {
    let Ok(entries) = fs::read_dir(path) else {
        return 0;
    };
    let mut sum = 0u64;
    for entry in entries.flatten() {
        if let Ok(meta) = entry.metadata()
            && meta.is_file() {
                sum += meta.len();
            }
    }
    sum
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(name: &str) -> PathBuf {
        let mut p = std::env::temp_dir();
        p.push(format!("earth_test_{}_{}", name, Utc::now().timestamp_nanos_opt().unwrap_or(0)));
        p
    }

    #[test]
    fn upsert_dedup_and_history_pruning_preserves_pinned() {
        let dir = temp_dir("prune");
        let storage = Storage::open_in_dir(&dir).unwrap();

        let id1 = storage
            .upsert_item(
                NewClipPayload {
                    content_hash: sha256_hex(b"pinned-text"),
                    kind: ClipKind::Text,
                    text_content: Some("pinned-text".into()),
                    html_content: None,
                    rtf_content: None,
                    image_png: None,
                    thumb_png: None,
                    image_width: None,
                    image_height: None,
                    files: vec![],
                    byte_size: 11,
                    source_app: Some("Notes".into()),
                    source_bundle_id: None,
                    sensitive_label: None,
                    detected_color: None,
                    is_url: false,
                },
                2,
            )
            .unwrap();
        storage.set_pinned(id1, true).unwrap();

        for i in 0..4 {
            let text = format!("item-{i}");
            storage
                .upsert_item(
                    NewClipPayload {
                        content_hash: sha256_hex(text.as_bytes()),
                        kind: ClipKind::Text,
                        text_content: Some(text),
                        html_content: None,
                        rtf_content: None,
                        image_png: None,
                        thumb_png: None,
                        image_width: None,
                        image_height: None,
                        files: vec![],
                        byte_size: 6,
                        source_app: None,
                        source_bundle_id: None,
                        sensitive_label: None,
                        detected_color: None,
                        is_url: false,
                    },
                    2,
                )
                .unwrap();
            std::thread::sleep(std::time::Duration::from_millis(2));
        }

        // Duplicate copy of item-3 increments copy_count instead of inserting
        storage
            .upsert_item(
                NewClipPayload {
                    content_hash: sha256_hex(b"item-3"),
                    kind: ClipKind::Text,
                    text_content: Some("item-3".into()),
                    html_content: None,
                    rtf_content: None,
                    image_png: None,
                    thumb_png: None,
                    image_width: None,
                    image_height: None,
                    files: vec![],
                    byte_size: 6,
                    source_app: None,
                    source_bundle_id: None,
                    sensitive_label: None,
                    detected_color: None,
                    is_url: false,
                },
                2,
            )
            .unwrap();

        let items = storage.list_items().unwrap();
        // 1 pinned + max 2 unpinned = 3 items
        assert_eq!(items.len(), 3);
        assert!(items.iter().any(|it| it.id == id1 && it.pinned));
        let item3 = items.iter().find(|it| it.text_content.as_deref() == Some("item-3")).unwrap();
        assert_eq!(item3.copy_count, 2);

        let _ = fs::remove_dir_all(&dir);
    }
}
