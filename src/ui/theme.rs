use crate::model::{MatchHighlight, ThemeMode};
use egui::{Color32, CornerRadius, FontId, Stroke, Style, TextFormat, Ui, Visuals};
use std::collections::HashSet;

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
            Self {
                is_dark: true,
                window_bg: Color32::from_rgb(20, 20, 20),       // #141414 AntD dark base
                surface_bg: Color32::from_rgb(31, 31, 31),      // #1F1F1F AntD dark container
                card_bg: Color32::from_rgb(38, 38, 38),         // #262626 AntD dark fill tertiary
                row_hover_bg: Color32::from_rgb(44, 44, 44),    // AntD dark item hover
                row_selected_bg: Color32::from_rgb(17, 29, 44), // #111D2C AntD darkItemSelectedBg
                border: Color32::from_rgb(66, 66, 66),          // #424242 AntD dark colorBorder
                border_subtle: Color32::from_rgb(48, 48, 48),   // #303030 AntD dark colorBorderSecondary
                border_strong: Color32::from_rgb(21, 65, 126),  // #15417E AntD dark colorPrimaryBorder
                text_primary: Color32::from_rgb(230, 230, 230), // rgba(255,255,255,0.88)
                text_secondary: Color32::from_rgb(166, 166, 166), // rgba(255,255,255,0.65)
                text_muted: Color32::from_rgb(115, 115, 115),   // rgba(255,255,255,0.45)
                accent: Color32::from_rgb(22, 119, 255),        // #1677FF AntD brand blue
                accent_hover: Color32::from_rgb(64, 150, 255),  // #4096FF
                accent_active: Color32::from_rgb(21, 84, 173),  // #1554AD
                accent_soft: Color32::from_rgb(17, 29, 44),     // #111D2C dark colorPrimaryBg
                accent_text: Color32::from_rgb(64, 150, 255),   // #4096FF legible dark primary text
                accent_secondary: Color32::from_rgb(19, 168, 168), // #13A8A8 AntD dark cyan
                pin_amber: Color32::from_rgb(216, 150, 20),     // #D89614 AntD dark gold
                success: Color32::from_rgb(73, 170, 25),        // #49AA19 AntD dark green
                success_bg: Color32::from_rgb(22, 35, 18),      // #162312 AntD dark colorSuccessBg
                success_border: Color32::from_rgb(39, 73, 22),  // #274916 AntD dark colorSuccessBorder
                success_text: Color32::from_rgb(106, 190, 57),  // #6ABE39
                warning_bg: Color32::from_rgb(43, 33, 17),      // #2B2111 AntD dark colorWarningBg
                warning_border: Color32::from_rgb(89, 66, 20),  // #594214 AntD dark colorWarningBorder
                warning_text: Color32::from_rgb(232, 179, 57),  // #E8B339
                danger: Color32::from_rgb(220, 68, 70),         // #DC4446 AntD dark red
                danger_bg: Color32::from_rgb(42, 18, 21),       // #2A1215 AntD dark colorErrorBg
                danger_border: Color32::from_rgb(88, 24, 28),   // #58181C AntD dark colorErrorBorder
                danger_text: Color32::from_rgb(232, 110, 107),  // #E86E6B
                info_bg: Color32::from_rgb(17, 26, 44),         // #111A2C AntD dark colorInfoBg
                info_border: Color32::from_rgb(21, 57, 91),     // #15395B AntD dark colorInfoBorder
                keycap_bg: Color32::from_rgb(38, 38, 38),
                keycap_text: Color32::from_rgb(166, 166, 166),
                on_accent: Color32::from_rgb(255, 255, 255),
            }
        } else {
            Self {
                is_dark: false,
                window_bg: Color32::from_rgb(245, 245, 245),    // #F5F5F5 AntD surface-layout
                surface_bg: Color32::from_rgb(255, 255, 255),   // #FFFFFF AntD surface
                card_bg: Color32::from_rgb(250, 250, 250),      // #FAFAFA AntD surface-container
                row_hover_bg: Color32::from_rgb(245, 245, 245), // #F5F5F5 AntD controlItemBgHover
                row_selected_bg: Color32::from_rgb(230, 244, 255), // #E6F4FF AntD menu-item-selected
                border: Color32::from_rgb(217, 217, 217),       // #D9D9D9 AntD outline / colorBorder
                border_subtle: Color32::from_rgb(240, 240, 240), // #F0F0F0 AntD outline-variant
                border_strong: Color32::from_rgb(145, 202, 255), // #91CAFF AntD colorPrimaryBorder
                text_primary: Color32::from_rgb(31, 31, 31),    // #1F1F1F rgba(0,0,0,0.88)
                text_secondary: Color32::from_rgb(89, 89, 89),  // #595959 rgba(0,0,0,0.65)
                text_muted: Color32::from_rgb(140, 140, 140),   // #8C8C8C rgba(0,0,0,0.45)
                accent: Color32::from_rgb(22, 119, 255),        // #1677FF AntD Daybreak Blue
                accent_hover: Color32::from_rgb(64, 150, 255),  // #4096FF AntD colorPrimaryHover
                accent_active: Color32::from_rgb(9, 88, 217),   // #0958D9 AntD colorPrimaryActive
                accent_soft: Color32::from_rgb(230, 244, 255),  // #E6F4FF AntD colorPrimaryBg
                accent_text: Color32::from_rgb(9, 88, 217),     // #0958D9 AntD blue-7
                accent_secondary: Color32::from_rgb(19, 194, 194), // #13C2C2 AntD Cyan
                pin_amber: Color32::from_rgb(250, 173, 20),     // #FAAD14 AntD Calendula Gold
                success: Color32::from_rgb(82, 196, 26),        // #52C41A AntD Polar Green
                success_bg: Color32::from_rgb(246, 255, 237),   // #F6FFED AntD alert-success
                success_border: Color32::from_rgb(183, 235, 143), // #B7EB8F
                success_text: Color32::from_rgb(56, 158, 13),   // #389E0D
                warning_bg: Color32::from_rgb(255, 251, 230),   // #FFFBE6 AntD alert-warning
                warning_border: Color32::from_rgb(255, 229, 143), // #FFE58F
                warning_text: Color32::from_rgb(212, 136, 6),   // #D48806
                danger: Color32::from_rgb(255, 77, 79),         // #FF4D4F AntD Dust Red
                danger_bg: Color32::from_rgb(255, 242, 240),    // #FFF2F0 AntD alert-error
                danger_border: Color32::from_rgb(255, 204, 199), // #FFCCC7
                danger_text: Color32::from_rgb(207, 19, 34),    // #CF1322
                info_bg: Color32::from_rgb(230, 244, 255),      // #E6F4FF AntD alert-info
                info_border: Color32::from_rgb(145, 202, 255),  // #91CAFF
                keycap_bg: Color32::from_rgb(250, 250, 250),    // #FAFAFA
                keycap_text: Color32::from_rgb(89, 89, 89),     // #595959
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
        visuals.window_stroke = Stroke::new(1.0, self.border_subtle);
        visuals.window_corner_radius = CornerRadius::same(8);
        visuals.selection.bg_fill = self.accent_soft;
        visuals.selection.stroke = Stroke::new(1.0, self.accent);
        visuals.hyperlink_color = self.accent;
        visuals.text_cursor.stroke = Stroke::new(1.5, self.accent);

        // Noninteractive elements (separators / static frames) use subtle border (#F0F0F0 / #303030)
        visuals.widgets.noninteractive.bg_fill = self.surface_bg;
        visuals.widgets.noninteractive.fg_stroke = Stroke::new(1.0, self.text_primary);
        visuals.widgets.noninteractive.bg_stroke = Stroke::new(1.0, self.border_subtle);
        visuals.widgets.noninteractive.corner_radius = CornerRadius::same(6);

        // Default buttons / inputs / selects sit on surface_bg with 1px #D9D9D9 border and 6px radius
        visuals.widgets.inactive.bg_fill = self.surface_bg;
        visuals.widgets.inactive.weak_bg_fill = self.surface_bg;
        visuals.widgets.inactive.fg_stroke = Stroke::new(1.0, self.text_primary);
        visuals.widgets.inactive.bg_stroke = Stroke::new(1.0, self.border);
        visuals.widgets.inactive.corner_radius = CornerRadius::same(6);

        // AntD button-default-hover: tint text and border to #4096FF
        visuals.widgets.hovered.bg_fill = self.row_hover_bg;
        visuals.widgets.hovered.weak_bg_fill = self.surface_bg;
        visuals.widgets.hovered.fg_stroke = Stroke::new(1.0, self.accent_hover);
        visuals.widgets.hovered.bg_stroke = Stroke::new(1.0, self.accent_hover);
        visuals.widgets.hovered.corner_radius = CornerRadius::same(6);

        // AntD button-default-active: #0958D9 text/border on primary tint
        visuals.widgets.active.bg_fill = self.accent_soft;
        visuals.widgets.active.weak_bg_fill = self.accent_soft;
        visuals.widgets.active.fg_stroke = Stroke::new(1.0, self.accent_active);
        visuals.widgets.active.bg_stroke = Stroke::new(1.0, self.accent_active);
        visuals.widgets.active.corner_radius = CornerRadius::same(6);

        // Open ComboBox / dropdown trigger
        visuals.widgets.open.bg_fill = self.surface_bg;
        visuals.widgets.open.weak_bg_fill = self.surface_bg;
        visuals.widgets.open.fg_stroke = Stroke::new(1.0, self.accent);
        visuals.widgets.open.bg_stroke = Stroke::new(1.0, self.accent);
        visuals.widgets.open.corner_radius = CornerRadius::same(6);

        style.visuals = visuals;
        style.spacing.item_spacing = egui::vec2(6.0, 5.0);
        style.spacing.button_padding = egui::vec2(14.0, 6.0);
        style.spacing.scroll.bar_width = 5.0;
        style.spacing.scroll.bar_inner_margin = 2.0;

        ctx.set_theme(if self.is_dark {
            egui::Theme::Dark
        } else {
            egui::Theme::Light
        });
        ctx.set_global_style(style);
    }
}

/// Renders a button with scoped padding so compact/tag/footer buttons keep comfortable breathing room.
pub fn padded_button(ui: &mut Ui, btn: egui::Button<'_>, padding: egui::Vec2) -> egui::Response {
    ui.scope(|ui| {
        ui.spacing_mut().button_padding = padding;
        ui.add(btn)
    })
    .inner
}

/// Compact Ant Design `sm` button replacement for `ui.small_button` (which forces 0px vertical padding).
pub fn compact_button(ui: &mut Ui, text: impl Into<egui::WidgetText>) -> egui::Response {
    padded_button(
        ui,
        egui::Button::new(text).corner_radius(CornerRadius::same(6)),
        egui::vec2(10.0, 4.5),
    )
}

/// Compact dismiss `"×"` button with balanced padding around the glyph.
pub fn dismiss_button(ui: &mut Ui) -> egui::Response {
    padded_button(
        ui,
        egui::Button::new("×").corner_radius(CornerRadius::same(4)),
        egui::vec2(7.0, 3.0),
    )
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
