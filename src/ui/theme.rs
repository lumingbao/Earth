use crate::model::{MatchHighlight, ThemeMode};
use egui::epaint::text::{FontData, FontFamily, FontInsert, FontPriority, InsertFontFamily};
use egui::epaint::FontColorTransferFunction;
use egui::{Color32, CornerRadius, FontId, Stroke, Style, TextFormat, Ui, Visuals};
use std::collections::HashSet;
use std::path::Path;
use std::sync::Arc;

#[derive(Debug, Clone, Copy)]
pub struct EarthPalette {
    pub is_dark: bool,
    pub window_bg: Color32,
    pub surface_bg: Color32,
    pub card_bg: Color32,
    pub row_hover_bg: Color32,
    pub row_selected_bg: Color32,
    pub border: Color32,
    pub border_subtle: Color32,
    pub border_strong: Color32,
    pub text_primary: Color32,
    pub text_secondary: Color32,
    pub text_muted: Color32,
    pub accent: Color32,
    pub accent_hover: Color32,
    pub accent_active: Color32,
    pub accent_soft: Color32,
    pub accent_text: Color32,
    pub accent_secondary: Color32,
    pub pin_amber: Color32,
    pub success: Color32,
    pub success_bg: Color32,
    pub success_border: Color32,
    pub success_text: Color32,
    pub warning_bg: Color32,
    pub warning_border: Color32,
    pub warning_text: Color32,
    pub danger: Color32,
    pub danger_bg: Color32,
    pub danger_border: Color32,
    pub danger_text: Color32,
    pub info_bg: Color32,
    pub info_border: Color32,
    pub keycap_bg: Color32,
    pub keycap_text: Color32,
    pub on_accent: Color32,
}

impl EarthPalette {
    pub fn resolve(mode: ThemeMode, _system_dark: bool) -> Self {
        let dark = matches!(mode, ThemeMode::Dark);
        if dark {
            // macOS Dark Mode (Apple HIG): neutral charcoal/graphite elevation + System Blue (#0A84FF)
            Self {
                is_dark: true,
                window_bg: Color32::from_rgb(30, 30, 30),         // #1E1E1E macOS windowBackgroundColor
                surface_bg: Color32::from_rgb(40, 40, 42),        // #28282A elevated card / control surface
                card_bg: Color32::from_rgb(51, 51, 54),           // #333336 search pill / capsules / icon wells
                row_hover_bg: Color32::from_rgb(56, 56, 60),      // #38383C neutral graphite hover
                row_selected_bg: Color32::from_rgb(25, 48, 78),   // #19304E subtle System Blue selection tint
                border: Color32::from_rgb(68, 68, 72),            // #444448 macOS dark window/control bezel
                border_subtle: Color32::from_rgb(52, 52, 56),     // #343438 macOS separatorColor
                border_strong: Color32::from_rgb(56, 132, 224),   // #3884E0 focused System Blue ring
                text_primary: Color32::from_rgb(245, 245, 247),   // #F5F5F7 Apple primary labelColor
                text_secondary: Color32::from_rgb(152, 152, 157), // #98989D Apple secondaryLabelColor
                text_muted: Color32::from_rgb(110, 110, 115),     // #6E6E73 Apple tertiaryLabelColor
                accent: Color32::from_rgb(10, 132, 255),          // #0A84FF Apple Dark Mode systemBlue
                accent_hover: Color32::from_rgb(64, 156, 255),    // #409CFF lighter systemBlue hover
                accent_active: Color32::from_rgb(0, 113, 227),    // #0071E3 pressed systemBlue
                accent_soft: Color32::from_rgb(24, 46, 74),       // #182E4A soft translucent blue pill fill
                accent_text: Color32::from_rgb(100, 178, 255),    // #64B2FF high-contrast blue label
                accent_secondary: Color32::from_rgb(100, 210, 255), // #64D2FF Apple Dark Mode systemCyan
                pin_amber: Color32::from_rgb(255, 159, 10),       // #FF9F0A Apple Dark Mode systemOrange
                success: Color32::from_rgb(48, 209, 88),          // #30D158 Apple Dark Mode systemGreen
                success_bg: Color32::from_rgb(24, 48, 32),        // #183020 tinted systemGreen surface
                success_border: Color32::from_rgb(42, 92, 56),    // #2A5C38
                success_text: Color32::from_rgb(68, 219, 106),    // #44DB6A
                warning_bg: Color32::from_rgb(54, 42, 22),        // #362A16 tinted systemOrange surface
                warning_border: Color32::from_rgb(108, 78, 28),   // #6C4E1C
                warning_text: Color32::from_rgb(255, 180, 64),    // #FFB440
                danger: Color32::from_rgb(255, 69, 58),           // #FF453A Apple Dark Mode systemRed
                danger_bg: Color32::from_rgb(56, 28, 28),         // #381C1C tinted systemRed surface
                danger_border: Color32::from_rgb(112, 46, 44),    // #702E2C
                danger_text: Color32::from_rgb(255, 105, 97),     // #FF6961
                info_bg: Color32::from_rgb(26, 42, 62),           // #1A2A3E
                info_border: Color32::from_rgb(46, 82, 124),      // #2E527C
                keycap_bg: Color32::from_rgb(54, 54, 58),         // #36363A neutral dark keycap
                keycap_text: Color32::from_rgb(174, 174, 178),    // #AEAEB2 Apple systemGray2
                on_accent: Color32::from_rgb(255, 255, 255),
            }
        } else {
            Self {
                is_dark: false,
                window_bg: Color32::from_rgb(248, 249, 253),    // #F8F9FD GOSTOCK cool off-white canvas
                surface_bg: Color32::from_rgb(255, 255, 255),   // #FFFFFF pure white elevated cards
                card_bg: Color32::from_rgb(241, 244, 249),      // #F1F4F9 soft grayish-blue pill/search fill
                row_hover_bg: Color32::from_rgb(243, 245, 250), // #F3F5FA soft hover surface
                row_selected_bg: Color32::from_rgb(239, 244, 255), // #EFF4FF GOSTOCK active cobalt tint
                border: Color32::from_rgb(226, 232, 240),       // #E2E8F0 crisp light hairline
                border_subtle: Color32::from_rgb(236, 239, 244), // #ECEFF4 ultra-light divider
                border_strong: Color32::from_rgb(199, 213, 254), // #C7D5FE royal blue tint border
                text_primary: Color32::from_rgb(5, 9, 20),      // #050914 GOSTOCK deep ink slate
                text_secondary: Color32::from_rgb(100, 116, 139), // #64748B GOSTOCK cool slate
                text_muted: Color32::from_rgb(113, 127, 148),   // #717F94 muted slate caption
                accent: Color32::from_rgb(69, 110, 253),        // #456EFD GOSTOCK Electric Royal Blue
                accent_hover: Color32::from_rgb(94, 131, 254),  // #5E83FE hover cobalt
                accent_active: Color32::from_rgb(47, 85, 235),  // #2F55EB pressed cobalt
                accent_soft: Color32::from_rgb(239, 246, 255),  // #EFF6FF GOSTOCK soft edit/info blue
                accent_text: Color32::from_rgb(52, 87, 213),    // #3457D5 high-contrast cobalt label
                accent_secondary: Color32::from_rgb(124, 155, 255), // #7C9BFF soft periwinkle ramp
                pin_amber: Color32::from_rgb(245, 158, 11),     // #F59E0B GOSTOCK golden amber crown
                success: Color32::from_rgb(34, 197, 94),        // #22C55E GOSTOCK emerald green
                success_bg: Color32::from_rgb(220, 252, 231),   // #DCFCE7 pastel emerald
                success_border: Color32::from_rgb(187, 247, 208), // #BBF7D0
                success_text: Color32::from_rgb(22, 163, 74),   // #16A34A
                warning_bg: Color32::from_rgb(254, 243, 199),   // #FEF3C7 pastel amber
                warning_border: Color32::from_rgb(253, 230, 138), // #FDE68A
                warning_text: Color32::from_rgb(180, 83, 9),    // #B45309
                danger: Color32::from_rgb(239, 68, 68),         // #EF4444 GOSTOCK coral red
                danger_bg: Color32::from_rgb(254, 226, 226),    // #FEE2E2 GOSTOCK pastel delete surface
                danger_border: Color32::from_rgb(254, 202, 202), // #FECACA
                danger_text: Color32::from_rgb(220, 38, 38),    // #DC2626
                info_bg: Color32::from_rgb(239, 246, 255),      // #EFF6FF
                info_border: Color32::from_rgb(199, 213, 254),  // #C7D5FE
                keycap_bg: Color32::from_rgb(241, 244, 249),    // #F1F4F9
                keycap_text: Color32::from_rgb(100, 116, 139),  // #64748B
                on_accent: Color32::from_rgb(255, 255, 255),
            }
        }
    }

    pub fn apply_to_ctx(&self, ctx: &egui::Context) {
        let mut style = Style::default();
        let mut visuals = if self.is_dark {
            Visuals::dark()
        } else {
            Visuals::light()
        };

        visuals.window_fill = self.surface_bg;
        visuals.panel_fill = self.window_bg;
        visuals.extreme_bg_color = self.surface_bg;
        visuals.faint_bg_color = self.card_bg;
        visuals.window_stroke = Stroke::new(
            1.0,
            if self.is_dark {
                self.border
            } else {
                self.border_subtle
            },
        );
        visuals.window_corner_radius = CornerRadius::same(16);
        visuals.selection.bg_fill = self.accent_soft;
        visuals.selection.stroke = Stroke::new(1.0, self.accent);
        visuals.hyperlink_color = self.accent;
        visuals.text_cursor.stroke = Stroke::new(1.5, self.accent);

        // Hairlines / separators (#ECEFF4 light, #1E2844 dark)
        visuals.widgets.noninteractive.bg_fill = self.surface_bg;
        visuals.widgets.noninteractive.fg_stroke = Stroke::new(1.0, self.text_primary);
        visuals.widgets.noninteractive.bg_stroke = Stroke::new(1.0, self.border_subtle);
        visuals.widgets.noninteractive.corner_radius = CornerRadius::same(12);

        // GOSTOCK secondary pill/squircle controls sit cleanly on #FFFFFF / #F1F4F9
        visuals.widgets.inactive.bg_fill = self.surface_bg;
        visuals.widgets.inactive.weak_bg_fill = self.surface_bg;
        visuals.widgets.inactive.fg_stroke = Stroke::new(1.0, self.text_primary);
        visuals.widgets.inactive.bg_stroke = Stroke::new(1.0, self.border);
        visuals.widgets.inactive.corner_radius = CornerRadius::same(255);

        // Hover state: soft cobalt tint & #456EFD outline
        visuals.widgets.hovered.bg_fill = self.accent_soft;
        visuals.widgets.hovered.weak_bg_fill = self.accent_soft;
        visuals.widgets.hovered.fg_stroke = Stroke::new(1.0, self.accent_text);
        visuals.widgets.hovered.bg_stroke = Stroke::new(1.0, self.accent);
        visuals.widgets.hovered.corner_radius = CornerRadius::same(255);

        // Active / pressed state
        visuals.widgets.active.bg_fill = self.accent_soft;
        visuals.widgets.active.weak_bg_fill = self.accent_soft;
        visuals.widgets.active.fg_stroke = Stroke::new(1.0, self.accent_active);
        visuals.widgets.active.bg_stroke = Stroke::new(1.0, self.accent_active);
        visuals.widgets.active.corner_radius = CornerRadius::same(255);

        // Open ComboBox / dropdown trigger
        visuals.widgets.open.bg_fill = self.surface_bg;
        visuals.widgets.open.weak_bg_fill = self.surface_bg;
        visuals.widgets.open.fg_stroke = Stroke::new(1.0, self.accent);
        visuals.widgets.open.bg_stroke = Stroke::new(1.0, self.accent);
        visuals.widgets.open.corner_radius = CornerRadius::same(16);

        // Crisp glyph rendering: snap Latin/digit stems without subpixel horizontal blurring,
        // keep hinting on, and gently boost light-mode coverage on 1x (non-Retina) displays.
        visuals.text_options.font_hinting = true;
        visuals.text_options.subpixel_binning = false;
        visuals.text_options.color_transfer_function = if self.is_dark {
            FontColorTransferFunction::DARK_MODE_DEFAULT
        } else if ctx.pixels_per_point() < 1.5 {
            FontColorTransferFunction::Gamma(0.75)
        } else {
            FontColorTransferFunction::Gamma(0.88)
        };

        style.visuals = visuals;
        style.spacing.item_spacing = egui::vec2(6.0, 5.0);
        style.spacing.button_padding = egui::vec2(14.0, 6.0);
        style.spacing.scroll.bar_width = 5.0;
        style.spacing.scroll.bar_inner_margin = 2.0;
        // Disable accidental label text selection so dragging anywhere across UI labels moves the window
        style.interaction.selectable_labels = false;

        ctx.set_theme(if self.is_dark {
            egui::Theme::Dark
        } else {
            egui::Theme::Light
        });
        ctx.set_global_style(style);
    }
}

/// Subtle ambient elevation shadow inspired by Behance GOSTOCK cards.
pub fn card_shadow(palette: &EarthPalette) -> egui::Shadow {
    if palette.is_dark {
        egui::Shadow::NONE
    } else {
        egui::Shadow {
            offset: [0, 3],
            blur: 14,
            spread: 0,
            color: Color32::from_rgba_unmultiplied(5, 9, 20, 10),
        }
    }
}

/// Renders a button with custom padding directly in `ui` (without `ui.scope`) so
/// `ui.horizontal_wrapped` can wrap the entire button onto the next row instead of
/// squeezing button text vertically at the end of a line.
pub fn padded_button(ui: &mut Ui, btn: egui::Button<'_>, padding: egui::Vec2) -> egui::Response {
    let prev = ui.spacing().button_padding;
    ui.spacing_mut().button_padding = padding;
    let resp = ui.add(btn.wrap_mode(egui::TextWrapMode::Extend));
    ui.spacing_mut().button_padding = prev;
    resp
}

/// Compact pill button replacement for `ui.small_button` with balanced breathing room.
pub fn compact_button(ui: &mut Ui, text: impl Into<egui::WidgetText>) -> egui::Response {
    padded_button(
        ui,
        egui::Button::new(text).corner_radius(CornerRadius::same(255)),
        egui::vec2(11.0, 4.5),
    )
}

/// Compact dismiss `"×"` button with balanced circular pill padding around the glyph.
pub fn dismiss_button(ui: &mut Ui) -> egui::Response {
    padded_button(
        ui,
        egui::Button::new("×").corner_radius(CornerRadius::same(255)),
        egui::vec2(7.0, 3.0),
    )
}

/// Keeps mouse-wheel/trackpad and scrollbar-handle scrolling active while letting
/// content drags bubble through to window dragging.
pub fn desktop_scroll_source() -> egui::scroll_area::ScrollSource {
    egui::scroll_area::ScrollSource {
        scroll_bar: true,
        drag: egui::scroll_area::DragScroll::Never,
        mouse_wheel: true,
    }
}

pub fn build_highlighted_layout_job(
    text: &str,
    matched_indices: &HashSet<usize>,
    highlight_mode: MatchHighlight,
    palette: &EarthPalette,
    font_size: f32,
    max_width: f32,
    _selected: bool,
) -> egui::text::LayoutJob {
    let mut job = egui::text::LayoutJob {
        wrap: egui::epaint::text::TextWrapping {
            max_width,
            max_rows: 1,
            break_anywhere: true,
            overflow_character: Some('…'),
        },
        ..Default::default()
    };
    let base_color = palette.text_primary;

    if matched_indices.is_empty() {
        job.append(
            text,
            0.0,
            TextFormat {
                font_id: FontId::proportional(font_size),
                color: base_color,
                ..Default::default()
            },
        );
        return job;
    }

    let mut run = String::new();
    let mut run_matched = false;

    for (char_idx, ch) in text.chars().enumerate() {
        let is_matched = matched_indices.contains(&char_idx);
        if char_idx > 0 && is_matched != run_matched && !run.is_empty() {
            append_run(&mut job, &run, run_matched, highlight_mode, palette, font_size, base_color);
            run.clear();
        }
        run_matched = is_matched;
        run.push(ch);
    }
    if !run.is_empty() {
        append_run(&mut job, &run, run_matched, highlight_mode, palette, font_size, base_color);
    }

    job
}

fn append_run(
    job: &mut egui::text::LayoutJob,
    segment: &str,
    is_matched: bool,
    highlight_mode: MatchHighlight,
    palette: &EarthPalette,
    font_size: f32,
    base_color: Color32,
) {
    let mut fmt = TextFormat {
        font_id: FontId::proportional(font_size),
        color: base_color,
        ..Default::default()
    };

    if is_matched {
        fmt.color = palette.accent_text;
        match highlight_mode {
            MatchHighlight::Bold => {
                fmt.font_id = FontId::proportional(font_size + 0.4);
                fmt.background = palette.accent_soft;
            }
            MatchHighlight::Italic => {
                fmt.italics = true;
            }
            MatchHighlight::Underline => {
                fmt.underline = Stroke::new(1.5, palette.accent);
            }
        }
    }

    job.append(segment, 0.0, fmt);
}

pub fn parse_hex_color32(hex: &str) -> Option<Color32> {
    let rgba = crate::transforms::detect_hex_color(hex)?;
    Some(Color32::from_rgba_unmultiplied(rgba[0], rgba[1], rgba[2], rgba[3]))
}

/// Installs crisp native system UI & CJK fonts (`PingFang SC` + `Menlo` on macOS,
/// `Segoe UI` + `Microsoft YaHei` + `Consolas` on Windows) ahead of egui's thin defaults
/// while preserving symbol and emoji fallbacks (`SystemFontProvider`, `NotoEmoji`).
pub fn install_crisp_fonts(ctx: &egui::Context) {
    let preferred = crate::platform::preferred_system_fonts();

    // Insert UI/CJK fonts in reverse priority order with `FontPriority::Highest` so index 0
    // becomes the first candidate in Proportional and sits cleanly as CJK fallback in Monospace.
    for (idx, candidate) in preferred.ui.iter().enumerate().rev() {
        let Some(font_data) = load_mmap_font_data(&candidate.path, candidate.postscript_name)
        else {
            continue;
        };
        let families = if idx == 0 {
            vec![
                InsertFontFamily {
                    family: FontFamily::Proportional,
                    priority: FontPriority::Highest,
                },
                InsertFontFamily {
                    family: FontFamily::Monospace,
                    priority: FontPriority::Highest,
                },
            ]
        } else {
            vec![InsertFontFamily {
                family: FontFamily::Proportional,
                priority: FontPriority::Highest,
            }]
        };
        ctx.add_font(FontInsert::new(
            &format!("earth_ui_{}", candidate.postscript_name),
            font_data,
            families,
        ));
    }

    // Insert dedicated Monospace faces after UI fallback so code ASCII uses Menlo/Consolas first
    // and embedded CJK still falls back to PingFang/YaHei.
    for candidate in preferred.monospace.iter().rev() {
        let Some(font_data) = load_mmap_font_data(&candidate.path, candidate.postscript_name)
        else {
            continue;
        };
        ctx.add_font(FontInsert::new(
            &format!("earth_mono_{}", candidate.postscript_name),
            font_data,
            vec![InsertFontFamily {
                family: FontFamily::Monospace,
                priority: FontPriority::Highest,
            }],
        ));
    }
}

fn load_mmap_font_data(path: &Path, target_ps_name: &str) -> Option<FontData> {
    let file = std::fs::File::open(path).ok()?;
    // SAFETY: OS system font collections are read-only immutable assets mapped for the app lifetime.
    let mapped = unsafe { memmap2::Mmap::map(&file).ok()? };
    let index = find_ttc_face_index(&mapped, target_ps_name);
    Some(FontData::from_blob(Arc::new(mapped), index))
}

fn read_u16_be(bytes: &[u8], off: usize) -> Option<u16> {
    let slice = bytes.get(off..off.checked_add(2)?)?;
    Some(u16::from_be_bytes([slice[0], slice[1]]))
}

fn read_u32_be(bytes: &[u8], off: usize) -> Option<u32> {
    let slice = bytes.get(off..off.checked_add(4)?)?;
    Some(u32::from_be_bytes([slice[0], slice[1], slice[2], slice[3]]))
}

/// Finds the sub-font face index inside a `.ttc` TrueType Collection whose PostScript name
/// (`nameID == 6`) matches `target_ps_name`, returning `0` if single-face or not found.
pub(crate) fn find_ttc_face_index(bytes: &[u8], target_ps_name: &str) -> u32 {
    if bytes.get(0..4) != Some(b"ttcf") {
        return 0;
    }
    let Some(num_fonts) = read_u32_be(bytes, 8) else {
        return 0;
    };
    let count = num_fonts.min(128);
    for face_idx in 0..count {
        let Some(face_off_u32) = read_u32_be(bytes, 12 + face_idx as usize * 4) else {
            break;
        };
        if face_postscript_matches(bytes, face_off_u32 as usize, target_ps_name) {
            return face_idx;
        }
    }
    0
}

fn face_postscript_matches(bytes: &[u8], face_off: usize, target_ps_name: &str) -> bool {
    let Some(num_tables) = read_u16_be(bytes, face_off.saturating_add(4)) else {
        return false;
    };
    let records_start = face_off.saturating_add(12);
    let mut name_table_off = None;
    for i in 0..num_tables.min(128) as usize {
        let rec_off = records_start + i * 16;
        if bytes.get(rec_off..rec_off + 4) == Some(b"name") {
            name_table_off = read_u32_be(bytes, rec_off + 8).map(|v| v as usize);
            break;
        }
    }
    let Some(name_off) = name_table_off else {
        return false;
    };
    let Some(name_count) = read_u16_be(bytes, name_off + 2) else {
        return false;
    };
    let Some(str_storage_rel) = read_u16_be(bytes, name_off + 4) else {
        return false;
    };
    let str_storage = name_off.saturating_add(str_storage_rel as usize);
    let recs_off = name_off.saturating_add(6);

    for i in 0..name_count.min(512) as usize {
        let r = recs_off + i * 12;
        let Some(platform_id) = read_u16_be(bytes, r) else {
            break;
        };
        let Some(name_id) = read_u16_be(bytes, r + 6) else {
            break;
        };
        if name_id != 6 {
            continue;
        }
        let (Some(len), Some(rel)) = (read_u16_be(bytes, r + 8), read_u16_be(bytes, r + 10)) else {
            continue;
        };
        let start = str_storage.saturating_add(rel as usize);
        let Some(raw) = bytes.get(start..start.saturating_add(len as usize)) else {
            continue;
        };
        let matches = if platform_id == 1 {
            raw == target_ps_name.as_bytes()
        } else if platform_id == 3 || platform_id == 0 {
            if raw.len() == target_ps_name.len() * 2 {
                raw.as_chunks::<2>()
                    .0
                    .iter()
                    .zip(target_ps_name.bytes())
                    .all(|(pair, b)| pair[0] == 0 && pair[1] == b)
            } else {
                false
            }
        } else {
            false
        };
        if matches {
            return true;
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn crisp_text_options_disable_subpixel_blur_and_tune_gamma_by_dpi() {
        let light = EarthPalette::resolve(ThemeMode::Light, false);
        let ctx_1x = egui::Context::default();
        light.apply_to_ctx(&ctx_1x);
        let opts_1x = ctx_1x.global_style().visuals.text_options;
        assert!(opts_1x.font_hinting);
        assert!(!opts_1x.subpixel_binning);
        assert_eq!(opts_1x.color_transfer_function, FontColorTransferFunction::Gamma(0.75));

        let ctx_2x = egui::Context::default();
        let mut raw_2x = egui::RawInput::default();
        raw_2x
            .viewports
            .entry(egui::ViewportId::ROOT)
            .or_default()
            .native_pixels_per_point = Some(2.0);
        let mut out_2x = ctx_2x.run_ui(raw_2x, |ui| {
            light.apply_to_ctx(ui.ctx());
        });
        out_2x.textures_delta.clear();
        assert_eq!(
            ctx_2x.global_style().visuals.text_options.color_transfer_function,
            FontColorTransferFunction::Gamma(0.88)
        );

        let dark = EarthPalette::resolve(ThemeMode::Dark, true);
        dark.apply_to_ctx(&ctx_1x);
        assert_eq!(
            ctx_1x.global_style().visuals.text_options.color_transfer_function,
            FontColorTransferFunction::DARK_MODE_DEFAULT
        );
    }

    #[test]
    fn dark_palette_uses_neutral_macos_graphite_and_system_blue() {
        let dark = EarthPalette::resolve(ThemeMode::Dark, true);
        assert!(dark.is_dark);

        let is_neutral = |c: Color32| {
            let max_ch = c.r().max(c.g()).max(c.b());
            let min_ch = c.r().min(c.g()).min(c.b());
            max_ch - min_ch <= 6
        };
        assert!(is_neutral(dark.window_bg), "window_bg should be neutral macOS charcoal");
        assert!(is_neutral(dark.surface_bg), "surface_bg should be neutral macOS graphite");
        assert!(is_neutral(dark.card_bg), "card_bg should be neutral macOS control fill");
        assert!(is_neutral(dark.border), "border should be neutral macOS bezel");
        assert!(is_neutral(dark.text_primary), "text_primary should be neutral labelColor");
        assert!(
            dark.window_bg.r() < dark.surface_bg.r() && dark.surface_bg.r() < dark.card_bg.r(),
            "dark surfaces should step cleanly upward in elevation"
        );
        assert_eq!(dark.accent, Color32::from_rgb(10, 132, 255));
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn install_crisp_fonts_loads_pingfang_and_menlo_with_correct_ttc_indices() {
        let preferred = crate::platform::preferred_system_fonts();
        let pf = preferred.ui.first().expect("macOS should resolve primary CJK UI font");
        let bytes = std::fs::read(&pf.path).unwrap();
        let idx = find_ttc_face_index(&bytes, pf.postscript_name);
        if pf.postscript_name == "PingFangSC-Regular" {
            assert_ne!(idx, 0, "PingFangSC-Regular is not index 0 in PingFang.ttc (got {idx})");
        }

        let ctx = egui::Context::default();
        install_crisp_fonts(&ctx);
        let raw = egui::RawInput::default();
        let mut out = ctx.run_ui(raw, |ui| {
            ui.label("测试 Crisp PingFang 字体");
        });
        out.textures_delta.clear();

        ctx.fonts(|f| {
            let prop_primary = f
                .definitions()
                .families
                .get(&FontFamily::Proportional)
                .and_then(|v| v.first())
                .cloned();
            assert_eq!(
                prop_primary.as_deref(),
                Some(format!("earth_ui_{}", pf.postscript_name).as_str())
            );
            let mono_primary = f
                .definitions()
                .families
                .get(&FontFamily::Monospace)
                .and_then(|v| v.first())
                .cloned();
            assert_eq!(mono_primary.as_deref(), Some("earth_mono_Menlo-Regular"));
        });
    }
}
