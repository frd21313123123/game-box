//! Procedurally painted playing cards and casino chips (no image assets).
//! Shapes and the texture pipeline are shared with the Uno cards.

use bevy::prelude::*;
use blackjack_core::{Card, Rank, Suit};
use tiny_skia::{GradientStop, LinearGradient, Path, PathBuilder, Pixmap, Point, SpreadMode, Stroke, Transform};

use crate::uno::art::{
    card_body, circle, mask_from, new_painter, paint, rounded_rect, sk, to_image, Painter, BORDER, RADIUS, TEX_H, TEX_W,
};

pub const FACE_COUNT: usize = 52;

const RED: [u8; 3] = [0xC4, 0x1E, 0x2A];
const BLACK: [u8; 3] = [0x1B, 0x1C, 0x24];
const PAPER: [u8; 3] = [0xFD, 0xFB, 0xF6];
const GOLD: [u8; 3] = [0xD9, 0xA4, 0x3A];

/// Index of a face texture: suit × 13 + rank.
pub fn face_index(card: &Card) -> usize {
    card.suit.index() * 13 + card.rank.index()
}

fn ink(suit: Suit) -> [u8; 3] {
    if suit.is_red() { RED } else { BLACK }
}

/// The outline of a suit symbol centred at (cx, cy), `s` units tall, as
/// separate parts (each is filled on its own, so overlaps never cancel out).
fn suit_paths(suit: Suit, cx: f32, cy: f32, s: f32) -> Vec<Path> {
    // Points are given in a unit box (-0.5..0.5, y down) and scaled.
    let p = |x: f32, y: f32| (cx + x * s, cy + y * s);
    let heart = |flip: f32, dy: f32, k: f32| {
        let mut pb = PathBuilder::new();
        let q = |x: f32, y: f32| p(x * k, y * flip * k + dy);
        let (x, y) = q(0.0, 0.5);
        pb.move_to(x, y);
        let segs = [
            [(-0.12, 0.36), (-0.5, 0.12), (-0.5, -0.17)],
            [(-0.5, -0.4), (-0.36, -0.52), (-0.24, -0.52)],
            [(-0.1, -0.52), (0.0, -0.42), (0.0, -0.3)],
            [(0.0, -0.42), (0.1, -0.52), (0.24, -0.52)],
            [(0.36, -0.52), (0.5, -0.4), (0.5, -0.17)],
            [(0.5, 0.12), (0.12, 0.36), (0.0, 0.5)],
        ];
        for seg in segs {
            let a = q(seg[0].0, seg[0].1);
            let b = q(seg[1].0, seg[1].1);
            let c = q(seg[2].0, seg[2].1);
            pb.cubic_to(a.0, a.1, b.0, b.1, c.0, c.1);
        }
        pb.close();
        pb.finish().expect("valid heart")
    };
    let stem = || {
        let mut pb = PathBuilder::new();
        let (x0, y0) = p(0.0, 0.05);
        let (x1, y1) = p(-0.2, 0.5);
        let (x2, y2) = p(0.2, 0.5);
        pb.move_to(x0, y0);
        pb.quad_to(cx - 0.02 * s, cy + 0.38 * s, x1, y1);
        pb.line_to(x2, y2);
        pb.quad_to(cx + 0.02 * s, cy + 0.38 * s, x0, y0);
        pb.close();
        pb.finish().expect("valid stem")
    };
    match suit {
        Suit::Hearts => vec![heart(1.0, 0.0, 1.0)],
        Suit::Spades => vec![heart(-1.0, -0.1, 0.84), stem()],
        Suit::Diamonds => {
            let mut pb = PathBuilder::new();
            let pts = [p(0.0, -0.5), p(0.38, 0.0), p(0.0, 0.5), p(-0.38, 0.0)];
            pb.move_to(pts[0].0, pts[0].1);
            for (x, y) in &pts[1..] {
                pb.line_to(*x, *y);
            }
            pb.close();
            vec![pb.finish().expect("valid diamond")]
        }
        Suit::Clubs => {
            let mut parts: Vec<Path> = [(0.0, -0.24, 0.235), (-0.24, 0.06, 0.235), (0.24, 0.06, 0.235), (0.0, 0.02, 0.12)]
                .into_iter()
                .map(|(x, y, r)| {
                    let (x, y) = p(x, y);
                    circle(x, y, r * s)
                })
                .collect();
            parts.push(stem());
            parts
        }
    }
}

fn draw_suit(p: &mut Painter, suit: Suit, cx: f32, cy: f32, s: f32, upside_down: bool) {
    let t = if upside_down {
        Transform::from_rotate_at(180.0, cx, cy)
    } else {
        Transform::identity()
    };
    for part in suit_paths(suit, cx, cy, s) {
        p.fill_t(&part, sk(ink(suit), 255), t);
    }
}

/// Pip positions of the number cards in a unit grid (x: -1..1, y: -1..1).
fn pips(rank: Rank) -> &'static [(f32, f32)] {
    const T: f32 = 1.0 / 3.0;
    match rank {
        Rank::Two => &[(0.0, -1.0), (0.0, 1.0)],
        Rank::Three => &[(0.0, -1.0), (0.0, 0.0), (0.0, 1.0)],
        Rank::Four => &[(-1.0, -1.0), (1.0, -1.0), (-1.0, 1.0), (1.0, 1.0)],
        Rank::Five => &[(-1.0, -1.0), (1.0, -1.0), (0.0, 0.0), (-1.0, 1.0), (1.0, 1.0)],
        Rank::Six => &[(-1.0, -1.0), (1.0, -1.0), (-1.0, 0.0), (1.0, 0.0), (-1.0, 1.0), (1.0, 1.0)],
        Rank::Seven => &[(-1.0, -1.0), (1.0, -1.0), (0.0, -0.5), (-1.0, 0.0), (1.0, 0.0), (-1.0, 1.0), (1.0, 1.0)],
        Rank::Eight => &[
            (-1.0, -1.0), (1.0, -1.0), (0.0, -0.5), (-1.0, 0.0), (1.0, 0.0), (0.0, 0.5), (-1.0, 1.0), (1.0, 1.0),
        ],
        Rank::Nine => &[
            (-1.0, -1.0), (1.0, -1.0), (-1.0, -T), (1.0, -T), (0.0, 0.0), (-1.0, T), (1.0, T), (-1.0, 1.0), (1.0, 1.0),
        ],
        Rank::Ten => &[
            (-1.0, -1.0), (1.0, -1.0), (0.0, -2.0 * T), (-1.0, -T), (1.0, -T), (-1.0, T), (1.0, T), (0.0, 2.0 * T),
            (-1.0, 1.0), (1.0, 1.0),
        ],
        _ => &[],
    }
}

fn paint_face(card: Card) -> Pixmap {
    let mut p = new_painter();
    let (w, h) = (TEX_W as f32, TEX_H as f32);
    let (cx, cy) = (w / 2.0, h / 2.0);
    let color = ink(card.suit);
    let rgba = [color[0], color[1], color[2], 255];

    p.fill(&rounded_rect(0.0, 0.0, w, h, RADIUS), sk(PAPER, 255));
    p.stroke(
        &rounded_rect(4.0, 4.0, w - 8.0, h - 8.0, RADIUS - 3.0),
        sk([0xD8, 0xD2, 0xC4], 255),
        2.5,
        Transform::identity(),
    );

    // Corner indices: rank above a small suit, mirrored in the far corner.
    let label = card.rank.label();
    for (x, y, rot) in [(30.0, 40.0, 0.0f32), (w - 30.0, h - 40.0, std::f32::consts::PI)] {
        let size = if label.len() > 1 { 38.0 } else { 46.0 };
        p.text(label, x, y, size, rgba, rot);
        let dy = if rot == 0.0 { 40.0 } else { -40.0 };
        draw_suit(&mut p, card.suit, x, y + dy, 28.0, rot != 0.0);
    }

    match card.rank {
        Rank::Ace => {
            let size = if card.suit == Suit::Spades { 150.0 } else { 120.0 };
            draw_suit(&mut p, card.suit, cx, cy, size, false);
        }
        Rank::Jack | Rank::Queen | Rank::King => {
            let (fx, fy, fw, fh) = (58.0, 64.0, w - 116.0, h - 128.0);
            let frame = rounded_rect(fx, fy, fw, fh, 12.0);
            let tint = if card.suit.is_red() { [0xFB, 0xE9, 0xE2] } else { [0xE6, 0xEA, 0xF4] };
            let shader = LinearGradient::new(
                Point::from_xy(0.0, fy),
                Point::from_xy(0.0, fy + fh),
                vec![
                    GradientStop::new(0.0, sk(tint, 255)),
                    GradientStop::new(1.0, sk([0xFF, 0xF4, 0xD8], 255)),
                ],
                SpreadMode::Pad,
                Transform::identity(),
            )
            .expect("valid gradient");
            p.fill_shader(&frame, shader);
            p.stroke(&frame, sk(GOLD, 255), 4.0, Transform::identity());
            p.stroke(&rounded_rect(fx + 7.0, fy + 7.0, fw - 14.0, fh - 14.0, 8.0), sk(color, 120), 1.5, Transform::identity());
            draw_crown(&mut p, card.rank, cx, fy + 52.0, color);
            p.text(label, cx + 3.0, cy + 14.0, 120.0, [0, 0, 0, 40], 0.0);
            p.text(label, cx, cy + 10.0, 120.0, rgba, 0.0);
            draw_suit(&mut p, card.suit, cx, fy + fh - 38.0, 40.0, false);
        }
        rank => {
            let (gx, gy) = (50.0, 112.0);
            let pip = 56.0;
            for &(x, y) in pips(rank) {
                draw_suit(&mut p, card.suit, cx + x * gx, cy + y * gy, pip, y > 0.01);
            }
        }
    }
    p.pm
}

/// A small crown (king), tiara (queen) or feather band (jack) above the letter.
fn draw_crown(p: &mut Painter, rank: Rank, cx: f32, cy: f32, color: [u8; 3]) {
    let mut pb = PathBuilder::new();
    match rank {
        Rank::King => {
            let pts = [(-34.0, 16.0), (-34.0, -10.0), (-17.0, 4.0), (0.0, -18.0), (17.0, 4.0), (34.0, -10.0), (34.0, 16.0)];
            pb.move_to(cx + pts[0].0, cy + pts[0].1);
            for (x, y) in &pts[1..] {
                pb.line_to(cx + x, cy + y);
            }
            pb.close();
        }
        Rank::Queen => {
            pb.move_to(cx - 30.0, cy + 14.0);
            pb.quad_to(cx - 26.0, cy - 6.0, cx - 14.0, cy + 2.0);
            pb.quad_to(cx, cy - 22.0, cx + 14.0, cy + 2.0);
            pb.quad_to(cx + 26.0, cy - 6.0, cx + 30.0, cy + 14.0);
            pb.close();
        }
        _ => {
            pb.move_to(cx - 30.0, cy + 14.0);
            pb.line_to(cx - 30.0, cy + 2.0);
            pb.quad_to(cx, cy - 14.0, cx + 30.0, cy + 2.0);
            pb.line_to(cx + 30.0, cy + 14.0);
            pb.close();
        }
    }
    let path = pb.finish().expect("valid crown");
    p.fill(&path, sk(GOLD, 255));
    p.stroke(&path, sk(color, 200), 2.0, Transform::identity());
    for dx in [-17.0, 0.0, 17.0] {
        p.fill(&circle(cx + dx, cy + 9.0, 3.5), sk(color, 255));
    }
}

fn paint_back() -> Pixmap {
    let mut p = new_painter();
    let (w, h) = (TEX_W as f32, TEX_H as f32);
    let (cx, cy) = (w / 2.0, h / 2.0);
    card_body(&mut p, [0x9E, 0x1B, 0x2C], [0x5C, 0x0D, 0x18]);
    let inner = rounded_rect(BORDER, BORDER, w - 2.0 * BORDER, h - 2.0 * BORDER, RADIUS - BORDER + 4.0);
    let mask = mask_from(&inner);
    // Diamond lattice.
    let mut pb = PathBuilder::new();
    let step = 26.0;
    let mut k = -h;
    while k < w + h {
        pb.move_to(k, 0.0);
        pb.line_to(k + h, h);
        pb.move_to(k + h, 0.0);
        pb.line_to(k, h);
        k += step;
    }
    let lattice = pb.finish().expect("lattice");
    let stroke = Stroke { width: 1.5, ..Stroke::default() };
    p.pm.stroke_path(&lattice, &paint(tiny_skia::Color::from_rgba8(255, 220, 200, 28)), &stroke, Transform::identity(), mask.as_ref());
    p.stroke(&rounded_rect(24.0, 24.0, w - 48.0, h - 48.0, 12.0), sk(GOLD, 200), 3.0, Transform::identity());
    // Emblem: «21» in a gold ring with the four suits around it.
    p.fill(&circle(cx + 3.0, cy + 4.0, 62.0), tiny_skia::Color::from_rgba8(0, 0, 0, 70));
    p.fill(&circle(cx, cy, 62.0), sk(PAPER, 255));
    p.stroke(&circle(cx, cy, 54.0), sk(GOLD, 255), 4.0, Transform::identity());
    p.text("21", cx, cy, 64.0, [RED[0], RED[1], RED[2], 255], 0.0);
    for (suit, (dx, dy)) in Suit::ALL.into_iter().zip([(0.0, -100.0), (100.0, 0.0), (0.0, 100.0), (-100.0, 0.0)]) {
        for part in suit_paths(suit, cx + dx, cy + dy, 30.0) {
            p.fill(&part, sk(PAPER, 235));
        }
    }
    p.pm
}

/// Chip denominations, largest first, with their colours.
pub const CHIPS: [(u32, [u8; 3]); 4] = [
    (500, [0x7B, 0x3F, 0xC4]),
    (100, [0x22, 0x23, 0x2A]),
    (50, [0xC8, 0x2A, 0x2A]),
    (10, [0x1F, 0x6F, 0xD6]),
];

/// Top of a chip: coloured disc with white edge spots and the value.
fn paint_chip(value: u32, color: [u8; 3]) -> Pixmap {
    let size = 128u32;
    let mut pm = Pixmap::new(size, size).expect("valid size");
    let c = size as f32 / 2.0;
    let fill = |pm: &mut Pixmap, path: &Path, col: [u8; 3], a: u8| {
        pm.fill_path(path, &paint(sk(col, a)), tiny_skia::FillRule::Winding, Transform::identity(), None);
    };
    // The cylinder's caps sample the whole square, so the base fills it.
    let full = PathBuilder::from_rect(tiny_skia::Rect::from_xywh(0.0, 0.0, size as f32, size as f32).expect("rect"));
    fill(&mut pm, &full, color, 255);
    for i in 0..8 {
        let a = i as f32 / 8.0 * std::f32::consts::TAU;
        let mut pb = PathBuilder::new();
        let (s0, s1) = ((a - 0.16).sin_cos(), (a + 0.16).sin_cos());
        pb.move_to(c + s0.1 * 46.0, c + s0.0 * 46.0);
        pb.line_to(c + s0.1 * 66.0, c + s0.0 * 66.0);
        pb.line_to(c + s1.1 * 66.0, c + s1.0 * 66.0);
        pb.line_to(c + s1.1 * 46.0, c + s1.0 * 46.0);
        pb.close();
        fill(&mut pm, &pb.finish().expect("spot"), [0xF4, 0xF1, 0xEA], 255);
    }
    fill(&mut pm, &circle(c, c, 40.0), [0xF4, 0xF1, 0xEA], 255);
    fill(&mut pm, &circle(c, c, 35.0), color, 255);
    let mut painter = new_painter_sized(pm);
    let text = value.to_string();
    let size = if text.len() > 2 { 26.0 } else { 32.0 };
    painter.text(&text, c, c, size, [0xF4, 0xF1, 0xEA, 255], 0.0);
    painter.pm
}

fn new_painter_sized(pm: Pixmap) -> Painter<'static> {
    let mut p = new_painter();
    p.pm = pm;
    p
}

/// Materials for every card face, the back, the chips and the shared meshes.
#[derive(Resource)]
pub struct BjArt {
    pub faces: Vec<Handle<StandardMaterial>>,
    pub back: Handle<StandardMaterial>,
    pub card_mesh: Handle<Mesh>,
    /// One material per entry of [`CHIPS`].
    pub chips: Vec<Handle<StandardMaterial>>,
    pub chip_mesh: Handle<Mesh>,
}

/// Card size in world units (the same as the Uno cards).
pub const CARD_W: f32 = crate::uno::art::CARD_W;
pub const CARD_H: f32 = crate::uno::art::CARD_H;
pub const CHIP_R: f32 = 0.085;
pub const CHIP_H: f32 = 0.016;

pub fn build_art(images: &mut Assets<Image>, materials: &mut Assets<StandardMaterial>, meshes: &mut Assets<Mesh>) -> BjArt {
    let card_material = |pm: &Pixmap, images: &mut Assets<Image>, materials: &mut Assets<StandardMaterial>| {
        materials.add(StandardMaterial {
            base_color_texture: Some(images.add(to_image(pm))),
            alpha_mode: AlphaMode::Mask(0.5),
            perceptual_roughness: 0.42,
            reflectance: 0.35,
            ..default()
        })
    };
    let mut faces = vec![Handle::default(); FACE_COUNT];
    for suit in Suit::ALL {
        for rank in Rank::ALL {
            let card = Card { id: 0, suit, rank };
            faces[face_index(&card)] = card_material(&paint_face(card), images, materials);
        }
    }
    let back = card_material(&paint_back(), images, materials);
    let chips = CHIPS
        .iter()
        .map(|(value, color)| {
            materials.add(StandardMaterial {
                base_color_texture: Some(images.add(to_image(&paint_chip(*value, *color)))),
                perceptual_roughness: 0.5,
                reflectance: 0.3,
                ..default()
            })
        })
        .collect();
    BjArt {
        faces,
        back,
        card_mesh: meshes.add(Rectangle::new(CARD_W, CARD_H)),
        chips,
        chip_mesh: meshes.add(Cylinder::new(CHIP_R, CHIP_H).mesh().resolution(32)),
    }
}

/// Splits an amount into chips (denomination index into [`CHIPS`]), largest
/// first, at most `max` chips (the rest are left out of the picture).
pub fn chip_breakdown(mut amount: u32, max: usize) -> Vec<usize> {
    let mut out = Vec::new();
    for (i, (value, _)) in CHIPS.iter().enumerate() {
        while amount >= *value && out.len() < max {
            amount -= value;
            out.push(i);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn face_indices_are_unique() {
        let mut seen = std::collections::HashSet::new();
        for c in blackjack_core::full_shoe().iter().take(52) {
            seen.insert(face_index(c));
        }
        assert_eq!(seen.len(), FACE_COUNT);
        assert!(seen.iter().all(|i| *i < FACE_COUNT));
    }

    #[test]
    fn faces_paint_with_transparent_corners() {
        for (suit, rank) in [(Suit::Hearts, Rank::Ace), (Suit::Clubs, Rank::Ten), (Suit::Spades, Rank::King)] {
            let face = paint_face(Card { id: 0, suit, rank });
            assert_eq!(face.pixel(0, 0).unwrap().alpha(), 0);
            assert_eq!(face.pixel(TEX_W / 2, TEX_H / 2).unwrap().alpha(), 255);
        }
        assert_eq!(paint_back().pixel(0, 0).unwrap().alpha(), 0);
    }

    #[test]
    fn chips_add_up() {
        let total = |v: &[usize]| v.iter().map(|i| CHIPS[*i].0).sum::<u32>();
        assert_eq!(total(&chip_breakdown(670, 20)), 670);
        assert_eq!(chip_breakdown(670, 20), vec![0, 1, 2, 3, 3]);
        assert_eq!(chip_breakdown(10_000, 6).len(), 6);
        assert!(chip_breakdown(0, 10).is_empty());
    }

    /// Writes PNGs of a few faces (run with --ignored, set CARD_DUMP_DIR).
    #[test]
    #[ignore]
    fn dump_faces() {
        let dir = std::env::var("CARD_DUMP_DIR").unwrap_or_else(|_| "/tmp".into());
        let cards = [
            (Suit::Hearts, Rank::Ace),
            (Suit::Spades, Rank::Ace),
            (Suit::Diamonds, Rank::Seven),
            (Suit::Clubs, Rank::Ten),
            (Suit::Spades, Rank::King),
            (Suit::Hearts, Rank::Queen),
            (Suit::Clubs, Rank::Jack),
            (Suit::Diamonds, Rank::Three),
        ];
        for (i, (suit, rank)) in cards.iter().enumerate() {
            paint_face(Card { id: 0, suit: *suit, rank: *rank }).save_png(format!("{dir}/bj{i}.png")).unwrap();
        }
        paint_back().save_png(format!("{dir}/bj-back.png")).unwrap();
        for (v, c) in CHIPS {
            paint_chip(v, c).save_png(format!("{dir}/chip{v}.png")).unwrap();
        }
    }
}
