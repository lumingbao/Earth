use super::*;
use crate::search::MatchMask;

fn safety_test_item() -> ClipItem {
    ClipItem {
        id: 1,
        content_hash: "sample".into(),
        kind: ClipKind::Text,
        text_content: Some("privacy-sample-content".into()),
        html_content: None,
        rtf_content: None,
        image_path: None,
        thumb_path: None,
        image_width: None,
        image_height: None,
        files: vec![],
        byte_size: 22,
        source_app: None,
        source_bundle_id: None,
        pinned: false,
        collection_id: None,
        first_copied_at: 0,
        last_copied_at: 0,
        copy_count: 1,
        sensitive_label: Some("Test fixture".into()),
        detected_color: None,
        is_url: false,
        note: None,
        text_truncated: false,
        has_rich_text: false,
    }
}

fn keyboard_test_frame(
    state: &mut PopupState,
    key: Key,
    mods: Modifiers,
    repeat: bool,
) -> (Vec<PopupAction>, Vec<egui::Event>) {
    let ctx = egui::Context::default();
    // egui derives repeats from held-key state, overriding the supplied flag.
    if repeat {
        let mut output = ctx.run_ui(
            egui::RawInput {
                events: vec![
                    egui::Event::ModifiersChanged(mods),
                    egui::Event::Key {
                        key,
                        physical_key: None,
                        pressed: true,
                        repeat: false,
                        modifiers: mods,
                    },
                ],
                ..Default::default()
            },
            |_| {},
        );
        output.textures_delta.clear();
    }
    let results = vec![SearchResultItem {
        item: safety_test_item().into(),
        score: 0,
        matched_indices: MatchMask::default(),
        display_text: None,
    }];
    let mut settings = AppSettings {
        hotkey_delete: "Alt+Backspace".into(),
        ..Default::default()
    };
    let mut actions = vec![];
    let mut remaining = vec![];
    let mut output = ctx.run_ui(
        egui::RawInput {
            events: vec![
                egui::Event::ModifiersChanged(mods),
                egui::Event::Key {
                    key,
                    physical_key: None,
                    pressed: true,
                    repeat,
                    modifiers: mods,
                },
            ],
            ..Default::default()
        },
        |ui| {
            handle_popup_keyboard(ui.ctx(), &mut settings, &results, &[], state, &mut actions);
            remaining = ui.ctx().input(|i| i.events.clone());
        },
    );
    output.textures_delta.clear();
    (actions, remaining)
}

#[test]
fn focused_search_keeps_word_delete_and_repeated_delete_never_removes_items() {
    let mut state = PopupState {
        selected_id: Some(1),
        search_focused: true,
        request_search_focus: false,
        ..Default::default()
    };
    let (actions, remaining) =
        keyboard_test_frame(&mut state, Key::Backspace, Modifiers::ALT, false);
    assert!(actions.is_empty());
    assert!(remaining.iter().any(|e| matches!(
        e,
        egui::Event::Key {
            key: Key::Backspace,
            ..
        }
    )));
    state.search_focused = false;
    let (actions, _) = keyboard_test_frame(&mut state, Key::Backspace, Modifiers::ALT, true);
    assert!(actions.is_empty());
    let (actions, remaining) =
        keyboard_test_frame(&mut state, Key::Backspace, Modifiers::ALT, false);
    assert!(matches!(actions.as_slice(), [PopupAction::DeleteItem(1)]));
    assert!(!remaining.iter().any(|e| matches!(
        e,
        egui::Event::Key {
            key: Key::Backspace,
            ..
        }
    )));
}

#[test]
fn escape_dismisses_banner_before_search_or_window() {
    let mut state = PopupState {
        search_query: "keep search".into(),
        request_search_focus: false,
        ..Default::default()
    };
    state.set_status_banner("message", false);
    let (actions, _) = keyboard_test_frame(&mut state, Key::Escape, Modifiers::NONE, false);
    assert!(state.status_banner.is_none());
    assert_eq!(state.search_query, "keep search");
    assert!(actions.is_empty());
    let (actions, _) = keyboard_test_frame(&mut state, Key::Escape, Modifiers::NONE, false);
    assert!(state.search_query.is_empty());
    assert!(actions.is_empty());
}

#[test]
fn sensitive_reveal_is_scoped_to_selection_and_popup_session() {
    let item = safety_test_item();
    let mut state = PopupState {
        selected_id: Some(1),
        ..Default::default()
    };
    assert!(is_masked(&item, true, &state));
    assert!(!is_masked(&item, false, &state));
    state.revealed_sensitive_ids.insert(1);
    state.inline_edit_text = Some((1, "draft".into()));
    state.active_transform = Some((1, QuickActionKind::Base64Encode, Ok("result".into())));
    state.qr_preview_id = Some(1);
    assert!(!is_masked(&item, true, &state));
    state.select_item(Some(2));
    state.select_item(Some(1));
    assert!(is_masked(&item, true, &state));
    assert!(state.inline_edit_text.is_none());
    assert!(state.active_transform.is_none());
    assert!(state.qr_preview_id.is_none());
    state.revealed_sensitive_ids.insert(1);
    state.clear_sensitive_context();
    assert!(is_masked(&item, true, &state));
}

#[test]
fn masking_consistently_protects_list_preview_editor_and_transform_output() {
    let item = Arc::new(safety_test_item());
    let result = SearchResultItem {
        item: item.clone(),
        score: 0,
        matched_indices: MatchMask::default(),
        display_text: None,
    };
    for masking in [true, false] {
        let ctx = egui::Context::default();
        let palette = EarthPalette::resolve(crate::model::ThemeMode::Light, false);
        palette.apply_to_ctx(&ctx);
        let settings = AppSettings {
            enable_privacy_masking: masking,
            ..Default::default()
        };
        let mut state = PopupState {
            selected_id: Some(1),
            preview_open: true,
            inline_edit_text: Some((1, "editor-fixture".into())),
            active_transform: Some((
                1,
                QuickActionKind::Base64Encode,
                Ok("transform-fixture".into()),
            )),
            ..Default::default()
        };
        let mut output = ctx.run_ui(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    pos2(0.0, 0.0),
                    vec2(760.0, 900.0),
                )),
                ..Default::default()
            },
            |ui| {
                let mut actions = vec![];
                render_item_row(
                    ui,
                    &result,
                    None,
                    true,
                    &settings,
                    &[],
                    &mut state,
                    &palette,
                    &mut actions,
                );
                render_preview_pane(
                    ui,
                    Some(&item),
                    None,
                    masking,
                    &[],
                    &mut state,
                    &palette,
                    &mut actions,
                );
            },
        );
        let text = output
            .shapes
            .iter()
            .filter_map(|shape| match &shape.shape {
                egui::epaint::Shape::Text(t) => Some(t.galley.text()),
                _ => None,
            })
            .collect::<Vec<_>>()
            .join("\n");
        for marker in [
            "privacy-sample-content",
            "editor-fixture",
            "transform-fixture",
        ] {
            assert_eq!(
                text.contains(marker),
                !masking,
                "mask={masking} marker={marker}: {text}"
            );
        }
        assert_eq!(text.contains("二维码"), !masking, "{text}");
        output.textures_delta.clear();
    }
}

#[test]
fn file_uris_escape_reserved_characters_and_keep_path_separators() {
    assert_eq!(
        path_to_file_uri("/tmp/a #1%?.png"),
        "file:///tmp/a%20%231%25%3F.png"
    );
    assert_eq!(
        path_to_file_uri(r"C:\Images\a #1%?.png"),
        "file:///C:/Images/a%20%231%25%3F.png"
    );
}

#[test]
fn high_density_qr_preview_uses_constant_shapes_per_frame() {
    let ctx = egui::Context::default();
    let pixels = qr_pixels(177, &vec![true; 177 * 177]).unwrap();
    let texture = ctx.load_texture("dense-qr-test", pixels, egui::TextureOptions::NEAREST);
    let start = Instant::now();
    for _ in 0..32 {
        let mut output = ctx.run_ui(egui::RawInput::default(), |ui| {
            draw_qr_square(ui, 180.0, &texture);
        });
        assert!(
            output.shapes.len() <= 8,
            "unexpected QR shape growth: {}",
            output.shapes.len()
        );
        output.textures_delta.clear();
    }
    println!(
        "Dense QR preview: {:.3} ms/frame (32 headless frames)",
        start.elapsed().as_secs_f64() * 1000.0 / 32.0
    );
}

#[test]
fn footer_fits_within_launcher_width() {
    let ctx = egui::Context::default();
    let palette = EarthPalette::resolve(crate::model::ThemeMode::Light, false);
    palette.apply_to_ctx(&ctx);

    let mut settings = AppSettings::default();
    let collections = vec![
        Collection {
            id: 1,
            name: "常用指令".into(),
            color_hex: "#456EFD".into(),
            created_at: 0,
        },
        Collection {
            id: 2,
            name: "代码片段".into(),
            color_hex: "#22C55E".into(),
            created_at: 0,
        },
    ];
    let results: Vec<SearchResultItem> = (1..=10)
        .map(|id| SearchResultItem {
            item: ClipItem {
                id,
                content_hash: format!("h{id}"),
                kind: ClipKind::Text,
                text_content: Some("https://www.behance.net/gallery/218352145".into()),
                html_content: None,
                rtf_content: None,
                image_path: None,
                thumb_path: None,
                image_width: None,
                image_height: None,
                files: Vec::new(),
                byte_size: 128,
                source_app: Some("Safari浏览器".into()),
                source_bundle_id: None,
                pinned: false,
                collection_id: None,
                first_copied_at: 0,
                last_copied_at: 0,
                copy_count: 1,
                sensitive_label: None,
                detected_color: None,
                is_url: true,
                note: None,
                text_truncated: false,
                has_rich_text: false,
            }
            .into(),
            score: 100,
            matched_indices: MatchMask::default(),
            display_text: None,
        })
        .collect();
    let mut state = PopupState::default();

    if std::path::Path::new("/Library/Fonts/Arial Unicode.ttf").exists()
        && let Ok(bytes) = std::fs::read("/Library/Fonts/Arial Unicode.ttf")
    {
        let mut fonts = egui::FontDefinitions::default();
        fonts.font_data.insert(
            "arial_unicode".into(),
            egui::FontData::from_owned(bytes).into(),
        );
        if let Some(vec) = fonts.families.get_mut(&egui::FontFamily::Proportional) {
            vec.insert(0, "arial_unicode".into());
        }
        ctx.set_fonts(fonts);
    }

    let raw = egui::RawInput {
        screen_rect: Some(egui::Rect::from_min_size(
            egui::pos2(0.0, 0.0),
            vec2(424.0, 520.0),
        )),
        ..Default::default()
    };

    let mut min_r = egui::Rect::NOTHING;
    let mut max_shape_right = 0.0f32;
    for _ in 0..2 {
        let mut out = ctx.run_ui(raw.clone(), |ui| {
            let ctx = ui.ctx().clone();
            #[allow(deprecated)]
            egui::CentralPanel::default()
                .frame(egui::Frame::NONE)
                .show_inside(ui, |ui| {
                    let panel_rect = ui.max_rect();
                    let outer_rect = panel_rect.shrink(1.0);
                    let content_rect = outer_rect.shrink(12.0);
                    let resp = ui.scope_builder(
                        egui::UiBuilder::new()
                            .max_rect(content_rect)
                            .layout(Layout::top_down(Align::Min)),
                        |ui| {
                            ui.set_min_size(content_rect.size());
                            ui.set_max_size(content_rect.size());
                            let _ = render_popup_view(
                                ui,
                                &ctx,
                                PopupViewData {
                                    settings: &mut settings,
                                    results: &results,
                                    collections: &collections,
                                    palette: &palette,
                                    storage_bytes: 109_500,
                                    total_items: 53,
                                    has_accessibility: true,
                                    load_full_item: None,
                                },
                                &mut state,
                            );
                        },
                    );
                    min_r = resp.response.rect;
                });
        });
        max_shape_right = out
            .shapes
            .iter()
            .map(|cs| cs.shape.visual_bounding_rect().right())
            .filter(|r| r.is_finite())
            .fold(0.0f32, f32::max);
        out.textures_delta.clear();
    }
    assert!(
        min_r.right() <= 411.5 && max_shape_right <= 411.5,
        "popup content exceeded right edge: min_r={min_r:?} max_shape_right={max_shape_right}"
    );
}

#[test]
fn preview_cache_rebuilds_for_same_length_edits_without_copying_source_payload() {
    let original = Arc::new(safety_test_item());
    let mut state = PopupState::default();
    let cached = state.ensure_preview_cache(&original);
    assert!(Arc::ptr_eq(&cached.source, &original));
    let mut edited = (*original).clone();
    edited.text_content = Some("revised-sample-content".into());
    edited.content_hash = "different-content".into();
    assert_eq!(
        edited.text_content.as_ref().unwrap().len(),
        original.text_content.as_ref().unwrap().len()
    );
    assert_eq!(edited.byte_size, original.byte_size);
    assert_eq!(edited.last_copied_at, original.last_copied_at);
    let edited = Arc::new(edited);
    let cached = state.ensure_preview_cache(&edited);
    assert_eq!(cached.display_text, "revised-sample-content");
    assert!(Arc::ptr_eq(&cached.source, &edited));
    assert!(cached.qr_matrix.is_none());
}

#[test]
fn workbench_preview_caches_and_renders_large_item_without_lag() {
    let ctx = egui::Context::default();
    let palette = EarthPalette::resolve(crate::model::ThemeMode::Light, false);
    palette.apply_to_ctx(&ctx);

    let collections = Vec::new();
    let mut settings = AppSettings::default();
    let large_text = "{\"key\":\"value\",\"list\":[1,2,3]}\n".repeat(4000);
    let results = vec![SearchResultItem {
        item: ClipItem {
            id: 1,
            content_hash: "h1".into(),
            kind: ClipKind::Text,
            text_content: Some(large_text),
            html_content: None,
            rtf_content: None,
            image_path: None,
            thumb_path: None,
            image_width: None,
            image_height: None,
            files: Vec::new(),
            byte_size: 120_000,
            source_app: Some("Safari".into()),
            source_bundle_id: None,
            pinned: false,
            collection_id: None,
            first_copied_at: 1_700_000_000_000,
            last_copied_at: 1_700_000_000_000,
            copy_count: 1,
            sensitive_label: None,
            detected_color: None,
            is_url: false,
            note: None,
            text_truncated: false,
            has_rich_text: false,
        }
        .into(),
        score: 100,
        matched_indices: MatchMask::default(),
        display_text: None,
    }];

    let mut state = PopupState {
        selected_id: Some(1),
        preview_open: true,
        ..Default::default()
    };

    let raw = egui::RawInput {
        screen_rect: Some(egui::Rect::from_min_size(
            egui::pos2(0.0, 0.0),
            vec2(760.0, 520.0),
        )),
        ..Default::default()
    };

    for _ in 0..2 {
        let mut out = ctx.run_ui(raw.clone(), |ui| {
            let ctx = ui.ctx().clone();
            #[allow(deprecated)]
            egui::CentralPanel::default()
                .frame(egui::Frame::NONE)
                .show_inside(ui, |ui| {
                    let content_rect = ui.max_rect().shrink(13.0);
                    ui.scope_builder(
                        egui::UiBuilder::new()
                            .max_rect(content_rect)
                            .layout(Layout::top_down(Align::Min)),
                        |ui| {
                            ui.set_min_size(content_rect.size());
                            ui.set_max_size(content_rect.size());
                            let _ = render_popup_view(
                                ui,
                                &ctx,
                                PopupViewData {
                                    settings: &mut settings,
                                    results: &results,
                                    collections: &collections,
                                    palette: &palette,
                                    storage_bytes: 120_000,
                                    total_items: 1,
                                    has_accessibility: true,
                                    load_full_item: None,
                                },
                                &mut state,
                            );
                        },
                    );
                });
        });
        out.textures_delta.clear();
    }

    let cache = state
        .preview_cache
        .as_ref()
        .expect("preview cache populated");
    assert_eq!(cache.source.id, 1);
    assert!(cache.display_text.chars().count() < 4200);
}

#[test]
fn search_bar_hint_vertically_centered_and_mode_right_aligned() {
    let ctx = egui::Context::default();
    let palette = EarthPalette::resolve(crate::model::ThemeMode::Light, false);
    palette.apply_to_ctx(&ctx);

    if std::path::Path::new("/Library/Fonts/Arial Unicode.ttf").exists()
        && let Ok(bytes) = std::fs::read("/Library/Fonts/Arial Unicode.ttf")
    {
        let mut fonts = egui::FontDefinitions::default();
        fonts.font_data.insert(
            "arial_unicode".into(),
            egui::FontData::from_owned(bytes).into(),
        );
        if let Some(vec) = fonts.families.get_mut(&egui::FontFamily::Proportional) {
            vec.insert(0, "arial_unicode".into());
        }
        ctx.set_fonts(fonts);
    }

    let mut settings = AppSettings::default();
    let mut state = PopupState::default();
    let mut actions = Vec::new();

    let raw = egui::RawInput {
        screen_rect: Some(egui::Rect::from_min_size(
            egui::pos2(0.0, 0.0),
            vec2(424.0, 520.0),
        )),
        ..Default::default()
    };

    let mut out = ctx.run_ui(raw.clone(), |ui| {
        #[allow(deprecated)]
        egui::CentralPanel::default()
            .frame(egui::Frame::NONE)
            .show_inside(ui, |ui| {
                let content_rect = ui.max_rect().shrink(13.0);
                ui.scope_builder(
                    egui::UiBuilder::new()
                        .max_rect(content_rect)
                        .layout(Layout::top_down(Align::Min)),
                    |ui| {
                        ui.set_min_size(content_rect.size());
                        ui.set_max_size(content_rect.size());
                        render_header(ui, &mut settings, &mut state, &palette, &mut actions);
                    },
                );
            });
    });
    out.textures_delta.clear();

    let mut search_box_rect = None;
    let mut hint_text_rect = None;
    let mut mode_text_rect = None;
    let mut all_rects = Vec::new();

    for cs in &out.shapes {
        match &cs.shape {
            egui::epaint::Shape::Rect(r) => {
                all_rects.push(r.rect);
                if r.rect.width() > 180.0 && r.rect.height() >= 20.0 && r.rect.height() <= 40.0 {
                    search_box_rect = Some(r.rect);
                }
            }
            egui::epaint::Shape::Text(t) => {
                let txt = t.galley.text();
                let rect = egui::Rect::from_min_size(t.pos, t.galley.size());
                if txt.contains("搜索记录") {
                    hint_text_rect = Some(rect);
                } else if txt.contains("模糊") {
                    mode_text_rect = Some(rect);
                }
            }
            _ => {}
        }
    }

    let sb = search_box_rect.expect("search_box_rect found");
    let ht = hint_text_rect.expect("hint_text_rect found");
    let mt = mode_text_rect.expect("mode_text_rect found");
    let mp = all_rects
        .into_iter()
        .filter(|r| r.contains(mt.center()) && r.width() < sb.width() - 10.0 && r.height() >= 12.0)
        .min_by(|a, b| a.area().total_cmp(&b.area()))
        .expect("mode_pill_rect found");

    println!(
        "search_box={sb:?} center_y={:.2} | hint={ht:?} center_y={:.2} | mode_pill={mp:?} center_y={:.2} right_gap={:.2} | mode_text={mt:?} center_y={:.2}",
        sb.center().y,
        ht.center().y,
        mp.center().y,
        sb.right() - mp.right(),
        mt.center().y
    );

    assert!(
        sb.left() <= 16.0,
        "search box should start flush at left edge without top-left emblem: sb.left()={}",
        sb.left()
    );
    assert!(
        (sb.center().y - ht.center().y).abs() <= 1.5,
        "hint text not vertically centered: box_cy={} hint_cy={}",
        sb.center().y,
        ht.center().y
    );
    assert!(
        sb.right() - mp.right() <= 14.0,
        "mode pill not right aligned: sb.right()={} mp.right()={}",
        sb.right(),
        mp.right()
    );
}

#[test]
fn selected_row_bar_vertically_centered_and_hover_uses_pointing_hand() {
    let ctx = egui::Context::default();
    let palette = EarthPalette::resolve(crate::model::ThemeMode::Light, false);
    palette.apply_to_ctx(&ctx);

    let settings = AppSettings::default();
    let collections = Vec::new();
    let res = SearchResultItem {
        item: ClipItem {
            id: 1,
            content_hash: "h1".into(),
            kind: ClipKind::Text,
            text_content: Some("物化生缺，语文，数学，政治不缺".into()),
            html_content: None,
            rtf_content: None,
            image_path: None,
            thumb_path: None,
            image_width: None,
            image_height: None,
            files: Vec::new(),
            byte_size: 48,
            source_app: Some("微信".into()),
            source_bundle_id: None,
            pinned: false,
            collection_id: None,
            first_copied_at: 0,
            last_copied_at: 0,
            copy_count: 1,
            sensitive_label: None,
            detected_color: None,
            is_url: false,
            note: None,
            text_truncated: false,
            has_rich_text: false,
        }
        .into(),
        score: 100,
        matched_indices: MatchMask::default(),
        display_text: None,
    };
    let mut state = PopupState {
        selected_id: Some(1),
        ..Default::default()
    };
    let mut actions = Vec::new();

    // Hover directly over the text label area of the row
    let raw = egui::RawInput {
        screen_rect: Some(egui::Rect::from_min_size(
            egui::pos2(0.0, 0.0),
            vec2(424.0, 520.0),
        )),
        events: vec![egui::Event::PointerMoved(egui::pos2(120.0, 26.0))],
        ..Default::default()
    };

    let mut row_rect = egui::Rect::NOTHING;
    let mut out = ctx.run_ui(raw, |ui| {
        #[allow(deprecated)]
        egui::CentralPanel::default()
            .frame(egui::Frame::NONE)
            .show_inside(ui, |ui| {
                let content_rect =
                    egui::Rect::from_min_size(egui::pos2(13.0, 10.0), vec2(398.0, 40.0));
                ui.scope_builder(
                    egui::UiBuilder::new()
                        .max_rect(content_rect)
                        .layout(Layout::top_down(Align::Min)),
                    |ui| {
                        ui.set_min_size(content_rect.size());
                        ui.set_max_size(content_rect.size());
                        let resp = render_item_row(
                            ui,
                            &res,
                            Some(1),
                            true,
                            &settings,
                            &collections,
                            &mut state,
                            &palette,
                            &mut actions,
                        );
                        row_rect = resp.rect;
                    },
                );
            });
    });
    out.textures_delta.clear();

    let mut bar_rect = None;
    for cs in &out.shapes {
        if let egui::epaint::Shape::Rect(r) = &cs.shape
            && (r.rect.width() - 3.5).abs() < 0.2
            && r.rect.height() >= 15.0
        {
            bar_rect = Some(r.rect);
        }
    }

    let br = bar_rect.expect("selection bar found");
    assert!(
        (row_rect.center().y - br.center().y).abs() < 0.5,
        "selection bar not vertically centered: row_cy={} bar_cy={}",
        row_rect.center().y,
        br.center().y
    );
    assert_eq!(
        out.platform_output.cursor_icon,
        egui::CursorIcon::PointingHand,
        "expected PointingHand cursor on row hover, got {:?}",
        out.platform_output.cursor_icon
    );
}

#[test]
fn single_click_vs_double_click_copy_mode_avoids_workbench_conflict() {
    let ctx = egui::Context::default();
    let palette = EarthPalette::resolve(crate::model::ThemeMode::Light, false);
    palette.apply_to_ctx(&ctx);

    let mut settings = AppSettings {
        copy_trigger_mode: CopyTriggerMode::SingleClick,
        ..Default::default()
    };
    let collections = Vec::new();
    let res = SearchResultItem {
        item: ClipItem {
            id: 2,
            content_hash: "h2".into(),
            kind: ClipKind::Text,
            text_content: Some("single click test".into()),
            html_content: None,
            rtf_content: None,
            image_path: None,
            thumb_path: None,
            image_width: None,
            image_height: None,
            files: Vec::new(),
            byte_size: 17,
            source_app: None,
            source_bundle_id: None,
            pinned: false,
            collection_id: None,
            first_copied_at: 0,
            last_copied_at: 0,
            copy_count: 1,
            sensitive_label: None,
            detected_color: None,
            is_url: false,
            note: None,
            text_truncated: false,
            has_rich_text: false,
        }
        .into(),
        score: 100,
        matched_indices: MatchMask::default(),
        display_text: None,
    };

    let mut sim_time = 10.0f64;
    let mut run_click = |ctx: &egui::Context,
                         settings: &AppSettings,
                         state: &mut PopupState,
                         actions: &mut Vec<PopupAction>| {
        sim_time += 1.0;
        let pos = egui::pos2(100.0, 24.0);
        for (dt, events) in [
            (0.0, vec![egui::Event::PointerMoved(pos)]),
            (
                0.02,
                vec![egui::Event::PointerButton {
                    pos,
                    button: egui::PointerButton::Primary,
                    pressed: true,
                    modifiers: Modifiers::NONE,
                }],
            ),
            (
                0.04,
                vec![egui::Event::PointerButton {
                    pos,
                    button: egui::PointerButton::Primary,
                    pressed: false,
                    modifiers: Modifiers::NONE,
                }],
            ),
        ] {
            let raw = egui::RawInput {
                time: Some(sim_time + dt),
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::pos2(0.0, 0.0),
                    vec2(424.0, 520.0),
                )),
                events,
                ..Default::default()
            };
            let mut out = ctx.run_ui(raw, |ui| {
                #[allow(deprecated)]
                egui::CentralPanel::default()
                    .frame(egui::Frame::NONE)
                    .show_inside(ui, |ui| {
                        let content_rect =
                            egui::Rect::from_min_size(egui::pos2(13.0, 10.0), vec2(398.0, 40.0));
                        ui.scope_builder(
                            egui::UiBuilder::new()
                                .max_rect(content_rect)
                                .layout(Layout::top_down(Align::Min)),
                            |ui| {
                                ui.set_min_size(content_rect.size());
                                ui.set_max_size(content_rect.size());
                                let selected = state.selected_id == Some(res.item.id);
                                let _ = render_item_row(
                                    ui,
                                    &res,
                                    Some(1),
                                    selected,
                                    settings,
                                    &collections,
                                    state,
                                    &palette,
                                    actions,
                                );
                            },
                        );
                    });
            });
            out.textures_delta.clear();
        }
    };

    // 1. SingleClick mode with workbench closed: click copies immediately without opening workbench
    let mut state = PopupState {
        selected_id: Some(1),
        preview_open: false,
        ..Default::default()
    };
    let mut actions = Vec::new();
    run_click(&ctx, &settings, &mut state, &mut actions);
    assert!(
        !state.preview_open,
        "SingleClick must not auto-open workbench"
    );
    assert!(
        actions
            .iter()
            .any(|a| matches!(a, PopupAction::PasteItem { id: 2, .. })),
        "SingleClick with closed workbench should emit PasteItem"
    );

    // 2. SingleClick mode with workbench already open:
    //    First click on unselected item switches preview; second click copies.
    let mut state = PopupState {
        selected_id: Some(1),
        preview_open: true,
        ..Default::default()
    };
    let mut actions = Vec::new();
    run_click(&ctx, &settings, &mut state, &mut actions);
    assert_eq!(state.selected_id, Some(2));
    assert!(
        actions.is_empty(),
        "clicking unselected row while workbench open should switch preview first"
    );
    run_click(&ctx, &settings, &mut state, &mut actions);
    assert!(
        actions
            .iter()
            .any(|a| matches!(a, PopupAction::PasteItem { id: 2, .. })),
        "clicking already-selected row while workbench open should emit PasteItem"
    );

    // 3. DoubleClick mode: single click opens workbench without copying
    settings.copy_trigger_mode = CopyTriggerMode::DoubleClick;
    let mut state = PopupState {
        selected_id: Some(1),
        preview_open: false,
        ..Default::default()
    };
    let mut actions = Vec::new();
    run_click(&ctx, &settings, &mut state, &mut actions);
    assert!(
        state.preview_open,
        "DoubleClick mode single click should open workbench"
    );
    assert!(
        actions.is_empty(),
        "DoubleClick mode single click should not copy"
    );
}

#[test]
fn qr_preview_hides_duplicate_text_below() {
    let ctx = egui::Context::default();
    let palette = EarthPalette::resolve(crate::model::ThemeMode::Light, false);
    palette.apply_to_ctx(&ctx);

    let collections = Vec::new();
    let results = [SearchResultItem {
        item: ClipItem {
            id: 42,
            content_hash: "h42".into(),
            kind: ClipKind::Text,
            text_content: Some("UNIQUE_QR_PREVIEW_TEXT_PAYLOAD".into()),
            html_content: None,
            rtf_content: None,
            image_path: None,
            thumb_path: None,
            image_width: None,
            image_height: None,
            files: Vec::new(),
            byte_size: 30,
            source_app: None,
            source_bundle_id: None,
            pinned: false,
            collection_id: None,
            first_copied_at: 0,
            last_copied_at: 0,
            copy_count: 1,
            sensitive_label: None,
            detected_color: None,
            is_url: false,
            note: None,
            text_truncated: false,
            has_rich_text: false,
        }
        .into(),
        score: 100,
        matched_indices: MatchMask::default(),
        display_text: None,
    }];

    let run_workbench = |state: &mut PopupState| -> (Vec<String>, Vec<egui::Rect>) {
        let raw = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::pos2(0.0, 0.0),
                vec2(760.0, 520.0),
            )),
            ..Default::default()
        };
        let mut texts = Vec::new();
        let mut rects = Vec::new();
        for _ in 0..2 {
            texts.clear();
            rects.clear();
            let mut out = ctx.run_ui(raw.clone(), |ui| {
                #[allow(deprecated)]
                egui::CentralPanel::default()
                    .frame(egui::Frame::NONE)
                    .show_inside(ui, |ui| {
                        let r_rect =
                            egui::Rect::from_min_size(egui::pos2(330.0, 60.0), vec2(410.0, 400.0));
                        ui.scope_builder(
                            egui::UiBuilder::new()
                                .max_rect(r_rect)
                                .layout(Layout::top_down(Align::Min)),
                            |ui| {
                                ui.set_min_size(r_rect.size());
                                ui.set_max_size(r_rect.size());
                                let mut actions = Vec::new();
                                render_preview_pane(
                                    ui,
                                    Some(&results[0].item),
                                    None,
                                    true,
                                    &collections,
                                    state,
                                    &palette,
                                    &mut actions,
                                );
                            },
                        );
                    });
            });
            for cs in &out.shapes {
                match &cs.shape {
                    egui::epaint::Shape::Text(t) => {
                        texts.push(t.galley.text().to_string());
                    }
                    egui::epaint::Shape::Rect(r) => {
                        rects.push(r.rect);
                    }
                    _ => {}
                }
            }
            out.textures_delta.clear();
        }
        (texts, rects)
    };

    let mut state_text = PopupState {
        selected_id: Some(42),
        preview_open: true,
        ..Default::default()
    };
    let (texts_without_qr, _) = run_workbench(&mut state_text);
    assert!(
        texts_without_qr
            .iter()
            .any(|t| t.contains("UNIQUE_QR_PREVIEW_TEXT_PAYLOAD")),
        "expected text body when QR preview is closed"
    );

    let mut state_qr = PopupState {
        selected_id: Some(42),
        preview_open: true,
        qr_preview_id: Some(42),
        ..Default::default()
    };
    let (texts_with_qr, rects_with_qr) = run_workbench(&mut state_qr);
    assert!(
        texts_with_qr.iter().any(|t| t.contains("复制二维码图片")),
        "expected QR action button when QR preview is active"
    );
    assert!(
        !texts_with_qr
            .iter()
            .any(|t| t.contains("UNIQUE_QR_PREVIEW_TEXT_PAYLOAD")),
        "text body should be hidden when QR preview is active, got: {texts_with_qr:?}"
    );

    let qr_card = rects_with_qr
        .iter()
        .find(|r| r.width() > 370.0 && r.height() > 140.0 && r.height() < 300.0)
        .copied()
        .expect("full-width QR card rect found");
    let qr_square = rects_with_qr
        .iter()
        .find(|r| {
            r.width() > 90.0
                && r.width() < 200.0
                && (r.width() - r.height()).abs() < 1.5
                && qr_card.contains(r.center())
        })
        .copied()
        .expect("QR square rect found");
    assert!(
        (qr_card.center().x - qr_square.center().x).abs() < 3.0,
        "QR square not horizontally centered inside card: card_cx={} qr_cx={}",
        qr_card.center().x,
        qr_square.center().x
    );
}

#[test]
fn image_files_and_text_rows_have_uniform_height_and_loaded_thumbnail() {
    let dir = std::env::temp_dir().join("Application Support Earth Test");
    let _ = std::fs::create_dir_all(&dir);
    let thumb_path = dir.join("thumb test.png");
    let img = image::RgbaImage::from_pixel(64, 64, image::Rgba([69, 110, 253, 255]));
    image::DynamicImage::ImageRgba8(img)
        .write_to(
            &mut std::fs::File::create(&thumb_path).unwrap(),
            image::ImageFormat::Png,
        )
        .unwrap();
    let thumb_str = thumb_path.to_string_lossy().into_owned();

    let ctx = egui::Context::default();
    egui_extras::install_image_loaders(&ctx);
    let palette = EarthPalette::resolve(crate::model::ThemeMode::Light, false);
    palette.apply_to_ctx(&ctx);

    let settings = AppSettings::default();
    let collections = Vec::new();
    let results = [
        SearchResultItem {
            item: ClipItem {
                id: 1,
                content_hash: "img1".into(),
                kind: ClipKind::Image,
                text_content: None,
                html_content: None,
                rtf_content: None,
                image_path: Some(thumb_str.clone()),
                thumb_path: Some(thumb_str.clone()),
                image_width: Some(370),
                image_height: Some(370),
                files: Vec::new(),
                byte_size: 5200,
                source_app: None,
                source_bundle_id: None,
                pinned: false,
                collection_id: None,
                first_copied_at: 0,
                last_copied_at: 0,
                copy_count: 1,
                sensitive_label: None,
                detected_color: None,
                is_url: false,
                note: None,
                text_truncated: false,
                has_rich_text: false,
            }
            .into(),
            score: 100,
            matched_indices: MatchMask::default(),
            display_text: None,
        },
        SearchResultItem {
            item: ClipItem {
                id: 2,
                content_hash: "file2".into(),
                kind: ClipKind::Files,
                text_content: None,
                html_content: None,
                rtf_content: None,
                image_path: None,
                thumb_path: None,
                image_width: None,
                image_height: None,
                files: vec![crate::model::FileEntry {
                    path: "/tmp/demo.png".into(),
                    name: "demo.png".into(),
                    is_dir: false,
                    size_bytes: 1200,
                }],
                byte_size: 1200,
                source_app: None,
                source_bundle_id: None,
                pinned: false,
                collection_id: None,
                first_copied_at: 0,
                last_copied_at: 0,
                copy_count: 1,
                sensitive_label: None,
                detected_color: None,
                is_url: false,
                note: None,
                text_truncated: false,
                has_rich_text: false,
            }
            .into(),
            score: 100,
            matched_indices: MatchMask::default(),
            display_text: None,
        },
        SearchResultItem {
            item: ClipItem {
                id: 3,
                content_hash: "txt3".into(),
                kind: ClipKind::Text,
                text_content: Some("uniform row height text".into()),
                html_content: None,
                rtf_content: None,
                image_path: None,
                thumb_path: None,
                image_width: None,
                image_height: None,
                files: Vec::new(),
                byte_size: 24,
                source_app: None,
                source_bundle_id: None,
                pinned: false,
                collection_id: None,
                first_copied_at: 0,
                last_copied_at: 0,
                copy_count: 1,
                sensitive_label: None,
                detected_color: None,
                is_url: false,
                note: None,
                text_truncated: false,
                has_rich_text: false,
            }
            .into(),
            score: 100,
            matched_indices: MatchMask::default(),
            display_text: None,
        },
    ];

    let mut state = PopupState {
        selected_id: Some(1),
        ..Default::default()
    };
    let mut actions = Vec::new();
    let raw = egui::RawInput {
        screen_rect: Some(egui::Rect::from_min_size(
            egui::pos2(0.0, 0.0),
            vec2(424.0, 520.0),
        )),
        ..Default::default()
    };

    let mut row_heights = [0.0f32; 3];
    let mut saw_loaded_image = false;
    let mut shapes_last = Vec::new();
    for _ in 0..20 {
        let mut out = ctx.run_ui(raw.clone(), |ui| {
            #[allow(deprecated)]
            egui::CentralPanel::default()
                .frame(egui::Frame::NONE)
                .show_inside(ui, |ui| {
                    let content_rect =
                        egui::Rect::from_min_size(egui::pos2(13.0, 60.0), vec2(398.0, 380.0));
                    ui.scope_builder(
                        egui::UiBuilder::new()
                            .max_rect(content_rect)
                            .layout(Layout::top_down(Align::Min)),
                        |ui| {
                            ui.set_min_size(content_rect.size());
                            ui.set_max_size(content_rect.size());
                            for (i, res) in results.iter().enumerate() {
                                let resp = render_item_row(
                                    ui,
                                    res,
                                    Some(i + 1),
                                    i == 0,
                                    &settings,
                                    &collections,
                                    &mut state,
                                    &palette,
                                    &mut actions,
                                );
                                row_heights[i] = resp.rect.height();
                            }
                        },
                    );
                });
        });
        shapes_last = out.shapes.clone();
        if shapes_last.iter().any(|cs| match &cs.shape {
            egui::epaint::Shape::Rect(r) => r.fill_texture_id() != egui::TextureId::default(),
            egui::epaint::Shape::Mesh(m) => m.texture_id != egui::TextureId::default(),
            _ => false,
        }) {
            saw_loaded_image = true;
        }
        out.textures_delta.clear();
        if saw_loaded_image {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(20));
    }

    let _ = std::fs::remove_dir_all(&dir);

    for (idx, h) in row_heights.iter().enumerate() {
        assert!(
            (*h - LIST_ROW_HEIGHT).abs() < 0.5,
            "row {idx} height {h} != uniform {LIST_ROW_HEIGHT}: {row_heights:?}"
        );
    }
    assert!(
        saw_loaded_image,
        "expected file:// thumbnail PNG with space in path to render texture, shapes: {:?}",
        shapes_last
            .iter()
            .filter_map(|cs| match &cs.shape {
                egui::epaint::Shape::Text(t) => Some(t.galley.text().to_string()),
                _ => None,
            })
            .collect::<Vec<_>>()
    );

    let keycap_heights: Vec<f32> = shapes_last
        .iter()
        .filter_map(|cs| match &cs.shape {
            egui::epaint::Shape::Rect(r)
                if r.fill != Color32::TRANSPARENT
                    && r.rect.width() >= 24.0
                    && r.rect.width() <= 46.0
                    && r.rect.left() > 350.0 =>
            {
                Some(r.rect.height())
            }
            _ => None,
        })
        .collect();
    assert_eq!(
        keycap_heights.len(),
        3,
        "expected 3 keycap pills, got {keycap_heights:?}"
    );
    for kh in &keycap_heights {
        assert!(
            *kh <= 24.0,
            "keycap pill stretched too tall: {kh} in {keycap_heights:?}"
        );
    }
}

#[test]
fn popup_can_be_dragged_from_rows_footer_and_empty_space_but_not_search_text_edit() {
    let ctx = egui::Context::default();
    let palette = EarthPalette::resolve(crate::model::ThemeMode::Light, false);
    palette.apply_to_ctx(&ctx);

    let mut settings = AppSettings {
        copy_trigger_mode: CopyTriggerMode::DoubleClick,
        ..Default::default()
    };
    let collections = Vec::new();
    let results = vec![SearchResultItem {
        item: ClipItem {
            id: 1,
            content_hash: "h1".into(),
            kind: ClipKind::Text,
            text_content: Some("drag test clipboard record".into()),
            html_content: None,
            rtf_content: None,
            image_path: None,
            thumb_path: None,
            image_width: None,
            image_height: None,
            files: Vec::new(),
            byte_size: 25,
            source_app: Some("Notes".into()),
            source_bundle_id: None,
            pinned: false,
            collection_id: None,
            first_copied_at: 0,
            last_copied_at: 0,
            copy_count: 1,
            note: None,
            text_truncated: false,
            has_rich_text: false,
            is_url: false,
            detected_color: None,
            sensitive_label: None,
        }
        .into(),
        score: 0,
        matched_indices: MatchMask::default(),
        display_text: None,
    }];
    let mut state = PopupState {
        search_query: "drag".into(),
        ..Default::default()
    };

    let mut sim_time = 20.0f64;
    let mut run_pointer = |ctx: &egui::Context,
                           settings: &mut AppSettings,
                           state: &mut PopupState,
                           start: egui::Pos2,
                           end: egui::Pos2|
     -> (bool, Vec<PopupAction>) {
        let mut dragged = false;
        let mut collected_actions = Vec::new();
        for (dt, events) in [
            (0.0, vec![egui::Event::PointerMoved(start)]),
            (
                0.02,
                vec![egui::Event::PointerButton {
                    pos: start,
                    button: egui::PointerButton::Primary,
                    pressed: true,
                    modifiers: Modifiers::NONE,
                }],
            ),
            (0.04, vec![egui::Event::PointerMoved(end)]),
            (
                0.06,
                vec![egui::Event::PointerButton {
                    pos: end,
                    button: egui::PointerButton::Primary,
                    pressed: false,
                    modifiers: Modifiers::NONE,
                }],
            ),
        ] {
            let raw = egui::RawInput {
                time: Some(sim_time + dt),
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::pos2(0.0, 0.0),
                    vec2(424.0, 520.0),
                )),
                events,
                ..Default::default()
            };
            let mut out = ctx.run_ui(raw, |ui| {
                #[allow(deprecated)]
                egui::CentralPanel::default()
                    .frame(egui::Frame::NONE)
                    .show_inside(ui, |ui| {
                        let panel_rect = ctx.content_rect();
                        crate::app::handle_background_window_drag(
                            ui,
                            ctx,
                            panel_rect,
                            "test_popup_drag_bg",
                        );
                        let content_rect = panel_rect.shrink(13.0);
                        ui.scope_builder(
                            egui::UiBuilder::new()
                                .max_rect(content_rect)
                                .layout(Layout::top_down(Align::Min)),
                            |ui| {
                                ui.set_min_size(content_rect.size());
                                ui.set_max_size(content_rect.size());
                                let actions = render_popup_view(
                                    ui,
                                    ctx,
                                    PopupViewData {
                                        settings,
                                        results: &results,
                                        collections: &collections,
                                        palette: &palette,
                                        storage_bytes: 25,
                                        total_items: 1,
                                        has_accessibility: true,
                                        load_full_item: None,
                                    },
                                    state,
                                );
                                collected_actions.extend(actions);
                            },
                        );
                    });
            });
            if let Some(vp) = out.viewport_output.get(&egui::ViewportId::ROOT)
                && vp
                    .commands
                    .iter()
                    .any(|cmd| matches!(cmd, egui::ViewportCommand::StartDrag))
            {
                dragged = true;
            }
            out.textures_delta.clear();
        }
        sim_time += 1.0;
        (dragged, collected_actions)
    };

    // Prime first frame so layout rects exist.
    let _ = run_pointer(
        &ctx,
        &mut settings,
        &mut state,
        egui::pos2(418.0, 418.0),
        egui::pos2(418.0, 418.0),
    );

    // 1. Drag inside search input box keeps text selection (no window StartDrag).
    let (drag_search, _) = run_pointer(
        &ctx,
        &mut settings,
        &mut state,
        egui::pos2(120.0, 26.0),
        egui::pos2(160.0, 26.0),
    );
    assert!(
        !drag_search,
        "search TextEdit should keep text selection drag"
    );

    // 2. Dragging starting on a clipboard row drags window without opening preview or pasting.
    let (drag_row, row_actions) = run_pointer(
        &ctx,
        &mut settings,
        &mut state,
        egui::pos2(120.0, 100.0),
        egui::pos2(160.0, 120.0),
    );
    assert!(drag_row, "dragging on a clipboard row should drag window");
    assert!(!state.preview_open, "dragging row must not open workbench");
    assert!(
        row_actions.is_empty(),
        "dragging row must not paste or run actions"
    );

    // 3. Dragging on the footer bar / stats label drags window.
    let (drag_footer, footer_actions) = run_pointer(
        &ctx,
        &mut settings,
        &mut state,
        egui::pos2(50.0, 492.0),
        egui::pos2(90.0, 492.0),
    );
    assert!(drag_footer, "dragging on footer area should drag window");
    assert!(footer_actions.is_empty());

    // 4. Plain click on the clipboard row (without dragging) still opens workbench.
    let (click_row_drag, _) = run_pointer(
        &ctx,
        &mut settings,
        &mut state,
        egui::pos2(120.0, 100.0),
        egui::pos2(121.0, 100.0),
    );
    assert!(!click_row_drag, "clicking row must not emit StartDrag");
    assert!(
        state.preview_open,
        "clicking row should still open workbench"
    );
}

#[test]
fn workbench_renders_on_left_when_flipped_and_stays_uncramped_mid_animation() {
    let ctx = egui::Context::default();
    let palette = EarthPalette::resolve(crate::model::ThemeMode::Light, false);
    palette.apply_to_ctx(&ctx);

    let mut settings = AppSettings::default();
    let collections = Vec::new();
    let results = vec![SearchResultItem {
        item: ClipItem {
            id: 7,
            content_hash: "left7".into(),
            kind: ClipKind::Text,
            text_content: Some("UNIQUE_LEFT_WORKBENCH_PAYLOAD".into()),
            html_content: None,
            rtf_content: None,
            image_path: None,
            thumb_path: None,
            image_width: None,
            image_height: None,
            files: Vec::new(),
            byte_size: 29,
            source_app: Some("Terminal".into()),
            source_bundle_id: None,
            pinned: false,
            collection_id: None,
            first_copied_at: 0,
            last_copied_at: 0,
            copy_count: 1,
            note: None,
            text_truncated: false,
            has_rich_text: false,
            is_url: false,
            detected_color: None,
            sensitive_label: None,
        }
        .into(),
        score: 0,
        matched_indices: MatchMask::default(),
        display_text: None,
    }];

    let mut render_at_width = |card_w: f32, reveal: Option<f32>| -> Vec<(String, f32)> {
        let mut state = PopupState {
            selected_id: Some(7),
            preview_open: true,
            workbench_side: WorkbenchSide::Left,
            preview_reveal: reveal,
            ..Default::default()
        };
        let raw = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::pos2(0.0, 0.0),
                vec2(card_w, 520.0),
            )),
            ..Default::default()
        };
        let mut out = ctx.run_ui(raw, |ui| {
            #[allow(deprecated)]
            egui::CentralPanel::default()
                .frame(egui::Frame::NONE)
                .show_inside(ui, |ui| {
                    let content_rect = ui.ctx().content_rect().shrink(13.0);
                    ui.scope_builder(
                        egui::UiBuilder::new()
                            .max_rect(content_rect)
                            .layout(Layout::top_down(Align::Min)),
                        |ui| {
                            ui.set_min_size(content_rect.size());
                            ui.set_max_size(content_rect.size());
                            let _ = render_popup_view(
                                ui,
                                &ctx,
                                PopupViewData {
                                    settings: &mut settings,
                                    results: &results,
                                    collections: &collections,
                                    palette: &palette,
                                    storage_bytes: 29,
                                    total_items: 1,
                                    has_accessibility: true,
                                    load_full_item: None,
                                },
                                &mut state,
                            );
                        },
                    );
                });
        });
        let text_positions = out
            .shapes
            .iter()
            .filter_map(|cs| match &cs.shape {
                egui::epaint::Shape::Text(t) => Some((t.galley.text().to_string(), t.pos.x)),
                _ => None,
            })
            .collect();
        out.textures_delta.clear();
        text_positions
    };

    // Full expanded on Left: workbench actions/payload appear cleanly to the left of list items.
    let full_texts = render_at_width(760.0, Some(1.0));
    let wb_x = full_texts
        .iter()
        .find(|(t, _)| t.contains("UNIQUE_LEFT_WORKBENCH_PAYLOAD"))
        .map(|(_, x)| *x)
        .expect("workbench body should render");
    let expected_shortcut = if cfg!(target_os = "macos") {
        "⌘ 1"
    } else {
        "Alt+1"
    };
    let list_x = full_texts
        .iter()
        .find(|(t, _)| t.contains(expected_shortcut))
        .map(|(_, x)| *x)
        .expect("list row shortcut pill should stay visible on right");
    assert!(
        wb_x < 100.0 && list_x > 600.0,
        "expected workbench on left ({wb_x}) and list anchored on right ({list_x})"
    );

    // Mid-animation (590px card, reveal = 0.5): workbench still renders cleanly without wrapping away.
    let mid_texts = render_at_width(590.0, Some(0.5));
    assert!(
        mid_texts
            .iter()
            .any(|(t, _)| t.contains("UNIQUE_LEFT_WORKBENCH_PAYLOAD")),
        "expected workbench content during smooth reveal animation"
    );
}

#[test]
fn closing_left_workbench_preserves_reveal_on_toggle_frame_without_snap() {
    let mut state = PopupState {
        selected_id: Some(7),
        preview_open: true,
        workbench_side: WorkbenchSide::Left,
        preview_reveal: None,
        ..Default::default()
    };

    state.toggle_preview();
    assert!(!state.preview_open);
    assert_eq!(
        state.preview_reveal,
        Some(1.0),
        "closing toggle must keep reveal at 1.0 on frame 0 so the list does not jump full-width"
    );
}

#[test]
fn window_pin_button_toggles_and_prevents_escape_hide() {
    let ctx = egui::Context::default();
    let palette = EarthPalette::resolve(crate::model::ThemeMode::Light, false);
    palette.apply_to_ctx(&ctx);

    let mut settings = AppSettings::default();
    let mut state = PopupState::default();
    let collections = Vec::new();
    let results = Vec::new();

    // Render header once to discover the pin icon rect (5th icon from right edge -> leftmost of 5 icons)
    let raw_init = egui::RawInput {
        screen_rect: Some(egui::Rect::from_min_size(
            egui::pos2(0.0, 0.0),
            vec2(424.0, 520.0),
        )),
        ..Default::default()
    };
    let mut out = ctx.run_ui(raw_init, |ui| {
        #[allow(deprecated)]
        egui::CentralPanel::default()
            .frame(egui::Frame::NONE)
            .show_inside(ui, |ui| {
                let content_rect = ui.max_rect().shrink(13.0);
                ui.scope_builder(
                    egui::UiBuilder::new()
                        .max_rect(content_rect)
                        .layout(Layout::top_down(Align::Min)),
                    |ui| {
                        ui.set_min_size(content_rect.size());
                        ui.set_max_size(content_rect.size());
                        let mut actions = Vec::new();
                        render_header(ui, &mut settings, &mut state, &palette, &mut actions);
                    },
                );
            });
    });
    let mut squircles: Vec<egui::Rect> = out
        .shapes
        .iter()
        .filter_map(|cs| match &cs.shape {
            egui::epaint::Shape::Rect(r)
                if (r.rect.width() - 28.0).abs() < 0.5 && (r.rect.height() - 28.0).abs() < 0.5 =>
            {
                Some(r.rect)
            }
            _ => None,
        })
        .collect();
    squircles.sort_by(|a, b| a.min.x.total_cmp(&b.min.x));
    squircles.dedup_by(|a, b| (a.min.x - b.min.x).abs() < 0.5);
    out.textures_delta.clear();
    assert_eq!(squircles.len(), 5, "expected 5 header icon buttons");
    let pin_center = squircles[0].center();

    // Click the pin button and verify ToggleWindowPin action & state flip
    let mut captured_actions = Vec::new();
    for (time, pressed) in [(0.1, true), (0.14, false)] {
        let raw = egui::RawInput {
            time: Some(time),
            screen_rect: Some(egui::Rect::from_min_size(
                egui::pos2(0.0, 0.0),
                vec2(424.0, 520.0),
            )),
            events: vec![
                egui::Event::PointerMoved(pin_center),
                egui::Event::PointerButton {
                    pos: pin_center,
                    button: egui::PointerButton::Primary,
                    pressed,
                    modifiers: Modifiers::NONE,
                },
            ],
            ..Default::default()
        };
        let mut click_out = ctx.run_ui(raw, |ui| {
            let ctx = ui.ctx().clone();
            #[allow(deprecated)]
            egui::CentralPanel::default()
                .frame(egui::Frame::NONE)
                .show_inside(ui, |ui| {
                    let content_rect = ui.max_rect().shrink(13.0);
                    ui.scope_builder(
                        egui::UiBuilder::new()
                            .max_rect(content_rect)
                            .layout(Layout::top_down(Align::Min)),
                        |ui| {
                            ui.set_min_size(content_rect.size());
                            ui.set_max_size(content_rect.size());
                            captured_actions = render_popup_view(
                                ui,
                                &ctx,
                                PopupViewData {
                                    settings: &mut settings,
                                    results: &results,
                                    collections: &collections,
                                    palette: &palette,
                                    storage_bytes: 0,
                                    total_items: 0,
                                    has_accessibility: true,
                                    load_full_item: None,
                                },
                                &mut state,
                            );
                        },
                    );
                });
        });
        click_out.textures_delta.clear();
    }
    assert!(
        state.window_pinned,
        "clicking header pin button must pin the window"
    );
    assert!(
        captured_actions
            .iter()
            .any(|a| matches!(a, PopupAction::ToggleWindowPin)),
        "clicking pin button should emit PopupAction::ToggleWindowPin"
    );

    // Pressing Esc while pinned with empty search should not emit HidePopup
    let esc_raw = egui::RawInput {
        time: Some(0.25),
        screen_rect: Some(egui::Rect::from_min_size(
            egui::pos2(0.0, 0.0),
            vec2(424.0, 520.0),
        )),
        events: vec![egui::Event::Key {
            key: Key::Escape,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: Modifiers::NONE,
        }],
        ..Default::default()
    };
    let mut esc_actions = Vec::new();
    let mut esc_out = ctx.run_ui(esc_raw, |ui| {
        let ctx = ui.ctx().clone();
        esc_actions = render_popup_view(
            ui,
            &ctx,
            PopupViewData {
                settings: &mut settings,
                results: &results,
                collections: &collections,
                palette: &palette,
                storage_bytes: 0,
                total_items: 0,
                has_accessibility: true,
                load_full_item: None,
            },
            &mut state,
        );
    });
    esc_out.textures_delta.clear();
    assert!(
        !esc_actions
            .iter()
            .any(|a| matches!(a, PopupAction::HidePopup)),
        "Esc should not dismiss window while pinned"
    );
}

#[test]
fn left_and_right_arrows_cycle_categories_with_wraparound() {
    let ctx = egui::Context::default();
    let palette = EarthPalette::resolve(crate::model::ThemeMode::Light, false);
    palette.apply_to_ctx(&ctx);

    let mut settings = AppSettings::default();
    let results = Vec::new();
    let collections = vec![
        Collection {
            id: 10,
            name: "常用指令".into(),
            color_hex: "#456EFD".into(),
            created_at: 0,
        },
        Collection {
            id: 20,
            name: "代码片段".into(),
            color_hex: "#22C55E".into(),
            created_at: 0,
        },
    ];

    let mut state = PopupState::default();
    let mut press_key = |state: &mut PopupState, key: Key, time: f64| {
        let raw = egui::RawInput {
            time: Some(time),
            screen_rect: Some(egui::Rect::from_min_size(
                egui::pos2(0.0, 0.0),
                vec2(424.0, 520.0),
            )),
            events: vec![egui::Event::Key {
                key,
                physical_key: None,
                pressed: true,
                repeat: false,
                modifiers: Modifiers::NONE,
            }],
            ..Default::default()
        };
        let mut out = ctx.run_ui(raw, |ui| {
            let ctx = ui.ctx().clone();
            let _ = render_popup_view(
                ui,
                &ctx,
                PopupViewData {
                    settings: &mut settings,
                    results: &results,
                    collections: &collections,
                    palette: &palette,
                    storage_bytes: 0,
                    total_items: 0,
                    has_accessibility: true,
                    load_full_item: None,
                },
                state,
            );
        });
        out.textures_delta.clear();
    };

    // Right arrow cycles across all built-in + collection filters and wraps back to All.
    let expected_right = [
        CategoryFilter::Text,
        CategoryFilter::Image,
        CategoryFilter::Files,
        CategoryFilter::Pinned,
        CategoryFilter::Collection(10),
        CategoryFilter::Collection(20),
        CategoryFilter::All,
    ];
    for (idx, expected) in expected_right.into_iter().enumerate() {
        press_key(&mut state, Key::ArrowRight, 0.1 + idx as f64 * 0.05);
        assert_eq!(state.filter, expected, "step {idx} on ArrowRight");
    }

    // Left arrow from All wraps directly to the rightmost category (Collection(20)), then steps left.
    press_key(&mut state, Key::ArrowLeft, 1.0);
    assert_eq!(state.filter, CategoryFilter::Collection(20));
    press_key(&mut state, Key::ArrowLeft, 1.05);
    assert_eq!(state.filter, CategoryFilter::Collection(10));
    press_key(&mut state, Key::ArrowLeft, 1.10);
    assert_eq!(state.filter, CategoryFilter::Pinned);

    // When typing non-empty search text in the focused search box, arrows edit text instead of switching tabs.
    state.search_query = "docker".into();
    state.search_focused = true;
    press_key(&mut state, Key::ArrowLeft, 1.20);
    assert_eq!(state.filter, CategoryFilter::Pinned);

    // Once search box loses focus, arrows still switch categories even with non-empty search text.
    state.search_focused = false;
    press_key(&mut state, Key::ArrowRight, 1.30);
    assert_eq!(state.filter, CategoryFilter::Collection(10));
}

#[test]
fn nav_repeat_interval_waits_for_hold_then_accelerates_smoothly() {
    assert!(nav_repeat_interval(Duration::from_millis(0)).is_none());
    assert!(nav_repeat_interval(Duration::from_millis(120)).is_none());
    assert!(nav_repeat_interval(Duration::from_millis(195)).is_none());

    let start = nav_repeat_interval(Duration::from_millis(200)).expect("repeat starts at 200ms");
    let mid = nav_repeat_interval(Duration::from_millis(550)).expect("repeat active at 550ms");
    let fast = nav_repeat_interval(Duration::from_millis(900)).expect("repeat active at 900ms");
    let max_speed =
        nav_repeat_interval(Duration::from_millis(1200)).expect("repeat active at 1200ms");
    let sustained =
        nav_repeat_interval(Duration::from_millis(2500)).expect("repeat active at 2500ms");

    assert!((start.as_secs_f32() * 1000.0 - NAV_HOLD_START_INTERVAL_MS).abs() < 0.5);
    assert!(
        mid < start,
        "mid ({mid:?}) should accelerate past start ({start:?})"
    );
    assert!(
        fast < mid,
        "fast ({fast:?}) should accelerate past mid ({mid:?})"
    );
    assert!(
        max_speed < fast,
        "max_speed ({max_speed:?}) should accelerate past fast ({fast:?})"
    );
    assert!((max_speed.as_secs_f32() * 1000.0 - NAV_HOLD_MIN_INTERVAL_MS).abs() < 0.5);
    assert_eq!(max_speed, sustained);
}

#[test]
fn smooth_scroll_target_uses_lookahead_margin_and_clamps_bounds() {
    // Middle of viewport stays steady without unnecessary scrolling
    assert_eq!(
        compute_smooth_scroll_target(0.0, 120.0, 34.0, 360.0, 1000.0),
        0.0
    );

    // Approaching the bottom edge starts scrolling early with ~1.25-row lookahead margin
    let down_target = compute_smooth_scroll_target(0.0, 312.0, 34.0, 360.0, 1000.0);
    assert!(
        down_target > 30.0 && down_target < 40.0,
        "down_target={down_target}"
    );

    // Approaching the top edge scrolls upward early while clamping cleanly at 0
    let up_target = compute_smooth_scroll_target(200.0, 210.0, 34.0, 360.0, 1000.0);
    assert!(
        up_target < 200.0 && up_target > 150.0,
        "up_target={up_target}"
    );
    assert_eq!(
        compute_smooth_scroll_target(20.0, 0.0, 34.0, 360.0, 1000.0),
        0.0
    );

    // Bottom-most row clamps cleanly at max_scroll
    assert_eq!(
        compute_smooth_scroll_target(800.0, 1326.0, 34.0, 360.0, 1000.0),
        1000.0
    );

    // Degenerate tiny viewport never produces NaN or out-of-bounds target
    let tiny = compute_smooth_scroll_target(0.0, 40.0, 34.0, 60.0, 300.0);
    assert!((0.0..=300.0).contains(&tiny));
}

#[test]
fn holding_down_arrow_accelerates_past_single_step_and_stops_on_release() {
    let ctx = egui::Context::default();
    let palette = EarthPalette::resolve(crate::model::ThemeMode::Dark, true);
    palette.apply_to_ctx(&ctx);

    let mut settings = AppSettings::default();
    let collections = Vec::new();
    let results: Vec<SearchResultItem> = (1..=30)
        .map(|id| SearchResultItem {
            item: ClipItem {
                id,
                content_hash: format!("nav-{id}"),
                kind: ClipKind::Text,
                text_content: Some(format!("剪贴板记录 #{id}")),
                html_content: None,
                rtf_content: None,
                image_path: None,
                thumb_path: None,
                image_width: None,
                image_height: None,
                files: Vec::new(),
                byte_size: 32,
                source_app: Some("Safari".into()),
                source_bundle_id: None,
                pinned: false,
                collection_id: None,
                first_copied_at: 0,
                last_copied_at: 0,
                copy_count: 1,
                sensitive_label: None,
                detected_color: None,
                is_url: false,
                note: None,
                text_truncated: false,
                has_rich_text: false,
            }
            .into(),
            score: 100,
            matched_indices: MatchMask::default(),
            display_text: None,
        })
        .collect();

    let mut state = PopupState::default();
    let mut step_frame = |state: &mut PopupState, time: f64, events: Vec<egui::Event>| {
        let raw = egui::RawInput {
            time: Some(time),
            screen_rect: Some(egui::Rect::from_min_size(
                egui::pos2(0.0, 0.0),
                vec2(424.0, 520.0),
            )),
            events,
            ..Default::default()
        };
        let mut out = ctx.run_ui(raw, |ui| {
            let ctx = ui.ctx().clone();
            let _ = render_popup_view(
                ui,
                &ctx,
                PopupViewData {
                    settings: &mut settings,
                    results: &results,
                    collections: &collections,
                    palette: &palette,
                    storage_bytes: 0,
                    total_items: results.len(),
                    has_accessibility: true,
                    load_full_item: None,
                },
                state,
            );
        });
        out.textures_delta.clear();
    };

    // Initial render selects first item (id = 1)
    step_frame(&mut state, 0.00, Vec::new());
    assert_eq!(state.selected_id, Some(1));

    // Slow tap (press at 0.10, release at 0.18) moves strictly 1 row
    step_frame(
        &mut state,
        0.10,
        vec![egui::Event::Key {
            key: Key::ArrowDown,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: Modifiers::NONE,
        }],
    );
    assert_eq!(state.selected_id, Some(2));
    step_frame(
        &mut state,
        0.18,
        vec![egui::Event::Key {
            key: Key::ArrowDown,
            physical_key: None,
            pressed: false,
            repeat: false,
            modifiers: Modifiers::NONE,
        }],
    );
    assert_eq!(state.selected_id, Some(2));
    assert!(state.nav_hold.is_none());

    // Press and hold ArrowDown starting at t = 0.50: immediate +1 to id = 3
    step_frame(
        &mut state,
        0.50,
        vec![egui::Event::Key {
            key: Key::ArrowDown,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: Modifiers::NONE,
        }],
    );
    assert_eq!(state.selected_id, Some(3));
    assert!(state.nav_hold.is_some());

    // Still inside the 200ms hold threshold (t = 0.62) -> no accidental extra step
    step_frame(&mut state, 0.62, Vec::new());
    assert_eq!(state.selected_id, Some(3));

    // Hold across 60Hz frames until t = 1.40 -> smoothly accelerates through many rows
    let mut t = 0.64;
    while t <= 1.40 {
        step_frame(&mut state, t, Vec::new());
        t += 1.0 / 60.0;
    }
    let after_hold_id = state.selected_id.expect("selected id after hold");
    assert!(
        after_hold_id >= 15,
        "holding ArrowDown for ~0.9s should accelerate smoothly past 15+ rows, got id={after_hold_id}"
    );
    assert!(
        state.list_scroll_y > 0.0 || state.scroll_anim_target.is_some_and(|y| y > 0.0),
        "holding ArrowDown down a long list should trigger smooth vertical scrolling"
    );
    assert!(state.selection_visual_y.is_some());

    // Releasing ArrowDown immediately stops hold progression
    step_frame(
        &mut state,
        1.42,
        vec![egui::Event::Key {
            key: Key::ArrowDown,
            physical_key: None,
            pressed: false,
            repeat: false,
            modifiers: Modifiers::NONE,
        }],
    );
    assert!(state.nav_hold.is_none());
    let stopped_id = state.selected_id;
    step_frame(&mut state, 1.60, Vec::new());
    assert_eq!(state.selected_id, stopped_id);
}

#[test]
fn rgb_color_record_renders_normalized_hex_rgb_hsl_in_workbench_card() {
    let ctx = egui::Context::default();
    let palette = EarthPalette::resolve(crate::model::ThemeMode::Dark, true);
    palette.apply_to_ctx(&ctx);

    let mut settings = AppSettings::default();
    let collections = Vec::new();
    let rgba = crate::transforms::detect_color("rgba(10, 132, 255, 0.5)").unwrap();
    let results = vec![SearchResultItem {
        item: ClipItem {
            id: 1,
            content_hash: "rgb-1".into(),
            kind: ClipKind::Text,
            text_content: Some("rgba(10, 132, 255, 0.5)".into()),
            html_content: None,
            rtf_content: None,
            image_path: None,
            thumb_path: None,
            image_width: None,
            image_height: None,
            files: Vec::new(),
            byte_size: 23,
            source_app: Some("Figma".into()),
            source_bundle_id: None,
            pinned: false,
            collection_id: None,
            first_copied_at: 0,
            last_copied_at: 0,
            copy_count: 1,
            sensitive_label: None,
            detected_color: Some(rgba),
            is_url: false,
            note: None,
            text_truncated: false,
            has_rich_text: false,
        }
        .into(),
        score: 100,
        matched_indices: MatchMask::default(),
        display_text: None,
    }];

    let mut state = PopupState {
        selected_id: Some(1),
        preview_open: true,
        preview_reveal: Some(1.0),
        ..Default::default()
    };

    let raw = egui::RawInput {
        time: Some(0.1),
        screen_rect: Some(egui::Rect::from_min_size(
            egui::pos2(0.0, 0.0),
            vec2(760.0, 520.0),
        )),
        ..Default::default()
    };
    let mut out = ctx.run_ui(raw, |ui| {
        let ctx = ui.ctx().clone();
        let _ = render_popup_view(
            ui,
            &ctx,
            PopupViewData {
                settings: &mut settings,
                results: &results,
                collections: &collections,
                palette: &palette,
                storage_bytes: 23,
                total_items: 1,
                has_accessibility: true,
                load_full_item: None,
            },
            &mut state,
        );
    });

    let mut texts = Vec::new();
    for clipped in &out.shapes {
        if let egui::epaint::Shape::Text(t) = &clipped.shape {
            texts.push(t.galley.job.text.clone());
        }
    }
    out.textures_delta.clear();

    assert!(
        texts.iter().any(|t| t == "#0A84FF80"),
        "workbench color card should normalize RGBA input to #0A84FF80, got texts={texts:?}"
    );
    assert!(
        texts.iter().any(|t| t == "rgba(10, 132, 255, 0.5)"),
        "workbench color card should display RGBA value"
    );
    assert!(
        texts.iter().any(|t| t == "hsla(210, 100%, 52%, 0.5)"),
        "workbench color card should display HSLA value"
    );
    assert!(
        !texts.iter().any(|t| t.starts_with("#RGBA")),
        "workbench color card must never prefix # onto rgb()/rgba() strings"
    );
}

#[test]
fn popup_status_banner_auto_dismisses_after_three_seconds() {
    let ctx = egui::Context::default();
    let palette = EarthPalette::resolve(crate::model::ThemeMode::Light, false);
    palette.apply_to_ctx(&ctx);

    let mut settings = AppSettings::default();
    let collections = Vec::new();
    let results = Vec::new();
    let mut state = PopupState::default();
    state.set_status_banner("✓ 已粘贴到当前目标窗口", false);

    let mut render_at = |state: &mut PopupState, time: f64| -> Vec<String> {
        let raw = egui::RawInput {
            time: Some(time),
            screen_rect: Some(egui::Rect::from_min_size(
                egui::pos2(0.0, 0.0),
                vec2(424.0, 520.0),
            )),
            ..Default::default()
        };
        let mut out = ctx.run_ui(raw, |ui| {
            let ctx = ui.ctx().clone();
            let _ = render_popup_view(
                ui,
                &ctx,
                PopupViewData {
                    settings: &mut settings,
                    results: &results,
                    collections: &collections,
                    palette: &palette,
                    storage_bytes: 0,
                    total_items: 0,
                    has_accessibility: true,
                    load_full_item: None,
                },
                state,
            );
        });
        let texts = out
            .shapes
            .iter()
            .filter_map(|cs| match &cs.shape {
                egui::epaint::Shape::Text(t) => Some(t.galley.job.text.clone()),
                _ => None,
            })
            .collect();
        out.textures_delta.clear();
        texts
    };

    // Frame 1 (t = 1.0s): banner appears and starts its 3s timer
    let t1 = render_at(&mut state, 1.0);
    assert!(t1.iter().any(|s| s.contains("已粘贴到当前目标窗口")));
    assert!(state.status_banner.is_some());

    // At t = 3.5s (2.5s elapsed): banner is still visible
    let t2 = render_at(&mut state, 3.5);
    assert!(t2.iter().any(|s| s.contains("已粘贴到当前目标窗口")));
    assert!(state.status_banner.is_some());

    // At t = 4.05s (3.05s elapsed): banner auto-dismisses cleanly
    let t3 = render_at(&mut state, 4.05);
    assert!(!t3.iter().any(|s| s.contains("已粘贴到当前目标窗口")));
    assert!(state.status_banner.is_none());
}

#[test]
fn accessibility_copy_status_banner_does_not_overlap_action_button_or_overflow() {
    for lang in [
        crate::i18n::ResolvedLanguage::ZhCn,
        crate::i18n::ResolvedLanguage::EnUs,
    ] {
        crate::i18n::with_test_language(lang, || {
            let ctx = egui::Context::default();
            let palette = EarthPalette::resolve(crate::model::ThemeMode::Light, false);
            palette.apply_to_ctx(&ctx);

            let mut settings = AppSettings::default();
            let collections = Vec::new();
            let results = Vec::new();
            let mut state = PopupState::default();
            state.set_status_banner(crate::i18n::t("app.accessibility_paste_hint"), true);

            let raw = egui::RawInput {
                time: Some(1.0),
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::pos2(0.0, 0.0),
                    vec2(424.0, 520.0),
                )),
                ..Default::default()
            };
            let mut out = ctx.run_ui(raw, |ui| {
                let ctx = ui.ctx().clone();
                egui::CentralPanel::default()
                    .frame(egui::Frame::NONE)
                    .show(ui, |ui| {
                        let _ = render_popup_view(
                            ui,
                            &ctx,
                            PopupViewData {
                                settings: &mut settings,
                                results: &results,
                                collections: &collections,
                                palette: &palette,
                                storage_bytes: 0,
                                total_items: 0,
                                has_accessibility: false,
                                load_full_item: None,
                            },
                            &mut state,
                        );
                    });
            });
            out.textures_delta.clear();

            let mut rects = Vec::new();
            fn walk(shape: &egui::epaint::Shape, out: &mut Vec<(String, egui::Rect)>) {
                match shape {
                    egui::epaint::Shape::Text(t) => {
                        out.push((t.galley.text().to_string(), t.visual_bounding_rect()));
                    }
                    egui::epaint::Shape::Vec(v) => {
                        for child in v {
                            walk(child, out);
                        }
                    }
                    _ => {}
                }
            }
            for clipped in &out.shapes {
                walk(&clipped.shape, &mut rects);
            }

            let expected_msg = crate::i18n::t("app.accessibility_paste_hint");
            let (_, msg_rect) = rects
                .iter()
                .find(|(text, _)| text == expected_msg)
                .unwrap_or_else(|| panic!("[{lang:?}] banner message should render"));
            assert!(
                msg_rect.right() <= 394.0,
                "[{lang:?}] banner message overflowed into dismiss button or right edge: {msg_rect:?}"
            );

            #[cfg(target_os = "macos")]
            {
                let expected_btn = crate::i18n::t("footer.open_accessibility");
                let (_, btn_rect) = rects
                    .iter()
                    .find(|(text, _)| text == expected_btn)
                    .unwrap_or_else(|| {
                        panic!("[{lang:?}] accessibility action button should render")
                    });
                assert!(
                    msg_rect.bottom() <= btn_rect.top(),
                    "[{lang:?}] banner text overlapped accessibility action button: {msg_rect:?} vs {btn_rect:?}"
                );
                assert!(
                    btn_rect.right() <= 410.0,
                    "[{lang:?}] accessibility action button overflowed viewport: {btn_rect:?}"
                );
            }
        });
    }
}

#[cfg(target_os = "macos")]
#[test]
fn persistent_accessibility_banner_wraps_full_message_without_truncation() {
    for lang in [
        crate::i18n::ResolvedLanguage::ZhCn,
        crate::i18n::ResolvedLanguage::EnUs,
    ] {
        crate::i18n::with_test_language(lang, || {
            let ctx = egui::Context::default();
            let palette = EarthPalette::resolve(crate::model::ThemeMode::Light, false);
            palette.apply_to_ctx(&ctx);

            let mut settings = AppSettings {
                auto_paste: true,
                ..Default::default()
            };
            let collections = Vec::new();
            let results = Vec::new();
            let mut state = PopupState::default();

            let raw = egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::pos2(0.0, 0.0),
                    vec2(424.0, 520.0),
                )),
                ..Default::default()
            };
            let mut out = ctx.run_ui(raw, |ui| {
                let ctx = ui.ctx().clone();
                egui::CentralPanel::default()
                    .frame(egui::Frame::NONE)
                    .show(ui, |ui| {
                        let _ = render_popup_view(
                            ui,
                            &ctx,
                            PopupViewData {
                                settings: &mut settings,
                                results: &results,
                                collections: &collections,
                                palette: &palette,
                                storage_bytes: 0,
                                total_items: 0,
                                has_accessibility: false,
                                load_full_item: None,
                            },
                            &mut state,
                        );
                    });
            });
            out.textures_delta.clear();

            let mut rects = Vec::new();
            let mut full_galley_text = None;
            fn walk(
                shape: &egui::epaint::Shape,
                out: &mut Vec<(String, egui::Rect)>,
                needle: &str,
                elided_hit: &mut Option<bool>,
            ) {
                match shape {
                    egui::epaint::Shape::Text(t) => {
                        let txt = t.galley.text().to_string();
                        if txt == needle {
                            *elided_hit = Some(t.galley.elided);
                        }
                        out.push((txt, t.visual_bounding_rect()));
                    }
                    egui::epaint::Shape::Vec(v) => {
                        for child in v {
                            walk(child, out, needle, elided_hit);
                        }
                    }
                    _ => {}
                }
            }
            let expected_msg = crate::i18n::t("banner.accessibility_warn");
            for clipped in &out.shapes {
                walk(
                    &clipped.shape,
                    &mut rects,
                    expected_msg,
                    &mut full_galley_text,
                );
            }

            assert_eq!(
                full_galley_text,
                Some(false),
                "[{lang:?}] persistent accessibility warning must not be elided/truncated"
            );
            let (_, msg_rect) = rects.iter().find(|(text, _)| text == expected_msg).unwrap();
            let expected_btn = crate::i18n::t("banner.accessibility_btn");
            let (_, btn_rect) = rects.iter().find(|(text, _)| text == expected_btn).unwrap();
            assert!(
                msg_rect.right() <= 394.0,
                "[{lang:?}] warning text overflowed into dismiss button: {msg_rect:?}"
            );
            assert!(
                msg_rect.bottom() <= btn_rect.top(),
                "[{lang:?}] warning text overlapped action button: {msg_rect:?} vs {btn_rect:?}"
            );
        });
    }
}

#[test]
fn truncated_item_resolves_full_text_in_workbench_and_english_toolbar_wraps_cleanly() {
    crate::i18n::with_test_language(crate::i18n::ResolvedLanguage::EnUs, || {
        let ctx = egui::Context::default();
        let palette = EarthPalette::resolve(crate::model::ThemeMode::Light, false);
        palette.apply_to_ctx(&ctx);

        let full_json = format!("{{\"prefix\":\"{}\",\"deep_key\":42}}", "a".repeat(1200));
        let full_item = ClipItem {
            id: 99,
            content_hash: "trunc-99".into(),
            kind: ClipKind::Text,
            text_content: Some(full_json.clone()),
            html_content: Some("<b>rich</b>".into()),
            rtf_content: None,
            image_path: None,
            thumb_path: None,
            image_width: None,
            image_height: None,
            files: Vec::new(),
            byte_size: full_json.len() as u64,
            source_app: Some("Safari".into()),
            source_bundle_id: None,
            pinned: true,
            collection_id: None,
            first_copied_at: 1000,
            last_copied_at: 1000,
            copy_count: 1,
            sensitive_label: Some("API Key".into()),
            detected_color: None,
            is_url: false,
            note: Some("prod".into()),
            text_truncated: false,
            has_rich_text: true,
        };
        let mut truncated = full_item.clone();
        truncated.text_content = Some(format!("{{\"prefix\":\"{}…", "a".repeat(256)));
        truncated.html_content = None;
        truncated.text_truncated = true;

        let results = vec![SearchResultItem {
            item: Arc::new(truncated),
            score: 100,
            matched_indices: MatchMask::default(),
            display_text: None,
        }];
        let mut settings = AppSettings::default();
        let collections = Vec::new();
        let mut state = PopupState {
            selected_id: Some(99),
            preview_open: true,
            preview_reveal: Some(1.0),
            ..Default::default()
        };
        state.revealed_sensitive_ids.insert(99);

        let raw = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::pos2(0.0, 0.0),
                vec2(760.0, 520.0),
            )),
            ..Default::default()
        };
        let load_full = |id: i64| (id == 99).then(|| full_item.clone());
        let mut max_right = 0.0f32;
        let mut out = ctx.run_ui(raw, |ui| {
            let ctx = ui.ctx().clone();
            egui::CentralPanel::default()
                .frame(egui::Frame::NONE)
                .show(ui, |ui| {
                    let content_rect = ui.max_rect().shrink(13.0);
                    ui.scope_builder(
                        egui::UiBuilder::new()
                            .max_rect(content_rect)
                            .layout(Layout::top_down(Align::Min)),
                        |ui| {
                            ui.set_min_size(content_rect.size());
                            ui.set_max_size(content_rect.size());
                            let _ = render_popup_view(
                                ui,
                                &ctx,
                                PopupViewData {
                                    settings: &mut settings,
                                    results: &results,
                                    collections: &collections,
                                    palette: &palette,
                                    storage_bytes: 1400,
                                    total_items: 1,
                                    has_accessibility: true,
                                    load_full_item: Some(&load_full),
                                },
                                &mut state,
                            );
                        },
                    );
                });
        });
        let mut max_text_right = 0.0f32;
        for cs in &out.shapes {
            let r = cs.shape.visual_bounding_rect().right();
            if r.is_finite() {
                max_right = max_right.max(r);
            }
            if let egui::epaint::Shape::Text(t) = &cs.shape {
                max_text_right = max_text_right.max(t.visual_bounding_rect().right());
            }
        }
        out.textures_delta.clear();

        let cache = state
            .preview_cache
            .as_ref()
            .expect("preview cache populated");
        assert_eq!(
            cache.resolved.text_content.as_deref(),
            Some(full_json.as_str())
        );
        assert!(
            cache.quick_actions.contains(&QuickActionKind::JsonPretty),
            "Quick actions must inspect resolved full JSON rather than truncated snippet"
        );
        assert!(
            max_text_right <= 747.0 && max_right <= 760.0,
            "English workbench action toolbar must wrap cleanly without overflowing card/viewport: max_text_right={max_text_right} max_right={max_right}"
        );
    });
}
