use super::*;
use base64::{Engine as _, engine::general_purpose::STANDARD};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::io::{self, Cursor, Read, Write};

const MAX_BACKUP_BYTES: u64 = 128 * 1024 * 1024;
// Match the largest image accepted by capture settings so every captured image
// can make a portable round trip (subject to the total backup size limit).
const MAX_IMAGE_BYTES: usize = 64 * 1024 * 1024;
const MAX_IMAGE_PIXELS: u64 = 64_000_000;
const MAX_THUMB_BYTES: usize = 4 * 1024 * 1024;
const MAX_ITEMS: usize = 10_000;

#[derive(Debug, Default, PartialEq, Eq)]
pub struct ImportSummary {
    pub imported: usize,
    pub skipped_duplicates: usize,
    pub missing_images: usize,
    pub legacy_format: bool,
}

#[derive(Serialize, Deserialize)]
struct Backup {
    format: String,
    version: u32,
    items: Vec<BackupItem>,
    #[serde(default)]
    collections: Vec<Collection>,
}

#[derive(Serialize, Deserialize)]
struct BackupItem {
    #[serde(flatten)]
    item: ClipItem,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    image_png_base64: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    thumbnail_png_base64: Option<String>,
}

impl Storage {
    pub fn export_backup(&self, destination: &Path) -> Result<(), String> {
        self.export_backup_with_limits(destination, MAX_BACKUP_BYTES as usize, MAX_ITEMS)
    }

    fn export_backup_with_limits(
        &self,
        destination: &Path,
        byte_limit: usize,
        record_limit: usize,
    ) -> Result<(), String> {
        // The Immediate transaction also keeps media references stable against
        // writers that clean up files. Preflight happens before loading any row.
        let tx = self.begin_write()?;
        self.preflight_backup(byte_limit, record_limit)?;
        let mut output = LimitedBuffer::new(byte_limit);
        output.append(br#"{"format":"earth-clipboard","version":2,"items":["#)?;
        {
            let mut statement = self
                .conn
                .prepare("SELECT id FROM items ORDER BY last_copied_at DESC, id DESC")
                .map_err(|e| format!("查询备份记录失败: {e}"))?;
            let ids = statement
                .query_map([], |row| row.get::<_, i64>(0))
                .map_err(|e| format!("读取备份记录失败: {e}"))?;
            for (index, id) in ids.enumerate() {
                if index != 0 {
                    output.append(b",")?;
                }
                // Only one record and its media are resident alongside the bounded
                // output. Never load all text/rich formats or all base64 at once.
                let mut item = self.item(id.map_err(|e| format!("读取备份记录失败: {e}"))?)?;
                let image_path = item.image_path.take();
                let thumb_path = item.thumb_path.take();
                let mut record = BackupItem {
                    item,
                    image_png_base64: None,
                    thumbnail_png_base64: None,
                };
                if record.item.kind == ClipKind::Image {
                    let path = image_path.as_deref().ok_or_else(|| {
                        tf(
                            "backup.err_missing_image",
                            &[("id", &record.item.id.to_string())],
                        )
                    })?;
                    // Account for escaped metadata and the actual base64 field
                    // punctuation before reading/encoding a potentially large PNG.
                    let mut counter = LimitedCounter::new(output.remaining());
                    serde_json::to_writer(&mut counter, &record)
                        .map_err(|e| format!("序列化备份失败: {e}"))?;
                    let fields = b",\"image_png_base64\":\"\"".len()
                        + thumb_path
                            .as_ref()
                            .map_or(0, |_| b",\"thumbnail_png_base64\":\"\"".len());
                    let mut media_budget = counter
                        .remaining
                        .checked_sub(fields)
                        .ok_or_else(backup_too_large)?;
                    let image =
                        encode_export_media(Path::new(path), MAX_IMAGE_BYTES, media_budget)?;
                    media_budget -= image.len();
                    record.image_png_base64 = Some(image);
                    record.thumbnail_png_base64 = thumb_path
                        .as_deref()
                        .map(|path| {
                            encode_export_media(Path::new(path), MAX_THUMB_BYTES, media_budget)
                        })
                        .transpose()?;
                }
                output.json(&record)?;
            }
        }
        output.append(b"],\"collections\":[")?;
        {
            let mut statement = self
                .conn
                .prepare("SELECT id, name, color_hex, created_at FROM collections ORDER BY id")
                .map_err(|e| format!("查询备份分组失败: {e}"))?;
            let rows = statement
                .query_map([], |row| {
                    Ok(Collection {
                        id: row.get(0)?,
                        name: row.get(1)?,
                        color_hex: row.get(2)?,
                        created_at: row.get(3)?,
                    })
                })
                .map_err(|e| format!("读取备份分组失败: {e}"))?;
            for (index, row) in rows.enumerate() {
                if index != 0 {
                    output.append(b",")?;
                }
                output.json(&row.map_err(|e| format!("读取备份分组失败: {e}"))?)?;
            }
        }
        output.append(b"]}")?;
        // No destination write occurs on a query, encoding, limit, or transaction
        // failure. The successful bounded result uses the existing atomic writer.
        tx.commit().map_err(|e| format!("完成备份失败: {e}"))?;
        atomic_private_write(destination, &output.bytes).map_err(|e| format!("写入备份失败: {e}"))
    }

    fn preflight_backup(&self, byte_limit: usize, record_limit: usize) -> Result<(), String> {
        let (items, item_bytes): (i64, i64) = self
            .conn
            .query_row(
                "SELECT COUNT(*), COALESCE(SUM(
                COALESCE(length(CAST(content_hash AS BLOB)),0) +
                COALESCE(length(CAST(kind AS BLOB)),0) +
                COALESCE(length(CAST(text_content AS BLOB)),0) +
                COALESCE(length(CAST(html_content AS BLOB)),0) +
                COALESCE(length(CAST(rtf_content AS BLOB)),0) +
                COALESCE(length(CAST(image_path AS BLOB)),0) +
                COALESCE(length(CAST(thumb_path AS BLOB)),0) +
                COALESCE(length(CAST(files_json AS BLOB)),0) +
                COALESCE(length(CAST(source_app AS BLOB)),0) +
                COALESCE(length(CAST(source_bundle_id AS BLOB)),0) +
                COALESCE(length(CAST(sensitive_label AS BLOB)),0) +
                COALESCE(length(CAST(detected_color AS BLOB)),0) +
                COALESCE(length(CAST(note AS BLOB)),0)),0) FROM items",
                [],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .map_err(|e| format!("预检备份记录失败: {e}"))?;
        let (collections, collection_bytes): (i64, i64) = self
            .conn
            .query_row(
                "SELECT COUNT(*), COALESCE(SUM(
                length(CAST(name AS BLOB)) + length(CAST(color_hex AS BLOB))),0)
             FROM collections",
                [],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .map_err(|e| format!("预检备份分组失败: {e}"))?;
        if usize::try_from(items).map_or(true, |count| count > record_limit)
            || usize::try_from(collections).map_or(true, |count| count > record_limit)
        {
            return Err(tf(
                "backup.err_record_limit",
                &[("limit", &record_limit.to_string())],
            ));
        }
        if usize::try_from(item_bytes)
            .ok()
            .zip(usize::try_from(collection_bytes).ok())
            .and_then(|(items, collections)| items.checked_add(collections))
            .is_none_or(|bytes| bytes > byte_limit)
        {
            return Err(backup_too_large());
        }
        Ok(())
    }

    pub fn import_backup(
        &self,
        source: &Path,
        max_history: usize,
    ) -> Result<ImportSummary, String> {
        let raw = read_limited(source, MAX_BACKUP_BYTES)?;
        let first = raw.iter().find(|b| !b.is_ascii_whitespace());
        if first == Some(&b'[') {
            let items: Vec<ClipItem> =
                serde_json::from_slice(&raw).map_err(|e| format!("解析旧版 JSON 失败: {e}"))?;
            return self.import_records(
                items.into_iter().map(legacy_item).collect(),
                &[],
                max_history,
                true,
            );
        }
        let backup: Backup =
            serde_json::from_slice(&raw).map_err(|e| format!("解析备份失败: {e}"))?;
        if backup.format != "earth-clipboard" || backup.version != 2 {
            return Err(t("backup.err_unsupported_version").to_string());
        }
        self.import_records(backup.items, &backup.collections, max_history, false)
    }

    /// Compatibility entry point for old JSON arrays. Legacy absolute paths are
    /// accepted only when they already refer to this installation's owned media.
    #[cfg(test)]
    pub fn import_items(&self, items: &[ClipItem], max_history: usize) -> Result<usize, String> {
        self.import_records(
            items.iter().cloned().map(legacy_item).collect(),
            &[],
            max_history,
            true,
        )
        .map(|summary| summary.imported)
    }

    fn import_records(
        &self,
        records: Vec<BackupItem>,
        collections: &[Collection],
        max_history: usize,
        legacy_format: bool,
    ) -> Result<ImportSummary, String> {
        if records.len() > MAX_ITEMS || collections.len() > MAX_ITEMS {
            return Err(tf(
                "backup.err_record_limit",
                &[("limit", &MAX_ITEMS.to_string())],
            ));
        }
        let mut summary = ImportSummary {
            legacy_format,
            ..Default::default()
        };
        let tx = self.begin_write()?;
        let mut pending = PendingMedia::default();
        let mut collection_ids = HashMap::new();
        for collection in collections {
            let name = collection.name.trim();
            if name.is_empty() {
                return Err("备份包含空收藏分组名称".to_string());
            }
            tx.execute(
                "INSERT INTO collections(name, color_hex, created_at) VALUES (?1, ?2, ?3)
                 ON CONFLICT(name) DO NOTHING",
                params![name, collection.color_hex, collection.created_at],
            )
            .map_err(|e| format!("导入收藏分组失败: {e}"))?;
            let id: i64 = tx
                .query_row("SELECT id FROM collections WHERE name = ?1", [name], |r| {
                    r.get(0)
                })
                .map_err(|e| format!("读取收藏分组失败: {e}"))?;
            collection_ids.insert(collection.id, id);
        }
        for record in records {
            let mut item = record.item;
            let mut image_bytes = None;
            let mut thumb_bytes = None;
            // Imported paths never become new file authority. Only known-local
            // legacy assets may be reused; modern backups reconstruct from bytes.
            if item.kind == ClipKind::Image {
                if let Some(encoded) = record.image_png_base64 {
                    let bytes = decode_base64(&encoded, MAX_IMAGE_BYTES)?;
                    let image = decode_png(&bytes)?;
                    item.image_width = Some(image.width());
                    item.image_height = Some(image.height());
                    item.byte_size = bytes.len() as u64;
                    let thumb = if let Some(encoded) = record.thumbnail_png_base64 {
                        let bytes = decode_base64(&encoded, MAX_THUMB_BYTES)?;
                        let image = decode_png(&bytes)?;
                        if image.width() > 280 || image.height() > 180 {
                            return Err("缩略图尺寸超过 280×180 上限".to_string());
                        }
                        bytes
                    } else {
                        let thumbnail = image.thumbnail(280, 180);
                        let mut output = Cursor::new(Vec::new());
                        thumbnail
                            .write_to(&mut output, image::ImageFormat::Png)
                            .map_err(|e| format!("生成导入缩略图失败: {e}"))?;
                        output.into_inner()
                    };
                    item.content_hash = sha256_prefixed_hex(b"image:", &bytes);
                    image_bytes = Some(bytes);
                    thumb_bytes = Some(thumb);
                    item.image_path = None;
                    item.thumb_path = None;
                } else if legacy_format {
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
                } else {
                    return Err("图片备份缺少内嵌 PNG 数据，未导入任何记录".to_string());
                }
                if image_bytes.is_none() && item.image_path.is_none() {
                    summary.missing_images += 1;
                    item.thumb_path = None;
                }
                if item.content_hash.is_empty() {
                    // Stable marker lets legacy metadata remain visible without
                    // trusting its former absolute path or dropping the record.
                    item.content_hash = sha256_hex(
                        format!(
                            "missing-image:{}:{}:{}",
                            item.id, item.first_copied_at, item.byte_size
                        )
                        .as_bytes(),
                    );
                }
            } else {
                item.image_path = None;
                item.thumb_path = None;
                item.image_width = None;
                item.image_height = None;
                match item.kind {
                    ClipKind::Text => {
                        let text = item.text_content.as_deref().unwrap_or("");
                        item.content_hash = content_hash_for_text(text);
                        item.byte_size = text.len() as u64;
                        item.sensitive_label = detect_sensitive_label(text);
                        item.detected_color = detect_color(text);
                        item.is_url = detect_url(text);
                    }
                    ClipKind::Files => {
                        let mut paths: Vec<&str> =
                            item.files.iter().map(|f| f.path.as_str()).collect();
                        paths.sort_unstable();
                        item.content_hash =
                            sha256_hex(format!("files:{}", paths.join("\n")).as_bytes());
                    }
                    ClipKind::Image => unreachable!(),
                }
            }
            let duplicate: bool = tx
                .query_row(
                    "SELECT EXISTS(SELECT 1 FROM items WHERE content_hash = ?1
                 OR (?2 = 'text' AND kind = 'text' AND text_content = ?3))",
                    params![item.content_hash, item.kind.as_str(), item.text_content],
                    |r| r.get(0),
                )
                .map_err(|e| format!("检查导入重复记录失败: {e}"))?;
            if duplicate {
                summary.skipped_duplicates += 1;
                continue;
            }
            if let Some(bytes) = image_bytes {
                item.image_path = Some(pending.write(&self.images_dir, &bytes, "")?);
            }
            if let Some(bytes) = thumb_bytes {
                item.thumb_path = Some(pending.write(&self.thumbs_dir, &bytes, "_thumb")?);
            }
            let files_json = serde_json::to_string(&item.files)
                .map_err(|e| format!("序列化文件列表失败: {e}"))?;
            let color = item
                .detected_color
                .map(|c| format!("{},{},{},{}", c[0], c[1], c[2], c[3]));
            let collection = item
                .collection_id
                .and_then(|id| collection_ids.get(&id).copied());
            tx.execute(
                "INSERT INTO items (
                    content_hash, kind, text_content, html_content, rtf_content,
                    image_path, thumb_path, image_width, image_height,
                    files_json, byte_size, source_app, source_bundle_id,
                    pinned, collection_id, first_copied_at, last_copied_at,
                    copy_count, sensitive_label, detected_color, is_url, note
                 ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12,
                           ?13, ?14, ?15, ?16, ?17, ?18, ?19, ?20, ?21, ?22)",
                params![
                    item.content_hash,
                    item.kind.as_str(),
                    item.text_content,
                    item.html_content,
                    item.rtf_content,
                    item.image_path,
                    item.thumb_path,
                    item.image_width.map(i64::from),
                    item.image_height.map(i64::from),
                    files_json,
                    item.byte_size.min(i64::MAX as u64) as i64,
                    item.source_app,
                    item.source_bundle_id,
                    item.pinned,
                    collection,
                    item.first_copied_at,
                    item.last_copied_at,
                    item.copy_count.clamp(1, i64::MAX as u64) as i64,
                    item.sensitive_label,
                    color,
                    item.is_url,
                    item.note,
                ],
            )
            .map_err(|e| format!("导入记录失败: {e}"))?;
            summary.imported += 1;
        }
        let removed = Self::prune_rows(&tx, max_history)?;
        tx.commit().map_err(|e| format!("提交备份导入失败: {e}"))?;
        pending.commit();
        self.remove_unreferenced_media(&removed)?;
        Ok(summary)
    }
}

fn legacy_item(item: ClipItem) -> BackupItem {
    BackupItem {
        item,
        image_png_base64: None,
        thumbnail_png_base64: None,
    }
}

fn backup_too_large() -> String {
    t("backup.err_too_large").to_string()
}

struct LimitedBuffer {
    bytes: Vec<u8>,
    limit: usize,
}

impl LimitedBuffer {
    fn new(limit: usize) -> Self {
        Self {
            bytes: Vec::new(),
            limit,
        }
    }

    fn remaining(&self) -> usize {
        self.limit - self.bytes.len()
    }

    fn append(&mut self, bytes: &[u8]) -> Result<(), String> {
        self.write_all(bytes)
            .map_err(|e| format!("序列化备份失败: {e}"))
    }

    fn json(&mut self, value: &impl Serialize) -> Result<(), String> {
        serde_json::to_writer(self, value).map_err(|e| format!("序列化备份失败: {e}"))
    }
}

impl Write for LimitedBuffer {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        if bytes.len() > self.remaining() {
            return Err(io::Error::other(backup_too_large()));
        }
        let needed = self.bytes.len() + bytes.len();
        if needed > self.bytes.capacity() {
            // Grow geometrically, but never request capacity beyond the output cap.
            let capacity = self
                .bytes
                .capacity()
                .max(1024)
                .saturating_mul(2)
                .max(needed)
                .min(self.limit);
            self.bytes
                .try_reserve_exact(capacity - self.bytes.len())
                .map_err(io::Error::other)?;
        }
        self.bytes.extend_from_slice(bytes);
        Ok(bytes.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

struct LimitedCounter {
    remaining: usize,
}

impl LimitedCounter {
    fn new(remaining: usize) -> Self {
        Self { remaining }
    }
}

impl Write for LimitedCounter {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        self.remaining = self
            .remaining
            .checked_sub(bytes.len())
            .ok_or_else(|| io::Error::other(backup_too_large()))?;
        Ok(bytes.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

fn encode_export_media(
    path: &Path,
    file_limit: usize,
    output_budget: usize,
) -> Result<String, String> {
    // Four encoded bytes are required per three input bytes, including padding.
    // Apply that remaining budget to the file read, before allocating or encoding.
    let raw_limit = file_limit.min(output_budget / 4 * 3);
    let bytes = read_limited(path, raw_limit as u64)?;
    let encoded_len = bytes
        .len()
        .div_ceil(3)
        .checked_mul(4)
        .ok_or_else(backup_too_large)?;
    if encoded_len > output_budget {
        return Err(backup_too_large());
    }
    Ok(STANDARD.encode(bytes))
}

fn read_limited(path: &Path, limit: u64) -> Result<Vec<u8>, String> {
    let file = fs::File::open(path).map_err(|e| format!("读取备份文件失败: {e}"))?;
    if file
        .metadata()
        .map_err(|e| format!("读取备份大小失败: {e}"))?
        .len()
        > limit
    {
        return Err(format!("文件超过大小限制（{} MiB）", limit / 1024 / 1024));
    }
    let mut bytes = Vec::new();
    file.take(limit + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| format!("读取备份文件失败: {e}"))?;
    if bytes.len() as u64 > limit {
        return Err("文件超过大小限制".to_string());
    }
    Ok(bytes)
}

fn decode_base64(encoded: &str, limit: usize) -> Result<Vec<u8>, String> {
    if encoded.len() > limit.div_ceil(3) * 4 {
        return Err("内嵌图片超过大小限制".to_string());
    }
    let bytes = STANDARD
        .decode(encoded)
        .map_err(|e| format!("解析内嵌图片失败: {e}"))?;
    if bytes.len() > limit {
        return Err("内嵌图片超过大小限制".to_string());
    }
    Ok(bytes)
}

fn decode_png(bytes: &[u8]) -> Result<image::DynamicImage, String> {
    use image::ImageDecoder;
    let mut reader = image::ImageReader::with_format(Cursor::new(bytes), image::ImageFormat::Png);
    let mut limits = image::Limits::default();
    limits.max_image_width = Some(32_768);
    limits.max_image_height = Some(32_768);
    limits.max_alloc = Some(MAX_IMAGE_PIXELS * 8);
    reader.limits(limits);
    let decoder = reader
        .into_decoder()
        .map_err(|e| format!("内嵌 PNG 无效或超过像素限制: {e}"))?;
    let (width, height) = decoder.dimensions();
    if u64::from(width) * u64::from(height) > MAX_IMAGE_PIXELS
        || decoder.total_bytes() > MAX_IMAGE_PIXELS * 8
    {
        return Err("内嵌 PNG 超过 6400 万像素上限".to_string());
    }
    image::DynamicImage::from_decoder(decoder)
        .map_err(|e| format!("内嵌 PNG 无效或超过像素限制: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    struct TestDir(PathBuf);

    impl TestDir {
        fn new(name: &str) -> Self {
            Self(std::env::temp_dir().join(format!(
                "earth-backup-{name}-{}-{}",
                std::process::id(),
                Utc::now().timestamp_nanos_opt().unwrap()
            )))
        }
    }

    impl Drop for TestDir {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn payload(text: &str) -> NewClipPayload {
        NewClipPayload {
            content_hash: content_hash_for_text(text),
            kind: ClipKind::Text,
            text_content: Some(text.to_string()),
            html_content: None,
            rtf_content: None,
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

    fn image_payload() -> NewClipPayload {
        let mut encoded = Cursor::new(Vec::new());
        image::DynamicImage::new_rgba8(2, 1)
            .write_to(&mut encoded, image::ImageFormat::Png)
            .unwrap();
        let png = encoded.into_inner();
        NewClipPayload {
            content_hash: sha256_prefixed_hex(b"image:", &png),
            kind: ClipKind::Image,
            text_content: None,
            image_png: Some(png.clone()),
            thumb_png: Some(png.clone()),
            image_width: Some(2),
            image_height: Some(1),
            byte_size: png.len() as u64,
            ..payload("")
        }
    }

    #[test]
    fn export_preflight_counts_utf8_bytes_in_all_variable_length_columns() {
        let dir = TestDir::new("preflight-bytes");
        let storage = Storage::open_in_dir(&dir.0).unwrap();
        storage.upsert_item(payload("short"), 10).unwrap();
        let destination = dir.0.join("existing.json");
        fs::write(&destination, b"existing backup").unwrap();
        storage.preflight_backup(512, 10).unwrap();
        // Each field is below 512 Unicode scalar values but above 512 UTF-8 bytes.
        // Some values intentionally cannot decode as a ClipItem: preflight must
        // reject their byte count before loading/deserializing a record.
        let oversized = "界".repeat(200);
        for (table, column) in [
            ("items", "content_hash"),
            ("items", "kind"),
            ("items", "text_content"),
            ("items", "html_content"),
            ("items", "rtf_content"),
            ("items", "image_path"),
            ("items", "thumb_path"),
            ("items", "files_json"),
            ("items", "source_app"),
            ("items", "source_bundle_id"),
            ("items", "sensitive_label"),
            ("items", "detected_color"),
            ("items", "note"),
            ("collections", "name"),
            ("collections", "color_hex"),
        ] {
            let original: rusqlite::types::Value = storage
                .conn
                .query_row(
                    &format!("SELECT {column} FROM {table} ORDER BY id LIMIT 1"),
                    [],
                    |row| row.get(0),
                )
                .unwrap();
            let update = format!(
                "UPDATE {table} SET {column} = ?1 WHERE id = (SELECT MIN(id) FROM {table})"
            );
            storage.conn.execute(&update, [&oversized]).unwrap();
            assert!(
                storage
                    .preflight_backup(512, 10)
                    .unwrap_err()
                    .contains("上限"),
                "{table}.{column}"
            );
            assert!(
                storage
                    .export_backup_with_limits(&destination, 512, 10)
                    .is_err()
            );
            assert_eq!(fs::read(&destination).unwrap(), b"existing backup");
            storage.conn.execute(&update, [original]).unwrap();
        }
    }

    #[test]
    fn export_preflight_limits_both_items_and_collections() {
        let dir = TestDir::new("preflight-counts");
        let storage = Storage::open_in_dir(&dir.0).unwrap();
        let destination = dir.0.join("existing.json");
        fs::write(&destination, b"existing backup").unwrap();
        storage.create_collection("third group", "#123456").unwrap();
        assert!(
            storage
                .export_backup_with_limits(&destination, 16_384, 2)
                .unwrap_err()
                .contains("最多包含 2")
        );
        assert_eq!(fs::read(&destination).unwrap(), b"existing backup");
        storage.conn.execute("DELETE FROM collections", []).unwrap();
        for text in ["first", "second", "third"] {
            storage.upsert_item(payload(text), 10).unwrap();
        }
        // An invalid row must never reach the record decoder when count fails.
        storage
            .conn
            .execute("UPDATE items SET html_content = X'FF'", [])
            .unwrap();
        assert!(
            storage
                .export_backup_with_limits(&destination, 16_384, 2)
                .unwrap_err()
                .contains("最多包含 2")
        );
        assert_eq!(fs::read(&destination).unwrap(), b"existing backup");
    }

    #[test]
    fn export_limit_includes_json_escaping_and_preserves_existing_destination() {
        let dir = TestDir::new("escaping");
        let storage = Storage::open_in_dir(&dir.0).unwrap();
        storage.upsert_item(payload(&"\0".repeat(300)), 10).unwrap();
        storage.preflight_backup(1_024, 10).unwrap();
        let destination = dir.0.join("existing.json");
        fs::write(&destination, b"existing backup").unwrap();
        assert!(
            storage
                .export_backup_with_limits(&destination, 1_024, 10)
                .unwrap_err()
                .contains("上限")
        );
        assert_eq!(fs::read(destination).unwrap(), b"existing backup");
    }

    #[test]
    fn export_exact_serialized_limit_covers_rich_text_media_and_collections() {
        let dir = TestDir::new("exact-limit");
        let storage = Storage::open_in_dir(&dir.0).unwrap();
        let mut rich = payload("quoted \"text\" \n 界");
        rich.html_content = Some("<b>quoted \"text\"</b>".into());
        rich.rtf_content = Some(r"{\rtf1 quoted text}".into());
        rich.source_app = Some("App \"name\"".into());
        rich.source_bundle_id = Some("example.app".into());
        let id = storage.upsert_item(rich, 10).unwrap();
        storage
            .set_item_note(id, Some("note\nwith \"quotes\""))
            .unwrap();
        let collection = storage
            .create_collection("group \"with quotes\"", "#123456")
            .unwrap();
        storage.set_collection(id, Some(collection)).unwrap();
        storage.upsert_item(image_payload(), 10).unwrap();
        let destination = dir.0.join("backup.json");
        storage
            .export_backup_with_limits(&destination, 16_384, 10)
            .unwrap();
        let bytes = fs::read(&destination).unwrap();
        let parsed: Backup = serde_json::from_slice(&bytes).unwrap();
        let text = parsed
            .items
            .iter()
            .find(|entry| entry.item.id == id)
            .unwrap();
        assert_eq!(
            text.item.html_content.as_deref(),
            Some("<b>quoted \"text\"</b>")
        );
        assert_eq!(text.item.note.as_deref(), Some("note\nwith \"quotes\""));
        assert!(
            parsed
                .collections
                .iter()
                .any(|group| group.id == collection)
        );
        let image = parsed
            .items
            .iter()
            .find(|entry| entry.item.kind == ClipKind::Image)
            .unwrap();
        assert!(image.item.image_path.is_none() && image.item.thumb_path.is_none());
        assert_eq!(
            STANDARD
                .decode(image.image_png_base64.as_ref().unwrap())
                .unwrap(),
            image_payload().image_png.unwrap()
        );
        assert!(image.thumbnail_png_base64.is_some());
        storage
            .export_backup_with_limits(&destination, bytes.len(), 10)
            .unwrap();
        assert_eq!(fs::read(&destination).unwrap(), bytes);
        fs::write(&destination, b"existing backup").unwrap();
        assert!(
            storage
                .export_backup_with_limits(&destination, bytes.len() - 1, 10)
                .is_err()
        );
        assert_eq!(fs::read(destination).unwrap(), b"existing backup");
    }

    #[test]
    fn export_missing_image_preserves_existing_destination() {
        let dir = TestDir::new("missing-image");
        let storage = Storage::open_in_dir(&dir.0).unwrap();
        let id = storage.upsert_item(image_payload(), 10).unwrap();
        let path = storage.item(id).unwrap().image_path.unwrap();
        fs::remove_file(path).unwrap();
        let destination = dir.0.join("existing.json");
        fs::write(&destination, b"existing backup").unwrap();
        assert!(
            storage
                .export_backup_with_limits(&destination, 16_384, 10)
                .is_err()
        );
        assert_eq!(fs::read(destination).unwrap(), b"existing backup");
    }

    #[test]
    fn media_budget_includes_base64_padding_before_encoding() {
        let dir = TestDir::new("base64");
        fs::create_dir_all(&dir.0).unwrap();
        let path = dir.0.join("bytes");
        for (bytes, encoded) in [
            (b"a".as_slice(), "YQ=="),
            (b"ab".as_slice(), "YWI="),
            (b"abc".as_slice(), "YWJj"),
            (b"abcd".as_slice(), "YWJjZA=="),
        ] {
            fs::write(&path, bytes).unwrap();
            assert_eq!(
                encode_export_media(&path, 100, encoded.len()).unwrap(),
                encoded
            );
            assert!(encode_export_media(&path, 100, encoded.len() - 1).is_err());
            assert!(encode_export_media(&path, bytes.len() - 1, 100).is_err());
        }
    }

    #[test]
    fn bounded_writer_rejects_growth_without_appending_beyond_limit() {
        let mut output = LimitedBuffer::new(13);
        output.append(b"1234567890").unwrap();
        assert!(output.append(b"four").is_err());
        assert_eq!(output.bytes, b"1234567890");
        output.append(b"123").unwrap();
        assert_eq!(output.remaining(), 0);
        assert!(output.append(b"x").is_err());
        assert_eq!(output.bytes.len(), 13);
        assert!(output.bytes.capacity() <= 13);
    }

    #[test]
    fn imported_text_recomputes_sensitive_color_and_url_metadata() {
        let source_dir = TestDir::new("metadata-source");
        let source = Storage::open_in_dir(&source_dir.0).unwrap();
        let fixtures = [
            (
                "password=synthetic-test-value",
                Some("Secret Credential"),
                None,
                false,
            ),
            ("#123456", None, Some([0x12, 0x34, 0x56, 0xff]), false),
            ("https://example.invalid/path", None, None, true),
            ("ordinary text", None, None, false),
        ];
        let mut records = Vec::new();
        for (text, _, _, _) in fixtures {
            let id = source.upsert_item(payload(text), 10).unwrap();
            let mut item = source.item(id).unwrap();
            item.content_hash = "untrusted imported hash".into();
            item.byte_size = u64::MAX;
            item.sensitive_label = if text.starts_with("password=") {
                None
            } else {
                Some("stale label".into())
            };
            item.detected_color = if text.starts_with('#') {
                None
            } else {
                Some([1, 2, 3, 4])
            };
            item.is_url = !detect_url(text);
            records.push(item);
        }
        for legacy in [false, true] {
            let dir = TestDir::new(if legacy {
                "metadata-legacy"
            } else {
                "metadata-v2"
            });
            let storage = Storage::open_in_dir(&dir.0).unwrap();
            let path = dir.0.join("import.json");
            let bytes = if legacy {
                serde_json::to_vec(&records).unwrap()
            } else {
                serde_json::to_vec(&Backup {
                    format: "earth-clipboard".into(),
                    version: 2,
                    items: records.iter().cloned().map(legacy_item).collect(),
                    collections: vec![],
                })
                .unwrap()
            };
            fs::write(&path, bytes).unwrap();
            let summary = storage.import_backup(&path, 10).unwrap();
            assert_eq!(summary.imported, fixtures.len());
            assert_eq!(summary.legacy_format, legacy);
            let imported = storage.list_items().unwrap();
            for (text, sensitive, color, url) in fixtures {
                let item = imported
                    .iter()
                    .find(|item| item.text_content.as_deref() == Some(text))
                    .unwrap();
                assert_eq!(item.content_hash, content_hash_for_text(text));
                assert_eq!(item.byte_size, text.len() as u64);
                assert_eq!(item.sensitive_label.as_deref(), sensitive);
                assert_eq!(item.detected_color, color);
                assert_eq!(item.is_url, url);
            }
        }
    }

    #[test]
    fn recaptured_text_refreshes_missing_or_stale_sensitive_metadata() {
        let dir = TestDir::new("recapture-sensitive");
        let storage = Storage::open_in_dir(&dir.0).unwrap();
        for (text, old_label, expected_label) in [
            (
                "password=synthetic-test-value",
                None,
                Some("Secret Credential"),
            ),
            ("ordinary text", Some("stale label"), None),
        ] {
            let mut original = payload(text);
            original.sensitive_label = old_label.map(str::to_string);
            let id = storage.upsert_item(original, 10).unwrap();
            assert_eq!(
                storage.item(id).unwrap().sensitive_label.as_deref(),
                old_label
            );
            assert_eq!(storage.upsert_item(payload(text), 10).unwrap(), id);
            let recaptured = storage.item(id).unwrap();
            assert_eq!(recaptured.sensitive_label.as_deref(), expected_label);
            assert_eq!(recaptured.copy_count, 2);
        }
        assert_eq!(storage.list_items().unwrap().len(), 2);
    }

    #[test]
    fn backup_accepts_capture_width_above_the_previous_16384_limit() {
        let mut encoded = Cursor::new(Vec::new());
        image::DynamicImage::new_rgba8(20_000, 1)
            .write_to(&mut encoded, image::ImageFormat::Png)
            .unwrap();
        let decoded = decode_png(encoded.get_ref()).unwrap();
        assert_eq!((decoded.width(), decoded.height()), (20_000, 1));
    }
}
