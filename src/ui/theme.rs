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
                window_bg: Color32::from_rgb(11, 16, 32),       // #0B1020 deep midnight slate
                surface_bg: Color32::from_rgb(19, 26, 45),      // #131A2D elevated dark card
                card_bg: Color32::from_rgb(27, 36, 59),         // #1B243B secondary container / pill fill
                row_hover_bg: Color32::from_rgb(31, 41, 68),    // #1F2944 subtle cobalt hover
                row_selected_bg: Color32::from_rgb(28, 41, 82), // #1C2952 royal blue tint selection
                border: Color32::from_rgb(42, 54, 86),          // #2A3656 dark outline
                border_subtle: Color32::from_rgb(30, 40, 68),   // #1E2844 hairline divider
                border_strong: Color32::from_rgb(69, 110, 253), // #456EFD active cobalt outline
                text_primary: Color32::from_rgb(241, 245, 249), // #F1F5F9 crisp cool white
                text_secondary: Color32::from_rgb(148, 163, 184), // #94A3B8 slate secondary
                text_muted: Color32::from_rgb(100, 116, 139),   // #64748B slate muted
                accent: Color32::from_rgb(69, 110, 253),        // #456EFD Electric Royal Blue
                accent_hover: Color32::from_rgb(102, 136, 254), // #6688FE brighter hover cobalt
                accent_active: Color32::from_rgb(47, 85, 235),  // #2F55EB pressed cobalt
                accent_soft: Color32::from_rgb(25, 37, 74),     // #19254A dark cobalt soft tint
                accent_text: Color32::from_rgb(129, 158, 255),  // #819EFF legible cobalt text
                accent_secondary: Color32::from_rgb(96, 165, 250), // #60A5FA periwinkle companion
                pin_amber: Color32::from_rgb(245, 158, 11),     // #F59E0B warm pro amber
                success: Color32::from_rgb(34, 197, 94),        // #22C55E vibrant emerald
                success_bg: Color32::from_rgb(17, 41, 30),      // dark emerald soft
                success_border: Color32::from_rgb(28, 77, 50),
                success_text: Color32::from_rgb(74, 222, 128),
                warning_bg: Color32::from_rgb(46, 33, 16),
                warning_border: Color32::from_rgb(92, 65, 24),
                warning_text: Color32::from_rgb(251, 191, 36),
                danger: Color32::from_rgb(239, 68, 68),         // #EF4444 vibrant coral red
                danger_bg: Color32::from_rgb(51, 20, 26),       // dark coral soft
                danger_border: Color32::from_rgb(102, 34, 42),
                danger_text: Color32::from_rgb(248, 113, 113),
                info_bg: Color32::from_rgb(24, 34, 68),
                info_border: Color32::from_rgb(49, 71, 138),
                keycap_bg: Color32::from_rgb(27, 36, 59),
                keycap_text: Color32::from_rgb(148, 163, 184),
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
        visuals.window_stroke = Stroke::new(1.0, self.border_subtle);
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

/// Renders a button with scoped padding so compact/tag/footer buttons keep comfortable breathing room.
pub fn padded_button(ui: &mut Ui, btn: egui::Button<'_>, padding: egui::Vec2) -> egui::Response {
    ui.scope(|ui| {
        ui.spacing_mut().button_padding = padding;
        ui.add(btn)
    })
    .inner
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
