//! Board geometry in view-box units (the original SVG board is a 1110×1110
//! view box, x to the right, y down) and its mapping onto the 3D board, which
//! lies on the table with the «СТАРТ» corner nearest to the camera on the right.

use bevy::math::{Vec2, Vec3};
use monopoly_core::{BOARD_SIZE, JAIL_INDEX};

pub const VIEW: f32 = 1110.0;
pub const CORNER: f32 = 150.0;
pub const LONG: f32 = 150.0;
pub const SHORT: f32 = 90.0;
pub const STRIPE: f32 = 28.0;
pub const FAR: f32 = VIEW - CORNER;

/// Side of the board in world units.
pub const BOARD_SIDE: f32 = 6.0;
/// World units per view-box unit.
pub const UNIT: f32 = BOARD_SIDE / VIEW;
/// Height of the printed surface of the board.
pub const BOARD_TOP: f32 = 0.0;

/// The jail cell inside the «Тюрьма» corner (the rest of the corner is
/// «Просто посещение»).
pub const JAIL_CELL: VRect = VRect {
    min: Vec2::new(50.0, FAR),
    max: Vec2::new(CORNER, FAR + 100.0),
};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Side {
    Bottom,
    Left,
    Top,
    Right,
    Corner,
}

/// Axis-aligned rectangle in view-box units.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct VRect {
    pub min: Vec2,
    pub max: Vec2,
}

impl VRect {
    pub fn new(x: f32, y: f32, w: f32, h: f32) -> Self {
        VRect {
            min: Vec2::new(x, y),
            max: Vec2::new(x + w, y + h),
        }
    }

    pub fn center(&self) -> Vec2 {
        (self.min + self.max) / 2.0
    }

    pub fn size(&self) -> Vec2 {
        self.max - self.min
    }

    pub fn contains(&self, p: Vec2) -> bool {
        p.x >= self.min.x && p.x <= self.max.x && p.y >= self.min.y && p.y <= self.max.y
    }
}

/// Tile rectangle in view-box units and the side of the board it is on.
pub fn tile_rect(i: usize) -> (VRect, Side) {
    let r = VRect::new;
    match i % BOARD_SIZE {
        0 => (r(FAR, FAR, CORNER, CORNER), Side::Corner),
        i @ 1..=9 => (r(FAR - SHORT * i as f32, FAR, SHORT, LONG), Side::Bottom),
        10 => (r(0.0, FAR, CORNER, CORNER), Side::Corner),
        i @ 11..=19 => (r(0.0, FAR - SHORT * (i - 10) as f32, LONG, SHORT), Side::Left),
        20 => (r(0.0, 0.0, CORNER, CORNER), Side::Corner),
        i @ 21..=29 => (r(CORNER + SHORT * (i - 21) as f32, 0.0, SHORT, LONG), Side::Top),
        30 => (r(FAR, 0.0, CORNER, CORNER), Side::Corner),
        i => (r(FAR, CORNER + SHORT * (i - 31) as f32, LONG, SHORT), Side::Right),
    }
}

/// Side a tile belongs to for orientation purposes; a corner counts as the
/// side that starts at it.
pub fn side_of(i: usize) -> Side {
    match i % BOARD_SIZE {
        0..=9 => Side::Bottom,
        10..=19 => Side::Left,
        20..=29 => Side::Top,
        _ => Side::Right,
    }
}

/// Direction a pawn walks along the side of tile `i`.
pub fn travel_dir(i: usize) -> Vec2 {
    match side_of(i) {
        Side::Bottom => Vec2::new(-1.0, 0.0),
        Side::Left => Vec2::new(0.0, -1.0),
        Side::Top => Vec2::new(1.0, 0.0),
        _ => Vec2::new(0.0, 1.0),
    }
}

/// From the middle of the board towards the edge, across a side tile.
pub fn outward(side: Side) -> Vec2 {
    match side {
        Side::Bottom => Vec2::new(0.0, 1.0),
        Side::Left => Vec2::new(-1.0, 0.0),
        Side::Top => Vec2::new(0.0, -1.0),
        Side::Right => Vec2::new(1.0, 0.0),
        Side::Corner => Vec2::ZERO,
    }
}

/// Rotation of the print on a side (radians, clockwise in view-box axes):
/// the bottom of the text points to the edge of the board.
pub fn side_angle(side: Side) -> f32 {
    use std::f32::consts::{FRAC_PI_2, PI};
    match side {
        Side::Bottom | Side::Corner => 0.0,
        Side::Left => FRAC_PI_2,
        Side::Top => PI,
        Side::Right => -FRAC_PI_2,
    }
}

/// The colour stripe of a street tile.
pub fn stripe_rect(r: VRect, side: Side) -> Option<VRect> {
    let (x, y, w, h) = (r.min.x, r.min.y, r.size().x, r.size().y);
    Some(match side {
        Side::Bottom => VRect::new(x, y, w, STRIPE),
        Side::Left => VRect::new(x + w - STRIPE, y, STRIPE, h),
        Side::Top => VRect::new(x, y + h - STRIPE, w, STRIPE),
        Side::Right => VRect::new(x, y, STRIPE, h),
        Side::Corner => return None,
    })
}

/// View-box point to a world point at height `y`.
pub fn to_world(p: Vec2, y: f32) -> Vec3 {
    Vec3::new((p.x - VIEW / 2.0) * UNIT, y, (p.y - VIEW / 2.0) * UNIT)
}

/// World point to view-box coordinates (height is dropped).
pub fn from_world(p: Vec3) -> Vec2 {
    Vec2::new(p.x / UNIT + VIEW / 2.0, p.z / UNIT + VIEW / 2.0)
}

/// Tile under a view-box point.
pub fn tile_at(p: Vec2) -> Option<usize> {
    (0..BOARD_SIZE).find(|&i| tile_rect(i).0.contains(p))
}

/// Yaw (rotation about +Y) that turns a model's forward (-Z) towards the
/// view-box direction `d`.
pub fn yaw_of(d: Vec2) -> f32 {
    (-d.x).atan2(-d.y)
}

/// Yaw that lays print drawn for the bottom side onto `side`.
pub fn side_yaw(side: Side) -> f32 {
    -side_angle(side)
}

/// Where the `k`-th of `n` pawns standing together stands (view-box units),
/// and how much the pawns shrink so that they all fit.
pub fn pawn_slot(tile: usize, k: usize, n: usize, in_jail: bool) -> (Vec2, f32) {
    let tile = tile % BOARD_SIZE;
    let n = n.max(1);
    let k = k.min(n - 1);
    if tile == JAIL_INDEX {
        if in_jail {
            let c = JAIL_CELL.center();
            return grid_slot(c, Vec2::X, Vec2::Y, k, n, 26.0, 44.0);
        }
        // «Просто посещение»: along the L-shaped strip around the cell.
        const SPOTS: [(f32, f32); 5] = [(26.0, 1085.0), (75.0, 1085.0), (124.0, 1085.0), (25.0, 1035.0), (25.0, 986.0)];
        let (x, y) = SPOTS[k % SPOTS.len()];
        let lap = (k / SPOTS.len()) as f32;
        let scale = if n > SPOTS.len() { 0.72 } else { 0.9 };
        return (Vec2::new(x + lap * 9.0, y - lap * 9.0), scale);
    }
    let (r, side) = tile_rect(tile);
    if side == Side::Corner {
        return grid_slot(r.center(), Vec2::X, Vec2::Y, k, n, 30.0, 44.0);
    }
    // Side tile: the free part beyond the colour stripe. Pawns line up
    // across the tile first (one behind another as seen from the road), so
    // long pieces like the car never overlap.
    let v = outward(side);
    let u = travel_dir(tile);
    let c = r.center() + v * (STRIPE / 2.0);
    let (du, dv, scale): (&[f32], &[f32], f32) = match n {
        1 => (&[0.0], &[0.0], 1.0),
        2 => (&[0.0], &[-27.0, 27.0], 1.0),
        3 => (&[0.0], &[-40.0, 0.0, 40.0], 0.75),
        4 => (&[-21.0, 21.0], &[-27.0, 27.0], 0.78),
        5 | 6 => (&[-21.0, 21.0], &[-40.0, 0.0, 40.0], 0.74),
        _ => (&[-21.0, 21.0], &[-45.0, -15.0, 15.0, 45.0], 0.56),
    };
    let col = k % du.len();
    let row = (k / du.len()).min(dv.len() - 1);
    (c + u * du[col] + v * dv[row], scale)
}

/// Pawns on a square area: one in the middle, then 2×2, then 3×3.
fn grid_slot(c: Vec2, u: Vec2, v: Vec2, k: usize, n: usize, step2: f32, step3: f32) -> (Vec2, f32) {
    if n == 1 {
        return (c, 1.0);
    }
    if n <= 4 {
        let (x, y) = ((k % 2) as f32 - 0.5, (k / 2) as f32 - 0.5);
        return (c + u * x * step2 * 2.0 + v * y * step2 * 2.0, 0.85);
    }
    let (x, y) = ((k % 3) as f32 - 1.0, (k / 3) as f32 - 1.0);
    (c + u * x * step3 * 0.75 + v * y * step3 * 0.75, 0.62)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tiles_tile_the_ring() {
        // Every tile is found back from its own centre and the ring has no gaps.
        for i in 0..BOARD_SIZE {
            let (r, _) = tile_rect(i);
            assert_eq!(tile_at(r.center()), Some(i));
        }
        assert_eq!(tile_at(Vec2::splat(VIEW / 2.0)), None);
    }

    #[test]
    fn world_round_trip() {
        let p = Vec2::new(123.0, 987.0);
        assert!((from_world(to_world(p, 0.3)) - p).length() < 1e-3);
        // «СТАРТ» is near the camera (+z) on the right (+x).
        let go = to_world(tile_rect(0).0.center(), 0.0);
        assert!(go.x > 2.0 && go.z > 2.0);
    }

    #[test]
    fn yaw_points_forward_along_direction() {
        for d in [Vec2::X, Vec2::NEG_X, Vec2::Y, Vec2::NEG_Y] {
            let q = bevy::math::Quat::from_rotation_y(yaw_of(d));
            let f = q * Vec3::NEG_Z;
            assert!((f - Vec3::new(d.x, 0.0, d.y)).length() < 1e-5, "{d:?} -> {f:?}");
        }
    }

    #[test]
    fn pawn_slots_stay_on_their_tile() {
        for tile in 0..BOARD_SIZE {
            let (r, _) = tile_rect(tile);
            for n in 1..=8 {
                for k in 0..n {
                    let (p, s) = pawn_slot(tile, k, n, false);
                    assert!(r.contains(p), "tile {tile}: {k}/{n} at {p:?}");
                    assert!(s > 0.5 && s <= 1.0);
                }
            }
        }
        for n in 1..=8 {
            for k in 0..n {
                assert!(JAIL_CELL.contains(pawn_slot(JAIL_INDEX, k, n, true).0));
            }
        }
    }
}
