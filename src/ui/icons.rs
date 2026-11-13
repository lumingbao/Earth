use crate::ui::theme::EarthPalette;
use egui::{pos2, vec2, Color32, CornerRadius, Rect, Sense, Stroke, Ui};

pub fn draw_earth_emblem(ui: &mut Ui, size: f32, palette: &EarthPalette, paused: bool) -> egui::Response {
    let (rect, response) = ui.allocate_exact_size(vec2(size, size), Sense::drag());
    if ui.is_rect_visible(rect) {
        let p = ui.painter();
        let center = rect.center();
        let r = size * 0.36;

        // Ant Design Daybreak Blue -> Cyan radial globe
        let (c_outer, c_inner) = if paused {
            (palette.text_muted, palette.text_secondary)
        } else {
            (palette.accent, palette.accent_secondary)
        };
        let steps = 10;
        let highlight = pos2(center.x - r * 0.28, center.y - r * 0.28);
        for i in 0..steps {
            let t = i as f32 / (steps - 1) as f32;
            let rr = r * (1.0 - t * 0.78);
            let c = lerp_color(c_outer, c_inner, t);
            let cc = pos2(
                center.x + (highlight.x - center.x) * t,
                center.y + (highlight.y - center.y) * t,
            );
            p.circle_filled(cc, rr, c);
        }

        // Meridian arc inside globe
        p.line_segment(
            [pos2(center.x, center.y - r * 0.85), pos2(center.x, center.y + r * 0.85)],
            Stroke::new(1.0, Color32::from_rgba_unmultiplied(255, 255, 255, 175)),
        );
        p.line_segment(
            [pos2(center.x - r * 0.85, center.y), pos2(center.x + r * 0.85, center.y)],
            Stroke::new(1.0, Color32::from_rgba_unmultiplied(255, 255, 255, 155)),
        );

        // Tilted orbital ring
        let ring_color = if paused {
            palette.text_secondary
        } else if palette.is_dark {
            palette.accent_hover
        } else {
            palette.accent_active
        };
        let pts: Vec<egui::Pos2> = (0..=20)
            .map(|i| {
                let t = -0.2 + (i as f32 / 20.0) * std::f32::consts::PI * 1.4;
                let rx = t.cos() * size * 0.48;
                let ry = t.sin() * size * 0.17;
                let angle = -0.38f32;
                let x = center.x + rx * angle.cos() - ry * angle.sin();
                let y = center.y + rx * angle.sin() + ry * angle.cos();
                pos2(x, y)
            })
            .collect();
        p.add(egui::Shape::line(pts, Stroke::new(1.6, ring_color)));
    }
    response
}

fn lerp_color(a: Color32, b: Color32, t: f32) -> Color32 {
    let l = |x: u8, y: u8| (x as f32 + (y as f32 - x as f32) * t).round() as u8;
    Color32::from_rgb(l(a.r(), b.r()), l(a.g(), b.g()), l(a.b(), b.b()))
}

pub fn icon_button(
    ui: &mut Ui,
    size: f32,
    active: bool,
    tooltip: &str,
    palette: &EarthPalette,
    draw_fn: impl FnOnce(&egui::Painter, Rect, Color32),
) -> egui::Response {
    let (rect, response) = ui.allocate_exact_size(vec2(size, size), Sense::click());
    if ui.is_rect_visible(rect) {
        let p = ui.painter();
        if active {
            p.rect_filled(rect, CornerRadius::same(6), palette.accent_soft);
            p.rect_stroke(
                rect,
                CornerRadius::same(6),
                Stroke::new(1.0, palette.border_strong),
                egui::StrokeKind::Inside,
            );
        } else if response.hovered() {
            p.rect_filled(rect, CornerRadius::same(6), palette.row_hover_bg);
        }
        let color = if active {
            palette.accent
        } else if response.hovered() {
            palette.accent_hover
        } else {
            palette.text_secondary
        };
        draw_fn(p, rect.shrink(size * 0.24), color);
    }
    response.on_hover_text(tooltip)
}

pub fn draw_split_pane_glyph(p: &egui::Painter, r: Rect, color: Color32) {
    p.rect_stroke(r, CornerRadius::same(2), Stroke::new(1.4, color), egui::StrokeKind::Inside);
    let split_x = r.left() + r.width() * 0.44;
    p.line_segment([pos2(split_x, r.top()), pos2(split_x, r.bottom())], Stroke::new(1.4, color));
}

pub fn draw_gear_glyph(p: &egui::Painter, r: Rect, color: Color32) {
    let c = r.center();
    let half = r.width().min(r.height()) * 0.52;
    let r_outer = half * 0.98;
    let r_valley = half * 0.72;
    let teeth = 8usize;
    let step = std::f32::consts::TAU / teeth as f32;

    let mut pts = Vec::with_capacity(teeth * 5);
    for i in 0..teeth {
        let base = i as f32 * step;
        let offsets = [
            (-0.24, r_valley),
            (-0.14, r_outer),
            (0.14, r_outer),
            (0.24, r_valley),
            (0.50, r_valley),
        ];
        for (frac, rad) in offsets {
            let a = base + frac * step;
            pts.push(pos2(c.x + a.cos() * rad, c.y + a.sin() * rad));
        }
    }

    p.add(egui::Shape::closed_line(pts, Stroke::new(1.3, color)));
    p.circle_stroke(c, half * 0.34, Stroke::new(1.35, color));
}

pub fn draw_pause_glyph(p: &egui::Painter, r: Rect, color: Color32) {
    let w = r.width() * 0.26;
    let left_bar = Rect::from_min_max(pos2(r.left() + r.width() * 0.18, r.top()), pos2(r.left() + r.width() * 0.18 + w, r.bottom()));
    let right_bar = Rect::from_min_max(pos2(r.right() - r.width() * 0.18 - w, r.top()), pos2(r.right() - r.width() * 0.18, r.bottom()));
    p.rect_filled(left_bar, CornerRadius::same(1), color);
    p.rect_filled(right_bar, CornerRadius::same(1), color);
}

pub fn draw_play_glyph(p: &egui::Painter, r: Rect, color: Color32) {
    let pts = vec![
        pos2(r.left() + r.width() * 0.2, r.top()),
        pos2(r.right() - r.width() * 0.1, r.center().y),
        pos2(r.left() + r.width() * 0.2, r.bottom()),
    ];
    p.add(egui::Shape::convex_polygon(pts, color, Stroke::NONE));
}

pub fn draw_shield_glyph(p: &egui::Painter, r: Rect, color: Color32) {
    let pts = vec![
        pos2(r.center().x, r.top()),
        pos2(r.right(), r.top() + r.height() * 0.24),
        pos2(r.right() - r.width() * 0.12, r.center().y + r.height() * 0.15),
        pos2(r.center().x, r.bottom()),
        pos2(r.left() + r.width() * 0.12, r.center().y + r.height() * 0.15),
        pos2(r.left(), r.top() + r.height() * 0.24),
    ];
    p.add(egui::Shape::closed_line(pts, Stroke::new(1.4, color)));
}

pub fn draw_search_glyph(p: &egui::Painter, r: Rect, color: Color32) {
    let rad = r.width().min(r.height()) * 0.34;
    let c = pos2(r.left() + rad + 1.0, r.top() + rad + 1.0);
    p.circle_stroke(c, rad, Stroke::new(1.5, color));
    let dir = std::f32::consts::FRAC_PI_4;
    p.line_segment(
        [
            pos2(c.x + dir.cos() * rad, c.y + dir.sin() * rad),
            pos2(r.right() - 1.0, r.bottom() - 1.0),
        ],
        Stroke::new(1.7, color),
    );
}

pub fn draw_pin_badge(p: &egui::Painter, r: Rect, color: Color32) {
    let c = r.center();
    p.circle_filled(pos2(c.x, c.y - r.height() * 0.18), r.width() * 0.24, color);
    p.line_segment(
        [pos2(c.x, c.y + r.height() * 0.05), pos2(c.x, r.bottom() - 1.0)],
        Stroke::new(1.6, color),
    );
}

pub fn draw_file_badge(p: &egui::Painter, r: Rect, color: Color32) {
    let fold = r.width() * 0.3;
    let pts = vec![
        r.left_top(),
        pos2(r.right() - fold, r.top()),
        pos2(r.right(), r.top() + fold),
        r.right_bottom(),
        r.left_bottom(),
    ];
    p.add(egui::Shape::closed_line(pts, Stroke::new(1.4, color)));
    p.line_segment(
        [pos2(r.right() - fold, r.top()), pos2(r.right() - fold, r.top() + fold)],
        Stroke::new(1.2, color),
    );
    p.line_segment(
        [pos2(r.right() - fold, r.top() + fold), pos2(r.right(), r.top() + fold)],
        Stroke::new(1.2, color),
    );
}

pub fn draw_link_badge(p: &egui::Painter, r: Rect, color: Color32) {
    let left_box = Rect::from_min_max(
        pos2(r.left(), r.center().y - r.height() * 0.25),
        pos2(r.center().x + 1.5, r.center().y + r.height() * 0.25),
    );
    let right_box = Rect::from_min_max(
        pos2(r.center().x - 1.5, r.center().y - r.height() * 0.25),
        pos2(r.right(), r.center().y + r.height() * 0.25),
    );
    p.rect_stroke(left_box, CornerRadius::same(3), Stroke::new(1.3, color), egui::StrokeKind::Inside);
    p.rect_stroke(right_box, CornerRadius::same(3), Stroke::new(1.3, color), egui::StrokeKind::Inside);
}
