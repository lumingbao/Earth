use super::*;
use std::io::Cursor;

struct TestDir(PathBuf);
impl TestDir {
    fn new(name: &str) -> Self {
        let path = std::env::temp_dir().join(format!(
            "earth-storage-{name}-{}-{}",
            std::process::id(),
            Utc::now().timestamp_nanos_opt().unwrap()
        ));
        Self(path)
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
    image::DynamicImage::new_rgba8(8, 6)
        .write_to(&mut encoded, image::ImageFormat::Png)
        .unwrap();
    let png = encoded.into_inner();
    NewClipPayload {
        content_hash: sha256_hex(&[b"image:".as_slice(), &png].concat()),
        kind: ClipKind::Image,
        image_width: Some(8),
        image_height: Some(6),
        byte_size: png.len() as u64,
        image_png: Some(png.clone()),
        thumb_png: Some(png),
        text_content: None,
        ..payload("")
    }
}

#[test]
fn edit_collision_preserves_both_records_and_annotations() {
    let dir = TestDir::new("edit");
    let storage = Storage::open_in_dir(&dir.0).unwrap();
    let first = storage.upsert_item(payload("first text"), 20).unwrap();
    let second = storage.upsert_item(payload("second text"), 20).unwrap();
    let group = storage.create_collection("Saved", "#123456").unwrap();
    storage.set_pinned(first, true).unwrap();
    storage.set_collection(first, Some(group)).unwrap();
    storage
        .set_item_note(first, Some("Keep this note"))
        .unwrap();
    storage.set_item_note(second, Some("Another note")).unwrap();
    let before = serde_json::to_value(storage.list_items().unwrap()).unwrap();
    assert!(
        storage
            .update_item_text(second, "first text")
            .unwrap_err()
            .contains("均已保留")
    );
    assert_eq!(
        before,
        serde_json::to_value(storage.list_items().unwrap()).unwrap()
    );
    storage.update_item_text(second, "edited text").unwrap();
    let copied = storage.upsert_item(payload("edited text"), 20).unwrap();
    assert_eq!(copied, second);
    assert_eq!(storage.list_items().unwrap().len(), 2);
}

#[test]
fn plain_text_recapture_clears_previous_rich_formats() {
    let dir = TestDir::new("rich");
    let storage = Storage::open_in_dir(&dir.0).unwrap();
    let mut rich = payload("example");
    rich.html_content = Some("<b>example</b>".into());
    rich.rtf_content = Some("rich example".into());
    let id = storage.upsert_item(rich, 20).unwrap();
    assert_eq!(storage.upsert_item(payload("example"), 20).unwrap(), id);
    let item = storage.list_items().unwrap().remove(0);
    assert!(item.html_content.is_none() && item.rtf_content.is_none());
}

#[cfg(unix)]
#[test]
fn new_and_existing_data_files_are_private_including_sqlite_sidecars() {
    use std::os::unix::fs::PermissionsExt;
    let dir = TestDir::new("permissions");
    let storage = Storage::open_in_dir(&dir.0).unwrap();
    storage.save_settings(&AppSettings::default()).unwrap();
    storage.upsert_item(image_payload(), 20).unwrap();
    for p in [&dir.0, &storage.images_dir, &storage.thumbs_dir] {
        assert_eq!(fs::metadata(p).unwrap().permissions().mode() & 0o777, 0o700);
    }
    for p in [
        storage.base_dir.join("earth.db"),
        storage.base_dir.join("earth.db-wal"),
        storage.base_dir.join("earth.db-shm"),
        storage.settings_path(),
    ] {
        assert_eq!(
            fs::metadata(&p).unwrap().permissions().mode() & 0o777,
            0o600,
            "{}",
            p.display()
        );
        fs::set_permissions(p, fs::Permissions::from_mode(0o644)).unwrap();
    }
    let images = storage.list_items().unwrap();
    for p in [
        images[0].image_path.as_ref().unwrap(),
        images[0].thumb_path.as_ref().unwrap(),
    ] {
        assert_eq!(fs::metadata(p).unwrap().permissions().mode() & 0o777, 0o600);
        fs::set_permissions(p, fs::Permissions::from_mode(0o644)).unwrap();
    }
    drop(storage);
    fs::set_permissions(&dir.0, fs::Permissions::from_mode(0o755)).unwrap();
    let reopened = Storage::open_in_dir(&dir.0).unwrap();
    for p in [
        reopened.base_dir.join("earth.db"),
        reopened.settings_path(),
        PathBuf::from(images[0].image_path.as_ref().unwrap()),
        PathBuf::from(images[0].thumb_path.as_ref().unwrap()),
    ] {
        assert_eq!(fs::metadata(p).unwrap().permissions().mode() & 0o777, 0o600);
    }
}

#[test]
fn cleanup_only_owns_media_and_preserves_shared_references() {
    let dir = TestDir::new("cleanup");
    let outside = TestDir::new("unmanaged");
    fs::create_dir_all(&outside.0).unwrap();
    let unrelated = outside.0.join("personal.png");
    fs::write(&unrelated, b"personal file").unwrap();
    let storage = Storage::open_in_dir(&dir.0).unwrap();
    assert!(managed_media_path(&storage.images_dir, &unrelated.to_string_lossy()).is_none());
    let first = storage.upsert_item(image_payload(), 20).unwrap();
    let second = storage.upsert_item(payload("kept"), 20).unwrap();
    let image = storage
        .list_items()
        .unwrap()
        .into_iter()
        .find(|i| i.id == first)
        .unwrap()
        .image_path
        .unwrap();
    // Existing database references may share a local asset. Removing one owner
    // must not invalidate another, nor treat arbitrary older paths as owned.
    storage
        .conn
        .execute(
            "UPDATE items SET image_path = ?1, thumb_path = ?2 WHERE id = ?3",
            params![image, unrelated.to_string_lossy(), second],
        )
        .unwrap();
    storage.delete_item(first).unwrap();
    assert!(Path::new(&image).exists());
    storage.clear_history(true).unwrap();
    assert!(!Path::new(&image).exists());
    assert_eq!(fs::read(unrelated).unwrap(), b"personal file");
}

#[cfg(unix)]
#[test]
fn media_directory_and_asset_symlinks_are_not_owned() {
    use std::os::unix::fs::symlink;
    let dir = TestDir::new("links");
    let outside = TestDir::new("link-target");
    fs::create_dir_all(&outside.0).unwrap();
    let external = outside.0.join("kept.png");
    fs::write(&external, b"kept").unwrap();
    let storage = Storage::open_in_dir(&dir.0).unwrap();
    let link = storage.images_dir.join("0000000000000000.png");
    symlink(&external, &link).unwrap();
    assert!(managed_media_path(&storage.images_dir, &link.to_string_lossy()).is_none());
    storage.cleanup_orphaned_media().unwrap();
    assert!(external.exists());
    assert!(
        fs::symlink_metadata(&link)
            .unwrap()
            .file_type()
            .is_symlink()
    );
    drop(storage);
    fs::remove_file(link).unwrap();
    fs::remove_dir(dir.0.join("images")).unwrap();
    symlink(&outside.0, dir.0.join("images")).unwrap();
    assert!(Storage::open_in_dir(&dir.0).is_err());
}

#[test]
fn settings_recover_last_valid_snapshot_and_preserve_corrupt_bytes() {
    let dir = TestDir::new("settings");
    let storage = Storage::open_in_dir(&dir.0).unwrap();
    let mut settings = AppSettings {
        max_history: 321,
        ignored_regexes: vec!["private".into()],
        ..Default::default()
    };
    storage.save_settings(&settings).unwrap();
    settings.max_history = 654;
    storage.save_settings(&settings).unwrap();
    fs::write(storage.settings_path(), b"incomplete settings").unwrap();
    let (recovered, warning) = storage.load_settings_with_warning();
    assert_eq!(recovered.max_history, 321);
    assert_eq!(recovered.ignored_regexes, vec!["private"]);
    assert!(warning.unwrap().contains("恢复"));
    let corrupt = fs::read_dir(&dir.0)
        .unwrap()
        .flatten()
        .find(|e| {
            e.file_name()
                .to_string_lossy()
                .starts_with("settings.json.corrupt-")
        })
        .unwrap();
    assert_eq!(fs::read(corrupt.path()).unwrap(), b"incomplete settings");
    assert_eq!(storage.load_settings_with_warning().0.max_history, 321);
    fs::remove_file(dir.0.join("settings.json.bak")).unwrap();
    fs::write(storage.settings_path(), b"incomplete again").unwrap();
    let (fallback, warning) = storage.load_settings_with_warning();
    assert!(fallback.paused);
    assert!(warning.unwrap().contains("暂停"));
}

#[test]
fn portable_backup_restores_images_notes_and_collections_without_source_files() {
    let source_dir = TestDir::new("backup-source");
    let target_dir = TestDir::new("backup-target");
    let backup_dir = TestDir::new("backup-file");
    fs::create_dir_all(&backup_dir.0).unwrap();
    let backup_path = backup_dir.0.join("backup.json");
    let source = Storage::open_in_dir(&source_dir.0).unwrap();
    let id = source.upsert_item(image_payload(), 20).unwrap();
    let group = source.create_collection("Portable", "#112233").unwrap();
    source.set_collection(id, Some(group)).unwrap();
    source.set_item_note(id, Some("portable image")).unwrap();
    source.set_pinned(id, true).unwrap();
    let expected = image_payload().image_png.unwrap();
    source.export_backup(&backup_path).unwrap();
    let raw = fs::read_to_string(&backup_path).unwrap();
    assert!(!raw.contains(&source_dir.0.to_string_lossy().to_string()));
    drop(source);
    fs::remove_dir_all(&source_dir.0).unwrap();
    let target = Storage::open_in_dir(&target_dir.0).unwrap();
    let summary = target.import_backup(&backup_path, 20).unwrap();
    assert_eq!(summary.imported, 1);
    assert_eq!(summary.missing_images, 0);
    let restored = target.list_items().unwrap().remove(0);
    assert!(restored.pinned);
    assert_eq!(restored.note.as_deref(), Some("portable image"));
    assert_eq!(fs::read(restored.image_path.unwrap()).unwrap(), expected);
    assert!(Path::new(&restored.thumb_path.unwrap()).exists());
    assert!(
        target
            .list_collections()
            .unwrap()
            .iter()
            .any(|c| Some(c.id) == restored.collection_id && c.name == "Portable")
    );
    assert_eq!(
        target
            .import_backup(&backup_path, 20)
            .unwrap()
            .skipped_duplicates,
        1
    );
}

#[test]
fn legacy_backup_reports_missing_images_and_keeps_text() {
    let source_dir = TestDir::new("legacy-source");
    let target_dir = TestDir::new("legacy-target");
    let source = Storage::open_in_dir(&source_dir.0).unwrap();
    source.upsert_item(image_payload(), 20).unwrap();
    source.upsert_item(payload("legacy text"), 20).unwrap();
    let backup = source_dir.0.join("old.json");
    fs::write(
        &backup,
        serde_json::to_vec(&source.list_items().unwrap()).unwrap(),
    )
    .unwrap();
    let target = Storage::open_in_dir(&target_dir.0).unwrap();
    let summary = target.import_backup(&backup, 20).unwrap();
    assert!(summary.legacy_format);
    assert_eq!(summary.missing_images, 1);
    assert_eq!(summary.imported, 2);
    let image = target
        .list_items()
        .unwrap()
        .into_iter()
        .find(|i| i.kind == ClipKind::Image)
        .unwrap();
    assert!(image.image_path.is_none() && image.thumb_path.is_none());
    target.clear_history(true).unwrap();
    assert!(
        source
            .list_items()
            .unwrap()
            .iter()
            .find(|i| i.kind == ClipKind::Image)
            .unwrap()
            .image_path
            .is_some()
    );
}

#[test]
fn invalid_image_backup_rolls_back_the_entire_import() {
    let source_dir = TestDir::new("batch-source");
    let target_dir = TestDir::new("batch-target");
    let source = Storage::open_in_dir(&source_dir.0).unwrap();
    source.upsert_item(payload("ordinary text"), 20).unwrap();
    source.upsert_item(image_payload(), 20).unwrap();
    let path = source_dir.0.join("backup.json");
    source.export_backup(&path).unwrap();
    let mut backup: serde_json::Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    let items = backup["items"].as_array_mut().unwrap();
    // Keep a valid item before an invalid item: a failed batch must not leave
    // either partial metadata or already-created assets behind.
    items.sort_by_key(|item| item["kind"].as_str() == Some("Image"));
    for item in items {
        if item["kind"] == "Image" {
            item["image_png_base64"] = serde_json::Value::String("bm90IGEgcG5n".into());
        }
    }
    fs::write(&path, serde_json::to_vec(&backup).unwrap()).unwrap();
    let target = Storage::open_in_dir(&target_dir.0).unwrap();
    assert!(target.import_backup(&path, 20).is_err());
    assert!(target.list_items().unwrap().is_empty());
    assert_eq!(fs::read_dir(&target.images_dir).unwrap().count(), 0);
    assert_eq!(fs::read_dir(&target.thumbs_dir).unwrap().count(), 0);
    backup["version"] = serde_json::Value::from(99);
    fs::write(&path, serde_json::to_vec(&backup).unwrap()).unwrap();
    assert!(
        target
            .import_backup(&path, 20)
            .unwrap_err()
            .contains("版本")
    );
}

#[test]
fn failed_image_insert_rolls_back_files_and_database() {
    let dir = TestDir::new("rollback");
    let storage = Storage::open_in_dir(&dir.0).unwrap();
    storage.conn.execute_batch("CREATE TRIGGER reject_test_insert BEFORE INSERT ON items BEGIN SELECT RAISE(ABORT, 'test storage failure'); END;").unwrap();
    assert!(storage.upsert_item(image_payload(), 20).is_err());
    assert!(storage.list_items().unwrap().is_empty());
    assert_eq!(fs::read_dir(&storage.images_dir).unwrap().count(), 0);
    assert_eq!(fs::read_dir(&storage.thumbs_dir).unwrap().count(), 0);
}

#[test]
fn schema_migration_preserves_empty_collections_and_rejects_future_versions() {
    let dir = TestDir::new("migration");
    let storage = Storage::open_in_dir(&dir.0).unwrap();
    storage.upsert_item(payload("retained"), 20).unwrap();
    storage
        .conn
        .execute_batch(
            "DELETE FROM collections; ALTER TABLE items DROP COLUMN note; PRAGMA user_version = 0;",
        )
        .unwrap();
    drop(storage);
    let migrated = Storage::open_in_dir(&dir.0).unwrap();
    assert!(migrated.list_collections().unwrap().is_empty());
    assert_eq!(migrated.list_items().unwrap().len(), 1);
    assert!(
        fs::read_dir(&dir.0)
            .unwrap()
            .flatten()
            .any(|e| e.file_name().to_string_lossy().contains("before-v1"))
    );
    migrated
        .conn
        .pragma_update(None, "user_version", 99)
        .unwrap();
    drop(migrated);
    assert!(Storage::open_in_dir(&dir.0).err().unwrap().contains("升级"));
}

#[test]
fn startup_gc_only_cleans_generated_unreferenced_media() {
    let dir = TestDir::new("gc");
    let storage = Storage::open_in_dir(&dir.0).unwrap();
    storage.upsert_item(image_payload(), 20).unwrap();
    let kept = storage.list_items().unwrap().remove(0).image_path.unwrap();
    let orphan = storage
        .images_dir
        .join(format!("{}.png", sha256_hex(b"orphan")));
    let user_file = storage.images_dir.join("notes.txt");
    fs::write(&orphan, b"unused").unwrap();
    fs::write(&user_file, b"leave alone").unwrap();
    drop(storage);
    let _reopened = Storage::open_in_dir(&dir.0).unwrap();
    assert!(!orphan.exists());
    assert!(user_file.exists());
    assert!(Path::new(&kept).exists());
}

#[test]
fn tied_timestamps_have_stable_eviction_and_new_copy_is_protected() {
    let dir = TestDir::new("ordering");
    let storage = Storage::open_in_dir(&dir.0).unwrap();
    for text in ["a", "b", "c"] {
        storage.upsert_item(payload(text), 20).unwrap();
    }
    storage
        .conn
        .execute("UPDATE items SET last_copied_at = ?1", [i64::MAX - 1])
        .unwrap();
    storage.enforce_history_limit(2).unwrap();
    let items = storage.list_items().unwrap();
    assert_eq!(
        items
            .iter()
            .map(|i| i.text_content.as_deref().unwrap())
            .collect::<Vec<_>>(),
        vec!["c", "b"]
    );
    let id = storage.upsert_item(payload("new"), 1).unwrap();
    assert_eq!(storage.list_items().unwrap().remove(0).id, id);
}
