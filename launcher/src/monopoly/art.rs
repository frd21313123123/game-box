//! Procedural art of the 3D board, painted once with tiny-skia (shapes) and
//! ab_glyph (text): the printed board with its tile icons, the card decks, a
//! drawn card, the dice faces, the overlay of a mortgaged tile and the wood of
//! the table. No image assets are needed.

use std::f32::consts::{FRAC_PI_2, FRAC_PI_4, PI, TAU};

use ab_glyph::{Font, FontRef, PxScale, ScaleFont};
use bevy::image::{ImageAddressMode, ImageSampler, ImageSamplerDescriptor};
use bevy::math::Vec2;
use bevy::prelude::Image;
use monopoly_core::{tile, Deck, TileKind, BOARD_SIZE};
use tiny_skia::{
    Color as SkColor, FillRule, FilterQuality, GradientStop, LineCap, LineJoin, LinearGradient, Paint, Path,
    PathBuilder, Pixmap, PixmapPaint, Point, PremultipliedColorU8, RadialGradient, Rect, Shader, SpreadMode,
    Stroke, StrokeDash, Transform,
};

use super::geom::{side_angle, stripe_rect, tile_rect, Side, VRect, CORNER, FAR, JAIL_CELL, VIEW};

const REGULAR: &[u8] = include_bytes!("../../../assets/fonts/Inter-Regular.otf");
const SEMIBOLD: &[u8] = include_bytes!("../../../assets/fonts/Inter-SemiBold.otf");
const BOLD: &[u8] = include_bytes!("../../../assets/fonts/Inter-ExtraBold.otf");

/// Side of the square board texture in pixels.
pub const BOARD_TEX: u32 = 2048;

pub const INK: [u8; 3] = [0x1a, 0x1d, 0x22];
pub const CREAM: [u8; 3] = [0xfb, 0xf8, 0xef];
const MINT: [u8; 3] = [0xd3, 0xe9, 0xd5];
const MINT_EDGE: [u8; 3] = [0xbf, 0xdc, 0xc4];
const PRICE: [u8; 3] = [0x3a, 0x3f, 0x47];
pub const CHANCE: [u8; 3] = [0xf7, 0x94, 0x1d];
pub const CHEST: [u8; 3] = [0x3f, 0x86, 0xd6];
const RED: [u8; 3] = [0xc8, 0x14, 0x2b];
const GOLD: [u8; 3] = [0xf2, 0xc1, 0x4e];

pub fn deck_color(deck: Deck) -> [u8; 3] {
    match deck {
        Deck::Chance => CHANCE,
        Deck::Chest => CHEST,
    }
}

fn sk(c: [u8; 3], a: u8) -> SkColor {
    SkColor::from_rgba8(c[0], c[1], c[2], a)
}

fn paint(color: SkColor) -> Paint<'static> {
    let mut p = Paint::default();
    p.set_color(color);
    p.anti_alias = true;
    p
}

fn fill(pm: &mut Pixmap, path: &Path, color: SkColor, t: Transform) {
    pm.fill_path(path, &paint(color), FillRule::Winding, t, None);
}

fn fill_shader(pm: &mut Pixmap, path: &Path, shader: Shader<'static>, t: Transform) {
    let p = Paint {
        shader,
        anti_alias: true,
        ..Paint::default()
    };
    pm.fill_path(path, &p, FillRule::Winding, t, None);
}

fn stroke(pm: &mut Pixmap, path: &Path, color: SkColor, width: f32, t: Transform) {
    let s = Stroke {
        width,
        line_cap: LineCap::Round,
        line_join: LineJoin::Round,
        ..Stroke::default()
    };
    pm.stroke_path(path, &paint(color), &s, t, None);
}

fn rect_path(r: VRect) -> Path {
    PathBuilder::from_rect(Rect::from_ltrb(r.min.x, r.min.y, r.max.x, r.max.y).expect("valid rect"))
}

fn rounded(x: f32, y: f32, w: f32, h: f32, r: f32) -> Path {
    let r = r.min(w / 2.0).min(h / 2.0);
    let k = 0.552_284_8 * r;
    let mut pb = PathBuilder::new();
    pb.move_to(x + r, y);
    pb.line_to(x + w - r, y);
    pb.cubic_to(x + w - r + k, y, x + w, y + r - k, x + w, y + r);
    pb.line_to(x + w, y + h - r);
    pb.cubic_to(x + w, y + h - r + k, x + w - r + k, y + h, x + w - r, y + h);
    pb.line_to(x + r, y + h);
    pb.cubic_to(x + r - k, y + h, x, y + h - r + k, x, y + h - r);
    pb.line_to(x, y + r);
    pb.cubic_to(x, y + r - k, x + r - k, y, x + r, y);
    pb.close();
    pb.finish().expect("valid rounded rect")
}

/// A rounded rectangle centred on the origin.
fn rounded_c(w: f32, h: f32, r: f32) -> Path {
    rounded(-w / 2.0, -h / 2.0, w, h, r)
}

fn circle(cx: f32, cy: f32, r: f32) -> Path {
    PathBuilder::from_circle(cx, cy, r).expect("valid circle")
}

fn polygon(points: &[(f32, f32)]) -> Path {
    let mut pb = PathBuilder::new();
    pb.move_to(points[0].0, points[0].1);
    for &(x, y) in &points[1..] {
        pb.line_to(x, y);
    }
    pb.close();
    pb.finish().expect("valid polygon")
}

/// Rotation about a point, then a uniform scale (local units to the parent's).
fn place(parent: Transform, at: Vec2, angle: f32, scale: f32) -> Transform {
    parent
        .pre_translate(at.x, at.y)
        .pre_rotate(angle.to_degrees())
        .pre_scale(scale, scale)
}

// ---------------------------------------------------------------------------
// Text
// ---------------------------------------------------------------------------

#[derive(Clone, Copy)]
pub enum Weight {
    Regular,
    Semibold,
    Bold,
}

/// The Inter faces and text layout: measuring, word wrap, and rotated lines
/// drawn through a bicubic transform so that text at any angle stays smooth.
pub struct Fonts {
    regular: FontRef<'static>,
    semibold: FontRef<'static>,
    bold: FontRef<'static>,
}

impl Default for Fonts {
    fn default() -> Self {
        Fonts {
            regular: FontRef::try_from_slice(REGULAR).expect("bundled font parses"),
            semibold: FontRef::try_from_slice(SEMIBOLD).expect("bundled font parses"),
            bold: FontRef::try_from_slice(BOLD).expect("bundled font parses"),
        }
    }
}

impl Fonts {
    fn get(&self, w: Weight) -> &FontRef<'static> {
        match w {
            Weight::Regular => &self.regular,
            Weight::Semibold => &self.semibold,
            Weight::Bold => &self.bold,
        }
    }

    /// `size` is the em size in pixels.
    fn scale(&self, w: Weight, size: f32) -> PxScale {
        let f = self.get(w);
        // ab_glyph scales so that ascent − descent equals the given height.
        let units = f.units_per_em().unwrap_or(1000.0);
        PxScale::from(size * f.height_unscaled() / units)
    }

    pub fn width(&self, w: Weight, size: f32, text: &str) -> f32 {
        let f = self.get(w).as_scaled(self.scale(w, size));
        let mut width = 0.0;
        let mut prev = None;
        for ch in text.chars() {
            let id = f.glyph_id(ch);
            if let Some(p) = prev {
                width += f.kern(p, id);
            }
            width += f.h_advance(id);
            prev = Some(id);
        }
        width
    }

    pub fn has_glyph(&self, w: Weight, ch: char) -> bool {
        self.get(w).glyph_id(ch).0 != 0
    }

    /// One line rasterised into its own pixmap, plus the point of it that is
    /// the line's centre (middle of the advance, middle of the capitals).
    fn line(&self, w: Weight, size: f32, text: &str, color: [u8; 4]) -> Option<(Pixmap, f32, f32)> {
        let font = self.get(w);
        let scale = self.scale(w, size);
        let f = font.as_scaled(scale);
        let width = self.width(w, size, text);
        let pad = 3.0;
        let pw = (width + 2.0 * pad).ceil() as u32 + 1;
        let ph = (f.ascent() - f.descent() + 2.0 * pad).ceil() as u32 + 1;
        let mut cov = vec![0f32; (pw * ph) as usize];
        let baseline = pad + f.ascent();
        let mut x = pad;
        let mut prev = None;
        for ch in text.chars() {
            let id = f.glyph_id(ch);
            if let Some(p) = prev {
                x += f.kern(p, id);
            }
            let glyph = id.with_scale_and_position(scale, ab_glyph::point(x, baseline));
            x += f.h_advance(id);
            prev = Some(id);
            let Some(outlined) = font.outline_glyph(glyph) else { continue };
            let b = outlined.px_bounds();
            outlined.draw(|gx, gy, c| {
                let px = b.min.x as i32 + gx as i32;
                let py = b.min.y as i32 + gy as i32;
                if px >= 0 && py >= 0 && (px as u32) < pw && (py as u32) < ph {
                    let i = (py as u32 * pw + px as u32) as usize;
                    cov[i] = (cov[i] + c).min(1.0);
                }
            });
        }
        let mut pm = Pixmap::new(pw, ph)?;
        for (px, c) in pm.pixels_mut().iter_mut().zip(cov) {
            let a = c * color[3] as f32 / 255.0;
            if a <= 0.0 {
                continue;
            }
            let m = |v: u8| (v as f32 * a).round() as u8;
            if let Some(c) = PremultipliedColorU8::from_rgba(m(color[0]), m(color[1]), m(color[2]), (a * 255.0).round() as u8) {
                *px = c;
            }
        }
        // Inter's capitals are 0.727 em tall, the ascent 0.969 em.
        let cap = f.ascent() * 0.75;
        Some((pm, pad + width / 2.0, baseline - cap / 2.0))
    }

    /// Draws one line centred at `at` (pixels of `t`'s target), rotated by
    /// `angle` (radians, clockwise).
    #[allow(clippy::too_many_arguments)]
    pub fn draw(&self, pm: &mut Pixmap, t: Transform, w: Weight, size: f32, text: &str, color: [u8; 4], at: Vec2, angle: f32) {
        // Rasterise at the final pixel size so small print stays crisp.
        let px_scale = (t.sx * t.sx + t.ky * t.ky).sqrt().max(1e-3);
        let Some((line, ax, ay)) = self.line(w, size * px_scale, text, color) else { return };
        let t = t
            .pre_translate(at.x, at.y)
            .pre_rotate(angle.to_degrees())
            .pre_scale(1.0 / px_scale, 1.0 / px_scale)
            .pre_translate(-ax, -ay);
        let paint = PixmapPaint {
            quality: FilterQuality::Bicubic,
            ..PixmapPaint::default()
        };
        pm.draw_pixmap(0, 0, line.as_ref(), &paint, t, None);
    }

    /// Greedy word wrap; a `\n` forces a break.
    pub fn wrap(&self, w: Weight, size: f32, text: &str, width: f32) -> Vec<String> {
        let mut lines = Vec::new();
        for para in text.split('\n') {
            let mut line = String::new();
            for word in para.split_whitespace() {
                let candidate = if line.is_empty() { word.to_string() } else { format!("{line} {word}") };
                if line.is_empty() || self.width(w, size, &candidate) <= width {
                    line = candidate;
                } else {
                    lines.push(std::mem::take(&mut line));
                    line = word.to_string();
                }
            }
            lines.push(line);
        }
        lines
    }

    /// A centred block of word-wrapped text. The font shrinks until every
    /// line fits `width` and there are at most `max_lines` lines, so words are
    /// never broken in the middle. Returns the font size used.
    #[allow(clippy::too_many_arguments)]
    pub fn block(
        &self,
        pm: &mut Pixmap,
        t: Transform,
        w: Weight,
        size: f32,
        text: &str,
        color: [u8; 4],
        at: Vec2,
        angle: f32,
        width: f32,
        max_lines: usize,
    ) -> f32 {
        let (size, lines) = self.fit(w, size, text, width, max_lines);
        let lh = size * 1.18;
        let n = lines.len() as f32;
        let (s, c) = angle.sin_cos();
        for (i, l) in lines.iter().enumerate() {
            let dy = (i as f32 - (n - 1.0) / 2.0) * lh;
            self.draw(pm, t, w, size, l, color, at + Vec2::new(-s * dy, c * dy), angle);
        }
        size
    }

    /// Font size and lines for [`Fonts::block`].
    pub fn fit(&self, w: Weight, mut size: f32, text: &str, width: f32, max_lines: usize) -> (f32, Vec<String>) {
        loop {
            let lines = self.wrap(w, size, text, width);
            let fits = lines.len() <= max_lines && lines.iter().all(|l| self.width(w, size, l) <= width);
            if fits || size < 3.0 {
                return (size, lines);
            }
            size *= 0.96;
        }
    }
}

/// «₽120»: the rouble sign is in Inter, but fall back to a suffix if not.
pub fn money(fonts: &Fonts, amount: i32) -> String {
    if fonts.has_glyph(Weight::Semibold, '₽') {
        format!("₽{amount}")
    } else {
        format!("{amount} р.")
    }
}

// ---------------------------------------------------------------------------
// Tile icons (drawn in a 100×100 box centred on the origin)
// ---------------------------------------------------------------------------

fn icon_train(pm: &mut Pixmap, t: Transform) {
    let ink = sk(INK, 255);
    let cream = sk(CREAM, 255);
    // Boiler, cab, roof, chimney and the cow-catcher of a steam engine.
    fill(pm, &rounded(-40.0, -10.0, 60.0, 28.0, 10.0), ink, t);
    fill(pm, &rounded(12.0, -30.0, 30.0, 48.0, 3.0), ink, t);
    fill(pm, &rounded(8.0, -38.0, 38.0, 9.0, 3.0), ink, t);
    fill(pm, &polygon(&[(-31.0, -10.0), (-33.0, -30.0), (-17.0, -30.0), (-19.0, -10.0)]), ink, t);
    fill(pm, &rounded(-37.0, -36.0, 24.0, 7.0, 2.0), ink, t);
    fill(pm, &rounded(-10.0, -19.0, 12.0, 10.0, 5.0), ink, t);
    fill(pm, &polygon(&[(-40.0, 12.0), (-52.0, 34.0), (-36.0, 34.0)]), ink, t);
    fill(pm, &rounded(17.0, -24.0, 20.0, 14.0, 2.0), cream, t);
    for (x, r) in [(-24.0, 11.0), (2.0, 11.0), (28.0, 13.0)] {
        let y = 36.0 - r;
        fill(pm, &circle(x, y + 8.0, r), ink, t);
        fill(pm, &circle(x, y + 8.0, r * 0.42), cream, t);
    }
    let mut pb = PathBuilder::new();
    pb.move_to(-24.0, 31.0);
    pb.line_to(28.0, 29.0);
    stroke(pm, &pb.finish().expect("rod"), cream, 3.0, t);
    // A puff of steam.
    for (x, y, r) in [(-26.0, -44.0, 6.0), (-16.0, -50.0, 8.0), (-3.0, -53.0, 9.0)] {
        fill(pm, &circle(x, y, r), SkColor::from_rgba8(150, 156, 166, 160), t);
    }
}

fn icon_bulb(pm: &mut Pixmap, t: Transform) {
    let ink = sk(INK, 255);
    // Rays.
    for i in 0..7 {
        let a = PI + i as f32 / 6.0 * PI;
        let (s, c) = a.sin_cos();
        let mut pb = PathBuilder::new();
        pb.move_to(c * 36.0, -10.0 + s * 36.0);
        pb.line_to(c * 46.0, -10.0 + s * 46.0);
        stroke(pm, &pb.finish().expect("ray"), sk([0xe8, 0xa3, 0x17], 255), 4.0, t);
    }
    let glass = circle(0.0, -10.0, 27.0);
    let shader = RadialGradient::new(
        Point::from_xy(-8.0, -20.0),
        Point::from_xy(0.0, -10.0),
        30.0,
        vec![
            GradientStop::new(0.0, sk([0xff, 0xf6, 0xc8], 255)),
            GradientStop::new(1.0, sk([0xf9, 0xc7, 0x2c], 255)),
        ],
        SpreadMode::Pad,
        Transform::identity(),
    )
    .expect("gradient");
    fill_shader(pm, &glass, shader, t);
    stroke(pm, &glass, ink, 3.5, t);
    fill(pm, &polygon(&[(-14.0, 12.0), (14.0, 12.0), (11.0, 22.0), (-11.0, 22.0)]), sk([0xf9, 0xc7, 0x2c], 255), t);
    stroke(pm, &polygon(&[(-14.0, 12.0), (14.0, 12.0), (11.0, 22.0), (-11.0, 22.0)]), ink, 3.0, t);
    fill(pm, &rounded(-12.0, 22.0, 24.0, 17.0, 4.0), sk([0x8c, 0x93, 0x9e], 255), t);
    for y in [27.0, 33.0] {
        let mut pb = PathBuilder::new();
        pb.move_to(-12.0, y);
        pb.line_to(12.0, y);
        stroke(pm, &pb.finish().expect("thread"), ink, 2.0, t);
    }
    fill(pm, &rounded(-5.0, 39.0, 10.0, 5.0, 2.0), ink, t);
    // Filament.
    let mut pb = PathBuilder::new();
    pb.move_to(-8.0, 12.0);
    pb.line_to(-8.0, -6.0);
    pb.line_to(-4.0, -14.0);
    pb.line_to(0.0, -6.0);
    pb.line_to(4.0, -14.0);
    pb.line_to(8.0, -6.0);
    pb.line_to(8.0, 12.0);
    stroke(pm, &pb.finish().expect("filament"), sk([0x9a, 0x5b, 0x10], 255), 2.5, t);
}

fn icon_water(pm: &mut Pixmap, t: Transform) {
    let ink = sk(INK, 255);
    // A tap pouring a drop.
    fill(pm, &rounded(-40.0, -40.0, 46.0, 16.0, 5.0), sk([0x8c, 0x93, 0x9e], 255), t);
    stroke(pm, &rounded(-40.0, -40.0, 46.0, 16.0, 5.0), ink, 3.0, t);
    let mut pb = PathBuilder::new();
    pb.move_to(-2.0, -40.0);
    pb.line_to(14.0, -40.0);
    pb.cubic_to(26.0, -40.0, 28.0, -32.0, 28.0, -22.0);
    pb.line_to(16.0, -22.0);
    pb.cubic_to(16.0, -27.0, 14.0, -28.0, 10.0, -28.0);
    pb.line_to(-2.0, -28.0);
    pb.close();
    let spout = pb.finish().expect("spout");
    fill(pm, &spout, sk([0x8c, 0x93, 0x9e], 255), t);
    stroke(pm, &spout, ink, 3.0, t);
    fill(pm, &rounded(-24.0, -52.0, 14.0, 12.0, 3.0), ink, t);
    fill(pm, &rounded(-34.0, -56.0, 34.0, 6.0, 3.0), ink, t);
    let mut pb = PathBuilder::new();
    pb.move_to(22.0, -12.0);
    pb.cubic_to(28.0, 0.0, 40.0, 12.0, 40.0, 24.0);
    pb.cubic_to(40.0, 35.0, 32.0, 42.0, 22.0, 42.0);
    pb.cubic_to(12.0, 42.0, 4.0, 35.0, 4.0, 24.0);
    pb.cubic_to(4.0, 12.0, 16.0, 0.0, 22.0, -12.0);
    pb.close();
    let drop = pb.finish().expect("drop");
    fill(pm, &drop, sk([0x2f, 0x8f, 0xdc], 255), t);
    stroke(pm, &drop, ink, 3.0, t);
    fill(pm, &circle(15.0, 26.0, 4.5), SkColor::from_rgba8(255, 255, 255, 200), t);
}

fn icon_chest(pm: &mut Pixmap, t: Transform, gold: [u8; 3]) {
    let ink = sk(INK, 255);
    let wood = sk([0xa8, 0x66, 0x2c], 255);
    let wood_hi = sk([0xc4, 0x80, 0x3e], 255);
    let body = rounded(-38.0, -4.0, 76.0, 38.0, 4.0);
    fill(pm, &body, wood, t);
    let mut pb = PathBuilder::new();
    pb.move_to(-38.0, -4.0);
    pb.line_to(-38.0, -16.0);
    pb.cubic_to(-38.0, -38.0, 38.0, -38.0, 38.0, -16.0);
    pb.line_to(38.0, -4.0);
    pb.close();
    let lid = pb.finish().expect("lid");
    fill(pm, &lid, wood_hi, t);
    for x in [-26.0, 20.0] {
        fill(pm, &rect_path(VRect::new(x, -30.0, 6.0, 64.0)), sk(gold, 255), t);
    }
    stroke(pm, &lid, ink, 3.0, t);
    stroke(pm, &body, ink, 3.0, t);
    fill(pm, &rounded(-8.0, -12.0, 16.0, 18.0, 3.0), sk(gold, 255), t);
    stroke(pm, &rounded(-8.0, -12.0, 16.0, 18.0, 3.0), ink, 2.5, t);
    fill(pm, &circle(0.0, -5.0, 2.5), ink, t);
    fill(pm, &rect_path(VRect::new(-1.2, -5.0, 2.4, 7.0)), ink, t);
}

fn icon_money_bag(pm: &mut Pixmap, t: Transform, fonts: &Fonts) {
    let ink = sk(INK, 255);
    let mut pb = PathBuilder::new();
    pb.move_to(-12.0, -22.0);
    pb.cubic_to(-44.0, -6.0, -44.0, 40.0, 0.0, 40.0);
    pb.cubic_to(44.0, 40.0, 44.0, -6.0, 12.0, -22.0);
    pb.close();
    let bag = pb.finish().expect("bag");
    fill(pm, &bag, sk([0xc9, 0xa2, 0x5c], 255), t);
    stroke(pm, &bag, ink, 3.0, t);
    let neck = polygon(&[(-12.0, -22.0), (12.0, -22.0), (20.0, -40.0), (8.0, -34.0), (0.0, -42.0), (-8.0, -34.0), (-20.0, -40.0)]);
    fill(pm, &neck, sk([0xc9, 0xa2, 0x5c], 255), t);
    stroke(pm, &neck, ink, 3.0, t);
    fill(pm, &rounded(-14.0, -25.0, 28.0, 6.0, 3.0), sk(RED, 255), t);
    let sign = if fonts.has_glyph(Weight::Bold, '₽') { "₽" } else { "Р" };
    fonts.draw(pm, t, Weight::Bold, 34.0, sign, [INK[0], INK[1], INK[2], 255], Vec2::new(0.0, 14.0), 0.0);
}

fn icon_diamond(pm: &mut Pixmap, t: Transform) {
    let ink = sk(INK, 255);
    // A ring with a cut stone.
    let ring = circle(0.0, 18.0, 24.0);
    stroke(pm, &ring, sk([0xd9, 0xa4, 0x1e], 255), 8.0, t);
    stroke(pm, &circle(0.0, 18.0, 28.0), ink, 1.5, t);
    let stone = polygon(&[(-24.0, -20.0), (-14.0, -34.0), (14.0, -34.0), (24.0, -20.0), (0.0, 4.0)]);
    fill(pm, &stone, sk([0xa8, 0xdc, 0xf5], 255), t);
    fill(pm, &polygon(&[(-24.0, -20.0), (24.0, -20.0), (0.0, 4.0)]), sk([0x6e, 0xbd, 0xe8], 255), t);
    stroke(pm, &stone, ink, 3.0, t);
    let mut pb = PathBuilder::new();
    pb.move_to(-24.0, -20.0);
    pb.line_to(24.0, -20.0);
    pb.move_to(-8.0, -34.0);
    pb.line_to(-12.0, -20.0);
    pb.line_to(0.0, 4.0);
    pb.line_to(12.0, -20.0);
    pb.line_to(8.0, -34.0);
    stroke(pm, &pb.finish().expect("facets"), ink, 2.0, t);
}

fn icon_parking(pm: &mut Pixmap, t: Transform, fonts: &Fonts) {
    let sign = rounded_c(70.0, 70.0, 14.0);
    fill(pm, &sign, sk([0x1f, 0x5f, 0xbf], 255), t);
    stroke(pm, &rounded_c(60.0, 60.0, 10.0), sk(CREAM, 255), 3.0, t);
    stroke(pm, &sign, sk(INK, 255), 2.5, t);
    fonts.draw(pm, t, Weight::Bold, 54.0, "P", [CREAM[0], CREAM[1], CREAM[2], 255], Vec2::new(1.0, 0.0), 0.0);
}

fn icon_handcuffs(pm: &mut Pixmap, t: Transform) {
    let steel = sk([0x59, 0x60, 0x6b], 255);
    let ink = sk(INK, 255);
    for x in [-24.0, 24.0] {
        stroke(pm, &circle(x, 10.0, 21.0), ink, 11.0, t);
        stroke(pm, &circle(x, 10.0, 21.0), steel, 7.0, t);
        fill(pm, &rounded(x - 7.0, -20.0, 14.0, 10.0, 3.0), steel, t);
        stroke(pm, &rounded(x - 7.0, -20.0, 14.0, 10.0, 3.0), ink, 2.0, t);
    }
    for x in [-9.0, 0.0, 9.0] {
        let link = rounded(x - 6.0, -27.0, 12.0, 8.0, 4.0);
        stroke(pm, &link, ink, 4.5, t);
        stroke(pm, &link, steel, 2.5, t);
    }
}

fn icon_arrow(pm: &mut Pixmap, t: Transform) {
    let arrow = polygon(&[(-50.0, 0.0), (-18.0, -22.0), (-18.0, -9.0), (50.0, -9.0), (50.0, 9.0), (-18.0, 9.0), (-18.0, 22.0)]);
    fill(pm, &arrow, sk([0xd4, 0x2a, 0x2a], 255), t);
    stroke(pm, &arrow, sk(INK, 255), 2.5, t);
}

fn question(pm: &mut Pixmap, t: Transform, fonts: &Fonts, size: f32, color: [u8; 3]) {
    fonts.draw(pm, t, Weight::Bold, size, "?", [INK[0], INK[1], INK[2], 200], Vec2::new(2.0, 3.0), 0.0);
    fonts.draw(pm, t, Weight::Bold, size, "?", [color[0], color[1], color[2], 255], Vec2::ZERO, 0.0);
}

// ---------------------------------------------------------------------------
// The board
// ---------------------------------------------------------------------------

fn ink(a: u8) -> [u8; 4] {
    [INK[0], INK[1], INK[2], a]
}

/// Paints the whole board (tiles, corners, logo and the places for the decks).
pub fn paint_board(fonts: &Fonts) -> Pixmap {
    let k = BOARD_TEX as f32 / VIEW;
    let mut pm = Pixmap::new(BOARD_TEX, BOARD_TEX).expect("valid size");
    let t = Transform::from_scale(k, k);
    fill(&mut pm, &rect_path(VRect::new(0.0, 0.0, VIEW, VIEW)), sk(CREAM, 255), t);

    // The middle: mint, a touch lighter in the centre.
    let mid = VRect::new(CORNER, CORNER, VIEW - 2.0 * CORNER, VIEW - 2.0 * CORNER);
    let shader = RadialGradient::new(
        Point::from_xy(VIEW / 2.0, VIEW / 2.0),
        Point::from_xy(VIEW / 2.0, VIEW / 2.0),
        VIEW * 0.55,
        vec![GradientStop::new(0.0, sk(MINT, 255)), GradientStop::new(1.0, sk(MINT_EDGE, 255))],
        SpreadMode::Pad,
        Transform::identity(),
    )
    .expect("gradient");
    fill_shader(&mut pm, &rect_path(mid), shader, t);

    for i in 0..BOARD_SIZE {
        paint_tile(&mut pm, t, fonts, i);
    }
    stroke(&mut pm, &rect_path(mid), sk(INK, 255), 2.5, t);
    stroke(&mut pm, &rect_path(VRect::new(1.5, 1.5, VIEW - 3.0, VIEW - 3.0)), sk(INK, 255), 3.0, t);

    paint_centre(&mut pm, t, fonts);
    pm
}

fn paint_tile(pm: &mut Pixmap, t: Transform, fonts: &Fonts, i: usize) {
    let tl = tile(i);
    let (r, side) = tile_rect(i);
    stroke(pm, &rect_path(r), sk(INK, 255), 1.5, t);
    if side == Side::Corner {
        paint_corner(pm, t, fonts, i, r);
        return;
    }
    if let (Some(g), Some(sr)) = (tl.group(), stripe_rect(r, side)) {
        let path = rect_path(sr);
        fill(pm, &path, sk(g.rgb(), 255), t);
        stroke(pm, &path, sk(INK, 255), 1.5, t);
    }
    let angle = side_angle(side);
    let (s, c) = angle.sin_cos();
    let centre = r.center();
    // In the tile's own frame "down" points to the edge of the board.
    let at = |dy: f32| centre + Vec2::new(-s * dy, c * dy);
    let local = |dy: f32, size: f32| place(t, at(dy), angle, size / 100.0);
    let name_size = 15.0;
    let wrap = 80.0;
    let price = |pm: &mut Pixmap, amount: i32| {
        fonts.draw(pm, t, Weight::Semibold, 14.0, &money(fonts, amount), [PRICE[0], PRICE[1], PRICE[2], 255], at(55.0), angle);
    };
    match tl.kind {
        TileKind::Street { price: p, .. } => {
            fonts.block(pm, t, Weight::Semibold, name_size, tl.name, ink(255), at(-27.0), angle, wrap, 2);
            price(pm, p);
        }
        TileKind::Station { price: p } => {
            fonts.block(pm, t, Weight::Semibold, name_size, tl.name, ink(255), at(-52.0), angle, wrap, 2);
            icon_train(pm, local(5.0, 52.0));
            price(pm, p);
        }
        TileKind::Utility { price: p } => {
            fonts.block(pm, t, Weight::Semibold, name_size, tl.name, ink(255), at(-52.0), angle, wrap, 2);
            if tl.name.starts_with("Электро") {
                icon_bulb(pm, local(4.0, 50.0));
            } else {
                icon_water(pm, local(5.0, 50.0));
            }
            price(pm, p);
        }
        TileKind::Chance => {
            fonts.block(pm, t, Weight::Semibold, name_size, tl.name, ink(255), at(-52.0), angle, wrap, 2);
            question(pm, local(10.0, 100.0), fonts, 64.0, [0xe0, 0x6f, 0x12]);
        }
        TileKind::Chest => {
            fonts.block(pm, t, Weight::Semibold, name_size, tl.name, ink(255), at(-52.0), angle, wrap, 2);
            icon_chest(pm, local(12.0, 52.0), GOLD);
        }
        TileKind::Tax { amount } => {
            fonts.block(pm, t, Weight::Semibold, name_size, tl.name, ink(255), at(-50.0), angle, wrap, 3);
            if i < 20 {
                icon_money_bag(pm, local(10.0, 46.0), fonts);
            } else {
                icon_diamond(pm, local(10.0, 46.0));
            }
            let text = format!("Заплатите {}", money(fonts, amount));
            fonts.block(pm, t, Weight::Semibold, 13.0, &text, [PRICE[0], PRICE[1], PRICE[2], 255], at(55.0), angle, 80.0, 1);
        }
        _ => {}
    }
}

fn paint_corner(pm: &mut Pixmap, t: Transform, fonts: &Fonts, i: usize, r: VRect) {
    let c = r.center();
    // Corner print runs diagonally and always reads from the near side.
    let a = if i == 0 || i == 20 { -FRAC_PI_4 } else { FRAC_PI_4 };
    let (s, co) = a.sin_cos();
    let at = |dy: f32| c + Vec2::new(-s * dy, co * dy);
    // Length of the diagonal line across the square `dy` from its middle.
    let room = |dy: f32| 2.0 * (CORNER * std::f32::consts::FRAC_1_SQRT_2 - dy.abs()) - 18.0;
    let line = |pm: &mut Pixmap, w: Weight, size: f32, text: &str, dy: f32| {
        fonts.block(pm, t, w, size, text, ink(255), at(dy), a, room(dy), 1);
    };
    match i {
        0 => {
            fonts.block(pm, t, Weight::Semibold, 11.5, "Проходя, получите ₽200", ink(220), at(-50.0), a, room(-50.0), 2);
            fonts.draw(pm, t, Weight::Bold, 38.0, "СТАРТ", [0xc0, 0x20, 0x20, 255], at(-14.0), a);
            icon_arrow(pm, place(t, Vec2::new(r.center().x - 4.0, r.max.y - 22.0), 0.0, 1.0));
        }
        10 => {
            let cell = JAIL_CELL;
            fill(pm, &rect_path(cell), sk(CHANCE, 255), t);
            let window = VRect::new(cell.min.x + 9.0, cell.min.y + 9.0, cell.size().x - 18.0, cell.size().y - 18.0);
            fill(pm, &rect_path(window), sk([0x3b, 0x2c, 0x22], 255), t);
            for k in 0..5 {
                let x = window.min.x + 8.0 + k as f32 * (window.size().x - 16.0) / 4.0;
                let mut pb = PathBuilder::new();
                pb.move_to(x, window.min.y);
                pb.line_to(x, window.max.y);
                stroke(pm, &pb.finish().expect("bar"), sk([0xc9, 0xcd, 0xd4], 255), 4.0, t);
            }
            stroke(pm, &rect_path(window), sk(INK, 255), 2.0, t);
            stroke(pm, &rect_path(cell), sk(INK, 255), 2.0, t);
            let label = place(t, cell.center() + Vec2::new(0.0, 0.0), a, 1.0);
            fill(pm, &rounded_c(88.0, 24.0, 6.0), sk(CREAM, 245), label);
            stroke(pm, &rounded_c(88.0, 24.0, 6.0), sk(INK, 255), 1.5, label);
            fonts.draw(pm, t, Weight::Bold, 14.5, "ТЮРЬМА", ink(255), cell.center(), a);
            fonts.draw(pm, t, Weight::Semibold, 12.5, "ПРОСТО", ink(255), Vec2::new(25.0, FAR + 50.0), FRAC_PI_2);
            fonts.draw(pm, t, Weight::Semibold, 12.5, "ПОСЕЩЕНИЕ", ink(255), Vec2::new(r.center().x, r.max.y - 25.0), 0.0);
        }
        20 => {
            line(pm, Weight::Bold, 15.0, "БЕСПЛАТНАЯ", -44.0);
            icon_parking(pm, place(t, at(0.0), a, 0.62), fonts);
            line(pm, Weight::Bold, 15.0, "СТОЯНКА", 44.0);
        }
        _ => {
            line(pm, Weight::Semibold, 13.0, "ОТПРАВЛЯЙТЕСЬ", -44.0);
            icon_handcuffs(pm, place(t, at(2.0), a, 0.62));
            line(pm, Weight::Bold, 16.0, "В ТЮРЬМУ!", 44.0);
        }
    }
}

/// Where the two decks lie in the middle of the board (view-box units) and
/// their rotation (radians, clockwise).
pub const DECK_SPOTS: [(Deck, Vec2); 2] = [
    (Deck::Chance, Vec2::new(372.0, 372.0)),
    (Deck::Chest, Vec2::new(738.0, 738.0)),
];
pub const DECK_ANGLE: f32 = -FRAC_PI_4;
/// Size of the dashed place for a deck.
const DECK_SPOT_SIZE: (f32, f32) = (252.0, 168.0);

fn paint_centre(pm: &mut Pixmap, t: Transform, fonts: &Fonts) {
    let c = Vec2::splat(VIEW / 2.0);
    let a = -FRAC_PI_4;
    // The logo: a red banner across the diagonal.
    let banner = place(t, c, a, 1.0);
    fill(pm, &rounded_c(600.0, 132.0, 16.0), SkColor::from_rgba8(0, 0, 0, 55), banner.pre_translate(5.0, 9.0));
    fill(pm, &rounded_c(600.0, 132.0, 16.0), sk(RED, 255), banner);
    stroke(pm, &rounded_c(578.0, 110.0, 10.0), sk(CREAM, 255), 4.0, banner);
    let (size, _) = fonts.fit(Weight::Bold, 86.0, "МОНОПОЛИЯ", 520.0, 1);
    fonts.draw(pm, t, Weight::Bold, size, "МОНОПОЛИЯ", [0, 0, 0, 70], c + Vec2::new(2.0, 4.0), a);
    fonts.draw(pm, t, Weight::Bold, size, "МОНОПОЛИЯ", [CREAM[0], CREAM[1], CREAM[2], 255], c, a);
    let (s, co) = a.sin_cos();
    let below = |dy: f32| c + Vec2::new(-s * dy, co * dy);
    fonts.draw(pm, t, Weight::Semibold, 22.0, "МОСКОВСКОЕ ИЗДАНИЕ", [RED[0], RED[1], RED[2], 230], below(96.0), a);

    // Dashed places for the decks.
    for (deck, at) in DECK_SPOTS {
        let col = deck_color(deck);
        let tt = place(t, at, DECK_ANGLE, 1.0);
        let spot = rounded_c(DECK_SPOT_SIZE.0, DECK_SPOT_SIZE.1, 14.0);
        fill(pm, &spot, sk(col, 45), tt);
        let s = Stroke {
            width: 3.0,
            dash: StrokeDash::new(vec![14.0, 9.0], 0.0),
            line_cap: LineCap::Round,
            ..Stroke::default()
        };
        pm.stroke_path(&spot, &paint(sk(col, 210)), &s, tt, None);
        let label = match deck {
            Deck::Chance => "ШАНС",
            Deck::Chest => "ОБЩЕСТВЕННАЯ КАЗНА",
        };
        let (sa, ca) = DECK_ANGLE.sin_cos();
        let under = at + Vec2::new(-sa * 104.0, ca * 104.0);
        fonts.draw(pm, t, Weight::Bold, 17.0, label, [col[0], col[1], col[2], 235], under, DECK_ANGLE);
    }
}

// ---------------------------------------------------------------------------
// Cards
// ---------------------------------------------------------------------------

pub const DECK_TEX: (u32, u32) = (512, 320);

/// The back of a card: what lies on top of a deck.
pub fn paint_deck_back(fonts: &Fonts, deck: Deck) -> Pixmap {
    let (w, h) = (DECK_TEX.0 as f32, DECK_TEX.1 as f32);
    let mut pm = Pixmap::new(DECK_TEX.0, DECK_TEX.1).expect("valid size");
    let t = Transform::identity();
    let col = deck_color(deck);
    let dark = col.map(|v| (v as f32 * 0.74) as u8);
    let body = rounded(0.0, 0.0, w, h, 30.0);
    let shader = LinearGradient::new(
        Point::from_xy(0.0, 0.0),
        Point::from_xy(w * 0.4, h),
        vec![GradientStop::new(0.0, sk(col, 255)), GradientStop::new(1.0, sk(dark, 255))],
        SpreadMode::Pad,
        Transform::identity(),
    )
    .expect("gradient");
    fill_shader(&mut pm, &body, shader, t);
    stroke(&mut pm, &rounded(16.0, 16.0, w - 32.0, h - 32.0, 18.0), sk(CREAM, 230), 5.0, t);
    let cream = [CREAM[0], CREAM[1], CREAM[2], 255];
    match deck {
        Deck::Chance => {
            fonts.draw(&mut pm, t, Weight::Bold, 168.0, "?", [0, 0, 0, 70], Vec2::new(w / 2.0 + 5.0, 128.0 + 7.0), 0.0);
            fonts.draw(&mut pm, t, Weight::Bold, 168.0, "?", cream, Vec2::new(w / 2.0, 128.0), 0.0);
            fonts.draw(&mut pm, t, Weight::Bold, 54.0, "ШАНС", cream, Vec2::new(w / 2.0, 252.0), 0.0);
        }
        Deck::Chest => {
            icon_chest(&mut pm, place(t, Vec2::new(w / 2.0, 126.0), 0.0, 2.0), GOLD);
            fonts.draw(&mut pm, t, Weight::Bold, 54.0, "КАЗНА", cream, Vec2::new(w / 2.0, 252.0), 0.0);
        }
    }
    pm
}

pub const CARD_TEX: (u32, u32) = (1024, 640);

/// The face of a drawn card with its text and who drew it.
pub fn paint_card_face(fonts: &Fonts, deck: Deck, text: &str, who: &str) -> Pixmap {
    let (w, h) = (CARD_TEX.0 as f32, CARD_TEX.1 as f32);
    let mut pm = Pixmap::new(CARD_TEX.0, CARD_TEX.1).expect("valid size");
    let t = Transform::identity();
    let col = deck_color(deck);
    let body = rounded(0.0, 0.0, w, h, 52.0);
    fill(&mut pm, &body, sk(CREAM, 255), t);
    // Header band clipped to the rounded top.
    let mut band = PathBuilder::new();
    band.move_to(0.0, 52.0);
    band.cubic_to(0.0, 23.0, 23.0, 0.0, 52.0, 0.0);
    band.line_to(w - 52.0, 0.0);
    band.cubic_to(w - 23.0, 0.0, w, 23.0, w, 52.0);
    band.line_to(w, 132.0);
    band.line_to(0.0, 132.0);
    band.close();
    fill(&mut pm, &band.finish().expect("band"), sk(col, 255), t);
    stroke(&mut pm, &rounded(4.0, 4.0, w - 8.0, h - 8.0, 48.0), sk(INK, 255), 6.0, t);
    let cream = [CREAM[0], CREAM[1], CREAM[2], 255];
    let title = match deck {
        Deck::Chance => "ШАНС",
        Deck::Chest => "ОБЩЕСТВЕННАЯ КАЗНА",
    };
    match deck {
        Deck::Chance => question(&mut pm, place(t, Vec2::new(78.0, 66.0), 0.0, 1.0), fonts, 84.0, CREAM),
        Deck::Chest => icon_chest(&mut pm, place(t, Vec2::new(80.0, 70.0), 0.0, 0.85), GOLD),
    }
    fonts.draw(&mut pm, t, Weight::Bold, 58.0, title, cream, Vec2::new(w / 2.0 + 30.0, 66.0), 0.0);
    fonts.block(&mut pm, t, Weight::Semibold, 54.0, text, ink(255), Vec2::new(w / 2.0, 330.0), 0.0, w - 130.0, 5);
    let mut pb = PathBuilder::new();
    pb.move_to(110.0, 540.0);
    pb.line_to(w - 110.0, 540.0);
    stroke(&mut pm, &pb.finish().expect("rule"), sk(INK, 40), 3.0, t);
    let (size, lines) = fonts.fit(Weight::Regular, 34.0, &format!("Тянет: {who}"), w - 200.0, 1);
    fonts.draw(&mut pm, t, Weight::Regular, size, &lines.join(" "), [PRICE[0], PRICE[1], PRICE[2], 255], Vec2::new(w / 2.0, 590.0), 0.0);
    pm
}

// ---------------------------------------------------------------------------
// Dice, overlays, wood
// ---------------------------------------------------------------------------

/// One die face per 256×256 cell, faces 1–6 in reading order (3×2).
pub const DICE_CELL: u32 = 256;

pub fn dice_cell(value: u8) -> (u32, u32) {
    let i = (value.clamp(1, 6) - 1) as u32;
    (i % 3, i / 3)
}

pub fn paint_dice() -> Pixmap {
    let cell = DICE_CELL as f32;
    let mut pm = Pixmap::new(DICE_CELL * 3, DICE_CELL * 2).expect("valid size");
    fill(&mut pm, &rect_path(VRect::new(0.0, 0.0, cell * 3.0, cell * 2.0)), sk([0xf7, 0xf3, 0xe8], 255), Transform::identity());
    for value in 1..=6u8 {
        let (cx, cy) = dice_cell(value);
        let o = Vec2::new(cx as f32 * cell + cell / 2.0, cy as f32 * cell + cell / 2.0);
        // Pip positions in half-face units.
        let d = 0.5;
        let pips: &[(f32, f32)] = match value {
            1 => &[(0.0, 0.0)],
            2 => &[(-d, -d), (d, d)],
            3 => &[(-d, -d), (0.0, 0.0), (d, d)],
            4 => &[(-d, -d), (d, -d), (-d, d), (d, d)],
            5 => &[(-d, -d), (d, -d), (0.0, 0.0), (-d, d), (d, d)],
            _ => &[(-d, -d), (d, -d), (-d, 0.0), (d, 0.0), (-d, d), (d, d)],
        };
        let (r, color) = if value == 1 { (34.0, [0xc8, 0x1e, 0x2a]) } else { (21.0, INK) };
        for (x, y) in pips {
            let p = o + Vec2::new(*x, *y) * cell * 0.5;
            // A darker rim makes the pip look drilled in.
            fill(&mut pm, &circle(p.x, p.y + 1.5, r + 2.5), SkColor::from_rgba8(0, 0, 0, 60), Transform::identity());
            fill(&mut pm, &circle(p.x, p.y, r), sk(color, 255), Transform::identity());
            fill(&mut pm, &circle(p.x - r * 0.25, p.y - r * 0.3, r * 0.35), SkColor::from_rgba8(255, 255, 255, 30), Transform::identity());
        }
    }
    pm
}

pub const MORTGAGE_TEX: (u32, u32) = (180, 300);

/// Dark veil over a mortgaged tile (a side tile drawn for the bottom row).
pub fn paint_mortgage(fonts: &Fonts) -> Pixmap {
    let (w, h) = (MORTGAGE_TEX.0 as f32, MORTGAGE_TEX.1 as f32);
    let mut pm = Pixmap::new(MORTGAGE_TEX.0, MORTGAGE_TEX.1).expect("valid size");
    let t = Transform::identity();
    fill(&mut pm, &rect_path(VRect::new(0.0, 0.0, w, h)), SkColor::from_rgba8(24, 24, 30, 150), t);
    let mut pb = PathBuilder::new();
    let mut x = -h;
    while x < w + h {
        pb.move_to(x, 0.0);
        pb.line_to(x + h, h);
        x += 22.0;
    }
    stroke(&mut pm, &pb.finish().expect("hatch"), SkColor::from_rgba8(255, 255, 255, 22), 5.0, t);
    let pill = place(t, Vec2::new(w / 2.0, h / 2.0), 0.0, 1.0);
    fill(&mut pm, &rounded_c(w - 14.0, 46.0, 12.0), sk([0x2a, 0x22, 0x10], 235), pill);
    stroke(&mut pm, &rounded_c(w - 14.0, 46.0, 12.0), sk([0xff, 0xd5, 0x4a], 255), 3.0, pill);
    let (size, _) = fonts.fit(Weight::Bold, 26.0, "ЗАЛОЖЕНО", w - 34.0, 1);
    fonts.draw(&mut pm, t, Weight::Bold, size, "ЗАЛОЖЕНО", [0xff, 0xd5, 0x4a, 255], Vec2::new(w / 2.0, h / 2.0), 0.0);
    pm
}

pub const WOOD_TEX: u32 = 1024;

/// Seamless dark wood for the table top.
pub fn paint_wood() -> Pixmap {
    let n = WOOD_TEX;
    let mut pm = Pixmap::new(n, n).expect("valid size");
    let dark = [0.105f32, 0.062, 0.038];
    let light = [0.2f32, 0.12, 0.07];
    let pixels = pm.pixels_mut();
    for y in 0..n {
        for x in 0..n {
            let u = x as f32 / n as f32;
            let v = y as f32 / n as f32;
            // Only whole periods across the texture, so it tiles.
            let t = v * 34.0 + 0.3 * (TAU * (u * 2.0 + v)).sin() + 0.12 * (TAU * (u * 5.0 + 3.0 * v)).sin();
            let ring = (0.5 + 0.5 * (TAU * t).sin()).powi(3);
            // Long thin streaks along the grain.
            let streak = crate::table3d::hash((y as u64) * 7919 + (x / 24) as u64);
            let fine = crate::table3d::hash((y as u64) * 104_729 + x as u64);
            let k = (ring * 0.45 + streak * 0.3 + fine * 0.08 + 0.1).clamp(0.0, 1.0);
            let c = |i: usize| ((dark[i] + (light[i] - dark[i]) * k) * 255.0) as u8;
            pixels[(y * n + x) as usize] = PremultipliedColorU8::from_rgba(c(0), c(1), c(2), 255).expect("opaque");
        }
    }
    pm
}

/// A texture that repeats (for the table).
pub fn repeating(mut image: Image) -> Image {
    if let ImageSampler::Descriptor(d) = &mut image.sampler {
        d.address_mode_u = ImageAddressMode::Repeat;
        d.address_mode_v = ImageAddressMode::Repeat;
    } else {
        image.sampler = ImageSampler::Descriptor(ImageSamplerDescriptor {
            address_mode_u: ImageAddressMode::Repeat,
            address_mode_v: ImageAddressMode::Repeat,
            ..ImageSamplerDescriptor::linear()
        });
    }
    image
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rouble_sign_is_in_the_font() {
        let fonts = Fonts::default();
        assert!(fonts.has_glyph(Weight::Semibold, '₽'));
        assert!(fonts.has_glyph(Weight::Bold, '₽'));
    }

    #[test]
    fn long_names_shrink_instead_of_breaking_words() {
        let fonts = Fonts::default();
        // Corners have their own print; every other tile shows its name.
        for i in (0..BOARD_SIZE).filter(|i| i % 10 != 0) {
            let name = tile(i).name;
            let (size, lines) = fonts.fit(Weight::Semibold, 15.0, name, 80.0, 3);
            // Every word of the name survives whole on some line.
            for word in name.split_whitespace() {
                assert!(lines.iter().any(|l| l.split_whitespace().any(|w| w == word)), "{name}: {lines:?}");
            }
            assert!(size > 9.0, "{name} became too small: {size}");
        }
    }

    #[test]
    fn board_paints() {
        let fonts = Fonts::default();
        let board = paint_board(&fonts);
        // Corners of the board are the cream of the tiles, the middle is mint.
        let mid = board.pixel(BOARD_TEX / 2 + 300, BOARD_TEX / 2).unwrap();
        assert!(mid.green() > mid.red());
        assert_eq!(board.pixel(BOARD_TEX / 2, BOARD_TEX / 2).unwrap().alpha(), 255);
        let face = paint_card_face(&fonts, Deck::Chance, "Перейдите на «СТАРТ». Получите ₽200", "Игрок");
        assert_eq!(face.pixel(0, 0).unwrap().alpha(), 0);
        assert_eq!(face.pixel(CARD_TEX.0 / 2, CARD_TEX.1 / 2).unwrap().alpha(), 255);
    }

    /// Writes PNGs of the art (run with --ignored, set MONO_DUMP_DIR).
    #[test]
    #[ignore]
    fn dump_art() {
        let dir = std::env::var("MONO_DUMP_DIR").unwrap_or_else(|_| "/tmp".into());
        let fonts = Fonts::default();
        paint_board(&fonts).save_png(format!("{dir}/board.png")).unwrap();
        paint_deck_back(&fonts, Deck::Chance).save_png(format!("{dir}/chance.png")).unwrap();
        paint_deck_back(&fonts, Deck::Chest).save_png(format!("{dir}/chest.png")).unwrap();
        paint_card_face(&fonts, Deck::Chest, "Наступила зрелость вашего страхового полиса. Получите ₽100", "Бот Арсений")
            .save_png(format!("{dir}/card.png"))
            .unwrap();
        paint_dice().save_png(format!("{dir}/dice.png")).unwrap();
        paint_mortgage(&fonts).save_png(format!("{dir}/mortgage.png")).unwrap();
        paint_wood().save_png(format!("{dir}/wood.png")).unwrap();
    }
}
