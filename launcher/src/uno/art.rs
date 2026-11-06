//! Procedurally painted card faces (no image assets needed).
//!
//! Every face is drawn once at startup with tiny-skia (shapes) and ab_glyph
//! (digits), then uploaded with a full mip chain so cards stay crisp when seen
//! at a grazing angle across the table.

use ab_glyph::{Font, FontRef, PxScale, ScaleFont};
use bevy::asset::RenderAssetUsages;
use bevy::image::{ImageAddressMode, ImageFilterMode, ImageSampler, ImageSamplerDescriptor};
use bevy::prelude::*;
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};
use tiny_skia::{
    Color as SkColor, FillRule, GradientStop, LineCap, LinearGradient, Paint, Path, PathBuilder,
    Pixmap, Point, Shader, SpreadMode, Stroke, Transform,
};
use uno_core::{Card, CardColor, Face};

pub const TEX_W: u32 = 256;
pub const TEX_H: u32 = 384;
pub(crate) const RADIUS: f32 = 26.0;
pub(crate) const BORDER: f32 = 11.0;

pub(crate) const FONT: &[u8] = include_bytes!("../../../assets/fonts/Inter-ExtraBold.otf");

/// Index of a face texture: 0..52 coloured cards, 52 wild, 53 wild draw four.
pub fn face_index(card: &Card) -> usize {
    let face_slot = match card.face {
        Face::Number(n) => n as usize,
        Face::Skip => 10,
        Face::Reverse => 11,
        Face::DrawTwo => 12,
        Face::Wild => return 52,
        Face::WildDrawFour => return 53,
    };
    card.color.map(|c| c.index()).unwrap_or(0) * 13 + face_slot
}

pub const FACE_COUNT: usize = 54;

pub(crate) fn sk(c: [u8; 3], a: u8) -> SkColor {
    SkColor::from_rgba8(c[0], c[1], c[2], a)
}

pub(crate) fn paint(color: SkColor) -> Paint<'static> {
    let mut p = Paint::default();
    p.set_color(color);
    p.anti_alias = true;
    p
}

pub(crate) fn rounded_rect(x: f32, y: f32, w: f32, h: f32, r: f32) -> Path {
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

pub(crate) fn circle(cx: f32, cy: f32, r: f32) -> Path {
    PathBuilder::from_circle(cx, cy, r).expect("valid circle")
}

pub(crate) struct Painter<'a> {
    pub(crate) pm: Pixmap,
    font: FontRef<'a>,
}

impl Painter<'_> {
    pub(crate) fn fill(&mut self, path: &Path, color: SkColor) {
        self.pm
            .fill_path(path, &paint(color), FillRule::Winding, Transform::identity(), None);
    }

    pub(crate) fn fill_t(&mut self, path: &Path, color: SkColor, t: Transform) {
        self.pm.fill_path(path, &paint(color), FillRule::Winding, t, None);
    }

    pub(crate) fn fill_shader(&mut self, path: &Path, shader: Shader<'static>) {
        let mut p = Paint::default();
        p.shader = shader;
        p.anti_alias = true;
        self.pm
            .fill_path(path, &p, FillRule::Winding, Transform::identity(), None);
    }

    pub(crate) fn stroke(&mut self, path: &Path, color: SkColor, width: f32, t: Transform) {
        let stroke = Stroke {
            width,
            line_cap: LineCap::Round,
            ..Stroke::default()
        };
        self.pm.stroke_path(path, &paint(color), &stroke, t, None);
    }

    /// Draws `text` centred at (cx, cy) with an optional rotation (radians).
    pub(crate) fn text(&mut self, text: &str, cx: f32, cy: f32, size: f32, color: [u8; 4], rotate: f32) {
        let scaled = self.font.as_scaled(PxScale::from(size));
        // Measure.
        let mut width = 0.0;
        let mut prev = None;
        for ch in text.chars() {
            let id = scaled.glyph_id(ch);
            if let Some(p) = prev {
                width += scaled.kern(p, id);
            }
            width += scaled.h_advance(id);
            prev = Some(id);
        }
        let ascent = scaled.ascent();
        let descent = scaled.descent();
        // Cap-height centring looks better for digits than ascent/descent.
        let cap = ascent * 0.72;
        let _ = descent;
        let mut x = -width / 2.0;
        let baseline = cap / 2.0;
        let (sin, cos) = rotate.sin_cos();
        let mut prev = None;
        let w = self.pm.width() as i32;
        let h = self.pm.height() as i32;
        for ch in text.chars() {
            let id = scaled.glyph_id(ch);
            if let Some(p) = prev {
                x += scaled.kern(p, id);
            }
            let glyph = id.with_scale_and_position(PxScale::from(size), ab_glyph::point(x, baseline));
            x += scaled.h_advance(id);
            prev = Some(id);
            let Some(outlined) = self.font.outline_glyph(glyph) else { continue };
            let bounds = outlined.px_bounds();
            let pixels = self.pm.pixels_mut();
            // Supersample the rotated glyph by rasterising into a local buffer
            // and splatting it through the rotation.
            outlined.draw(|gx, gy, coverage| {
                if coverage <= 0.0 {
                    return;
                }
                let lx = bounds.min.x + gx as f32 + 0.5;
                let ly = bounds.min.y + gy as f32 + 0.5;
                let px = cx + lx * cos - ly * sin;
                let py = cy + lx * sin + ly * cos;
                let (ix, iy) = (px.floor() as i32, py.floor() as i32);
                if ix < 0 || iy < 0 || ix >= w || iy >= h {
                    return;
                }
                let a = coverage.min(1.0) * color[3] as f32 / 255.0;
                let dst = &mut pixels[(iy * w + ix) as usize];
                let blend = |s: u8, d: u8| -> u8 {
                    (s as f32 * a + d as f32 * (1.0 - a)).round().clamp(0.0, 255.0) as u8
                };
                let r = blend(color[0], dst.red());
                let g = blend(color[1], dst.green());
                let b = blend(color[2], dst.blue());
                let al = (255.0 * a + dst.alpha() as f32 * (1.0 - a)).round().clamp(0.0, 255.0) as u8;
                if let Some(c) = tiny_skia::PremultipliedColorU8::from_rgba(r.min(al), g.min(al), b.min(al), al) {
                    *dst = c;
                }
            });
        }
    }
}

pub(crate) fn mask_from(path: &Path) -> Option<tiny_skia::Mask> {
    let mut mask = tiny_skia::Mask::new(TEX_W, TEX_H)?;
    mask.fill_path(path, FillRule::Winding, true, Transform::identity());
    Some(mask)
}

pub(crate) fn new_painter() -> Painter<'static> {
    Painter {
        pm: Pixmap::new(TEX_W, TEX_H).expect("valid size"),
        font: FontRef::try_from_slice(FONT).expect("bundled font parses"),
    }
}

pub(crate) const CREAM: [u8; 3] = [0xFB, 0xF7, 0xEE];

pub(crate) fn card_body(p: &mut Painter, field_top: [u8; 3], field_bottom: [u8; 3]) {
    let (w, h) = (TEX_W as f32, TEX_H as f32);
    p.fill(&rounded_rect(0.0, 0.0, w, h, RADIUS), sk(CREAM, 255));
    let inner = rounded_rect(BORDER, BORDER, w - 2.0 * BORDER, h - 2.0 * BORDER, RADIUS - BORDER + 4.0);
    let shader = LinearGradient::new(
        Point::from_xy(0.0, BORDER),
        Point::from_xy(0.0, h - BORDER),
        vec![
            GradientStop::new(0.0, sk(field_top, 255)),
            GradientStop::new(1.0, sk(field_bottom, 255)),
        ],
        SpreadMode::Pad,
        Transform::identity(),
    )
    .expect("valid gradient");
    p.fill_shader(&inner, shader);
    // Subtle diagonal sheen stripes.
    for i in 0..7 {
        let x0 = -120.0 + i as f32 * 70.0;
        let mut pb = PathBuilder::new();
        pb.move_to(x0, h);
        pb.line_to(x0 + 22.0, h);
        pb.line_to(x0 + 22.0 + 180.0, 0.0);
        pb.line_to(x0 + 180.0, 0.0);
        pb.close();
        let stripe = pb.finish().expect("valid stripe");
        let mut paint = paint(SkColor::from_rgba8(255, 255, 255, 14));
        paint.anti_alias = true;
        let mask = mask_from(&inner);
        p.pm.fill_path(&stripe, &paint, FillRule::Winding, Transform::identity(), mask.as_ref());
    }
}

pub(crate) fn shade(c: [u8; 3], k: f32) -> [u8; 3] {
    [
        (c[0] as f32 * k).min(255.0) as u8,
        (c[1] as f32 * k).min(255.0) as u8,
        (c[2] as f32 * k).min(255.0) as u8,
    ]
}

/// Draws an action symbol (skip / reverse) centred at (cx, cy) with size `s`.
fn draw_symbol(p: &mut Painter, face: Face, cx: f32, cy: f32, s: f32, color: [u8; 3], rotate: f32) {
    let t = Transform::from_rotate_at(rotate.to_degrees(), cx, cy);
    match face {
        Face::Skip => {
            let ring = circle(cx, cy, s * 0.42);
            p.stroke(&ring, sk(color, 255), s * 0.13, t);
            let mut pb = PathBuilder::new();
            let d = s * 0.42 * std::f32::consts::FRAC_1_SQRT_2;
            pb.move_to(cx - d, cy + d);
            pb.line_to(cx + d, cy - d);
            p.stroke(&pb.finish().expect("line"), sk(color, 255), s * 0.13, t);
        }
        Face::Reverse => {
            // Two curved arrows chasing each other.
            for flip in [0.0f32, 180.0] {
                let tt = t.pre_concat(Transform::from_rotate_at(flip, cx, cy));
                let r = s * 0.36;
                let mut pb = PathBuilder::new();
                let steps = 16;
                for i in 0..=steps {
                    let a = (200.0 + i as f32 * 110.0 / steps as f32).to_radians();
                    let (x, y) = (cx + r * a.cos(), cy + r * a.sin());
                    if i == 0 {
                        pb.move_to(x, y);
                    } else {
                        pb.line_to(x, y);
                    }
                }
                p.stroke(&pb.finish().expect("arc"), sk(color, 255), s * 0.12, tt);
                // Arrow head at the end of the arc.
                let a = 310f32.to_radians();
                let (ex, ey) = (cx + r * a.cos(), cy + r * a.sin());
                let tangent = Vec2::new(-a.sin(), a.cos());
                let normal = Vec2::new(a.cos(), a.sin());
                let tip = Vec2::new(ex, ey) + tangent * s * 0.2;
                let b1 = Vec2::new(ex, ey) + normal * s * 0.15;
                let b2 = Vec2::new(ex, ey) - normal * s * 0.15;
                let mut pb = PathBuilder::new();
                pb.move_to(tip.x, tip.y);
                pb.line_to(b1.x, b1.y);
                pb.line_to(b2.x, b2.y);
                pb.close();
                p.fill_t(&pb.finish().expect("head"), sk(color, 255), tt);
            }
        }
        _ => {}
    }
}

fn quadrant_disc(p: &mut Painter, cx: f32, cy: f32, r: f32) {
    let colors = [CardColor::Red, CardColor::Blue, CardColor::Yellow, CardColor::Green];
    for (i, c) in colors.iter().enumerate() {
        let start = (i as f32 * 90.0 - 90.0).to_radians();
        let mut pb = PathBuilder::new();
        pb.move_to(cx, cy);
        for k in 0..=12 {
            let a = start + (k as f32 / 12.0) * std::f32::consts::FRAC_PI_2;
            pb.line_to(cx + r * a.cos(), cy + r * a.sin());
        }
        pb.close();
        p.fill(&pb.finish().expect("quadrant"), sk(c.rgb(), 255));
    }
    p.stroke(&circle(cx, cy, r), sk(CREAM, 255), 5.0, Transform::identity());
}

fn label(face: Face) -> Option<&'static str> {
    Some(match face {
        Face::Number(n) => ["0", "1", "2", "3", "4", "5", "6", "7", "8", "9"][n as usize],
        Face::DrawTwo => "+2",
        Face::WildDrawFour => "+4",
        _ => return None,
    })
}

fn paint_face(card: Card) -> Pixmap {
    let mut p = new_painter();
    let (w, h) = (TEX_W as f32, TEX_H as f32);
    let (cx, cy) = (w / 2.0, h / 2.0);
    match card.color {
        Some(color) => {
            let base = color.rgb();
            card_body(&mut p, shade(base, 1.12), shade(base, 0.86));
            // Centre medallion.
            p.fill(&circle(cx + 3.0, cy + 5.0, 84.0), SkColor::from_rgba8(0, 0, 0, 50));
            p.fill(&circle(cx, cy, 84.0), sk(CREAM, 255));
            p.stroke(&circle(cx, cy, 72.0), sk(shade(base, 0.9), 255), 3.0, Transform::identity());
            let ink = if color == CardColor::Yellow { shade(base, 0.74) } else { shade(base, 0.92) };
            match label(card.face) {
                Some(text) => {
                    let size = if text.len() > 1 { 104.0 } else { 132.0 };
                    p.text(text, cx + 3.0, cy + 4.0, size, [0, 0, 0, 60], 0.0);
                    p.text(text, cx, cy, size, [ink[0], ink[1], ink[2], 255], 0.0);
                    // Underline 6 and 9 so they can't be confused.
                    if matches!(card.face, Face::Number(6) | Face::Number(9)) {
                        let mut pb = PathBuilder::new();
                        pb.move_to(cx - 26.0, cy + 58.0);
                        pb.line_to(cx + 26.0, cy + 58.0);
                        p.stroke(&pb.finish().expect("line"), sk(ink, 255), 7.0, Transform::identity());
                    }
                }
                None => draw_symbol(&mut p, card.face, cx, cy, 120.0, ink, 0.0),
            }
            // Corner indices.
            for (x, y, rot) in [(36.0, 44.0, 0.0f32), (w - 36.0, h - 44.0, std::f32::consts::PI)] {
                match label(card.face) {
                    Some(text) => {
                        let size = if text.len() > 1 { 34.0 } else { 44.0 };
                        p.text(text, x, y, size, [255, 255, 255, 255], rot);
                    }
                    None => draw_symbol(&mut p, card.face, x, y, 44.0, CREAM, rot),
                }
            }
        }
        None => {
            card_body(&mut p, [0x2A, 0x2D, 0x36], [0x12, 0x13, 0x18]);
            quadrant_disc(&mut p, cx, cy, 84.0);
            if let Some(text) = label(card.face) {
                p.text(text, cx + 3.0, cy + 4.0, 100.0, [0, 0, 0, 120], 0.0);
                p.text(text, cx, cy, 100.0, [255, 255, 255, 255], 0.0);
            }
            for (x, y, rot) in [(38.0, 46.0, 0.0f32), (w - 38.0, h - 46.0, std::f32::consts::PI)] {
                let t = Transform::from_rotate_at(rot.to_degrees(), x, y);
                for (i, c) in CardColor::ALL.iter().enumerate() {
                    let (dx, dy) = [(-7.0, -7.0), (7.0, -7.0), (-7.0, 7.0), (7.0, 7.0)][i];
                    p.fill_t(&circle(x + dx, y + dy, 7.0), sk(c.rgb(), 255), t);
                }
                if let Some(text) = label(card.face) {
                    p.text(text, x, y + 30.0 * if rot == 0.0 { 1.0 } else { -1.0 }, 26.0, [255, 255, 255, 255], rot);
                }
            }
        }
    }
    p.pm
}

fn paint_back() -> Pixmap {
    let mut p = new_painter();
    let (w, h) = (TEX_W as f32, TEX_H as f32);
    let (cx, cy) = (w / 2.0, h / 2.0);
    card_body(&mut p, [0x23, 0x2B, 0x45], [0x10, 0x14, 0x24]);
    // Diamond lattice.
    let inner = rounded_rect(BORDER, BORDER, w - 2.0 * BORDER, h - 2.0 * BORDER, RADIUS - BORDER + 4.0);
    let mask = mask_from(&inner);
    let mut pb = PathBuilder::new();
    let step = 32.0;
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
    p.pm.stroke_path(
        &lattice,
        &paint(SkColor::from_rgba8(255, 255, 255, 18)),
        &stroke,
        Transform::identity(),
        mask.as_ref(),
    );
    // Emblem: a tilted square split into the four colours.
    let t = Transform::from_rotate_at(45.0, cx, cy);
    let s = 52.0;
    let quads = [
        (CardColor::Red, -s, -s),
        (CardColor::Blue, 0.0, -s),
        (CardColor::Yellow, -s, 0.0),
        (CardColor::Green, 0.0, 0.0),
    ];
    p.fill_t(&rounded_rect(cx - s - 8.0, cy - s - 8.0, 2.0 * s + 16.0, 2.0 * s + 16.0, 18.0), sk(CREAM, 255), t);
    for (c, dx, dy) in quads {
        p.fill_t(&rounded_rect(cx + dx + 3.0, cy + dy + 3.0, s - 6.0, s - 6.0, 8.0), sk(c.rgb(), 255), t);
    }
    p.text("ПОСЛЕДНЯЯ", cx, 74.0, 30.0, [0xFB, 0xF7, 0xEE, 235], 0.0);
    p.text("КАРТА", cx, h - 70.0, 38.0, [0xFB, 0xF7, 0xEE, 235], 0.0);
    p.pm
}

/// Builds a texture with a full mip chain from a premultiplied pixmap.
pub(crate) fn to_image(pm: &Pixmap) -> Image {
    let (mut w, mut h) = (pm.width() as usize, pm.height() as usize);
    // Work in premultiplied f32 so averaging doesn't create dark fringes.
    let mut level: Vec<[f32; 4]> = pm
        .pixels()
        .iter()
        .map(|p| [p.red() as f32, p.green() as f32, p.blue() as f32, p.alpha() as f32])
        .collect();
    let mut data = Vec::new();
    let mut levels = 0;
    loop {
        for px in &level {
            let a = px[3];
            if a <= 0.0 {
                data.extend_from_slice(&[0, 0, 0, 0]);
            } else {
                let k = 255.0 / a;
                data.extend_from_slice(&[
                    (px[0] * k).round().clamp(0.0, 255.0) as u8,
                    (px[1] * k).round().clamp(0.0, 255.0) as u8,
                    (px[2] * k).round().clamp(0.0, 255.0) as u8,
                    a.round().clamp(0.0, 255.0) as u8,
                ]);
            }
        }
        levels += 1;
        if w == 1 && h == 1 {
            break;
        }
        let (nw, nh) = ((w / 2).max(1), (h / 2).max(1));
        let mut next = vec![[0.0f32; 4]; nw * nh];
        for y in 0..nh {
            for x in 0..nw {
                let mut acc = [0.0f32; 4];
                for (dx, dy) in [(0, 0), (1, 0), (0, 1), (1, 1)] {
                    let sx = (x * 2 + dx).min(w - 1);
                    let sy = (y * 2 + dy).min(h - 1);
                    let s = level[sy * w + sx];
                    for c in 0..4 {
                        acc[c] += s[c] * 0.25;
                    }
                }
                next[y * nw + x] = acc;
            }
        }
        level = next;
        w = nw;
        h = nh;
    }
    let mut image = Image::new_uninit(
        Extent3d {
            width: pm.width(),
            height: pm.height(),
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        TextureFormat::Rgba8UnormSrgb,
        RenderAssetUsages::RENDER_WORLD,
    );
    image.data = Some(data);
    image.texture_descriptor.mip_level_count = levels;
    image.sampler = ImageSampler::Descriptor(ImageSamplerDescriptor {
        address_mode_u: ImageAddressMode::ClampToEdge,
        address_mode_v: ImageAddressMode::ClampToEdge,
        address_mode_w: ImageAddressMode::ClampToEdge,
        mag_filter: ImageFilterMode::Linear,
        min_filter: ImageFilterMode::Linear,
        mipmap_filter: ImageFilterMode::Linear,
        anisotropy_clamp: 16,
        ..default()
    });
    image
}

/// Materials for every card face plus the back.
#[derive(Resource)]
pub struct CardArt {
    pub faces: Vec<Handle<StandardMaterial>>,
    pub back: Handle<StandardMaterial>,
    pub mesh: Handle<Mesh>,
}

/// Card size in world units (2:3 like a real card).
pub const CARD_W: f32 = 0.56;
pub const CARD_H: f32 = 0.84;

pub fn build_card_art(
    images: &mut Assets<Image>,
    materials: &mut Assets<StandardMaterial>,
    meshes: &mut Assets<Mesh>,
) -> CardArt {
    let make = |pm: &Pixmap, images: &mut Assets<Image>, materials: &mut Assets<StandardMaterial>| {
        let tex = images.add(to_image(pm));
        materials.add(StandardMaterial {
            base_color_texture: Some(tex),
            alpha_mode: AlphaMode::Mask(0.5),
            perceptual_roughness: 0.42,
            reflectance: 0.35,
            ..default()
        })
    };
    let mut faces = Vec::with_capacity(FACE_COUNT);
    for i in 0..FACE_COUNT {
        let card = if i < 52 {
            let color = CardColor::ALL[i / 13];
            let face = match i % 13 {
                n @ 0..=9 => Face::Number(n as u8),
                10 => Face::Skip,
                11 => Face::Reverse,
                _ => Face::DrawTwo,
            };
            Card { id: 0, color: Some(color), face }
        } else if i == 52 {
            Card { id: 0, color: None, face: Face::Wild }
        } else {
            Card { id: 0, color: None, face: Face::WildDrawFour }
        };
        faces.push(make(&paint_face(card), images, materials));
    }
    let back = make(&paint_back(), images, materials);
    let mesh = meshes.add(Rectangle::new(CARD_W, CARD_H));
    CardArt { faces, back, mesh }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn face_indices_are_unique() {
        let mut seen = std::collections::HashSet::new();
        for c in uno_core::full_deck() {
            seen.insert(face_index(&c));
        }
        assert_eq!(seen.len(), FACE_COUNT);
        assert!(seen.iter().all(|i| *i < FACE_COUNT));
    }

    #[test]
    fn faces_paint_and_build_mips() {
        let img = to_image(&paint_face(Card { id: 0, color: Some(CardColor::Red), face: Face::Number(7) }));
        assert_eq!(img.texture_descriptor.mip_level_count, 9);
        let back = paint_back();
        // Corners are transparent, the centre is opaque.
        assert_eq!(back.pixel(0, 0).unwrap().alpha(), 0);
        assert_eq!(back.pixel(TEX_W / 2, TEX_H / 2).unwrap().alpha(), 255);
    }

    /// Writes PNGs of a few faces to the scratch dir (run with --ignored).
    #[test]
    #[ignore]
    fn dump_faces() {
        let dir = std::env::var("CARD_DUMP_DIR").unwrap_or_else(|_| "/tmp".into());
        let cards = [
            Card { id: 0, color: Some(CardColor::Red), face: Face::Number(7) },
            Card { id: 0, color: Some(CardColor::Blue), face: Face::Skip },
            Card { id: 0, color: Some(CardColor::Green), face: Face::Reverse },
            Card { id: 0, color: Some(CardColor::Yellow), face: Face::DrawTwo },
            Card { id: 0, color: Some(CardColor::Blue), face: Face::Number(6) },
            Card { id: 0, color: None, face: Face::Wild },
            Card { id: 0, color: None, face: Face::WildDrawFour },
        ];
        for (i, c) in cards.iter().enumerate() {
            paint_face(*c).save_png(format!("{dir}/card{i}.png")).unwrap();
        }
        paint_back().save_png(format!("{dir}/back.png")).unwrap();
    }
}
