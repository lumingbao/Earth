use crate::model::{MatchHighlight, ThemeMode};
use egui::epaint::FontColorTransferFunction;
use egui::epaint::text::{FontData, FontFamily, FontInsert, FontPriority, InsertFontFamily};
use egui::{Color32, CornerRadius, FontId, Stroke, Style, TextFormat, Ui, Visuals};
use std::collections::HashSet;
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

pub const STATUS_TOAST_DURATION_SECS: f64 = 3.0;
const STATUS_TOAST_FADE_SECS: f64 = 0.22;

#[derive(Debug, Clone, PartialEq)]
pub struct StatusToast {
    pub message: String,
    pub is_alert: bool,
    pub shown_at: Option<f64>,
}

impl StatusToast {
    pub fn new(message: impl Into<String>, is_alert: bool) -> Self {
        Self {
            message: message.into(),
            is_alert,
            shown_at: None,
        }
    }

    /// Starts the 3-second auto-dismiss timer on the first frame the toast is displayed
    /// and returns the remaining seconds, or `None` once 3 seconds have elapsed.
    pub fn tick(&mut self, now_secs: f64) -> Option<f64> {
        let started = match self.shown_at {
            Some(prev) if now_secs >= prev => prev,
            _ => {
                self.shown_at = Some(now_secs);
                now_secs
            }
        };
        let remaining = STATUS_TOAST_DURATION_SECS - (now_secs - started);
        (remaining > 0.0).then_some(remaining)
    }

    /// Returns smooth fade-out opacity during the final 220ms before auto-dismiss.
    pub fn opacity_for_remaining(remaining_secs: f64) -> f32 {
        if remaining_secs >= STATUS_TOAST_FADE_SECS {
            1.0
        } else {
            (remaining_secs / STATUS_TOAST_FADE_SECS).clamp(0.0, 1.0) as f32
        }
    }

    /// Schedules reactive repaint so egui wakes cleanly for fade-out and expiry.
    pub fn schedule_repaint(ctx: &egui::Context, remaining_secs: f64) {
        if remaining_secs > STATUS_TOAST_FADE_SECS {
            let wait = (remaining_secs - STATUS_TOAST_FADE_SECS).max(0.01);
            ctx.request_repaint_after(Duration::from_secs_f64(wait));
        } else {
            ctx.request_repaint();
        }
    }
}

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
    pub fn resolve(mode: ThemeMode, system_dark: bool) -> Self {
        let dark = match mode {
            ThemeMode::Dark => true,
            ThemeMode::Light => false,
            ThemeMode::System => system_dark,
        };
        if dark {
            // macOS Dark Mode (Apple HIG): neutral charcoal/graphite elevation + System Blue (#0A84FF)
            Self {
                is_dark: true,
                window_bg: Color32::from_rgb(30, 30, 30), // #1E1E1E macOS windowBackgroundColor
                surface_bg: Color32::from_rgb(40, 40, 42), // #28282A elevated card / control surface
                card_bg: Color32::from_rgb(51, 51, 54), // #333336 search pill / capsules / icon wells
                row_hover_bg: Color32::from_rgb(56, 56, 60), // #38383C neutral graphite hover
                row_selected_bg: Color32::from_rgb(25, 48, 78), // #19304E subtle System Blue selection tint
                border: Color32::from_rgb(68, 68, 72), // #444448 macOS dark window/control bezel
                border_subtle: Color32::from_rgb(52, 52, 56), // #343438 macOS separatorColor
                border_strong: Color32::from_rgb(56, 132, 224), // #3884E0 focused System Blue ring
                text_primary: Color32::from_rgb(245, 245, 247), // #F5F5F7 Apple primary labelColor
                text_secondary: Color32::from_rgb(152, 152, 157), // #98989D Apple secondaryLabelColor
                text_muted: Color32::from_rgb(110, 110, 115), // #6E6E73 Apple tertiaryLabelColor
                accent: Color32::from_rgb(10, 132, 255),      // #0A84FF Apple Dark Mode systemBlue
                accent_hover: Color32::from_rgb(64, 156, 255), // #409CFF lighter systemBlue hover
                accent_active: Color32::from_rgb(0, 113, 227), // #0071E3 pressed systemBlue
                accent_soft: Color32::from_rgb(24, 46, 74), // #182E4A soft translucent blue pill fill
                accent_text: Color32::from_rgb(100, 178, 255), // #64B2FF high-contrast blue label
                accent_secondary: Color32::from_rgb(100, 210, 255), // #64D2FF Apple Dark Mode systemCyan
                pin_amber: Color32::from_rgb(255, 159, 10), // #FF9F0A Apple Dark Mode systemOrange
                success: Color32::from_rgb(48, 209, 88),    // #30D158 Apple Dark Mode systemGreen
                success_bg: Color32::from_rgb(24, 48, 32),  // #183020 tinted systemGreen surface
                success_border: Color32::from_rgb(42, 92, 56), // #2A5C38
                success_text: Color32::from_rgb(68, 219, 106), // #44DB6A
                warning_bg: Color32::from_rgb(54, 42, 22),  // #362A16 tinted systemOrange surface
                warning_border: Color32::from_rgb(108, 78, 28), // #6C4E1C
                warning_text: Color32::from_rgb(255, 180, 64), // #FFB440
                danger: Color32::from_rgb(255, 69, 58),     // #FF453A Apple Dark Mode systemRed
                danger_bg: Color32::from_rgb(56, 28, 28),   // #381C1C tinted systemRed surface
                danger_border: Color32::from_rgb(112, 46, 44), // #702E2C
                danger_text: Color32::from_rgb(255, 105, 97), // #FF6961
                info_bg: Color32::from_rgb(26, 42, 62),     // #1A2A3E
                info_border: Color32::from_rgb(46, 82, 124), // #2E527C
                keycap_bg: Color32::from_rgb(54, 54, 58),   // #36363A neutral dark keycap
                keycap_text: Color32::from_rgb(174, 174, 178), // #AEAEB2 Apple systemGray2
                on_accent: Color32::from_rgb(255, 255, 255),
            }
        } else {
            // macOS Light Mode (Apple HIG): neutral warm-cool paper grays + System Blue (#007AFF)
            Self {
                is_dark: false,
                window_bg: Color32::from_rgb(245, 245, 247), // #F5F5F7 macOS windowBackgroundColor
                surface_bg: Color32::from_rgb(255, 255, 255), // #FFFFFF elevated card / control surface
                card_bg: Color32::from_rgb(235, 235, 238), // #EBEBEE search pill / capsules / icon wells
                row_hover_bg: Color32::from_rgb(242, 242, 245), // #F2F2F5 neutral light hover
                row_selected_bg: Color32::from_rgb(232, 242, 255), // #E8F2FF subtle System Blue selection tint
                border: Color32::from_rgb(209, 209, 214), // #D1D1D6 Apple systemGray4 bezel
                border_subtle: Color32::from_rgb(229, 229, 234), // #E5E5EA Apple systemGray5 separator
                border_strong: Color32::from_rgb(153, 200, 255), // #99C8FF focused System Blue ring
                text_primary: Color32::from_rgb(29, 29, 31),     // #1D1D1F Apple primary labelColor
                text_secondary: Color32::from_rgb(110, 110, 115), // #6E6E73 Apple secondaryLabelColor
                text_muted: Color32::from_rgb(142, 142, 147), // #8E8E93 Apple tertiaryLabelColor
                accent: Color32::from_rgb(0, 122, 255),       // #007AFF Apple Light Mode systemBlue
                accent_hover: Color32::from_rgb(30, 140, 255), // #1E8CFF lighter systemBlue hover
                accent_active: Color32::from_rgb(0, 102, 214), // #0066D6 pressed systemBlue
                accent_soft: Color32::from_rgb(232, 242, 255), // #E8F2FF soft translucent blue pill fill
                accent_text: Color32::from_rgb(0, 102, 214),   // #0066D6 accessible blue label
                accent_secondary: Color32::from_rgb(50, 173, 230), // #32ADE6 Apple Light Mode systemCyan
                pin_amber: Color32::from_rgb(255, 149, 0), // #FF9500 Apple Light Mode systemOrange
                success: Color32::from_rgb(52, 199, 89),   // #34C759 Apple Light Mode systemGreen
                success_bg: Color32::from_rgb(232, 249, 236), // #E8F9EC tinted systemGreen surface
                success_border: Color32::from_rgb(179, 234, 193), // #B3EAC1
                success_text: Color32::from_rgb(36, 138, 61), // #248A3D accessible systemGreen text
                warning_bg: Color32::from_rgb(255, 244, 224), // #FFF4E0 tinted systemOrange surface
                warning_border: Color32::from_rgb(255, 216, 153), // #FFD899
                warning_text: Color32::from_rgb(178, 80, 0), // #B25000 accessible systemOrange text
                danger: Color32::from_rgb(255, 59, 48),    // #FF3B30 Apple Light Mode systemRed
                danger_bg: Color32::from_rgb(255, 236, 235), // #FFECEB tinted systemRed surface
                danger_border: Color32::from_rgb(255, 194, 190), // #FFC2BE
                danger_text: Color32::from_rgb(215, 0, 21), // #D70015 accessible systemRed text
                info_bg: Color32::from_rgb(232, 242, 255), // #E8F2FF
                info_border: Color32::from_rgb(179, 215, 255), // #B3D7FF
                keycap_bg: Color32::from_rgb(235, 235, 238), // #EBEBEE neutral light keycap
                keycap_text: Color32::from_rgb(110, 110, 115), // #6E6E73 Apple secondary gray
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

/// Subtle neutral ambient shadow matching macOS light cards.
pub fn card_shadow(palette: &EarthPalette) -> egui::Shadow {
    if palette.is_dark {
        egui::Shadow::NONE
    } else {
        egui::Shadow {
            offset: [0, 2],
            blur: 10,
            spread: 0,
            color: Color32::from_rgba_unmultiplied(0, 0, 0, 10),
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

/// Compact circular dismiss button with a geometrically centered vector `×` glyph
/// (avoids font baseline/ascent vertical drift inside small circular pills).
pub fn dismiss_button(ui: &mut Ui) -> egui::Response {
    let size = 20.0;
    let (rect, response) = ui.allocate_exact_size(egui::vec2(size, size), egui::Sense::click());
    let response = response.on_hover_cursor(egui::CursorIcon::PointingHand);
    if ui.is_rect_visible(rect) {
        let visuals = ui.style().interact(&response);
        let painter = ui.painter();
        let rounding = CornerRadius::same(255);
        painter.rect_filled(rect, rounding, visuals.weak_bg_fill);
        painter.rect_stroke(rect, rounding, visuals.bg_stroke, egui::StrokeKind::Inside);

        let center = rect.center();
        let half = 3.4;
        let stroke = Stroke::new(1.4, visuals.fg_stroke.color);
        painter.line_segment(
            [
                egui::pos2(center.x - half, center.y - half),
                egui::pos2(center.x + half, center.y + half),
            ],
            stroke,
        );
        painter.line_segment(
            [
                egui::pos2(center.x + half, center.y - half),
                egui::pos2(center.x - half, center.y + half),
            ],
            stroke,
        );
    }
    response
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
            append_run(
                &mut job,
                &run,
                run_matched,
                highlight_mode,
                palette,
                font_size,
                base_color,
            );
            run.clear();
        }
        run_matched = is_matched;
        run.push(ch);
    }
    if !run.is_empty() {
        append_run(
            &mut job,
            &run,
            run_matched,
            highlight_mode,
            palette,
            font_size,
            base_color,
        );
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
    Some(Color32::from_rgba_unmultiplied(
        rgba[0], rgba[1], rgba[2], rgba[3],
    ))
}

/// Installs crisp native system UI & CJK fonts (`PingFang SC` + `Menlo` on macOS,
/// `Segoe UI` + `Microsoft YaHei` + `Consolas` on Windows) ahead of egui's thin defaults
/// while preserving symbol and emoji fallbacks (`SystemFontProvider`, `NotoEmoji`).
pub fn install_crisp_fonts(ctx: &egui::Context) {
    let preferred = crate::platform::preferred_system_fonts();

    // Insert UI/CJK fonts in reverse priority order with `FontPriority::Highest` so index 0
    // becomes the first candidate in Proportional and sits cleanly as CJK fallback in Monospace.
    for candidate in preferred.ui.iter().rev() {
        let Some(font_data) = load_mmap_font_data(&candidate.path, candidate.postscript_name)
        else {
            continue;
        };
        let families = vec![
            InsertFontFamily {
                family: FontFamily::Proportional,
                priority: FontPriority::Highest,
            },
            InsertFontFamily {
                family: FontFamily::Monospace,
                priority: FontPriority::Highest,
            },
        ];
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
    let index = resolve_rasterizable_ttc_face(&mapped, target_ps_name)?;
    Some(FontData::from_blob(Arc::new(mapped), index))
}

/// Checks whether `path` is a font file containing standard vector outlines (`glyf`, `CFF `,
/// `CFF2`, or `VARC`) and (for `.ttc` collections) a matching `target_ps_name` face.
/// This filters out Apple `PingFangUI.ttc` (`hvgl`-only) on macOS 15+, which `epaint`/`skrifa`
/// cannot rasterize and would otherwise render blank glyphs.
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
pub(crate) fn font_file_supports_face(path: &Path, target_ps_name: &str) -> bool {
    let Ok(file) = std::fs::File::open(path) else {
        return false;
    };
    // SAFETY: OS system font collections are read-only immutable files.
    let Ok(mapped) = (unsafe { memmap2::Mmap::map(&file) }) else {
        return false;
    };
    resolve_rasterizable_ttc_face(&mapped, target_ps_name).is_some()
}

fn read_u16_be(bytes: &[u8], off: usize) -> Option<u16> {
    let slice = bytes.get(off..off.checked_add(2)?)?;
    Some(u16::from_be_bytes([slice[0], slice[1]]))
}

fn read_u32_be(bytes: &[u8], off: usize) -> Option<u32> {
    let slice = bytes.get(off..off.checked_add(4)?)?;
    Some(u32::from_be_bytes([slice[0], slice[1], slice[2], slice[3]]))
}

/// Returns `true` if the OpenType/TrueType face at `face_off` contains at least one outline
/// table supported by `epaint`/`skrifa` (`glyf`, `CFF `, `CFF2`, `VARC`).
fn face_has_supported_outlines(bytes: &[u8], face_off: usize) -> bool {
    let Some(num_tables) = read_u16_be(bytes, face_off.saturating_add(4)) else {
        return false;
    };
    let records_start = face_off.saturating_add(12);
    for i in 0..num_tables.min(128) as usize {
        let rec_off = records_start + i * 16;
        if let Some(tag) = bytes.get(rec_off..rec_off + 4)
            && matches!(tag, b"glyf" | b"CFF " | b"CFF2" | b"VARC")
        {
            return true;
        }
    }
    false
}

/// Finds the sub-font face index inside a `.ttc` TrueType Collection whose PostScript name
/// (`nameID == 6`) matches `target_ps_name` and has rasterizable outline tables.
/// Returns `Some(0)` for single-face outline fonts, or `None` if no rasterizable face matches.
pub(crate) fn resolve_rasterizable_ttc_face(bytes: &[u8], target_ps_name: &str) -> Option<u32> {
    if bytes.get(0..4) != Some(b"ttcf") {
        return face_has_supported_outlines(bytes, 0).then_some(0);
    }
    let num_fonts = read_u32_be(bytes, 8)?;
    let count = num_fonts.min(128);
    for face_idx in 0..count {
        let face_off_u32 = read_u32_be(bytes, 12 + face_idx as usize * 4)?;
        let face_off = face_off_u32 as usize;
        if face_postscript_matches(bytes, face_off, target_ps_name)
            && face_has_supported_outlines(bytes, face_off)
        {
            return Some(face_idx);
        }
    }
    None
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
        assert_eq!(
            opts_1x.color_transfer_function,
            FontColorTransferFunction::Gamma(0.75)
        );

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
            ctx_2x
                .global_style()
                .visuals
                .text_options
                .color_transfer_function,
            FontColorTransferFunction::Gamma(0.88)
        );

        let dark = EarthPalette::resolve(ThemeMode::Dark, true);
        dark.apply_to_ctx(&ctx_1x);
        assert_eq!(
            ctx_1x
                .global_style()
                .visuals
                .text_options
                .color_transfer_function,
            FontColorTransferFunction::DARK_MODE_DEFAULT
        );
    }

    #[test]
    fn system_theme_mode_follows_os_dark_flag_and_label_is_concise() {
        let light = EarthPalette::resolve(ThemeMode::System, false);
        let dark = EarthPalette::resolve(ThemeMode::System, true);
        assert!(!light.is_dark);
        assert!(dark.is_dark);
        assert_ne!(light.window_bg, dark.window_bg);

        assert_eq!(ThemeMode::System.label(), "跟随系统");
        crate::i18n::with_test_language(crate::i18n::ResolvedLanguage::EnUs, || {
            assert_eq!(ThemeMode::System.label(), "Follow System");
        });
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
        assert!(
            is_neutral(dark.window_bg),
            "window_bg should be neutral macOS charcoal"
        );
        assert!(
            is_neutral(dark.surface_bg),
            "surface_bg should be neutral macOS graphite"
        );
        assert!(
            is_neutral(dark.card_bg),
            "card_bg should be neutral macOS control fill"
        );
        assert!(
            is_neutral(dark.border),
            "border should be neutral macOS bezel"
        );
        assert!(
            is_neutral(dark.text_primary),
            "text_primary should be neutral labelColor"
        );
        assert!(
            dark.window_bg.r() < dark.surface_bg.r() && dark.surface_bg.r() < dark.card_bg.r(),
            "dark surfaces should step cleanly upward in elevation"
        );
        assert_eq!(dark.accent, Color32::from_rgb(10, 132, 255));

        let light = EarthPalette::resolve(ThemeMode::Light, false);
        assert!(!light.is_dark);
        assert!(
            is_neutral(light.window_bg),
            "light window_bg should be neutral macOS off-white"
        );
        assert!(
            is_neutral(light.card_bg),
            "light card_bg should be neutral macOS control fill"
        );
        assert!(
            is_neutral(light.border),
            "light border should be neutral macOS bezel"
        );
        assert!(
            is_neutral(light.text_primary),
            "light text_primary should be neutral labelColor"
        );
        assert_eq!(light.accent, Color32::from_rgb(0, 122, 255));
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn install_crisp_fonts_loads_pingfang_and_menlo_with_correct_ttc_indices() {
        let pingfang_ui = Path::new(
            "/System/Library/PrivateFrameworks/FontServices.framework/Resources/Reserved/PingFangUI.ttc",
        );
        if pingfang_ui.is_file() {
            assert!(
                !font_file_supports_face(pingfang_ui, "PingFangSC-Regular"),
                "hvgl-only PingFangUI.ttc must be rejected so epaint never loads invisible glyphs"
            );
        }

        let preferred = crate::platform::preferred_system_fonts();
        let pf = preferred
            .ui
            .first()
            .expect("macOS should resolve primary CJK UI font");
        let bytes = std::fs::read(&pf.path).unwrap();
        let idx = resolve_rasterizable_ttc_face(&bytes, pf.postscript_name).expect(
            "selected UI font must have rasterizable outlines and matching PostScript name",
        );
        if bytes.get(0..4) == Some(b"ttcf") {
            let face_off = read_u32_be(&bytes, 12 + idx as usize * 4)
                .expect("TTC face offset should exist") as usize;
            assert!(face_has_supported_outlines(&bytes, face_off));
            assert!(face_postscript_matches(
                &bytes,
                face_off,
                pf.postscript_name
            ));
        } else {
            assert_eq!(idx, 0, "single-face font must resolve to index 0");
            assert!(face_has_supported_outlines(&bytes, 0));
        }

        let ctx = egui::Context::default();
        install_crisp_fonts(&ctx);
        let raw = egui::RawInput::default();
        let mut text_width = 0.0;
        let mut out = ctx.run_ui(raw, |ui| {
            let resp = ui.label("测试 Crisp Font 字体");
            text_width = resp.rect.width();
        });
        out.textures_delta.clear();
        assert!(
            text_width > 20.0,
            "expected rendered CJK text width > 20px, got {text_width}"
        );

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

    #[test]
    fn status_toast_auto_dismisses_after_three_seconds_with_smooth_fade() {
        let mut toast = StatusToast::new("✓ 已粘贴到当前目标窗口", false);
        assert!(toast.shown_at.is_none());

        let rem0 = toast.tick(10.0).expect("toast visible on first render");
        assert!((rem0 - 3.0).abs() < 1e-6);
        assert_eq!(StatusToast::opacity_for_remaining(rem0), 1.0);

        let rem_mid = toast.tick(12.5).expect("toast still visible at 2.5s");
        assert!((rem_mid - 0.5).abs() < 1e-6);
        assert_eq!(StatusToast::opacity_for_remaining(rem_mid), 1.0);

        let rem_fade = toast.tick(12.89).expect("toast fading during final 220ms");
        let opacity = StatusToast::opacity_for_remaining(rem_fade);
        assert!(
            opacity > 0.0 && opacity < 1.0,
            "expected fade opacity, got {opacity}"
        );

        assert!(
            toast.tick(13.0).is_none(),
            "toast must expire cleanly at 3.0s"
        );
        assert!(toast.tick(14.0).is_none(), "toast stays expired past 3.0s");
    }

    #[test]
    fn dismiss_button_vector_cross_is_geometrically_centered() {
        let ctx = egui::Context::default();
        let palette = EarthPalette::resolve(ThemeMode::Light, false);
        palette.apply_to_ctx(&ctx);

        let mut btn_rect = egui::Rect::NOTHING;
        let mut out = ctx.run_ui(egui::RawInput::default(), |ui| {
            btn_rect = dismiss_button(ui).rect;
        });

        let segments: Vec<[egui::Pos2; 2]> = out
            .shapes
            .iter()
            .filter_map(|cs| match &cs.shape {
                egui::epaint::Shape::LineSegment { points, .. } => Some(*points),
                _ => None,
            })
            .collect();
        out.textures_delta.clear();

        assert_eq!(
            segments.len(),
            2,
            "dismiss button should paint two crossed vector strokes"
        );
        let c = btn_rect.center();
        for [a, b] in segments {
            let mid = egui::pos2((a.x + b.x) * 0.5, (a.y + b.y) * 0.5);
            assert!(
                (mid.x - c.x).abs() < 0.01 && (mid.y - c.y).abs() < 0.01,
                "cross stroke midpoint {mid:?} must equal button center {c:?}"
            );
        }
    }
}
