use crate::model::{MatchHighlight, ThemeMode};
use egui::{Color32, CornerRadius, FontId, Stroke, Style, TextFormat, Visuals};
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
    pub border_strong: Color32,
    pub text_primary: Color32,
    pub text_secondary: Color32,
    pub text_muted: Color32,
    pub accent: Color32,
    pub accent_soft: Color32,
    pub accent_secondary: Color32,
    pub pin_amber: Color32,
    pub danger: Color32,
    pub keycap_bg: Color32,
    pub keycap_text: Color32,
}

impl EarthPalette {
    pub fn resolve(mode: ThemeMode, _system_dark: bool) -> Self {
        let dark = matches!(mode, ThemeMode::Dark);
        if dark {
            Self {
                is_dark: true,
                window_bg: Color32::from_rgb(24, 24, 27),
                surface_bg: Color32::from_rgb(32, 32, 35),
                card_bg: Color32::from_rgb(39, 39, 42),
                row_hover_bg: Color32::from_rgb(46, 46, 51),
                row_selected_bg: Color32::from_rgb(56, 56, 62),
                border: Color32::from_rgb(52, 52, 58),
                border_strong: Color32::from_rgb(72, 72, 80),
                text_primary: Color32::from_rgb(244, 244, 245),
                text_secondary: Color32::from_rgb(161, 161, 170),
                text_muted: Color32::from_rgb(113, 113, 122),
                accent: Color32::from_rgb(228, 228, 231),
                accent_soft: Color32::from_rgb(56, 56, 62),
                accent_secondary: Color32::from_rgb(161, 161, 170),
                pin_amber: Color32::from_rgb(161, 161, 170),
                danger: Color32::from_rgb(248, 113, 113),
                keycap_bg: Color32::from_rgb(46, 46, 51),
                keycap_text: Color32::from_rgb(180, 180, 188),
            }
        } else {
            Self {
                is_dark: false,
                window_bg: Color32::from_rgb(247, 247, 248),
                surface_bg: Color32::from_rgb(255, 255, 255),
                card_bg: Color32::from_rgb(239, 239, 241),
                row_hover_bg: Color32::from_rgb(241, 241, 243),
                row_selected_bg: Color32::from_rgb(232, 232, 236),
                border: Color32::from_rgb(230, 230, 233),
                border_strong: Color32::from_rgb(212, 212, 216),
                text_primary: Color32::from_rgb(24, 24, 27),
                text_secondary: Color32::from_rgb(82, 82, 91),
                text_muted: Color32::from_rgb(142, 142, 147),
                accent: Color32::from_rgb(39, 39, 42),
                accent_soft: Color32::from_rgb(230, 230, 234),
                accent_secondary: Color32::from_rgb(113, 113, 122),
                pin_amber: Color32::from_rgb(113, 113, 122),
                danger: Color32::from_rgb(220, 38, 38),
                keycap_bg: Color32::from_rgb(239, 239, 241),
                keycap_text: Color32::from_rgb(113, 113, 122),
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

        visuals.window_fill = self.window_bg;
        visuals.panel_fill = self.window_bg;
        visuals.extreme_bg_color = self.surface_bg;
        visuals.faint_bg_color = self.card_bg;
        visuals.window_stroke = Stroke::new(1.0, self.border);
        visuals.window_corner_radius = CornerRadius::same(14);
        visuals.selection.bg_fill = self.accent_soft;
        visuals.selection.stroke = Stroke::new(1.0, self.accent);

        visuals.widgets.noninteractive.bg_fill = self.surface_bg;
        visuals.widgets.noninteractive.fg_stroke = Stroke::new(1.0, self.text_primary);
        visuals.widgets.noninteractive.bg_stroke = Stroke::new(1.0, self.border);
        visuals.widgets.noninteractive.corner_radius = CornerRadius::same(8);

        visuals.widgets.inactive.bg_fill = self.card_bg;
        visuals.widgets.inactive.weak_bg_fill = self.card_bg;
        visuals.widgets.inactive.fg_stroke = Stroke::new(1.0, self.text_primary);
        visuals.widgets.inactive.bg_stroke = Stroke::new(1.0, self.border);
        visuals.widgets.inactive.corner_radius = CornerRadius::same(8);

        visuals.widgets.hovered.bg_fill = self.row_hover_bg;
        visuals.widgets.hovered.weak_bg_fill = self.row_hover_bg;
        visuals.widgets.hovered.fg_stroke = Stroke::new(1.0, self.text_primary);
        visuals.widgets.hovered.bg_stroke = Stroke::new(1.0, self.border_strong);
        visuals.widgets.hovered.corner_radius = CornerRadius::same(8);

        visuals.widgets.active.bg_fill = self.accent_soft;
        visuals.widgets.active.weak_bg_fill = self.accent_soft;
        visuals.widgets.active.fg_stroke = Stroke::new(1.0, self.accent);
        visuals.widgets.active.bg_stroke = Stroke::new(1.0, self.border_strong);
        visuals.widgets.active.corner_radius = CornerRadius::same(8);

        style.visuals = visuals;
        style.spacing.item_spacing = egui::vec2(6.0, 5.0);
        style.spacing.button_padding = egui::vec2(9.0, 5.0);
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

pub fn build_highlighted_layout_job(
    text: &str,
    matched_indices: &HashSet<usize>,
    highlight_mode: MatchHighlight,
    palette: &EarthPalette,
    font_size: f32,
    _selected: bool,
) -> egui::text::LayoutJob {
    let mut job = egui::text::LayoutJob::default();
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
        fmt.color = palette.accent;
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
