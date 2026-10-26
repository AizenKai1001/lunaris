//! Small vector icons drawn with the painter (the fonts have no icon glyphs, and pictures would need files): tool buttons, play controls,
//! what an object is in the hierarchy, what a file is in the project.
use crate::state::Kind;
use eframe::egui::{self, Color32, CornerRadius, Painter, Pos2, Rect, Shape, Stroke, StrokeKind, Vec2};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Icon {
    Hand,
    Move,
    Rotate,
    Scale,
    Play,
    Pause,
    Step,
    Stop,
    Cube,
    Empty,
    Camera,
    Light,
    Sprite,
    Text,
    Ui,
    Emitter,
    Audio,
    Collider,
    Tilemap,
    Environment,
    Folder,
    Scene,
    Model,
    Image,
    Sound,
    File,
    Info,
    Warn,
    Error,
    Gear,
    Search,
}

impl Icon {
    pub fn of_kind(k: Kind) -> Icon {
        match k {
            Kind::Empty => Icon::Empty,
            Kind::Camera => Icon::Camera,
            Kind::Light => Icon::Light,
            Kind::Mesh => Icon::Cube,
            Kind::Sprite => Icon::Sprite,
            Kind::Text => Icon::Text,
            Kind::Ui => Icon::Ui,
            Kind::Emitter => Icon::Emitter,
            Kind::Audio => Icon::Audio,
            Kind::Collider => Icon::Collider,
            Kind::Tilemap => Icon::Tilemap,
            Kind::Environment => Icon::Environment,
        }
    }

    /// The colour an object icon has in the hierarchy.
    pub fn tint(self) -> Color32 {
        match self {
            Icon::Cube | Icon::Model => Color32::from_rgb(110, 170, 235),
            Icon::Camera => Color32::from_rgb(200, 205, 215),
            Icon::Light => Color32::from_rgb(250, 215, 110),
            Icon::Sprite | Icon::Image | Icon::Tilemap => Color32::from_rgb(120, 205, 150),
            Icon::Text | Icon::Ui => Color32::from_rgb(190, 160, 235),
            Icon::Emitter => Color32::from_rgb(255, 160, 120),
            Icon::Audio | Icon::Sound => Color32::from_rgb(120, 215, 225),
            Icon::Collider => Color32::from_rgb(150, 215, 130),
            Icon::Environment => Color32::from_rgb(150, 195, 240),
            Icon::Folder => Color32::from_rgb(190, 175, 140),
            Icon::Scene => Color32::from_rgb(130, 180, 240),
            Icon::Info => Color32::from_rgb(170, 190, 215),
            Icon::Warn => Color32::from_rgb(235, 195, 90),
            Icon::Error => Color32::from_rgb(230, 90, 80),
            _ => Color32::from_rgb(170, 170, 170),
        }
    }
}

fn tri(p: &Painter, a: Pos2, b: Pos2, c: Pos2, fill: Color32) {
    p.add(Shape::convex_polygon(vec![a, b, c], fill, Stroke::NONE));
}

fn arrow(p: &Painter, from: Pos2, to: Pos2, head: f32, color: Color32, w: f32) {
    let d = (to - from).normalized();
    let side = Vec2::new(-d.y, d.x);
    p.line_segment([from, to - d * head * 0.5], Stroke::new(w, color));
    tri(p, to, to - d * head + side * head * 0.55, to - d * head - side * head * 0.55, color);
}

/// Draw `icon` inside `r` (a square works best) in `color`.
pub fn draw(p: &Painter, r: Rect, icon: Icon, color: Color32) {
    let c = r.center();
    let u = r.width().min(r.height()) / 16.0; // one design unit
    let s = |w: f32| Stroke::new(w * u.max(0.75), color);
    let pt = |x: f32, y: f32| Pos2::new(c.x + x * u, c.y + y * u);
    match icon {
        Icon::Play => tri(p, pt(-4.0, -6.0), pt(-4.0, 6.0), pt(6.0, 0.0), color),
        Icon::Pause => {
            p.rect_filled(Rect::from_min_max(pt(-5.0, -6.0), pt(-1.5, 6.0)), CornerRadius::ZERO, color);
            p.rect_filled(Rect::from_min_max(pt(1.5, -6.0), pt(5.0, 6.0)), CornerRadius::ZERO, color);
        }
        Icon::Step => {
            tri(p, pt(-6.0, -6.0), pt(-6.0, 6.0), pt(3.0, 0.0), color);
            p.rect_filled(Rect::from_min_max(pt(4.0, -6.0), pt(6.5, 6.0)), CornerRadius::ZERO, color);
        }
        Icon::Stop => {
            p.rect_filled(Rect::from_min_max(pt(-5.0, -5.0), pt(5.0, 5.0)), CornerRadius::same(1), color);
        }
        Icon::Hand => {
            // a palm with four fingers and a thumb
            p.rect_filled(Rect::from_min_max(pt(-5.0, -1.0), pt(5.0, 6.0)), CornerRadius::same(3), color);
            for i in 0..4 {
                let x = -4.0 + i as f32 * 2.8;
                p.rect_filled(Rect::from_min_max(pt(x - 0.9, -6.5 + if i == 0 || i == 3 { 1.5 } else { 0.0 }), pt(x + 0.9, 0.0)), CornerRadius::same(1), color);
            }
            p.line_segment([pt(-5.0, 2.0), pt(-7.0, -0.5)], s(1.8));
        }
        Icon::Move => {
            for (dx, dy) in [(1.0, 0.0), (-1.0, 0.0), (0.0, 1.0), (0.0, -1.0)] {
                arrow(p, pt(0.0, 0.0), pt(dx * 7.0, dy * 7.0), 3.5 * u, color, 1.4 * u.max(0.75));
            }
        }
        Icon::Rotate => {
            let n = 14;
            let pts: Vec<Pos2> = (0..=n).map(|i| {
                let a = -0.4 + i as f32 / n as f32 * 4.4;
                pt(a.cos() * 6.0, a.sin() * 6.0)
            }).collect();
            p.add(Shape::line(pts.clone(), s(1.6)));
            let (e, prev) = (pts[n], pts[n - 1]);
            let d = (e - prev).normalized();
            let side = Vec2::new(-d.y, d.x);
            tri(p, e + d * 3.0 * u, e + side * 3.0 * u, e - side * 3.0 * u, color);
        }
        Icon::Scale => {
            p.rect_stroke(Rect::from_min_max(pt(-7.0, -1.0), pt(1.0, 7.0)), CornerRadius::ZERO, s(1.4), StrokeKind::Middle);
            arrow(p, pt(-2.0, 2.0), pt(7.0, -7.0), 4.0 * u, color, 1.4 * u.max(0.75));
        }
        Icon::Cube | Icon::Model => {
            // an isometric cube, three faces in three greys of the colour
            let top = vec![pt(0.0, -7.0), pt(6.5, -3.5), pt(0.0, 0.0), pt(-6.5, -3.5)];
            let left = vec![pt(-6.5, -3.5), pt(0.0, 0.0), pt(0.0, 7.0), pt(-6.5, 3.5)];
            let right = vec![pt(6.5, -3.5), pt(0.0, 0.0), pt(0.0, 7.0), pt(6.5, 3.5)];
            p.add(Shape::convex_polygon(top, color, Stroke::NONE));
            p.add(Shape::convex_polygon(left, color.gamma_multiply(0.55), Stroke::NONE));
            p.add(Shape::convex_polygon(right, color.gamma_multiply(0.78), Stroke::NONE));
        }
        Icon::Empty => {
            let pts = vec![pt(0.0, -7.0), pt(6.5, -3.5), pt(6.5, 3.5), pt(0.0, 7.0), pt(-6.5, 3.5), pt(-6.5, -3.5), pt(0.0, -7.0)];
            p.add(Shape::line(pts, s(1.1)));
            p.line_segment([pt(0.0, 0.0), pt(0.0, 7.0)], s(1.1));
            p.line_segment([pt(0.0, 0.0), pt(-6.5, -3.5)], s(1.1));
            p.line_segment([pt(0.0, 0.0), pt(6.5, -3.5)], s(1.1));
        }
        Icon::Camera => {
            p.rect_filled(Rect::from_min_max(pt(-7.0, -4.0), pt(2.0, 4.0)), CornerRadius::same(1), color);
            tri(p, pt(3.0, 0.0), pt(7.5, -4.0), pt(7.5, 4.0), color);
        }
        Icon::Light => {
            p.circle_filled(pt(0.0, -1.0), 3.8 * u, color);
            for k in 0..8 {
                let a = k as f32 * std::f32::consts::FRAC_PI_4;
                p.line_segment([pt(a.cos() * 5.4, -1.0 + a.sin() * 5.4), pt(a.cos() * 7.6, -1.0 + a.sin() * 7.6)], s(1.1));
            }
            p.rect_filled(Rect::from_min_max(pt(-2.0, 4.0), pt(2.0, 6.5)), CornerRadius::same(1), color.gamma_multiply(0.7));
        }
        Icon::Sprite | Icon::Image => {
            p.rect_stroke(Rect::from_min_max(pt(-7.0, -6.0), pt(7.0, 6.0)), CornerRadius::same(1), s(1.2), StrokeKind::Middle);
            p.circle_filled(pt(-3.0, -2.0), 1.6 * u, color);
            p.add(Shape::convex_polygon(vec![pt(-6.5, 5.5), pt(-1.0, 0.0), pt(2.0, 3.0), pt(4.0, 1.0), pt(6.5, 5.5)], color.gamma_multiply(0.8), Stroke::NONE));
        }
        Icon::Text => {
            p.line_segment([pt(-5.0, -5.0), pt(5.0, -5.0)], s(1.8));
            p.line_segment([pt(0.0, -5.0), pt(0.0, 6.0)], s(1.8));
        }
        Icon::Ui => {
            p.rect_stroke(Rect::from_min_max(pt(-7.0, -5.0), pt(7.0, 5.0)), CornerRadius::same(2), s(1.2), StrokeKind::Middle);
            p.line_segment([pt(-4.0, 0.0), pt(4.0, 0.0)], s(1.2));
        }
        Icon::Emitter => {
            for (x, y, r) in [(-4.0, 3.0, 1.5), (0.0, -1.0, 2.0), (4.0, 3.5, 1.3), (3.0, -4.5, 1.2), (-3.5, -4.0, 1.0)] {
                p.circle_filled(pt(x, y), r * u, color);
            }
        }
        Icon::Audio | Icon::Sound => {
            p.add(Shape::convex_polygon(vec![pt(-7.0, -2.5), pt(-3.0, -2.5), pt(1.0, -6.0), pt(1.0, 6.0), pt(-3.0, 2.5), pt(-7.0, 2.5)], color, Stroke::NONE));
            for k in 1..=2 {
                let rr = 3.0 + k as f32 * 2.2;
                let pts: Vec<Pos2> = (0..=8).map(|i| {
                    let a = -0.9 + i as f32 / 8.0 * 1.8;
                    pt(1.5 + a.cos() * rr * 0.8, a.sin() * rr)
                }).collect();
                p.add(Shape::line(pts, s(1.0)));
            }
        }
        Icon::Collider => {
            p.rect_stroke(Rect::from_min_max(pt(-6.0, -6.0), pt(6.0, 6.0)), CornerRadius::ZERO, Stroke::new(1.2 * u.max(0.75), color), StrokeKind::Middle);
            for (x, y) in [(-6.0, -6.0), (6.0, -6.0), (-6.0, 6.0), (6.0, 6.0)] {
                p.circle_filled(pt(x, y), 1.4 * u, color);
            }
        }
        Icon::Tilemap => {
            for i in 0..2 {
                for j in 0..2 {
                    let (x, y) = (-7.0 + i as f32 * 7.5, -7.0 + j as f32 * 7.5);
                    p.rect_filled(Rect::from_min_max(pt(x, y), pt(x + 6.5, y + 6.5)), CornerRadius::same(1), color.gamma_multiply(if (i + j) % 2 == 0 { 1.0 } else { 0.55 }));
                }
            }
        }
        Icon::Environment => {
            p.circle_filled(pt(-2.0, -2.0), 3.3 * u, color);
            p.rect_filled(Rect::from_min_max(pt(-7.0, 2.5), pt(7.0, 6.0)), CornerRadius::same(2), color.gamma_multiply(0.6));
            p.circle_filled(pt(2.5, 3.5), 3.0 * u, color.gamma_multiply(0.6));
        }
        Icon::Folder => {
            p.rect_filled(Rect::from_min_max(pt(-7.0, -5.0), pt(-1.0, -3.0)), CornerRadius::same(1), color.gamma_multiply(0.8));
            p.rect_filled(Rect::from_min_max(pt(-7.0, -3.5), pt(7.0, 5.5)), CornerRadius::same(1), color);
        }
        Icon::Scene => {
            p.rect_stroke(Rect::from_min_max(pt(-5.5, -7.0), pt(5.5, 7.0)), CornerRadius::same(1), s(1.2), StrokeKind::Middle);
            p.add(Shape::convex_polygon(vec![pt(0.0, -3.0), pt(3.5, -1.0), pt(0.0, 1.0), pt(-3.5, -1.0)], color, Stroke::NONE));
            p.add(Shape::convex_polygon(vec![pt(-3.5, -1.0), pt(0.0, 1.0), pt(0.0, 5.0), pt(-3.5, 3.0)], color.gamma_multiply(0.6), Stroke::NONE));
            p.add(Shape::convex_polygon(vec![pt(3.5, -1.0), pt(0.0, 1.0), pt(0.0, 5.0), pt(3.5, 3.0)], color.gamma_multiply(0.8), Stroke::NONE));
        }
        Icon::File => {
            p.add(Shape::closed_line(vec![pt(-5.5, -7.0), pt(2.0, -7.0), pt(5.5, -3.5), pt(5.5, 7.0), pt(-5.5, 7.0)], s(1.2)));
            p.line_segment([pt(-3.0, 0.0), pt(3.0, 0.0)], s(1.0));
            p.line_segment([pt(-3.0, 3.0), pt(3.0, 3.0)], s(1.0));
        }
        Icon::Info => {
            p.circle_stroke(pt(0.0, 0.0), 6.5 * u, s(1.3));
            p.circle_filled(pt(0.0, -3.0), 1.0 * u, color);
            p.line_segment([pt(0.0, -0.5), pt(0.0, 4.0)], s(1.6));
        }
        Icon::Warn => {
            p.add(Shape::convex_polygon(vec![pt(0.0, -7.0), pt(7.5, 6.0), pt(-7.5, 6.0)], color, Stroke::NONE));
            p.line_segment([pt(0.0, -2.5), pt(0.0, 1.5)], Stroke::new(1.6 * u.max(0.75), Color32::from_rgb(40, 30, 10)));
            p.circle_filled(pt(0.0, 3.8), 0.9 * u, Color32::from_rgb(40, 30, 10));
        }
        Icon::Error => {
            p.circle_filled(pt(0.0, 0.0), 7.0 * u, color);
            p.line_segment([pt(-2.8, -2.8), pt(2.8, 2.8)], Stroke::new(1.6 * u.max(0.75), Color32::WHITE));
            p.line_segment([pt(2.8, -2.8), pt(-2.8, 2.8)], Stroke::new(1.6 * u.max(0.75), Color32::WHITE));
        }
        Icon::Gear => {
            p.circle_stroke(pt(0.0, 0.0), 3.0 * u, s(1.6));
            for k in 0..8 {
                let a = k as f32 * std::f32::consts::FRAC_PI_4;
                p.line_segment([pt(a.cos() * 4.5, a.sin() * 4.5), pt(a.cos() * 7.0, a.sin() * 7.0)], s(2.0));
            }
        }
        Icon::Search => {
            p.circle_stroke(pt(-1.5, -1.5), 4.5 * u, s(1.4));
            p.line_segment([pt(1.8, 1.8), pt(6.5, 6.5)], s(1.8));
        }
    }
}

/// A square button with an icon. `selected` shows it pressed in (the active tool).
pub fn button(ui: &mut egui::Ui, icon: Icon, selected: bool, tip: &str) -> egui::Response {
    let (rect, resp) = ui.allocate_exact_size(Vec2::splat(24.0), egui::Sense::click());
    let hovered = resp.hovered();
    // flat, as Unreal's toolbars are: no box until the pointer is over it, and the tool that is on is a bright blue picture
    let fill = if selected { Color32::from_rgb(28, 28, 28) } else if hovered { crate::theme::BUTTON_HOVER } else { Color32::TRANSPARENT };
    ui.painter().rect_filled(rect, CornerRadius::same(3), fill);
    draw(ui.painter(), rect.shrink(4.0), icon, if selected { Color32::from_rgb(38, 187, 255) } else { crate::theme::TEXT });
    resp.on_hover_text(tip)
}

/// A button with an icon on a coloured background (the Play button).
pub fn colored_button(ui: &mut egui::Ui, icon: Icon, fill: Color32, enabled: bool, tip: &str) -> egui::Response {
    let (rect, resp) = ui.allocate_exact_size(Vec2::new(32.0, 24.0), if enabled { egui::Sense::click() } else { egui::Sense::hover() });
    // (a button that is not on is flat; one that is on keeps its colour; Play is Unreal's green)
    let on = fill != crate::theme::BUTTON;
    let f = if on { if resp.hovered() { fill.gamma_multiply(1.2) } else { fill } } else if resp.hovered() && enabled { crate::theme::BUTTON_HOVER } else { Color32::TRANSPARENT };
    ui.painter().rect_filled(rect, CornerRadius::same(3), f);
    let ink = if !enabled { Color32::from_rgb(78, 78, 78) } else if on { crate::theme::TEXT_BRIGHT } else if icon == Icon::Play { Color32::from_rgb(98, 204, 72) } else { crate::theme::TEXT_BRIGHT };
    draw(ui.painter(), rect.shrink2(Vec2::new(8.0, 4.0)), icon, ink);
    resp.on_hover_text(tip)
}
