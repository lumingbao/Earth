use crate::i18n::{t, tf};
use crate::model::{AppSettings, ClipItem, ClipKind, Collection, FileEntry};
use crate::transforms::{detect_color, detect_sensitive_label, detect_url};
use chrono::Utc;
use directories::ProjectDirs;
use rusqlite::{Connection, OptionalExtension, Transaction, TransactionBehavior, params};
use sha2::{Digest, Sha256};
use std::fs;
use std::path::{Path, PathBuf};

mod backup;
mod files;
pub mod history;
#[cfg(test)]
mod regression_tests;

pub use backup::ImportSummary;

use files::{
    PendingMedia, atomic_private_write, ensure_private_file, managed_media_path, private_dir,
    private_file,
};

const SCHEMA_VERSION: i64 = 1;
type MediaPaths = (Option<String>, Option<String>);

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
    pub fn open_primary() -> Result<Option<(Self, crate::instance::InstanceGuard)>, String> {
        let proj =
            ProjectDirs::from("io", "earth", "Earth").ok_or_else(|| t("storage.err_data_dir"))?;
        let dir = proj.data_dir();
        let open = || -> Result<_, String> {
            private_dir(dir).map_err(|e| e.to_string())?;
            let path = dir.join("instance.lock");
            ensure_private_file(&path).map_err(|e| e.to_string())?;
            let lock = std::fs::OpenOptions::new()
                .read(true)
                .write(true)
                .open(path)
                .map_err(|e| e.to_string())?;
            let Some(instance) = crate::instance::InstanceGuard::claim(dir, lock)? else {
                return Ok(None);
            };
            Ok(Some((Self::open_in_dir(dir)?, instance)))
        };
        open().map_err(|e| {
            let dir_str = dir.display().to_string();
            tf(
                "storage.err_open_primary_hint",
                &[("error", &e), ("dir", &dir_str)],
            )
        })
    }

    pub fn base_dir(&self) -> &Path {
        &self.base_dir
    }

    /// Opens a secondary connection to an already-initialized data directory (e.g. for
    /// background backup export/import) without repeating startup permission sweeps or GC.
    pub fn open_existing_in_dir(base_dir: &Path) -> Result<Self, String> {
        let base_dir = base_dir
            .canonicalize()
            .map_err(|e| format!("解析数据目录失败: {e}"))?;
        let images_dir = base_dir.join("images");
        let thumbs_dir = base_dir.join("thumbs");
        let db_path = base_dir.join("earth.db");
        let conn = Connection::open(&db_path).map_err(|e| format!("打开数据库失败: {e}"))?;
        conn.busy_timeout(std::time::Duration::from_secs(5))
            .map_err(|e| format!("配置数据库等待时间失败: {e}"))?;
        conn.execute_batch(
            "PRAGMA journal_mode = WAL;
             PRAGMA synchronous = NORMAL;
             PRAGMA foreign_keys = ON;",
        )
        .map_err(|e| format!("配置 SQLite PRAGMA 失败: {e}"))?;
        Ok(Self {
            base_dir,
            images_dir,
            thumbs_dir,
            conn,
        })
    }

    pub fn open_in_dir(base_dir: &Path) -> Result<Self, String> {
        private_dir(base_dir).map_err(|e| format!("保护数据目录失败: {e}"))?;
        let base_dir = base_dir
            .canonicalize()
            .map_err(|e| format!("解析数据目录失败: {e}"))?;
        let images_dir = base_dir.join("images");
        let thumbs_dir = base_dir.join("thumbs");
        private_dir(&images_dir).map_err(|e| format!("保护图片目录失败: {e}"))?;
        private_dir(&thumbs_dir).map_err(|e| format!("保护缩略图目录失败: {e}"))?;

        let db_path = base_dir.join("earth.db");
        ensure_private_file(&db_path).map_err(|e| format!("保护数据库文件失败: {e}"))?;
        for name in [
            "earth.db-wal",
            "earth.db-shm",
            "earth.db-journal",
            "settings.json",
            "settings.json.bak",
        ] {
            let path = base_dir.join(name);
            if fs::symlink_metadata(&path).is_ok() {
                private_file(&path).map_err(|e| format!("保护本地数据文件失败: {e}"))?;
            }
        }
        let conn = Connection::open(&db_path).map_err(|e| format!("打开数据库失败: {e}"))?;
        conn.busy_timeout(std::time::Duration::from_secs(5))
            .map_err(|e| format!("配置数据库等待时间失败: {e}"))?;
        conn.execute_batch(
            "PRAGMA journal_mode = WAL;
             PRAGMA synchronous = NORMAL;
             PRAGMA foreign_keys = ON;",
        )
        .map_err(|e| format!("配置 SQLite PRAGMA 失败: {e}"))?;

        let storage = Self {
            base_dir,
            images_dir,
            thumbs_dir,
            conn,
        };
        storage.init_schema()?;
        storage.protect_existing_files()?;
        storage.cleanup_orphaned_media()?;
        Ok(storage)
    }

    fn init_schema(&self) -> Result<(), String> {
        let version: i64 = self
            .conn
            .pragma_query_value(None, "user_version", |r| r.get(0))
            .map_err(|e| format!("读取数据库版本失败: {e}"))?;
        if version > SCHEMA_VERSION {
            return Err(tf(
                "storage.err_future_schema",
                &[
                    ("version", &version.to_string()),
                    ("supported", &SCHEMA_VERSION.to_string()),
                ],
            ));
        }
        if version == SCHEMA_VERSION {
            // Fail visibly if a supposedly migrated database is incomplete.
            self.conn
                .prepare("SELECT note FROM items LIMIT 0")
                .map_err(|e| format!("数据库结构校验失败: {e}"))?;
            return Ok(());
        }
        let existing: bool = self.conn.query_row(
            "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = 'items')",
            [], |r| r.get(0),
        ).map_err(|e| format!("检查已有数据库失败: {e}"))?;
        if existing {
            let backup = self.base_dir.join(format!(
                "earth.db.before-v{SCHEMA_VERSION}-{}.bak",
                Utc::now().timestamp_nanos_opt().unwrap_or(0)
            ));
            ensure_private_file(&backup).map_err(|e| format!("创建迁移备份失败: {e}"))?;
            self.conn
                .execute("VACUUM main INTO ?1", params![backup.to_string_lossy()])
                .map_err(|e| format!("备份数据库失败，未执行迁移: {e}"))?;
            private_file(&backup).map_err(|e| format!("保护迁移备份失败: {e}"))?;
        }
        let tx = self.begin_write()?;
        tx.execute_batch(
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
                    is_url INTEGER NOT NULL DEFAULT 0,
                    note TEXT
                );
                CREATE INDEX IF NOT EXISTS idx_items_last_copied ON items(last_copied_at DESC);
                CREATE INDEX IF NOT EXISTS idx_items_pinned ON items(pinned);",
        )
        .map_err(|e| format!("初始化数据库结构失败: {e}"))?;

        let has_note = {
            let mut columns = tx
                .prepare("PRAGMA table_info(items)")
                .map_err(|e| format!("检查数据库字段失败: {e}"))?;
            let names = columns
                .query_map([], |row| row.get::<_, String>(1))
                .map_err(|e| format!("读取数据库字段失败: {e}"))?
                .collect::<Result<Vec<_>, _>>()
                .map_err(|e| format!("读取数据库字段失败: {e}"))?;
            names.iter().any(|name| name == "note")
        };
        if !has_note {
            tx.execute("ALTER TABLE items ADD COLUMN note TEXT", [])
                .map_err(|e| format!("迁移备注字段失败，原数据库未修改: {e}"))?;
        }
        // Only a genuinely new database receives starter collections. An empty
        // legacy table is a valid user choice and must remain empty.
        if !existing {
            let now = Utc::now().timestamp_millis();
            tx.execute(
                "INSERT OR IGNORE INTO collections (name, color_hex, created_at) VALUES (?1, ?2, ?3)",
                params![t("storage.default_col_commands"), "#456EFD", now],
            ).map_err(|e| format!("创建初始收藏分组失败: {e}"))?;
            tx.execute(
                "INSERT OR IGNORE INTO collections (name, color_hex, created_at) VALUES (?1, ?2, ?3)",
                params![t("storage.default_col_snippets"), "#22C55E", now],
            ).map_err(|e| format!("创建初始收藏分组失败: {e}"))?;
        }
        tx.pragma_update(None, "user_version", SCHEMA_VERSION)
            .map_err(|e| format!("更新数据库版本失败: {e}"))?;
        tx.commit().map_err(|e| format!("提交数据库迁移失败: {e}"))
    }

    pub fn settings_path(&self) -> PathBuf {
        self.base_dir.join("settings.json")
    }

    pub fn load_settings_with_warning(&self) -> (AppSettings, Option<String>) {
        let path = self.settings_path();
        match fs::read(&path) {
            Ok(raw) => {
                if let Ok(parsed) = serde_json::from_slice::<AppSettings>(&raw) {
                    return (parsed, None);
                }
                let corrupt = self.base_dir.join(format!(
                    "settings.json.corrupt-{}",
                    Utc::now().timestamp_nanos_opt().unwrap_or(0)
                ));
                if let Err(err) = atomic_private_write(&corrupt, &raw) {
                    let defaults = AppSettings {
                        paused: true,
                        ..Default::default()
                    };
                    return (
                        defaults,
                        Some(tf(
                            "storage.warn_settings_corrupt_no_copy",
                            &[("error", &err.to_string())],
                        )),
                    );
                }
            }
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
                return (AppSettings::default(), None);
            }
            Err(err) => {
                let defaults = AppSettings {
                    paused: true,
                    ..Default::default()
                };
                return (
                    defaults,
                    Some(tf(
                        "storage.warn_settings_unreadable",
                        &[("error", &err.to_string())],
                    )),
                );
            }
        }
        let backup = self.base_dir.join("settings.json.bak");
        if let Ok(raw) = fs::read(&backup)
            && let Ok(settings) = serde_json::from_slice::<AppSettings>(&raw)
        {
            let restored = atomic_private_write(&path, &raw);
            return (
                settings,
                Some(if restored.is_ok() {
                    t("storage.warn_settings_restored").to_string()
                } else {
                    t("storage.warn_settings_loaded_bak").to_string()
                }),
            );
        }
        let defaults = AppSettings {
            paused: true,
            ..Default::default()
        };
        (
            defaults,
            Some(t("storage.warn_settings_reset_paused").to_string()),
        )
    }

    pub fn save_settings(&self, settings: &AppSettings) -> Result<(), String> {
        let json =
            serde_json::to_string_pretty(settings).map_err(|e| format!("序列化设置失败: {e}"))?;
        if let Ok(previous) = fs::read(self.settings_path())
            && serde_json::from_slice::<AppSettings>(&previous).is_ok()
        {
            atomic_private_write(&self.base_dir.join("settings.json.bak"), &previous)
                .map_err(|e| format!("备份设置失败: {e}"))?;
        }
        atomic_private_write(&self.settings_path(), json.as_bytes())
            .map_err(|e| format!("写入设置失败: {e}"))
    }

    pub fn upsert_item(&self, payload: NewClipPayload, max_history: usize) -> Result<i64, String> {
        let now = Utc::now().timestamp_millis();
        let tx = self.begin_write()?;

        let existing_id: Option<i64> = tx
            .query_row(
                "SELECT id FROM items WHERE content_hash = ?1",
                params![&payload.content_hash],
                |row| row.get(0),
            )
            .optional()
            .map_err(|e| format!("查询重复记录失败: {e}"))?;

        if let Some(id) = existing_id {
            tx.execute(
                "UPDATE items
                     SET last_copied_at = ?1,
                         copy_count = copy_count + 1,
                         source_app = COALESCE(?2, source_app),
                         source_bundle_id = COALESCE(?3, source_bundle_id),
                         html_content = ?4,
                         rtf_content = ?5,
                         sensitive_label = CASE WHEN kind = 'text' THEN ?6 ELSE sensitive_label END
                     WHERE id = ?7",
                params![
                    now,
                    payload.source_app,
                    payload.source_bundle_id,
                    payload.html_content,
                    payload.rtf_content,
                    detect_sensitive_label(payload.text_content.as_deref().unwrap_or("")),
                    id
                ],
            )
            .map_err(|e| format!("更新剪贴板记录失败: {e}"))?;
            tx.commit()
                .map_err(|e| format!("提交剪贴板记录失败: {e}"))?;
            return Ok(id);
        }

        let mut pending_media = PendingMedia::default();
        let (image_path, thumb_path) = if payload.kind == ClipKind::Image {
            (
                payload
                    .image_png
                    .as_deref()
                    .map(|bytes| pending_media.write(&self.images_dir, bytes, ""))
                    .transpose()?,
                payload
                    .thumb_png
                    .as_deref()
                    .map(|bytes| pending_media.write(&self.thumbs_dir, bytes, "_thumb"))
                    .transpose()?,
            )
        } else {
            (None, None)
        };

        let files_json = serde_json::to_string(&payload.files).unwrap_or_else(|_| "[]".to_string());
        let color_str = payload
            .detected_color
            .map(|c| format!("{},{},{},{}", c[0], c[1], c[2], c[3]));

        tx.execute(
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

        let new_id = tx.last_insert_rowid();
        let removed = Self::prune_rows_protecting(&tx, max_history, new_id)?;
        tx.commit()
            .map_err(|e| format!("提交剪贴板记录失败: {e}"))?;
        pending_media.commit();
        self.remove_unreferenced_media(&removed)?;
        Ok(new_id)
    }

    pub fn enforce_history_limit(&self, max_history: usize) -> Result<(), String> {
        let tx = self.begin_write()?;
        let removed = Self::prune_rows(&tx, max_history)?;
        tx.commit().map_err(|e| format!("提交历史清理失败: {e}"))?;
        self.remove_unreferenced_media(&removed)
    }

    fn prune_rows(conn: &Connection, max_history: usize) -> Result<Vec<MediaPaths>, String> {
        Self::prune_rows_protecting(conn, max_history, 0)
    }

    fn prune_rows_protecting(
        conn: &Connection,
        max_history: usize,
        protected_id: i64,
    ) -> Result<Vec<MediaPaths>, String> {
        let limit = max_history.max(1) as i64;
        let mut stmt = conn
            .prepare(
                "SELECT id, image_path, thumb_path
                 FROM items
                 WHERE pinned = 0 AND collection_id IS NULL
                 ORDER BY (id = ?2) DESC, last_copied_at DESC, id DESC
                 LIMIT -1 OFFSET ?1",
            )
            .map_err(|e| format!("查询超限历史失败: {e}"))?;

        let rows = stmt
            .query_map(params![limit, protected_id], |row| {
                Ok((
                    row.get::<_, i64>(0)?,
                    row.get::<_, Option<String>>(1)?,
                    row.get::<_, Option<String>>(2)?,
                ))
            })
            .map_err(|e| format!("读取超限历史失败: {e}"))?;

        let mut to_delete = Vec::new();
        for tuple in rows {
            to_delete.push(tuple.map_err(|e| format!("读取超限历史失败: {e}"))?);
        }
        drop(stmt);

        let mut removed = Vec::new();
        for (id, img, thumb) in to_delete {
            conn.execute("DELETE FROM items WHERE id = ?1", params![id])
                .map_err(|e| format!("清理超限历史失败: {e}"))?;
            removed.push((img, thumb));
        }
        Ok(removed)
    }

    #[cfg(test)]
    pub fn list_items(&self) -> Result<Vec<ClipItem>, String> {
        self.query_items_inner(false, "ORDER BY last_copied_at DESC, id DESC", [])
    }

    pub fn item(&self, id: i64) -> Result<ClipItem, String> {
        self.query_items_inner(false, "WHERE id = ?", [id])?
            .pop()
            .ok_or_else(|| t("storage.err_item_missing").into())
    }

    pub(super) fn query_items_lightweight<const N: usize>(
        &self,
        clause: &str,
        args: [i64; N],
    ) -> Result<Vec<ClipItem>, String> {
        self.query_items_inner(true, clause, args)
    }

    pub fn with_full_text_lookup<R>(
        &self,
        f: impl FnOnce(&mut dyn FnMut(i64) -> Option<String>) -> R,
    ) -> R {
        let mut stmt = self
            .conn
            .prepare("SELECT text_content FROM items WHERE id = ?1")
            .ok();
        let mut lookup = |id: i64| -> Option<String> {
            stmt.as_mut()?
                .query_row([id], |r| r.get::<_, Option<String>>(0))
                .ok()
                .flatten()
        };
        f(&mut lookup)
    }

    fn query_items_inner<const N: usize>(
        &self,
        lightweight: bool,
        clause: &str,
        args: [i64; N],
    ) -> Result<Vec<ClipItem>, String> {
        let (text_expr, rich_expr, trunc_expr) = if lightweight {
            (
                "CASE WHEN length(text_content) > 512 THEN substr(text_content, 1, 256) || '…' || substr(text_content, -32) ELSE text_content END",
                "NULL, NULL",
                "CASE WHEN length(text_content) > 512 THEN 1 ELSE 0 END",
            )
        } else {
            ("text_content", "html_content, rtf_content", "0")
        };
        let sql = format!(
            "SELECT id, content_hash, kind, {text_expr}, {rich_expr}, image_path, thumb_path, image_width, image_height, files_json, byte_size, source_app, source_bundle_id, pinned, collection_id, first_copied_at, last_copied_at, copy_count, sensitive_label, detected_color, is_url, note, {trunc_expr}, (html_content IS NOT NULL OR rtf_content IS NOT NULL) FROM items {clause}"
        );
        let mut stmt = self
            .conn
            .prepare(&sql)
            .map_err(|e| format!("查询记录失败: {e}"))?;

        let rows = stmt
            .query_map(rusqlite::params_from_iter(args), |row| {
                let kind_str: String = row.get(2)?;
                let kind = ClipKind::from_db_str(&kind_str);
                let text_content: Option<String> = row.get(3)?;
                let files_json: String = row.get(10)?;
                let files: Vec<FileEntry> = serde_json::from_str(&files_json).unwrap_or_default();
                let color_str: Option<String> = row.get(20)?;
                let text_truncated = row.get::<_, i64>(23)? != 0;
                let has_rich_text = row.get::<_, i64>(24)? != 0;
                let detected_color = color_str.as_deref().and_then(parse_rgba_csv).or_else(|| {
                    (kind == ClipKind::Text && !text_truncated)
                        .then(|| text_content.as_deref().and_then(detect_color))
                        .flatten()
                });

                Ok(ClipItem {
                    id: row.get(0)?,
                    content_hash: row.get(1)?,
                    kind,
                    text_content,
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
                    note: row.get(22)?,
                    text_truncated,
                    has_rich_text,
                })
            })
            .map_err(|e| format!("读取记录失败: {e}"))?;

        let mut items = Vec::new();
        for item in rows {
            let mut item = item.map_err(|e| format!("解析记录行失败: {e}"))?;
            item.image_path = item
                .image_path
                .as_deref()
                .and_then(|p| managed_media_path(&self.images_dir, p))
                .map(|p| p.to_string_lossy().into_owned());
            item.thumb_path = item
                .thumb_path
                .as_deref()
                .and_then(|p| managed_media_path(&self.thumbs_dir, p))
                .map(|p| p.to_string_lossy().into_owned());
            items.push(item);
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
            return Err(t("storage.err_collection_empty").to_string());
        }
        let now = Utc::now().timestamp_millis();
        self.conn
            .execute(
                "INSERT INTO collections (name, color_hex, created_at) VALUES (?1, ?2, ?3)",
                params![trimmed, color_hex, now],
            )
            .map_err(|e| {
                tf(
                    "storage.err_collection_create",
                    &[("error", &e.to_string())],
                )
            })?;
        Ok(self.conn.last_insert_rowid())
    }

    pub fn rename_collection(&self, id: i64, new_name: &str) -> Result<(), String> {
        let trimmed = new_name.trim();
        if trimmed.is_empty() {
            return Err(t("storage.err_collection_empty").to_string());
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

    pub fn set_item_note(&self, id: i64, note: Option<&str>) -> Result<(), String> {
        let cleaned = note.map(str::trim).filter(|s| !s.is_empty());
        self.conn
            .execute(
                "UPDATE items SET note = ?1 WHERE id = ?2",
                params![cleaned, id],
            )
            .map_err(|e| format!("更新记录备注失败: {e}"))?;
        Ok(())
    }

    pub fn update_item_text(&self, id: i64, new_text: &str) -> Result<(), String> {
        if new_text.trim().is_empty() {
            return Err(t("storage.err_text_empty").to_string());
        }
        let tx = self.begin_write()?;
        let kind: Option<String> = tx
            .query_row("SELECT kind FROM items WHERE id = ?1", [id], |r| r.get(0))
            .optional()
            .map_err(|e| format!("读取待编辑记录失败: {e}"))?;
        if kind.as_deref() != Some("text") {
            return Err(t("storage.err_edit_text_only").to_string());
        }
        let new_hash = content_hash_for_text(new_text);
        let byte_size = new_text.len() as i64;
        let sensitive_label = detect_sensitive_label(new_text);
        let detected_color = detect_color(new_text);
        let color_str = detected_color.map(|c| format!("{},{},{},{}", c[0], c[1], c[2], c[3]));
        let is_url = detect_url(new_text) as i64;

        // A collision is not permission to delete or merge someone else's
        // annotations, pin state or collection membership.
        let collision: bool = tx
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM items WHERE id != ?1
             AND (content_hash = ?2 OR (kind = 'text' AND text_content = ?3)))",
                params![id, &new_hash, new_text],
                |r| r.get(0),
            )
            .map_err(|e| format!("检查重复文本失败: {e}"))?;
        if collision {
            return Err(t("storage.err_edit_collision").to_string());
        }

        tx.execute(
            "UPDATE items
                 SET content_hash = ?1,
                     text_content = ?2,
                     html_content = NULL,
                     rtf_content = NULL,
                     byte_size = ?3,
                     sensitive_label = ?4,
                     detected_color = ?5,
                     is_url = ?6
                 WHERE id = ?7",
            params![
                new_hash,
                new_text,
                byte_size,
                sensitive_label,
                color_str,
                is_url,
                id
            ],
        )
        .map_err(|e| format!("保存修改后的文本失败: {e}"))?;
        tx.commit().map_err(|e| format!("提交文本修改失败: {e}"))
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
        let tx = self.begin_write()?;
        let paths: Option<(Option<String>, Option<String>)> = tx
            .query_row(
                "SELECT image_path, thumb_path FROM items WHERE id = ?1",
                params![id],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()
            .map_err(|e| format!("查询删除项失败: {e}"))?;

        tx.execute("DELETE FROM items WHERE id = ?1", params![id])
            .map_err(|e| format!("删除记录失败: {e}"))?;
        tx.commit().map_err(|e| format!("提交记录删除失败: {e}"))?;
        if let Some((img, thumb)) = paths {
            self.remove_unreferenced_media(&[(img, thumb)])?;
        }
        Ok(())
    }

    pub fn clear_history(&self, include_pinned_and_collections: bool) -> Result<(), String> {
        let tx = self.begin_write()?;
        let sql_select = if include_pinned_and_collections {
            "SELECT image_path, thumb_path FROM items"
        } else {
            "SELECT image_path, thumb_path FROM items WHERE pinned = 0 AND collection_id IS NULL"
        };

        let mut stmt = tx
            .prepare(sql_select)
            .map_err(|e| format!("查询清理图片失败: {e}"))?;
        let rows = stmt
            .query_map([], |r| {
                Ok((
                    r.get::<_, Option<String>>(0)?,
                    r.get::<_, Option<String>>(1)?,
                ))
            })
            .map_err(|e| format!("读取清理图片失败: {e}"))?;
        let mut to_remove = Vec::new();
        for pair in rows {
            to_remove.push(pair.map_err(|e| format!("读取清理图片失败: {e}"))?);
        }
        drop(stmt);

        if include_pinned_and_collections {
            tx.execute("DELETE FROM items", [])
                .map_err(|e| format!("清空所有历史失败: {e}"))?;
        } else {
            tx.execute(
                "DELETE FROM items WHERE pinned = 0 AND collection_id IS NULL",
                [],
            )
            .map_err(|e| format!("清空历史失败: {e}"))?;
        }

        tx.commit().map_err(|e| format!("提交历史清理失败: {e}"))?;
        self.remove_unreferenced_media(&to_remove)
    }

    fn begin_write(&self) -> Result<Transaction<'_>, String> {
        Transaction::new_unchecked(&self.conn, TransactionBehavior::Immediate)
            .map_err(|e| format!("开始数据库事务失败: {e}"))
    }

    fn protect_existing_files(&self) -> Result<(), String> {
        for entry in fs::read_dir(&self.base_dir).map_err(|e| format!("检查数据权限失败: {e}"))?
        {
            let entry = entry.map_err(|e| format!("检查数据权限失败: {e}"))?;
            let name = entry.file_name();
            let name = name.to_string_lossy();
            if name.starts_with("earth.db") || name.starts_with("settings.json") {
                private_file(&entry.path()).map_err(|e| format!("保护已有数据失败: {e}"))?;
            }
        }
        for dir in [&self.images_dir, &self.thumbs_dir] {
            for entry in fs::read_dir(dir).map_err(|e| format!("检查图片权限失败: {e}"))? {
                let entry = entry.map_err(|e| format!("检查图片权限失败: {e}"))?;
                if let Some(path) = managed_media_path(dir, &entry.path().to_string_lossy()) {
                    private_file(&path).map_err(|e| format!("保护已有图片失败: {e}"))?;
                }
            }
        }
        Ok(())
    }

    fn referenced_media(&self) -> Result<std::collections::HashSet<PathBuf>, String> {
        let mut stmt = self
            .conn
            .prepare("SELECT image_path, thumb_path FROM items")
            .map_err(|e| format!("读取图片引用失败: {e}"))?;
        let rows = stmt
            .query_map([], |r| {
                Ok((
                    r.get::<_, Option<String>>(0)?,
                    r.get::<_, Option<String>>(1)?,
                ))
            })
            .map_err(|e| format!("读取图片引用失败: {e}"))?;
        let mut refs = std::collections::HashSet::new();
        for row in rows {
            let (image, thumb) = row.map_err(|e| format!("读取图片引用失败: {e}"))?;
            for (dir, path) in [(&self.images_dir, image), (&self.thumbs_dir, thumb)] {
                if let Some(path) = path.and_then(|p| managed_media_path(dir, &p)) {
                    refs.insert(path);
                }
            }
        }
        Ok(refs)
    }

    fn remove_unreferenced_media(
        &self,
        paths: &[(Option<String>, Option<String>)],
    ) -> Result<(), String> {
        if paths.iter().all(|(i, t)| i.is_none() && t.is_none()) {
            return Ok(());
        }
        // Keep the write lock through deletion so another import cannot attach
        // a path between the reference check and file cleanup.
        let tx = self.begin_write()?;
        let referenced = self.referenced_media()?;
        for (image, thumb) in paths {
            for (dir, raw) in [(&self.images_dir, image), (&self.thumbs_dir, thumb)] {
                if let Some(path) = raw.as_deref().and_then(|p| managed_media_path(dir, p))
                    && !referenced.contains(&path)
                {
                    fs::remove_file(&path).map_err(|e| format!("清理图片失败: {e}"))?;
                }
            }
        }
        tx.commit().map_err(|e| format!("完成图片清理失败: {e}"))
    }

    fn cleanup_orphaned_media(&self) -> Result<(), String> {
        let tx = self.begin_write()?;
        let referenced = self.referenced_media()?;
        for dir in [&self.images_dir, &self.thumbs_dir] {
            for entry in fs::read_dir(dir).map_err(|e| format!("检查遗留图片失败: {e}"))? {
                let entry = entry.map_err(|e| format!("检查遗留图片失败: {e}"))?;
                let path = entry.path();
                // Only Earth-generated names are eligible for startup cleanup.
                let stem = path.file_stem().and_then(|n| n.to_str()).unwrap_or("");
                let hash = stem.strip_suffix("_thumb").unwrap_or(stem);
                if !matches!(hash.len(), 16 | 64) || !hash.bytes().all(|b| b.is_ascii_hexdigit()) {
                    continue;
                }
                if let Some(path) = managed_media_path(dir, &path.to_string_lossy())
                    && !referenced.contains(&path)
                {
                    fs::remove_file(path).map_err(|e| format!("清理遗留图片失败: {e}"))?;
                }
            }
        }
        tx.commit()
            .map_err(|e| format!("完成遗留图片清理失败: {e}"))
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
    sha256_prefixed_hex(b"", bytes)
}

pub fn sha256_prefixed_hex(prefix: &[u8], body: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(prefix);
    hasher.update(body);
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

pub(crate) fn content_hash_for_text(text: &str) -> String {
    sha256_prefixed_hex(b"text:", text.as_bytes())
}

fn dir_size(path: &Path) -> u64 {
    let Ok(entries) = fs::read_dir(path) else {
        return 0;
    };
    let mut sum = 0u64;
    for entry in entries.flatten() {
        if let Ok(meta) = entry.metadata()
            && meta.is_file()
        {
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
        p.push(format!(
            "earth_test_{}_{}",
            name,
            Utc::now().timestamp_nanos_opt().unwrap_or(0)
        ));
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
        let item3 = items
            .iter()
            .find(|it| it.text_content.as_deref() == Some("item-3"))
            .unwrap();
        assert_eq!(item3.copy_count, 2);

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn note_edit_text_and_json_import_roundtrip() {
        let dir = temp_dir("features");
        let storage = Storage::open_in_dir(&dir).unwrap();

        let id = storage
            .upsert_item(
                NewClipPayload {
                    content_hash: sha256_hex(b"ssh root@10.0.0.1"),
                    kind: ClipKind::Text,
                    text_content: Some("ssh root@10.0.0.1".into()),
                    html_content: None,
                    rtf_content: None,
                    image_png: None,
                    thumb_png: None,
                    image_width: None,
                    image_height: None,
                    files: vec![],
                    byte_size: 17,
                    source_app: Some("Terminal".into()),
                    source_bundle_id: None,
                    sensitive_label: None,
                    detected_color: None,
                    is_url: false,
                },
                50,
            )
            .unwrap();

        storage.set_item_note(id, Some("生产数据库跳板机")).unwrap();
        storage.update_item_text(id, "ssh root@10.0.0.2").unwrap();

        let items = storage.list_items().unwrap();
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].text_content.as_deref(), Some("ssh root@10.0.0.2"));
        assert_eq!(items[0].note.as_deref(), Some("生产数据库跳板机"));
        assert!(items[0].searchable_text().contains("生产数据库跳板机"));

        // Export & import into a fresh storage instance
        let dir2 = temp_dir("import");
        let storage2 = Storage::open_in_dir(&dir2).unwrap();
        let count = storage2.import_items(&items, 50).unwrap();
        assert_eq!(count, 1);
        // Second import deduplicates by content_hash
        let count_dup = storage2.import_items(&items, 50).unwrap();
        assert_eq!(count_dup, 0);

        let _ = fs::remove_dir_all(&dir);
        let _ = fs::remove_dir_all(&dir2);
    }

    #[test]
    fn list_items_and_edit_text_detect_rgb_and_hex_colors() {
        let dir = temp_dir("color_backfill");
        let storage = Storage::open_in_dir(&dir).unwrap();

        // Simulate a legacy row stored before RGB detection existed (detected_color = None)
        let id = storage
            .upsert_item(
                NewClipPayload {
                    content_hash: sha256_hex(b"rgb(10, 132, 255)"),
                    kind: ClipKind::Text,
                    text_content: Some("rgb(10, 132, 255)".into()),
                    html_content: None,
                    rtf_content: None,
                    image_png: None,
                    thumb_png: None,
                    image_width: None,
                    image_height: None,
                    files: vec![],
                    byte_size: 17,
                    source_app: Some("Figma".into()),
                    source_bundle_id: None,
                    sensitive_label: None,
                    detected_color: None,
                    is_url: false,
                },
                50,
            )
            .unwrap();

        let items = storage.list_items().unwrap();
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].detected_color, Some([10, 132, 255, 255]));

        // Editing text to RGBA updates persisted color cleanly
        storage
            .update_item_text(id, "rgba(255, 128, 0, 0.5)")
            .unwrap();
        let items2 = storage.list_items().unwrap();
        assert_eq!(items2[0].detected_color, Some([255, 128, 0, 128]));

        let _ = fs::remove_dir_all(&dir);
    }
}
