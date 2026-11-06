//! The 2D board: tiles, ownership, buildings, pawns, dice and card popups.
//! Geometry follows the original SVG board (a 1110×1110 view box).

use bevy_egui::egui::{
    self, emath::Rot2, epaint::TextShape, Align2, Color32, FontId, Painter, Pos2, Rect, RichText, Sense, Shape,
    StrokeKind, Vec2,
};
use monopoly_core::{
    mortgage_value, tile, unmortgage_cost, GameState, TileKind, BOARD, BOARD_SIZE, HOTEL_LEVEL, TOKENS,
};

use super::MonoUi;
use crate::net::Session;
use crate::theme::{self, bold, rgb, semibold, stroke};

const VIEW: f32 = 1110.0;
const CORNER: f32 = 150.0;
const LONG: f32 = 150.0;
const SHORT: f32 = 90.0;
const STRIPE: f32 = 28.0;
const FAR: f32 = VIEW - CORNER;

const CREAM: Color32 = Color32::from_rgb(0xfb, 0xf8, 0xef);
const INK: Color32 = Color32::from_rgb(0x1a, 0x1d, 0x22);
const FELT: Color32 = Color32::from_rgb(0xcf, 0xe6, 0xd2);

#[derive(Clone, Copy, PartialEq)]
enum Side {
    Bottom,
    Left,
    Top,
    Right,
    Corner,
}

/// Tile rectangle in view-box units.
fn tile_layout(i: usize) -> (Rect, Side) {
    let r = |x: f32, y: f32, w: f32, h: f32| Rect::from_min_size(Pos2::new(x, y), Vec2::new(w, h));
    match i {
        0 => (r(FAR, FAR, CORNER, CORNER), Side::Corner),
        1..=9 => (r(FAR - SHORT * (i as f32), FAR, SHORT, LONG), Side::Bottom),
        10 => (r(0.0, FAR, CORNER, CORNER), Side::Corner),
        11..=19 => (r(0.0, FAR - SHORT * ((i - 10) as f32), LONG, SHORT), Side::Left),
        20 => (r(0.0, 0.0, CORNER, CORNER), Side::Corner),
        21..=29 => (r(CORNER + SHORT * ((i - 21) as f32), 0.0, SHORT, LONG), Side::Top),
        30 => (r(FAR, 0.0, CORNER, CORNER), Side::Corner),
        _ => (r(FAR, CORNER + SHORT * ((i - 31) as f32), LONG, SHORT), Side::Right),
    }
}

fn side_angle(side: Side) -> f32 {
    use std::f32::consts::{FRAC_PI_2, PI};
    match side {
        Side::Bottom | Side::Corner => 0.0,
        Side::Left => FRAC_PI_2,
        Side::Top => PI,
        Side::Right => -FRAC_PI_2,
    }
}

fn stripe_rect(r: Rect, side: Side) -> Option<Rect> {
    Some(match side {
        Side::Bottom => Rect::from_min_size(r.min, Vec2::new(r.width(), STRIPE)),
        Side::Left => Rect::from_min_size(Pos2::new(r.right() - STRIPE, r.top()), Vec2::new(STRIPE, r.height())),
        Side::Top => Rect::from_min_size(Pos2::new(r.left(), r.bottom() - STRIPE), Vec2::new(r.width(), STRIPE)),
        Side::Right => Rect::from_min_size(r.min, Vec2::new(STRIPE, r.height())),
        Side::Corner => return None,
    })
}

/// Outer edge strip (for ownership colour).
fn outer_rect(r: Rect, side: Side) -> Option<Rect> {
    let t = 9.0;
    Some(match side {
        Side::Bottom => Rect::from_min_size(Pos2::new(r.left(), r.bottom() - t), Vec2::new(r.width(), t)),
        Side::Left => Rect::from_min_size(r.min, Vec2::new(t, r.height())),
        Side::Top => Rect::from_min_size(r.min, Vec2::new(r.width(), t)),
        Side::Right => Rect::from_min_size(Pos2::new(r.right() - t, r.top()), Vec2::new(t, r.height())),
        Side::Corner => return None,
    })
}

struct Mapper {
    origin: Pos2,
    scale: f32,
}

impl Mapper {
    fn p(&self, p: Pos2) -> Pos2 {
        self.origin + p.to_vec2() * self.scale
    }
    fn r(&self, r: Rect) -> Rect {
        Rect::from_min_max(self.p(r.min), self.p(r.max))
    }
    fn s(&self, v: f32) -> f32 {
        v * self.scale
    }
}

/// Text centred at `center` and rotated by `angle`, wrapped to `wrap` width.
fn rotated_text(painter: &Painter, center: Pos2, text: &str, font: FontId, color: Color32, angle: f32, wrap: f32) {
    let galley = painter.layout(text.to_string(), font, color, wrap);
    let half = galley.size() / 2.0;
    let pos = center - Rot2::from_angle(angle) * half;
    painter.add(Shape::Text(TextShape::new(pos, galley, color).with_angle(angle)));
}

fn glyph(kind: &TileKind, name: &str) -> Option<(&'static str, Color32)> {
    Some(match kind {
        TileKind::Station { .. } => ("🚂", INK),
        TileKind::Utility { .. } => {
            if name.starts_with("Электро") {
                ("💡", INK)
            } else {
                ("🚰", INK)
            }
        }
        TileKind::Chance => ("?", Color32::from_rgb(0xd9, 0x6a, 0x1c)),
        TileKind::Chest => ("📦", INK),
        TileKind::Tax { .. } => ("💰", INK),
        _ => return None,
    })
}

pub fn central(ctx: &egui::Context, state: &GameState, session: &Session, ui: &mut MonoUi, time: f32) {
    egui::CentralPanel::default()
        .frame(egui::Frame::new().fill(Color32::from_rgb(16, 30, 24)))
        .show(ctx, |area| {
            let avail = area.max_rect();
            let size = (avail.width().min(avail.height()) - 24.0).max(200.0);
            let board = Rect::from_center_size(avail.center(), Vec2::splat(size));
            let m = Mapper {
                origin: board.min,
                scale: size / VIEW,
            };
            let response = area.allocate_rect(board, Sense::hover());
            let painter = area.painter_at(avail);
            // Shadow and felt.
            painter.rect_filled(board.translate(Vec2::new(0.0, 8.0)).expand(4.0), 10, Color32::from_black_alpha(110));
            painter.rect_filled(board, 4, FELT);

            let hover_pos = response.hover_pos();
            ui.hovered_tile = None;
            for t in BOARD.iter() {
                let (r, side) = tile_layout(t.index);
                let rect = m.r(r);
                if hover_pos.is_some_and(|p| rect.contains(p)) {
                    ui.hovered_tile = Some(t.index);
                }
                draw_tile(&painter, &m, t.index, r, side, state);
            }

            // Centre decorations: logo and card decks.
            let center = m.p(Pos2::new(VIEW / 2.0, VIEW / 2.0));
            rotated_text(
                &painter,
                center,
                "МОНОПОЛИЯ",
                bold(m.s(84.0)),
                Color32::from_rgba_unmultiplied(0xb8, 0x1e, 0x1e, 225),
                -std::f32::consts::FRAC_PI_4,
                f32::INFINITY,
            );
            deck_card(&painter, &m, Pos2::new(380.0, 330.0), "ШАНС", Color32::from_rgb(0xf7, 0x94, 0x1d), "?");
            deck_card(&painter, &m, Pos2::new(730.0, 780.0), "КАЗНА", Color32::from_rgb(0x4a, 0x90, 0xd9), "📦");

            draw_ownership(&painter, &m, state, time);
            draw_pawns(&painter, &m, state, ui, time);
            draw_dice(&painter, &m, state, ui);

            if let Some(i) = ui.hovered_tile {
                if ui.card_popup.is_none() {
                    tile_tooltip(ctx, state, i);
                }
            }
            if let Some((deck, text, who, age)) = ui.card_popup.clone() {
                let alpha = (age / 0.25).min(1.0) * ((6.0 - age) / 0.5).clamp(0.0, 1.0);
                let rect = Rect::from_center_size(center, Vec2::new(m.s(460.0), m.s(270.0)));
                let painter = ctx.layer_painter(egui::LayerId::new(egui::Order::Foreground, egui::Id::new("card-popup")));
                let a = |c: Color32| theme::with_alpha(c, (c.a() as f32 * alpha) as u8);
                painter.rect_filled(rect.translate(Vec2::new(0.0, 6.0)), 14, a(Color32::from_black_alpha(120)));
                painter.rect_filled(rect, 14, a(CREAM));
                let band = Rect::from_min_size(rect.min, Vec2::new(rect.width(), m.s(56.0)));
                let band_color = if deck.starts_with("Шанс") { Color32::from_rgb(0xf7, 0x94, 0x1d) } else { Color32::from_rgb(0x4a, 0x90, 0xd9) };
                painter.rect_filled(band, egui::CornerRadius { nw: 14, ne: 14, sw: 0, se: 0 }, a(band_color));
                painter.text(band.center(), Align2::CENTER_CENTER, deck.to_uppercase(), bold(m.s(28.0)), a(Color32::WHITE));
                let galley = painter.layout(text, semibold(m.s(24.0)), a(INK), rect.width() - m.s(50.0));
                painter.galley(Pos2::new(rect.center().x - galley.size().x / 2.0, band.bottom() + m.s(26.0)), galley, a(INK));
                painter.text(
                    Pos2::new(rect.center().x, rect.bottom() - m.s(22.0)),
                    Align2::CENTER_CENTER,
                    format!("Тянет: {who}"),
                    theme::regular(m.s(18.0)),
                    a(Color32::from_rgb(90, 95, 105)),
                );
                if response.clicked() || ctx.input(|i| i.pointer.any_click()) && age > 0.4 {
                    ui.card_popup = None;
                }
            }
            let _ = session;
        });
}

fn deck_card(painter: &Painter, m: &Mapper, at: Pos2, label: &str, color: Color32, symbol: &str) {
    let angle = -std::f32::consts::FRAC_PI_4;
    let rot = Rot2::from_angle(angle);
    let c = m.p(at);
    let half = Vec2::new(m.s(120.0), m.s(75.0));
    let corners: Vec<Pos2> = [(-1.0, -1.0), (1.0, -1.0), (1.0, 1.0), (-1.0, 1.0)]
        .iter()
        .map(|(x, y)| c + rot * Vec2::new(x * half.x, y * half.y))
        .collect();
    painter.add(Shape::convex_polygon(corners.clone(), theme::with_alpha(color, 70), stroke(m.s(3.0), theme::with_alpha(color, 200))));
    rotated_text(painter, c - rot * Vec2::new(0.0, m.s(18.0)), symbol, bold(m.s(40.0)), theme::with_alpha(color, 230), angle, f32::INFINITY);
    rotated_text(painter, c + rot * Vec2::new(0.0, m.s(36.0)), label, bold(m.s(22.0)), theme::with_alpha(color, 230), angle, f32::INFINITY);
}

fn draw_tile(painter: &Painter, m: &Mapper, i: usize, r: Rect, side: Side, state: &GameState) {
    let t = tile(i);
    let rect = m.r(r);
    painter.rect_filled(rect, 0, CREAM);
    painter.rect_stroke(rect, 0, stroke(m.s(1.5).max(1.0), INK), StrokeKind::Inside);
    if let (Some(g), Some(sr)) = (t.group(), stripe_rect(r, side)) {
        let srect = m.r(sr);
        painter.rect_filled(srect, 0, rgb(g.rgb()));
        painter.rect_stroke(srect, 0, stroke(m.s(1.5).max(1.0), INK), StrokeKind::Inside);
        // Buildings on the stripe.
        let level = state.building_level(i);
        if level == HOTEL_LEVEL {
            let c = srect.center();
            let along = if matches!(side, Side::Bottom | Side::Top) { Vec2::new(m.s(34.0), m.s(16.0)) } else { Vec2::new(m.s(16.0), m.s(34.0)) };
            let h = Rect::from_center_size(c, along);
            painter.rect_filled(h, 2, Color32::from_rgb(0xd8, 0x22, 0x22));
            painter.rect_stroke(h, 2, stroke(1.0, Color32::WHITE), StrokeKind::Inside);
        } else if level > 0 {
            for k in 0..level {
                let off = (k as f32 - (level as f32 - 1.0) / 2.0) * m.s(19.0);
                let d = if matches!(side, Side::Bottom | Side::Top) { Vec2::new(off, 0.0) } else { Vec2::new(0.0, off) };
                let h = Rect::from_center_size(srect.center() + d, Vec2::splat(m.s(15.0)));
                painter.rect_filled(h, 2, Color32::from_rgb(0x1f, 0xa8, 0x4a));
                painter.rect_stroke(h, 2, stroke(1.0, Color32::WHITE), StrokeKind::Inside);
            }
        }
    }

    let angle = side_angle(side);
    let rot = Rot2::from_angle(angle);
    let c = rect.center();
    if side == Side::Corner {
        let (title, sym) = match t.kind {
            TileKind::Go => ("СТАРТ", "➡"),
            TileKind::Jail => ("ТЮРЬМА", "▦"),
            TileKind::FreeParking => ("БЕСПЛАТНАЯ СТОЯНКА", "🅿"),
            _ => ("В ТЮРЬМУ!", "👮"),
        };
        let a = if i == 0 || i == 20 { -std::f32::consts::FRAC_PI_4 } else { std::f32::consts::FRAC_PI_4 };
        rotated_text(painter, c - Rot2::from_angle(a) * Vec2::new(0.0, m.s(28.0)), title, bold(m.s(17.0)), INK, a, m.s(130.0));
        rotated_text(painter, c + Rot2::from_angle(a) * Vec2::new(0.0, m.s(24.0)), sym, bold(m.s(40.0)), if i == 0 { Color32::from_rgb(0xc0, 0x20, 0x20) } else { INK }, a, f32::INFINITY);
        return;
    }
    // In the rotated tile frame "down" points to the board edge.
    let local = |dy: f32| c + rot * Vec2::new(0.0, dy);
    let name_y = if t.group().is_some() { -24.0 } else { -38.0 };
    rotated_text(painter, local(m.s(name_y)), t.name, semibold(m.s(13.5)), INK, angle, m.s(SHORT - 8.0));
    if let Some((g, color)) = glyph(&t.kind, t.name) {
        rotated_text(painter, local(m.s(10.0)), g, bold(m.s(30.0)), color, angle, f32::INFINITY);
    }
    if let Some(price) = t.price() {
        rotated_text(painter, local(m.s(52.0)), &format!("₽{price}"), theme::regular(m.s(13.0)), Color32::from_rgb(0x3a, 0x3f, 0x47), angle, f32::INFINITY);
    } else if let TileKind::Tax { amount } = t.kind {
        rotated_text(painter, local(m.s(52.0)), &format!("₽{amount}"), theme::regular(m.s(13.0)), Color32::from_rgb(0x3a, 0x3f, 0x47), angle, f32::INFINITY);
    }
    if state.is_mortgaged(i) {
        painter.rect_filled(rect, 0, Color32::from_rgba_unmultiplied(40, 40, 40, 110));
        rotated_text(painter, c, "ЗАЛОЖЕНО", bold(m.s(12.0)), Color32::from_rgb(0xff, 0xe0, 0x60), angle - std::f32::consts::FRAC_PI_4 * 0.0, f32::INFINITY);
    }
}

fn draw_ownership(painter: &Painter, m: &Mapper, state: &GameState, _time: f32) {
    for p in state.players.iter().filter(|p| !p.bankrupt) {
        for &t in &p.owned {
            let (r, side) = tile_layout(t);
            if let Some(o) = outer_rect(r, side) {
                painter.rect_filled(m.r(o), 0, rgb(p.rgb()));
            }
        }
    }
}

fn tile_center(i: f32) -> Pos2 {
    let i = (i.round() as usize) % BOARD_SIZE;
    let (r, side) = tile_layout(i);
    // Keep pawns off the colour stripe.
    let shift = match side {
        Side::Bottom => Vec2::new(0.0, 18.0),
        Side::Left => Vec2::new(-18.0, 0.0),
        Side::Top => Vec2::new(0.0, -18.0),
        Side::Right => Vec2::new(18.0, 0.0),
        Side::Corner => Vec2::ZERO,
    };
    r.center() + shift
}

fn draw_pawns(painter: &Painter, m: &Mapper, state: &GameState, ui: &MonoUi, time: f32) {
    // Group pawns by displayed tile so several on one tile don't overlap.
    let mut by_tile: std::collections::BTreeMap<usize, Vec<usize>> = Default::default();
    for (idx, p) in state.players.iter().enumerate() {
        if p.bankrupt {
            continue;
        }
        let pos = ui.pawn_pos.get(&p.id).copied().unwrap_or(p.position as f32);
        by_tile.entry(pos.round() as usize % BOARD_SIZE).or_default().push(idx);
    }
    let current = state.current_player().map(|p| p.id);
    for (t, idxs) in by_tile {
        let base = tile_center(t as f32);
        let n = idxs.len();
        for (k, idx) in idxs.into_iter().enumerate() {
            let p = &state.players[idx];
            let offset = if n == 1 {
                Vec2::ZERO
            } else {
                let a = k as f32 / n as f32 * std::f32::consts::TAU;
                Vec2::new(a.cos(), a.sin()) * 22.0
            };
            let mut pos = m.p(base + offset);
            let moving = ui.moving == Some(p.id);
            if moving {
                pos.y -= m.s(26.0) * (ui.hop * std::f32::consts::PI).sin();
            }
            let r = m.s(20.0);
            let color = rgb(p.rgb());
            if current == Some(p.id) {
                let pulse = 1.0 + (time * 4.0).sin() * 0.08;
                painter.circle_stroke(pos, r * 1.45 * pulse, stroke(m.s(3.0), theme::with_alpha(theme::GOLD, 200)));
            }
            painter.circle_filled(pos + Vec2::new(m.s(2.0), m.s(4.0)), r, Color32::from_black_alpha(90));
            painter.circle_filled(pos, r, color);
            painter.circle_stroke(pos, r, stroke(m.s(2.5), Color32::WHITE));
            painter.text(pos, Align2::CENTER_CENTER, TOKENS[p.token].symbol, FontId::proportional(r * 1.15), Color32::WHITE);
            if p.in_jail {
                painter.text(pos + Vec2::new(r, -r), Align2::CENTER_CENTER, "🔒", FontId::proportional(r * 0.8), INK);
            }
        }
    }
}

fn draw_die(painter: &Painter, center: Pos2, size: f32, value: u8) {
    let rect = Rect::from_center_size(center, Vec2::splat(size));
    painter.rect_filled(rect.translate(Vec2::new(2.0, 4.0)), size * 0.18, Color32::from_black_alpha(90));
    painter.rect_filled(rect, size * 0.18, Color32::WHITE);
    painter.rect_stroke(rect, size * 0.18, stroke(1.5, Color32::from_rgb(200, 200, 205)), StrokeKind::Inside);
    let d = size * 0.27;
    let pips: &[(f32, f32)] = match value {
        1 => &[(0.0, 0.0)],
        2 => &[(-1.0, -1.0), (1.0, 1.0)],
        3 => &[(-1.0, -1.0), (0.0, 0.0), (1.0, 1.0)],
        4 => &[(-1.0, -1.0), (1.0, -1.0), (-1.0, 1.0), (1.0, 1.0)],
        5 => &[(-1.0, -1.0), (1.0, -1.0), (0.0, 0.0), (-1.0, 1.0), (1.0, 1.0)],
        _ => &[(-1.0, -1.0), (1.0, -1.0), (-1.0, 0.0), (1.0, 0.0), (-1.0, 1.0), (1.0, 1.0)],
    };
    for (x, y) in pips {
        painter.circle_filled(center + Vec2::new(x * d, y * d), size * 0.09, if value == 1 { Color32::from_rgb(0xc0, 0x20, 0x20) } else { INK });
    }
}

fn draw_dice(painter: &Painter, m: &Mapper, state: &GameState, ui: &MonoUi) {
    let (a, b) = if ui.dice_anim > 0.0 {
        ui.dice_faces
    } else if let Some(r) = state.last_roll {
        (r.a, r.b)
    } else {
        return;
    };
    let c = m.p(Pos2::new(VIEW / 2.0, VIEW / 2.0 + 150.0));
    let size = m.s(62.0);
    let wobble = if ui.dice_anim > 0.0 { (ui.dice_anim * 40.0).sin() * m.s(6.0) } else { 0.0 };
    draw_die(painter, c + Vec2::new(-size * 0.65, wobble), size, a);
    draw_die(painter, c + Vec2::new(size * 0.65, -wobble), size, b);
}

/// Property card shown when hovering a tile.
fn tile_tooltip(ctx: &egui::Context, state: &GameState, i: usize) {
    let t = tile(i);
    egui::Area::new(egui::Id::new("tile-tip"))
        .order(egui::Order::Tooltip)
        .fixed_pos(ctx.pointer_hover_pos().unwrap_or_default() + Vec2::new(18.0, 18.0))
        .interactable(false)
        .show(ctx, |ui| {
            egui::Frame::new()
                .fill(CREAM)
                .stroke(stroke(1.0, INK))
                .corner_radius(10)
                .inner_margin(egui::Margin::same(12))
                .show(ui, |ui| {
                    ui.set_width(230.0);
                    ui.visuals_mut().override_text_color = Some(INK);
                    if let Some(g) = t.group() {
                        let (rect, _) = ui.allocate_exact_size(Vec2::new(230.0, 34.0), Sense::hover());
                        ui.painter().rect_filled(rect, 6, rgb(g.rgb()));
                        ui.painter().text(rect.center(), Align2::CENTER_CENTER, t.name.to_uppercase(), bold(15.0), if theme::is_light(rgb(g.rgb())) { INK } else { Color32::WHITE });
                    } else {
                        ui.label(RichText::new(t.name).font(bold(17.0)));
                    }
                    let row = |ui: &mut egui::Ui, k: &str, v: String| {
                        ui.horizontal(|ui| {
                            ui.label(k);
                            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                ui.label(RichText::new(v).font(semibold(14.0)));
                            });
                        });
                    };
                    match t.kind {
                        TileKind::Street { group, price, rent } => {
                            row(ui, "Цена", format!("₽{price}"));
                            row(ui, "Рента", format!("₽{}", rent.base));
                            row(ui, "С монополией", format!("₽{}", rent.mono));
                            for (k, v) in rent.houses.iter().enumerate() {
                                row(ui, &format!("С {} дом.", k + 1), format!("₽{v}"));
                            }
                            row(ui, "С отелем", format!("₽{}", rent.hotel));
                            row(ui, "Дом / отель", format!("₽{}", group.house_cost()));
                        }
                        TileKind::Station { price } => {
                            row(ui, "Цена", format!("₽{price}"));
                            for n in 1..=4 {
                                row(ui, &format!("{n} вокзал(а)"), format!("₽{}", 25 * 2i32.pow(n - 1)));
                            }
                        }
                        TileKind::Utility { price } => {
                            row(ui, "Цена", format!("₽{price}"));
                            ui.label("1 предприятие: 4× кубики");
                            ui.label("2 предприятия: 10× кубики");
                        }
                        TileKind::Tax { amount } => {
                            ui.label(format!("Налог: ₽{amount}"));
                        }
                        TileKind::Chance => {
                            ui.label("Вытяните карту «Шанс»");
                        }
                        TileKind::Chest => {
                            ui.label("Вытяните карту «Общественная казна»");
                        }
                        TileKind::Go => {
                            ui.label("Пройдите СТАРТ — получите ₽200");
                        }
                        TileKind::Jail => {
                            ui.label("Тюрьма / просто посещение");
                        }
                        TileKind::FreeParking => {
                            ui.label("Ничего не происходит");
                        }
                        TileKind::GoToJail => {
                            ui.label("Отправляйтесь в тюрьму");
                        }
                    }
                    if t.is_purchasable() {
                        row(ui, "Залог", format!("₽{}", mortgage_value(i)));
                        row(ui, "Выкуп залога", format!("₽{}", unmortgage_cost(i)));
                        ui.separator();
                        match state.owner_of(i) {
                            Some(o) => {
                                ui.label(RichText::new(format!("Владелец: {} {}", o.symbol(), o.name)).font(semibold(14.0)));
                                if state.is_mortgaged(i) {
                                    ui.label(RichText::new("ЗАЛОЖЕНО").color(Color32::from_rgb(0xc0, 0x40, 0x20)));
                                } else if let Some((_, rent)) = state.compute_rent(i, 7) {
                                    let note = if matches!(t.kind, TileKind::Utility { .. }) { " (при 7 на кубиках)" } else { "" };
                                    ui.label(format!("Текущая рента: ₽{rent}{note}"));
                                }
                            }
                            None => {
                                ui.label("Не куплено — можно приобрести");
                            }
                        }
                    }
                });
        });
}
