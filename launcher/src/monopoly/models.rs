//! Procedural 3D models of the board game: the eight pawns, houses and
//! hotels, and rounded dice. Everything is built from a few mesh generators
//! (rounded boxes, lathes, lofts, extrusions, tubes) plus Bevy's primitives.

use std::f32::consts::{FRAC_PI_2, TAU};

use bevy::asset::RenderAssetUsages;
use bevy::prelude::*;
use bevy::render::mesh::{Indices, MeshBuilder as _, PrimitiveTopology};

// ---------------------------------------------------------------------------
// Mesh building
// ---------------------------------------------------------------------------

/// Collects vertices and triangles. Triangles are wound automatically so that
/// they face along the vertex normals, so generators need not care about the
/// winding order.
#[derive(Default)]
pub struct MeshBuilder {
    pos: Vec<[f32; 3]>,
    nrm: Vec<[f32; 3]>,
    uv: Vec<[f32; 2]>,
    idx: Vec<u32>,
}

impl MeshBuilder {
    pub fn vertex(&mut self, p: Vec3, n: Vec3, uv: Vec2) -> u32 {
        self.pos.push(p.to_array());
        self.nrm.push(n.normalize_or_zero().to_array());
        self.uv.push(uv.to_array());
        (self.pos.len() - 1) as u32
    }

    pub fn tri(&mut self, a: u32, b: u32, c: u32) {
        let p = |i: u32| Vec3::from(self.pos[i as usize]);
        let n = |i: u32| Vec3::from(self.nrm[i as usize]);
        let face = (p(b) - p(a)).cross(p(c) - p(a));
        if face.length_squared() < 1e-14 {
            return;
        }
        if face.dot(n(a) + n(b) + n(c)) >= 0.0 {
            self.idx.extend([a, b, c]);
        } else {
            self.idx.extend([a, c, b]);
        }
    }

    /// Quad a-b-c-d (in order around its edge).
    pub fn quad(&mut self, a: u32, b: u32, c: u32, d: u32) {
        self.tri(a, b, c);
        self.tri(a, c, d);
    }

    /// A flat-shaded triangle facing away from `inside`.
    pub fn flat_tri(&mut self, a: Vec3, b: Vec3, c: Vec3, inside: Vec3) {
        let mut n = (b - a).cross(c - a).normalize_or_zero();
        if n.dot((a + b + c) / 3.0 - inside) < 0.0 {
            n = -n;
        }
        let i = self.vertex(a, n, Vec2::ZERO);
        let j = self.vertex(b, n, Vec2::X);
        let k = self.vertex(c, n, Vec2::Y);
        self.tri(i, j, k);
    }

    pub fn build(self) -> Mesh {
        Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::default())
            .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, self.pos)
            .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, self.nrm)
            .with_inserted_attribute(Mesh::ATTRIBUTE_UV_0, self.uv)
            .with_inserted_indices(Indices::U32(self.idx))
    }
}

/// Faces of a box: normal and the two in-face axes.
const BOX_FACES: [(Vec3, Vec3, Vec3); 6] = [
    (Vec3::Y, Vec3::Z, Vec3::X),
    (Vec3::NEG_Y, Vec3::X, Vec3::Z),
    (Vec3::Z, Vec3::X, Vec3::Y),
    (Vec3::NEG_Z, Vec3::Y, Vec3::X),
    (Vec3::X, Vec3::Y, Vec3::Z),
    (Vec3::NEG_X, Vec3::Z, Vec3::Y),
];

/// A box with rounded edges and corners. `uv(face, s, t)` maps a point of a
/// face (`s`, `t` in 0..1 along the face's axes) to texture coordinates.
pub fn rounded_box_uv(size: Vec3, radius: f32, seg: u32, uv: impl Fn(usize, f32, f32) -> Vec2) -> Mesh {
    let he = size / 2.0;
    let r = radius.min(he.min_element() * 0.999);
    let inner = he - Vec3::splat(r);
    let mut b = MeshBuilder::default();
    for (f, (n, u, v)) in BOX_FACES.iter().enumerate() {
        let start = b.pos.len() as u32;
        for j in 0..=seg {
            for i in 0..=seg {
                let s = i as f32 / seg as f32;
                let t = j as f32 / seg as f32;
                let p = *n * (*n * he).abs().max_element() * 1.0
                    + *u * ((s * 2.0 - 1.0) * (*u * he).abs().max_element())
                    + *v * ((t * 2.0 - 1.0) * (*v * he).abs().max_element());
                let q = p.clamp(-inner, inner);
                let normal = (p - q).normalize_or(*n);
                b.vertex(q + normal * r, normal, uv(f, s, t));
            }
        }
        let row = seg + 1;
        for j in 0..seg {
            for i in 0..seg {
                let a = start + j * row + i;
                b.quad(a, a + 1, a + row + 1, a + row);
            }
        }
    }
    b.build()
}

pub fn rounded_box(size: Vec3, radius: f32) -> Mesh {
    rounded_box_uv(size, radius, 6, |_, s, t| Vec2::new(s, t))
}

/// A solid of revolution around +Y. `profile` is a list of (radius, height)
/// points; edges sharper than ~35° stay crisp, the rest is smooth.
pub fn lathe(profile: &[(f32, f32)], segments: u32) -> Mesh {
    let pts: Vec<Vec2> = profile.iter().map(|(r, y)| Vec2::new(*r, *y)).collect();
    let seg_normal = |i: usize| {
        let d = pts[i + 1] - pts[i];
        Vec2::new(d.y, -d.x).normalize_or_zero()
    };
    let mut b = MeshBuilder::default();
    let ring = |b: &mut MeshBuilder, p: Vec2, n: Vec2, v: f32| -> u32 {
        let start = b.pos.len() as u32;
        for k in 0..=segments {
            let a = k as f32 / segments as f32 * TAU;
            let (s, c) = a.sin_cos();
            b.vertex(Vec3::new(p.x * c, p.y, p.x * s), Vec3::new(n.x * c, n.y, n.x * s), Vec2::new(k as f32 / segments as f32, v));
        }
        start
    };
    let count = pts.len();
    let mut rings_in = vec![0u32; count];
    let mut rings_out = vec![0u32; count];
    for i in 0..count {
        let v = i as f32 / (count - 1).max(1) as f32;
        let before = (i > 0).then(|| seg_normal(i - 1));
        let after = (i + 1 < count).then(|| seg_normal(i));
        match (before, after) {
            (Some(a), Some(c)) if a.dot(c) > 0.82 => {
                let r = ring(&mut b, pts[i], (a + c).normalize_or(a), v);
                rings_in[i] = r;
                rings_out[i] = r;
            }
            _ => {
                if let Some(a) = before {
                    rings_in[i] = ring(&mut b, pts[i], a, v);
                }
                if let Some(c) = after {
                    rings_out[i] = ring(&mut b, pts[i], c, v);
                }
            }
        }
    }
    for i in 0..count - 1 {
        let (lo, hi) = (rings_out[i], rings_in[i + 1]);
        for k in 0..segments {
            b.quad(lo + k, lo + k + 1, hi + k + 1, hi + k);
        }
    }
    b.build()
}

/// A prism along Z (from −depth/2 to +depth/2) with a convex outline in the
/// XY plane: flat caps and flat sides.
pub fn extrude(outline: &[Vec2], depth: f32) -> Mesh {
    let mut b = MeshBuilder::default();
    let n = outline.len();
    let centre = outline.iter().copied().sum::<Vec2>() / n as f32;
    let inside = centre.extend(0.0);
    let z = depth / 2.0;
    for i in 0..n {
        let (p, q) = (outline[i], outline[(i + 1) % n]);
        for zz in [z, -z] {
            b.flat_tri(centre.extend(zz), p.extend(zz), q.extend(zz), inside);
        }
        b.flat_tri(p.extend(z), q.extend(z), q.extend(-z), inside);
        b.flat_tri(p.extend(z), q.extend(-z), p.extend(-z), inside);
    }
    b.build()
}

/// A smooth-sided solid between two convex outlines in the XZ plane (the same
/// number of points), at heights `y0` and `y1`, with flat caps.
pub fn loft(bottom: &[Vec2], y0: f32, top: &[Vec2], y1: f32) -> Mesh {
    let n = bottom.len();
    let mut b = MeshBuilder::default();
    let lo: Vec<Vec3> = bottom.iter().map(|p| Vec3::new(p.x, y0, p.y)).collect();
    let hi: Vec<Vec3> = top.iter().map(|p| Vec3::new(p.x, y1, p.y)).collect();
    let centre = (lo.iter().chain(hi.iter()).copied().sum::<Vec3>()) / (2 * n) as f32;
    let face_n = |i: usize| {
        let j = (i + 1) % n;
        let nn = (lo[j] - lo[i]).cross(hi[i] - lo[i]).normalize_or_zero();
        if nn.dot((lo[i] + lo[j]) / 2.0 - centre) < 0.0 { -nn } else { nn }
    };
    let vn: Vec<Vec3> = (0..n).map(|i| (face_n((i + n - 1) % n) + face_n(i)).normalize_or_zero()).collect();
    let lo_i: Vec<u32> = (0..n).map(|i| b.vertex(lo[i], vn[i], Vec2::new(i as f32 / n as f32, 1.0))).collect();
    let hi_i: Vec<u32> = (0..n).map(|i| b.vertex(hi[i], vn[i], Vec2::new(i as f32 / n as f32, 0.0))).collect();
    for i in 0..n {
        let j = (i + 1) % n;
        b.quad(lo_i[i], lo_i[j], hi_i[j], hi_i[i]);
    }
    let cl = lo.iter().copied().sum::<Vec3>() / n as f32;
    let ch = hi.iter().copied().sum::<Vec3>() / n as f32;
    for i in 0..n {
        let j = (i + 1) % n;
        b.flat_tri(cl, lo[i], lo[j], centre);
        b.flat_tri(ch, hi[i], hi[j], centre);
    }
    b.build()
}

/// A tube swept along `path` with a radius per point (tapering to a tip).
pub fn tube(path: &[Vec3], radii: &[f32], sides: u32) -> Mesh {
    let mut b = MeshBuilder::default();
    let n = path.len();
    let tangent = |i: usize| {
        let a = path[i.saturating_sub(1)];
        let c = path[(i + 1).min(n - 1)];
        (c - a).normalize_or(Vec3::Y)
    };
    let mut normal = tangent(0).any_orthonormal_vector();
    let mut rings = Vec::new();
    for i in 0..n {
        let t = tangent(i);
        // Parallel transport keeps the rings from twisting.
        normal = (normal - t * normal.dot(t)).normalize_or(t.any_orthonormal_vector());
        let bin = t.cross(normal);
        let start = b.pos.len() as u32;
        for k in 0..=sides {
            let a = k as f32 / sides as f32 * TAU;
            let dir = normal * a.cos() + bin * a.sin();
            b.vertex(path[i] + dir * radii[i], dir, Vec2::new(k as f32 / sides as f32, i as f32 / n as f32));
        }
        rings.push(start);
    }
    for i in 0..n - 1 {
        for k in 0..sides {
            let (a, c) = (rings[i] + k, rings[i + 1] + k);
            b.quad(a, a + 1, c + 1, c);
        }
    }
    // Cap the thick end.
    let centre = b.vertex(path[0], -tangent(0), Vec2::ZERO);
    for k in 0..sides {
        let (p, q) = (rings[0] + k, rings[0] + k + 1);
        let (pp, qp) = (Vec3::from(b.pos[p as usize]), Vec3::from(b.pos[q as usize]));
        let i = b.vertex(pp, -tangent(0), Vec2::ZERO);
        let j = b.vertex(qp, -tangent(0), Vec2::ZERO);
        b.tri(centre, i, j);
    }
    b.build()
}

/// A faceted five-pointed star in the XY plane, facing ±Z.
pub fn star(outer: f32, inner: f32, rim: f32, apex: f32) -> Mesh {
    let mut b = MeshBuilder::default();
    let pts: Vec<Vec2> = (0..10)
        .map(|i| {
            let a = FRAC_PI_2 + i as f32 / 10.0 * TAU;
            let r = if i % 2 == 0 { outer } else { inner };
            Vec2::new(a.cos(), a.sin()) * r
        })
        .collect();
    for i in 0..10 {
        let (p, q) = (pts[i], pts[(i + 1) % 10]);
        for s in [1.0f32, -1.0] {
            b.flat_tri(Vec3::new(0.0, 0.0, apex * s), p.extend(rim * s), q.extend(rim * s), Vec3::ZERO);
        }
        b.flat_tri(p.extend(rim), q.extend(rim), q.extend(-rim), Vec3::ZERO);
        b.flat_tri(p.extend(rim), q.extend(-rim), p.extend(-rim), Vec3::ZERO);
    }
    b.build()
}

// ---------------------------------------------------------------------------
// Dice
// ---------------------------------------------------------------------------

/// Which value each face of [`BOX_FACES`] shows: opposite faces add up to 7.
pub const DIE_FACE_VALUES: [u8; 6] = [1, 6, 2, 5, 3, 4];

/// Rotation that puts `value` on top.
pub fn die_rotation(value: u8) -> Quat {
    let face = DIE_FACE_VALUES.iter().position(|v| *v == value).unwrap_or(0);
    Quat::from_rotation_arc(BOX_FACES[face].0, Vec3::Y)
}

/// A rounded die whose faces sample a 3×2 atlas (see `art::paint_dice`).
pub fn die_mesh(size: f32) -> Mesh {
    rounded_box_uv(Vec3::splat(size), size * 0.16, 7, |face, s, t| {
        let (cx, cy) = super::art::dice_cell(DIE_FACE_VALUES[face]);
        Vec2::new((cx as f32 + s) / 3.0, (cy as f32 + t) / 2.0)
    })
}

// ---------------------------------------------------------------------------
// Pawns
// ---------------------------------------------------------------------------

/// Which material a part of a pawn wears.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum Paint {
    /// The player's colour, glossy.
    Body,
    /// The player's colour, darker (the stand).
    Stand,
    Dark,
    White,
    Chrome,
    Glass,
    Headlight,
    Taillight,
    Eye,
    CatEye,
    Gold,
    Deck,
    Window,
}

pub struct Part {
    pub mesh: Handle<Mesh>,
    pub paint: Paint,
    pub transform: Transform,
}

/// Pawns are modelled about one unit tall and scaled by this onto the board.
pub const PAWN_SCALE: f32 = 0.36;
/// Height of the stand under every pawn (model units).
pub const STAND_H: f32 = 0.08;

/// Builds the parts of every pawn, in the order of `monopoly_core::TOKENS`
/// (hat, car, dog, boot, ship, cat, rocket, star). The stand is first in each.
pub fn pawn_parts(meshes: &mut Assets<Mesh>) -> Vec<Vec<Part>> {
    let stand = meshes.add(lathe(
        &[(0.0, -STAND_H), (0.36, -STAND_H), (0.4, -STAND_H + 0.025), (0.4, -0.025), (0.37, 0.0), (0.0, 0.0)],
        40,
    ));
    fn part(meshes: &mut Assets<Mesh>, mesh: Mesh, paint: Paint, transform: Transform) -> Part {
        Part {
            mesh: meshes.add(mesh),
            paint,
            transform,
        }
    }
    let at = |x: f32, y: f32, z: f32| Transform::from_xyz(x, y, z);
    let along_z = Quat::from_rotation_x(FRAC_PI_2);
    let axle = Quat::from_rotation_z(FRAC_PI_2);

    let hat = vec![
        part(
            meshes,
            lathe(
                &[
                    (0.0, 0.0),
                    (0.42, 0.0),
                    (0.47, 0.022),
                    (0.47, 0.048),
                    (0.43, 0.068),
                    (0.31, 0.072),
                    (0.29, 0.1),
                    (0.30, 0.5),
                    (0.325, 0.83),
                    (0.315, 0.865),
                    (0.29, 0.878),
                    (0.0, 0.878),
                ],
                48,
            ),
            Paint::Body,
            Transform::IDENTITY,
        ),
        part(meshes, lathe(&[(0.296, 0.1), (0.31, 0.106), (0.312, 0.24), (0.3, 0.246)], 48), Paint::Dark, Transform::IDENTITY),
        part(meshes, rounded_box(Vec3::new(0.05, 0.1, 0.06), 0.02), Paint::Gold, at(0.0, 0.173, -0.3)),
    ];

    let mut car = vec![
        part(meshes, rounded_box(Vec3::new(0.5, 0.18, 0.92), 0.07), Paint::Body, at(0.0, 0.17, 0.0)),
        part(meshes, rounded_box(Vec3::new(0.42, 0.17, 0.46), 0.07), Paint::Glass, at(0.0, 0.32, 0.06)),
        part(meshes, rounded_box(Vec3::new(0.44, 0.05, 0.38), 0.025), Paint::Body, at(0.0, 0.405, 0.08)),
        part(meshes, rounded_box(Vec3::new(0.46, 0.045, 0.05), 0.02), Paint::Chrome, at(0.0, 0.1, -0.465)),
        part(meshes, rounded_box(Vec3::new(0.46, 0.045, 0.05), 0.02), Paint::Chrome, at(0.0, 0.1, 0.465)),
        part(meshes, rounded_box(Vec3::new(0.22, 0.05, 0.02), 0.01), Paint::Dark, at(0.0, 0.18, -0.462)),
    ];
    let wheel = meshes.add(Cylinder::new(0.11, 0.08).mesh().resolution(28).build());
    let hub = meshes.add(Cylinder::new(0.055, 0.086).mesh().resolution(20).build());
    for (x, z) in [(-0.235, -0.29), (0.235, -0.29), (-0.235, 0.29), (0.235, 0.29)] {
        car.push(Part { mesh: wheel.clone(), paint: Paint::Dark, transform: at(x, 0.11, z).with_rotation(axle) });
        car.push(Part { mesh: hub.clone(), paint: Paint::Chrome, transform: at(x, 0.11, z).with_rotation(axle) });
    }
    let lamp = meshes.add(Sphere::new(0.04).mesh().uv(16, 10));
    for x in [-0.16, 0.16] {
        car.push(Part { mesh: lamp.clone(), paint: Paint::Headlight, transform: at(x, 0.19, -0.45) });
        car.push(Part { mesh: lamp.clone(), paint: Paint::Taillight, transform: at(x, 0.2, 0.45).with_scale(Vec3::splat(0.85)) });
    }

    let mut dog = vec![
        part(meshes, Capsule3d::new(0.15, 0.34).mesh().rings(8).latitudes(16).longitudes(24).build(), Paint::Body, at(0.0, 0.36, 0.05).with_rotation(along_z)),
        part(meshes, Sphere::new(0.15).mesh().uv(24, 16), Paint::Body, at(0.0, 0.39, -0.15)),
        part(meshes, Sphere::new(0.13).mesh().uv(24, 16), Paint::Body, at(0.0, 0.59, -0.25)),
        part(meshes, rounded_box(Vec3::new(0.13, 0.11, 0.17), 0.05), Paint::Body, at(0.0, 0.54, -0.38)),
        part(meshes, Sphere::new(0.034).mesh().uv(12, 8), Paint::Eye, at(0.0, 0.575, -0.47)),
        part(meshes, Cone { radius: 0.06, height: 0.11 }.mesh().resolution(16).build(), Paint::Body, at(0.0, 0.45, -0.37).with_rotation(Quat::from_rotation_x(std::f32::consts::PI))),
        part(meshes, Cone { radius: 0.035, height: 0.2 }.mesh().resolution(12).build(), Paint::Body, at(0.0, 0.52, 0.3).with_rotation(Quat::from_rotation_x(0.55))),
        part(meshes, Torus::new(0.085, 0.125).mesh().minor_resolution(10).major_resolution(28).build(), Paint::Gold, at(0.0, 0.5, -0.2).with_rotation(Quat::from_rotation_x(-0.85))),
    ];
    let leg = meshes.add(Cylinder::new(0.055, 0.28).mesh().resolution(14).build());
    let paw = meshes.add(Sphere::new(0.06).mesh().uv(12, 8));
    for (x, z) in [(-0.085, -0.15), (0.085, -0.15), (-0.085, 0.22), (0.085, 0.22)] {
        dog.push(Part { mesh: leg.clone(), paint: Paint::Body, transform: at(x, 0.14, z) });
        dog.push(Part { mesh: paw.clone(), paint: Paint::Body, transform: at(x, 0.03, z - 0.02).with_scale(Vec3::new(1.0, 0.7, 1.2)) });
    }
    let ear = meshes.add(Cone { radius: 0.045, height: 0.13 }.mesh().resolution(12).build());
    let eye = meshes.add(Sphere::new(0.022).mesh().uv(10, 6));
    for s in [-1.0f32, 1.0] {
        dog.push(Part { mesh: ear.clone(), paint: Paint::Body, transform: at(s * 0.07, 0.72, -0.23).with_rotation(Quat::from_rotation_z(-s * 0.3)) });
        dog.push(Part { mesh: eye.clone(), paint: Paint::Eye, transform: at(s * 0.06, 0.63, -0.355) });
    }

    let boot = vec![
        part(meshes, rounded_box(Vec3::new(0.3, 0.05, 0.78), 0.022), Paint::Dark, at(0.0, 0.025, -0.06)),
        part(meshes, rounded_box(Vec3::new(0.26, 0.1, 0.2), 0.03), Paint::Dark, at(0.0, 0.05, 0.22)),
        part(meshes, Capsule3d::new(0.135, 0.34).mesh().rings(8).latitudes(16).longitudes(24).build(), Paint::Body, Transform::from_xyz(0.0, 0.16, -0.13).with_rotation(along_z).with_scale(Vec3::new(1.0, 1.0, 0.85))),
        part(
            meshes,
            lathe(&[(0.0, 0.1), (0.155, 0.1), (0.15, 0.4), (0.165, 0.66), (0.176, 0.7), (0.16, 0.712), (0.0, 0.712)], 32),
            Paint::Body,
            at(0.0, 0.0, 0.13),
        ),
        part(meshes, Torus::new(0.15, 0.19).mesh().minor_resolution(10).major_resolution(32).build(), Paint::Dark, at(0.0, 0.69, 0.13)),
        part(meshes, rounded_box(Vec3::new(0.13, 0.022, 0.03), 0.01), Paint::White, at(0.0, 0.3, -0.015)),
        part(meshes, rounded_box(Vec3::new(0.13, 0.022, 0.03), 0.01), Paint::White, at(0.0, 0.38, -0.012)),
        part(meshes, rounded_box(Vec3::new(0.13, 0.022, 0.03), 0.01), Paint::White, at(0.0, 0.46, -0.01)),
    ];

    let deck_outline: Vec<Vec2> = [
        (0.0, -0.5),
        (0.12, -0.36),
        (0.2, -0.18),
        (0.225, 0.0),
        (0.225, 0.2),
        (0.205, 0.34),
        (0.15, 0.42),
        (0.0, 0.445),
        (-0.15, 0.42),
        (-0.205, 0.34),
        (-0.225, 0.2),
        (-0.225, 0.0),
        (-0.2, -0.18),
        (-0.12, -0.36),
    ]
    .iter()
    .map(|(x, z)| Vec2::new(*x, *z))
    .collect();
    let keel: Vec<Vec2> = deck_outline.iter().map(|p| Vec2::new(p.x * 0.62, p.y * 0.84 + 0.03)).collect();
    let at_h = |h: f32, k: f32| -> Vec<Vec2> { keel.iter().zip(&deck_outline).map(|(a, b)| a.lerp(*b, h) * k).collect() };
    let ship = vec![
        part(meshes, loft(&keel, 0.0, &deck_outline, 0.25), Paint::Body, Transform::IDENTITY),
        part(meshes, loft(&at_h(0.0, 1.03), -0.005, &at_h(0.28, 1.03), 0.07), Paint::Dark, Transform::IDENTITY),
        part(meshes, loft(&at_h(1.0, 0.94), 0.24, &at_h(1.0, 0.94), 0.258), Paint::Deck, Transform::IDENTITY),
        part(meshes, rounded_box(Vec3::new(0.3, 0.13, 0.44), 0.03), Paint::White, at(0.0, 0.31, 0.05)),
        part(meshes, rounded_box(Vec3::new(0.306, 0.035, 0.4), 0.015), Paint::Glass, at(0.0, 0.33, 0.05)),
        part(meshes, rounded_box(Vec3::new(0.24, 0.1, 0.17), 0.03), Paint::White, at(0.0, 0.42, -0.1)),
        part(meshes, rounded_box(Vec3::new(0.246, 0.03, 0.12), 0.012), Paint::Glass, at(0.0, 0.43, -0.12)),
        part(meshes, Cylinder::new(0.055, 0.22).mesh().resolution(20).build(), Paint::Body, at(0.0, 0.48, 0.07)),
        part(meshes, Cylinder::new(0.058, 0.045).mesh().resolution(20).build(), Paint::Dark, at(0.0, 0.59, 0.07)),
        part(meshes, Cylinder::new(0.055, 0.22).mesh().resolution(20).build(), Paint::Body, at(0.0, 0.48, 0.21)),
        part(meshes, Cylinder::new(0.058, 0.045).mesh().resolution(20).build(), Paint::Dark, at(0.0, 0.59, 0.21)),
        part(meshes, Cylinder::new(0.013, 0.32).mesh().resolution(8).build(), Paint::Dark, at(0.0, 0.41, -0.3)),
    ];

    let mut cat = vec![
        part(meshes, Sphere::new(0.2).mesh().uv(28, 18), Paint::Body, at(0.0, 0.26, 0.06).with_scale(Vec3::new(1.0, 1.3, 1.05))),
        part(meshes, Sphere::new(0.14).mesh().uv(24, 16), Paint::Body, at(0.0, 0.34, -0.08).with_scale(Vec3::new(1.0, 1.2, 1.0))),
        part(meshes, Sphere::new(0.155).mesh().uv(28, 18), Paint::Body, at(0.0, 0.6, -0.08).with_scale(Vec3::new(1.1, 0.95, 1.0))),
        part(meshes, Sphere::new(0.065).mesh().uv(16, 10), Paint::White, at(0.0, 0.55, -0.212).with_scale(Vec3::new(1.3, 0.8, 0.9))),
        part(meshes, Sphere::new(0.022).mesh().uv(10, 6), Paint::Eye, at(0.0, 0.585, -0.27)),
        part(
            meshes,
            tube(
                &[
                    Vec3::new(0.04, 0.07, 0.24),
                    Vec3::new(0.16, 0.045, 0.27),
                    Vec3::new(0.27, 0.045, 0.16),
                    Vec3::new(0.29, 0.055, 0.0),
                    Vec3::new(0.24, 0.075, -0.13),
                    Vec3::new(0.15, 0.095, -0.2),
                ],
                &[0.05, 0.048, 0.045, 0.04, 0.034, 0.018],
                12,
            ),
            Paint::Body,
            Transform::IDENTITY,
        ),
    ];
    let cat_ear = meshes.add(Cone { radius: 0.062, height: 0.13 }.mesh().resolution(12).build());
    let cat_eye = meshes.add(Sphere::new(0.028).mesh().uv(12, 8));
    let fore = meshes.add(Capsule3d::new(0.045, 0.2).mesh().rings(4).latitudes(10).longitudes(14).build());
    let toe = meshes.add(Sphere::new(0.05).mesh().uv(12, 8));
    let haunch = meshes.add(Sphere::new(0.12).mesh().uv(20, 12));
    for s in [-1.0f32, 1.0] {
        cat.push(Part { mesh: cat_ear.clone(), paint: Paint::Body, transform: at(s * 0.095, 0.735, -0.08).with_rotation(Quat::from_rotation_z(-s * 0.35)) });
        cat.push(Part { mesh: cat_eye.clone(), paint: Paint::CatEye, transform: at(s * 0.062, 0.635, -0.21).with_scale(Vec3::new(1.0, 1.15, 0.7)) });
        cat.push(Part { mesh: fore.clone(), paint: Paint::Body, transform: at(s * 0.07, 0.15, -0.13) });
        cat.push(Part { mesh: toe.clone(), paint: Paint::White, transform: at(s * 0.07, 0.035, -0.17).with_scale(Vec3::new(1.0, 0.75, 1.2)) });
        cat.push(Part { mesh: haunch.clone(), paint: Paint::Body, transform: at(s * 0.13, 0.13, 0.1).with_scale(Vec3::new(0.8, 1.0, 1.2)) });
    }

    let mut rocket = vec![
        part(
            meshes,
            lathe(&[(0.0, 0.08), (0.1, 0.08), (0.14, 0.14), (0.163, 0.3), (0.165, 0.5), (0.15, 0.62), (0.122, 0.7), (0.0, 0.7)], 36),
            Paint::Body,
            Transform::IDENTITY,
        ),
        part(meshes, lathe(&[(0.124, 0.69), (0.105, 0.76), (0.072, 0.85), (0.032, 0.91), (0.0, 0.93)], 36), Paint::White, Transform::IDENTITY),
        part(meshes, ConicalFrustum { radius_top: 0.085, radius_bottom: 0.11, height: 0.09 }.mesh().resolution(28).build(), Paint::Chrome, at(0.0, 0.045, 0.0)),
        part(meshes, Torus::new(0.045, 0.068).mesh().minor_resolution(10).major_resolution(24).build(), Paint::Chrome, at(0.0, 0.47, -0.158).with_rotation(Quat::from_rotation_x(FRAC_PI_2))),
        part(meshes, Sphere::new(0.05).mesh().uv(16, 10), Paint::Window, at(0.0, 0.47, -0.152).with_scale(Vec3::new(1.0, 1.0, 0.4))),
    ];
    let fin = meshes.add(extrude(&[Vec2::new(0.12, 0.36), Vec2::new(0.12, 0.06), Vec2::new(0.31, 0.0), Vec2::new(0.31, 0.13)], 0.036));
    for k in 0..3 {
        let yaw = Quat::from_rotation_y(FRAC_PI_2 + k as f32 * TAU / 3.0);
        rocket.push(Part { mesh: fin.clone(), paint: Paint::White, transform: Transform::from_rotation(yaw) });
    }

    let star_pawn = vec![
        part(meshes, star(0.37, 0.165, 0.035, 0.11), Paint::Body, at(0.0, 0.57, 0.0)),
        part(meshes, Cylinder::new(0.035, 0.42).mesh().resolution(14).build(), Paint::Gold, at(0.0, 0.21, 0.0)),
        part(meshes, Sphere::new(0.06).mesh().uv(14, 10), Paint::Gold, at(0.0, 0.05, 0.0)),
    ];

    let mut all = vec![hat, car, dog, boot, ship, cat, rocket, star_pawn];
    for parts in &mut all {
        parts.insert(0, Part { mesh: stand.clone(), paint: Paint::Stand, transform: Transform::IDENTITY });
    }
    all
}

// ---------------------------------------------------------------------------
// Buildings
// ---------------------------------------------------------------------------

/// A house: a block with a gable roof, the ridge along Z.
pub fn house_mesh(width: f32, height: f32, roof: f32, depth: f32) -> Mesh {
    let w = width / 2.0;
    extrude(
        &[Vec2::new(-w, 0.0), Vec2::new(w, 0.0), Vec2::new(w, height), Vec2::new(0.0, height + roof), Vec2::new(-w, height)],
        depth,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn outward_ratio(mesh: &Mesh, centre: Vec3) -> f32 {
        let pos = mesh.attribute(Mesh::ATTRIBUTE_POSITION).unwrap().as_float3().unwrap();
        let Some(Indices::U32(idx)) = mesh.indices() else { panic!("indices") };
        let mut out = 0;
        let tris = idx.len() / 3;
        for t in idx.chunks(3) {
            let [a, b, c] = [t[0], t[1], t[2]].map(|i| Vec3::from(pos[i as usize]));
            let n = (b - a).cross(c - a);
            if n.dot((a + b + c) / 3.0 - centre) > 0.0 {
                out += 1;
            }
        }
        out as f32 / tris as f32
    }

    #[test]
    fn solids_face_outwards() {
        assert_eq!(outward_ratio(&rounded_box(Vec3::splat(1.0), 0.2), Vec3::ZERO), 1.0);
        assert_eq!(outward_ratio(&die_mesh(1.0), Vec3::ZERO), 1.0);
        assert_eq!(outward_ratio(&lathe(&[(0.0, 0.0), (0.5, 0.0), (0.5, 1.0), (0.0, 1.0)], 16), Vec3::Y * 0.5), 1.0);
        assert_eq!(outward_ratio(&extrude(&[Vec2::new(-1.0, 0.0), Vec2::new(1.0, 0.0), Vec2::new(0.0, 1.0)], 0.5), Vec3::new(0.0, 0.3, 0.0)), 1.0);
        assert_eq!(outward_ratio(&house_mesh(1.0, 1.0, 0.5, 1.0), Vec3::new(0.0, 0.6, 0.0)), 1.0);
        assert_eq!(outward_ratio(&star(1.0, 0.4, 0.1, 0.3), Vec3::ZERO), 1.0);
        let sq = [Vec2::new(-1.0, -1.0), Vec2::new(1.0, -1.0), Vec2::new(1.0, 1.0), Vec2::new(-1.0, 1.0)];
        assert_eq!(outward_ratio(&loft(&sq, 0.0, &sq, 1.0), Vec3::Y * 0.5), 1.0);
    }

    #[test]
    fn dice_show_the_right_face_on_top() {
        for v in 1..=6u8 {
            let face = DIE_FACE_VALUES.iter().position(|x| *x == v).unwrap();
            let up = die_rotation(v) * BOX_FACES[face].0;
            assert!((up - Vec3::Y).length() < 1e-5, "{v}: {up:?}");
            // Opposite faces add up to seven.
            let opposite = DIE_FACE_VALUES.iter().position(|x| *x == 7 - v).unwrap();
            assert!((BOX_FACES[face].0 + BOX_FACES[opposite].0).length() < 1e-6);
        }
    }

    #[test]
    fn every_pawn_has_a_stand_and_a_body() {
        let mut meshes = Assets::<Mesh>::default();
        let parts = pawn_parts(&mut meshes);
        assert_eq!(parts.len(), monopoly_core::TOKENS.len());
        for p in &parts {
            assert_eq!(p[0].paint, Paint::Stand);
            assert!(p.iter().any(|x| x.paint == Paint::Body));
        }
    }
}
