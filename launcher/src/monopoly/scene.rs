//! The 3D board on the table, seen from above: the camera frames the board
//! in the part of the window left of the side panel. Pawns hop from tile to
//! tile, houses and hotels pop up on the colour stripes, owners mark their
//! tiles with a bar in their colour, mortgaged tiles get a dark veil, the dice
//! are thrown onto the board and a drawn card rises from its deck to the
//! camera.

use std::collections::HashMap;
use std::f32::consts::{FRAC_PI_2, PI};

use bevy::pbr::{CascadeShadowConfigBuilder, NotShadowCaster};
use bevy::render::mesh::MeshBuilder as _;
use bevy::prelude::*;
use bevy::window::PrimaryWindow;
use monopoly_core::{tile, Deck, Phase, PlayerId, BOARD_SIZE, HOTEL_LEVEL, JAIL_INDEX, TOKENS};

use super::art::{self, Fonts, DECK_ANGLE, DECK_SPOTS};
use super::geom::{
    outward, pawn_slot, side_yaw, stripe_rect, tile_rect, to_world, travel_dir, yaw_of, VRect, BOARD_SIDE,
    BOARD_TOP, LONG, SHORT, UNIT, VIEW,
};
use super::models::{self, die_rotation, Paint, Part, PAWN_SCALE, STAND_H};
use super::{BoardView, MonoUi, DICE_ROLL_SECS};
use crate::net::Session;
use crate::table3d::hash;
use crate::uno::art::to_image;
use crate::MainCamera;

/// Camera elevation above the table.
const PITCH: f32 = 60.0 * PI / 180.0;
/// A longer lens than the card tables use: less perspective, so the far
/// row of the board stays readable.
const FOV: f32 = 30.0 * PI / 180.0;
/// Size of a die.
const DIE: f32 = 0.3;
/// Where the dice come to rest (view-box units).
const DICE_SPOT: Vec2 = Vec2::new(318.0, 818.0);
/// A card of the decks (world units, along and across the card).
const CARD_W: f32 = 1.0;
const CARD_D: f32 = 0.625;
const DECK_H: f32 = 0.07;
const TABLE_Y: f32 = -0.06;

/// Root of everything spawned for the board.
#[derive(Component)]
pub struct MonoRoot;

#[derive(Component)]
pub struct Pawn {
    yaw: f32,
    scale: f32,
}

/// Something that sits on the board and glides to its place; it pops in when
/// spawned and shrinks away when it is removed.
#[derive(Component)]
pub struct Piece {
    target: Transform,
    grow: f32,
    vanish: bool,
}

impl Piece {
    fn new(target: Transform) -> Self {
        Piece {
            target,
            grow: 0.0,
            vanish: false,
        }
    }
}

#[derive(Component)]
pub struct DieEntity(usize);

#[derive(Component)]
pub struct TurnRing;

#[derive(Component)]
pub struct TurnPointer;

#[derive(Component)]
pub struct Highlight;

/// Materials of one player.
struct PlayerPaint {
    body: Handle<StandardMaterial>,
    stand: Handle<StandardMaterial>,
    marker: Handle<StandardMaterial>,
}

#[derive(Resource)]
pub struct MonoAssets {
    fonts: Fonts,
    board: Handle<StandardMaterial>,
    board_mesh: Handle<Mesh>,
    slab_mesh: Handle<Mesh>,
    slab: Handle<StandardMaterial>,
    table: Handle<StandardMaterial>,
    table_mesh: Handle<Mesh>,
    deck_backs: HashMap<Deck, Handle<StandardMaterial>>,
    card_mesh: Handle<Mesh>,
    deck_block: Handle<Mesh>,
    card_edge: Handle<StandardMaterial>,
    die_mesh: Handle<Mesh>,
    die: Handle<StandardMaterial>,
    pawns: Vec<Vec<Part>>,
    paints: HashMap<Paint, Handle<StandardMaterial>>,
    house_mesh: Handle<Mesh>,
    hotel_mesh: Handle<Mesh>,
    house: Handle<StandardMaterial>,
    hotel: Handle<StandardMaterial>,
    marker_mesh: Handle<Mesh>,
    mortgage_mesh: Handle<Mesh>,
    mortgage: Handle<StandardMaterial>,
    unit_cube: Handle<Mesh>,
    gold_glow: Handle<StandardMaterial>,
    ring_mesh: Handle<Mesh>,
    pointer_mesh: Handle<Mesh>,
}

fn srgb(c: [u8; 3]) -> Color {
    Color::srgb_u8(c[0], c[1], c[2])
}

pub fn setup_assets(
    mut commands: Commands,
    mut images: ResMut<Assets<Image>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut meshes: ResMut<Assets<Mesh>>,
) {
    let fonts = Fonts::default();
    let board_tex = images.add(to_image(&art::paint_board(&fonts)));
    let board = materials.add(StandardMaterial {
        base_color_texture: Some(board_tex),
        perceptual_roughness: 0.62,
        reflectance: 0.32,
        ..default()
    });
    // The printed top of the board: one quad with the board texture.
    let h = BOARD_SIDE / 2.0;
    let mut b = models::MeshBuilder::default();
    let corners = [(-h, -h, 0.0, 0.0), (h, -h, 1.0, 0.0), (h, h, 1.0, 1.0), (-h, h, 0.0, 1.0)];
    let idx: Vec<u32> = corners
        .iter()
        .map(|(x, z, u, v)| b.vertex(Vec3::new(*x, 0.0, *z), Vec3::Y, Vec2::new(*u, *v)))
        .collect();
    b.quad(idx[0], idx[1], idx[2], idx[3]);
    let board_mesh = meshes.add(b.build());

    let wood = images.add(art::repeating(to_image(&art::paint_wood())));
    let table = materials.add(StandardMaterial {
        base_color_texture: Some(wood),
        perceptual_roughness: 0.45,
        reflectance: 0.3,
        ..default()
    });
    let mut b = models::MeshBuilder::default();
    let (s, rep) = (30.0, 9.0);
    let idx: Vec<u32> = [(-s, -s, 0.0, 0.0), (s, -s, rep, 0.0), (s, s, rep, rep), (-s, s, 0.0, rep)]
        .iter()
        .map(|(x, z, u, v)| b.vertex(Vec3::new(*x, 0.0, *z), Vec3::Y, Vec2::new(*u, *v)))
        .collect();
    b.quad(idx[0], idx[1], idx[2], idx[3]);
    let table_mesh = meshes.add(b.build());

    let card_material = |pm: &tiny_skia::Pixmap, images: &mut Assets<Image>, materials: &mut Assets<StandardMaterial>| {
        materials.add(StandardMaterial {
            base_color_texture: Some(images.add(to_image(pm))),
            alpha_mode: AlphaMode::Mask(0.5),
            perceptual_roughness: 0.5,
            reflectance: 0.3,
            ..default()
        })
    };
    let deck_backs = [Deck::Chance, Deck::Chest]
        .into_iter()
        .map(|d| (d, card_material(&art::paint_deck_back(&fonts, d), &mut images, &mut materials)))
        .collect();

    let die_tex = images.add(to_image(&art::paint_dice()));
    let die = materials.add(StandardMaterial {
        base_color_texture: Some(die_tex),
        perceptual_roughness: 0.25,
        reflectance: 0.45,
        clearcoat: 0.5,
        clearcoat_perceptual_roughness: 0.2,
        ..default()
    });

    let mortgage_tex = images.add(to_image(&art::paint_mortgage(&fonts)));
    let mortgage = materials.add(StandardMaterial {
        base_color_texture: Some(mortgage_tex),
        alpha_mode: AlphaMode::Blend,
        perceptual_roughness: 0.8,
        ..default()
    });

    let mut paints = HashMap::new();
    let mut paint = |p: Paint, m: StandardMaterial| {
        paints.insert(p, materials.add(m));
    };
    paint(Paint::Dark, StandardMaterial {
        base_color: Color::srgb(0.07, 0.07, 0.08),
        perceptual_roughness: 0.38,
        ..default()
    });
    paint(Paint::White, StandardMaterial {
        base_color: Color::srgb(0.93, 0.92, 0.88),
        perceptual_roughness: 0.35,
        ..default()
    });
    paint(Paint::Chrome, StandardMaterial {
        base_color: Color::srgb(0.82, 0.83, 0.86),
        metallic: 0.55,
        perceptual_roughness: 0.18,
        reflectance: 0.8,
        ..default()
    });
    paint(Paint::Glass, StandardMaterial {
        base_color: Color::srgb(0.09, 0.13, 0.19),
        perceptual_roughness: 0.06,
        reflectance: 0.9,
        ..default()
    });
    paint(Paint::Headlight, StandardMaterial {
        base_color: Color::srgb(1.0, 0.97, 0.85),
        emissive: LinearRgba::rgb(3.0, 2.7, 1.9),
        ..default()
    });
    paint(Paint::Taillight, StandardMaterial {
        base_color: Color::srgb(0.9, 0.1, 0.08),
        emissive: LinearRgba::rgb(2.4, 0.12, 0.08),
        ..default()
    });
    paint(Paint::Eye, StandardMaterial {
        base_color: Color::srgb(0.02, 0.02, 0.03),
        perceptual_roughness: 0.08,
        reflectance: 0.9,
        ..default()
    });
    paint(Paint::CatEye, StandardMaterial {
        base_color: Color::srgb(0.72, 0.86, 0.18),
        emissive: LinearRgba::rgb(0.35, 0.45, 0.05),
        perceptual_roughness: 0.1,
        ..default()
    });
    paint(Paint::Gold, StandardMaterial {
        base_color: Color::srgb(0.95, 0.74, 0.26),
        metallic: 0.45,
        perceptual_roughness: 0.25,
        reflectance: 0.7,
        ..default()
    });
    paint(Paint::Deck, StandardMaterial {
        base_color: Color::srgb(0.78, 0.6, 0.4),
        perceptual_roughness: 0.6,
        ..default()
    });
    paint(Paint::Window, StandardMaterial {
        base_color: Color::srgb(0.45, 0.75, 1.0),
        emissive: LinearRgba::rgb(0.15, 0.35, 0.6),
        perceptual_roughness: 0.05,
        ..default()
    });

    let gloss = |c: Color| StandardMaterial {
        base_color: c,
        perceptual_roughness: 0.3,
        reflectance: 0.5,
        clearcoat: 0.7,
        clearcoat_perceptual_roughness: 0.15,
        ..default()
    };
    let assets = MonoAssets {
        board,
        board_mesh,
        slab_mesh: meshes.add(Cuboid::new(BOARD_SIDE, -TABLE_Y, BOARD_SIDE)),
        slab: materials.add(StandardMaterial {
            base_color: Color::srgb(0.1, 0.08, 0.07),
            perceptual_roughness: 0.7,
            ..default()
        }),
        table,
        table_mesh,
        deck_backs,
        card_mesh: meshes.add(Rectangle::new(CARD_W, CARD_D)),
        deck_block: meshes.add(Cuboid::new(CARD_W * 0.97, DECK_H, CARD_D * 0.96)),
        card_edge: materials.add(StandardMaterial {
            base_color: Color::srgb(0.93, 0.91, 0.85),
            perceptual_roughness: 0.85,
            ..default()
        }),
        die_mesh: meshes.add(models::die_mesh(DIE)),
        die,
        pawns: models::pawn_parts(&mut meshes),
        paints,
        house_mesh: meshes.add(models::house_mesh(0.085, 0.062, 0.045, 0.11)),
        hotel_mesh: meshes.add(models::house_mesh(0.2, 0.075, 0.055, 0.12)),
        house: materials.add(gloss(Color::srgb_u8(0x1f, 0xa8, 0x4a))),
        hotel: materials.add(gloss(Color::srgb_u8(0xd8, 0x22, 0x22))),
        marker_mesh: meshes.add(models::rounded_box(Vec3::new((SHORT - 10.0) * UNIT, 0.02, 7.0 * UNIT), 0.008)),
        mortgage_mesh: meshes.add(Rectangle::new(SHORT * UNIT, LONG * UNIT)),
        mortgage,
        unit_cube: meshes.add(Cuboid::new(1.0, 1.0, 1.0)),
        gold_glow: materials.add(StandardMaterial {
            base_color: Color::srgb(1.0, 0.84, 0.3),
            emissive: LinearRgba::rgb(2.2, 1.6, 0.35),
            unlit: true,
            ..default()
        }),
        ring_mesh: meshes.add(Torus::new(0.15, 0.195).mesh().minor_resolution(12).major_resolution(48).build()),
        pointer_mesh: meshes.add(Cone { radius: 0.06, height: 0.13 }.mesh().resolution(20).build()),
        fonts,
    };
    commands.insert_resource(assets);
}

// ---------------------------------------------------------------------------
// Scene
// ---------------------------------------------------------------------------

/// State of the dice on the board.
struct DiceThrow {
    t: f32,
    from: [Vec3; 2],
    to: [Vec3; 2],
    axis: [Vec3; 2],
    spin: [f32; 2],
    end: [Quat; 2],
}

enum CardPhase {
    Rising,
    Showing,
    Returning,
}

struct DrawnCard {
    entity: Entity,
    deck: Deck,
    seq: u32,
    phase: CardPhase,
    t: f32,
    from: Transform,
}

/// Book-keeping of which entity shows what.
#[derive(Resource, Default)]
pub struct MonoScene {
    root: Option<Entity>,
    pawns: HashMap<PlayerId, Entity>,
    paints: HashMap<PlayerId, PlayerPaint>,
    buildings: HashMap<usize, (u8, Vec<Entity>)>,
    markers: HashMap<usize, (PlayerId, Entity)>,
    mortgages: HashMap<usize, Entity>,
    throw: Option<DiceThrow>,
    last_roll_seq: Option<u32>,
    card: Option<DrawnCard>,
    camera_placed: bool,
    highlighted: Option<usize>,
}

pub fn spawn_scene(
    mut commands: Commands,
    assets: Res<MonoAssets>,
    mut ambient: ResMut<AmbientLight>,
    mut camera: Query<&mut Projection, With<MainCamera>>,
) {
    set_fov(&mut camera, FOV);
    let root = commands.spawn((MonoRoot, Transform::default(), Visibility::default())).id();
    let child = |commands: &mut Commands, bundle: (Mesh3d, MeshMaterial3d<StandardMaterial>, Transform)| {
        let e = commands.spawn(bundle).id();
        commands.entity(root).add_child(e);
        e
    };
    // Table, board and its printed top.
    child(&mut commands, (Mesh3d(assets.table_mesh.clone()), MeshMaterial3d(assets.table.clone()), Transform::from_xyz(0.0, TABLE_Y, 0.0)));
    child(&mut commands, (Mesh3d(assets.slab_mesh.clone()), MeshMaterial3d(assets.slab.clone()), Transform::from_xyz(0.0, TABLE_Y / 2.0, 0.0)));
    let top = child(&mut commands, (Mesh3d(assets.board_mesh.clone()), MeshMaterial3d(assets.board.clone()), Transform::from_xyz(0.0, BOARD_TOP + 0.0008, 0.0)));
    commands.entity(top).insert(NotShadowCaster);

    // The decks lie on their dashed places.
    for (deck, spot) in DECK_SPOTS {
        let (pos, yaw) = deck_place(spot);
        child(&mut commands, (Mesh3d(assets.deck_block.clone()), MeshMaterial3d(assets.card_edge.clone()), Transform::from_translation(pos + Vec3::Y * DECK_H / 2.0).with_rotation(Quat::from_rotation_y(yaw))));
        child(&mut commands, (
            Mesh3d(assets.card_mesh.clone()),
            MeshMaterial3d(assets.deck_backs[&deck].clone()),
            Transform::from_translation(pos + Vec3::Y * (DECK_H + 0.001)).with_rotation(Quat::from_rotation_y(yaw) * Quat::from_rotation_x(-FRAC_PI_2)),
        ));
    }

    // Dice, resting until the first roll.
    for i in 0..2 {
        let (pos, rot) = dice_rest(i, 0, if i == 0 { 5 } else { 2 });
        let e = commands
            .spawn((DieEntity(i), Mesh3d(assets.die_mesh.clone()), MeshMaterial3d(assets.die.clone()), Transform::from_translation(pos).with_rotation(rot)))
            .id();
        commands.entity(root).add_child(e);
    }

    // Current player's ring and the pointer over the pawn.
    let ring = commands
        .spawn((TurnRing, Mesh3d(assets.ring_mesh.clone()), MeshMaterial3d(assets.gold_glow.clone()), Transform::default().with_scale(Vec3::new(1.0, 0.2, 1.0)), NotShadowCaster, Visibility::Hidden))
        .id();
    let pointer = commands
        .spawn((TurnPointer, Mesh3d(assets.pointer_mesh.clone()), MeshMaterial3d(assets.gold_glow.clone()), Transform::default(), NotShadowCaster, Visibility::Hidden))
        .id();
    // Outline of the tile under the mouse.
    let highlight = commands
        .spawn((Highlight, Transform::default(), Visibility::Hidden))
        .with_children(|p| {
            for _ in 0..4 {
                p.spawn((Mesh3d(assets.unit_cube.clone()), MeshMaterial3d(assets.gold_glow.clone()), Transform::default(), NotShadowCaster));
            }
        })
        .id();
    commands.entity(root).add_children(&[ring, pointer, highlight]);

    // A warm key light with soft shadows, a cool fill and ambient light.
    let key = commands
        .spawn((
            DirectionalLight {
                illuminance: 6_500.0,
                color: Color::srgb(1.0, 0.95, 0.86),
                shadows_enabled: true,
                ..default()
            },
            CascadeShadowConfigBuilder {
                num_cascades: 1,
                minimum_distance: 0.1,
                maximum_distance: 24.0,
                first_cascade_far_bound: 24.0,
                overlap_proportion: 0.2,
            }
            .build(),
            Transform::from_xyz(-3.5, 10.0, 5.5).looking_at(Vec3::ZERO, Vec3::Y),
        ))
        .id();
    let fill = commands
        .spawn((
            PointLight {
                intensity: 2_500_000.0,
                color: Color::srgb(0.7, 0.8, 1.0),
                range: 40.0,
                ..default()
            },
            Transform::from_xyz(6.0, 7.0, -4.0),
        ))
        .id();
    commands.entity(root).add_children(&[key, fill]);
    ambient.brightness = 420.0;
    ambient.color = Color::srgb(1.0, 0.95, 0.9);

    commands.insert_resource(MonoScene {
        root: Some(root),
        ..default()
    });
}

pub fn despawn_scene(
    mut commands: Commands,
    roots: Query<Entity, With<MonoRoot>>,
    mut ambient: ResMut<AmbientLight>,
    mut camera: Query<&mut Projection, With<MainCamera>>,
) {
    for e in &roots {
        commands.entity(e).despawn();
    }
    set_fov(&mut camera, crate::CAMERA_FOV);
    *ambient = AmbientLight::default();
    commands.remove_resource::<MonoScene>();
}

fn set_fov(camera: &mut Query<&mut Projection, With<MainCamera>>, fov: f32) {
    if let Ok(mut projection) = camera.single_mut() {
        if let Projection::Perspective(p) = projection.as_mut() {
            p.fov = fov;
        }
    }
}

/// Centre of a deck on the table and its yaw.
fn deck_place(spot: Vec2) -> (Vec3, f32) {
    (to_world(spot, BOARD_TOP), -DECK_ANGLE)
}

/// Resting place of a die after a roll.
fn dice_rest(i: usize, seq: u32, value: u8) -> (Vec3, Quat) {
    let seed = seq as u64 * 31 + i as u64 * 7;
    let side = if i == 0 { -1.0 } else { 1.0 };
    let p = DICE_SPOT + Vec2::new(side * 36.0 + (hash(seed) - 0.5) * 22.0, (hash(seed + 1) - 0.5) * 30.0);
    let yaw = (hash(seed + 2) - 0.5) * 1.4;
    (to_world(p, BOARD_TOP + DIE / 2.0), Quat::from_rotation_y(yaw) * die_rotation(value))
}

// ---------------------------------------------------------------------------
// Camera
// ---------------------------------------------------------------------------

/// Camera pose that shows the whole board as large as possible inside
/// `free` (window coordinates, logical pixels), centred in it.
fn frame_board(window: Vec2, free: Rect, fov: f32) -> Transform {
    let margin = 8.0 + 0.02 * free.size().min_element();
    let goal = Rect::from_corners(free.min + margin, free.max - margin);
    let goal_size = goal.size().max(Vec2::splat(64.0));
    let rot = Transform::from_translation(Vec3::new(0.0, PITCH.sin(), PITCH.cos()))
        .looking_at(Vec3::ZERO, Vec3::Y)
        .rotation;
    let (right, up, back) = (rot * Vec3::X, rot * Vec3::Y, rot * Vec3::Z);
    let tan = (fov / 2.0).tan();
    let aspect = window.x / window.y;
    let h = BOARD_SIDE / 2.0;
    let corners = [Vec3::new(-h, 0.0, -h), Vec3::new(h, 0.0, -h), Vec3::new(h, 0.0, h), Vec3::new(-h, 0.0, h)];
    let inv = rot.inverse();
    let bounds = |pos: Vec3| {
        let mut lo = Vec2::splat(f32::MAX);
        let mut hi = Vec2::splat(f32::MIN);
        for c in corners {
            let v = inv * (c - pos);
            let depth = (-v.z).max(0.01);
            let ndc = Vec2::new(v.x / (depth * tan * aspect), v.y / (depth * tan));
            let px = Vec2::new((ndc.x * 0.5 + 0.5) * window.x, (0.5 - ndc.y * 0.5) * window.y);
            lo = lo.min(px);
            hi = hi.max(px);
        }
        (lo, hi)
    };
    let mut focus = Vec3::ZERO;
    let mut dist = 12.0;
    for _ in 0..40 {
        let (lo, hi) = bounds(focus + back * dist);
        let size = hi - lo;
        dist *= (size.x / goal_size.x).max(size.y / goal_size.y);
        let (lo, hi) = bounds(focus + back * dist);
        let off = goal.center() - (lo + hi) / 2.0;
        let world_per_px = 2.0 * dist * tan / window.y;
        focus += -right * off.x * world_per_px + up * off.y * world_per_px;
    }
    Transform::from_translation(focus + back * dist).with_rotation(rot)
}

pub fn fit_camera(
    windows: Query<&Window, With<PrimaryWindow>>,
    view: Res<BoardView>,
    mut camera: Query<(&mut Transform, &Projection), With<MainCamera>>,
    mut scene: ResMut<MonoScene>,
    time: Res<Time>,
) {
    let Ok(window) = windows.single() else { return };
    let size = Vec2::new(window.width(), window.height());
    if size.min_element() < 32.0 {
        return;
    }
    let free = view.free.unwrap_or_else(|| Rect::from_corners(Vec2::ZERO, Vec2::new((size.x - 422.0).max(size.x * 0.5), size.y)));
    let Ok((mut t, projection)) = camera.single_mut() else { return };
    let fov = match projection {
        Projection::Perspective(p) => p.fov,
        _ => 42f32.to_radians(),
    };
    let goal = frame_board(size, free, fov);
    if !scene.camera_placed {
        *t = goal;
        scene.camera_placed = true;
    } else {
        let k = 1.0 - (-10.0 * time.delta_secs()).exp();
        t.translation = t.translation.lerp(goal.translation, k);
        t.rotation = t.rotation.slerp(goal.rotation, k);
    }
}

// ---------------------------------------------------------------------------
// Pawns
// ---------------------------------------------------------------------------

fn player_paint(materials: &mut Assets<StandardMaterial>, rgb: [u8; 3]) -> PlayerPaint {
    let c = srgb(rgb);
    let dark = srgb(rgb.map(|v| (v as f32 * 0.55) as u8));
    PlayerPaint {
        body: materials.add(StandardMaterial {
            base_color: c,
            perceptual_roughness: 0.24,
            reflectance: 0.5,
            clearcoat: 0.8,
            clearcoat_perceptual_roughness: 0.12,
            ..default()
        }),
        stand: materials.add(StandardMaterial {
            base_color: dark,
            perceptual_roughness: 0.35,
            reflectance: 0.45,
            ..default()
        }),
        marker: materials.add(StandardMaterial {
            base_color: c,
            emissive: c.to_linear() * 0.35,
            perceptual_roughness: 0.3,
            ..default()
        }),
    }
}

fn spawn_pawn(commands: &mut Commands, assets: &MonoAssets, paint: &PlayerPaint, token: usize, at: Transform) -> Entity {
    let parts = &assets.pawns[token.min(assets.pawns.len() - 1)];
    commands
        .spawn((Pawn { yaw: 0.0, scale: 0.0 }, at, Visibility::default()))
        .with_children(|p| {
            for part in parts {
                let material = match part.paint {
                    Paint::Body => paint.body.clone(),
                    Paint::Stand => paint.stand.clone(),
                    other => assets.paints[&other].clone(),
                };
                p.spawn((Mesh3d(part.mesh.clone()), MeshMaterial3d(material), part.transform));
            }
        })
        .id()
}

fn shortest_angle(from: f32, to: f32) -> f32 {
    let mut d = (to - from) % (2.0 * PI);
    if d > PI {
        d -= 2.0 * PI;
    } else if d < -PI {
        d += 2.0 * PI;
    }
    d
}

fn smoothstep(t: f32) -> f32 {
    let t = t.clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// Spawns, places and animates the pawns, and moves the turn marker.
#[allow(clippy::too_many_arguments, clippy::type_complexity)]
pub fn sync_pawns(
    mut commands: Commands,
    session: Res<Session>,
    ui: Res<MonoUi>,
    assets: Res<MonoAssets>,
    mut scene: ResMut<MonoScene>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut pawns: Query<(&mut Transform, &mut Pawn)>,
    mut ring: Query<(&mut Transform, &mut Visibility), (With<TurnRing>, Without<Pawn>, Without<TurnPointer>)>,
    mut pointer: Query<(&mut Transform, &mut Visibility), (With<TurnPointer>, Without<Pawn>, Without<TurnRing>)>,
    time: Res<Time>,
) {
    let Some(state) = session.monopoly.as_ref() else { return };
    let Some(root) = scene.root else { return };
    let dt = time.delta_secs();
    let follow = 1.0 - (-12.0 * dt).exp();

    // Where every standing pawn belongs: pawns on one tile share it.
    let mut groups: HashMap<(usize, bool), Vec<PlayerId>> = HashMap::new();
    for p in state.players.iter().filter(|p| !p.bankrupt) {
        if ui.moving == Some(p.id) {
            continue;
        }
        let shown = ui.pawn_pos.get(&p.id).copied().unwrap_or(p.position as f32).round() as usize % BOARD_SIZE;
        let jailed = p.in_jail && shown == JAIL_INDEX;
        groups.entry((shown, jailed)).or_default().push(p.id);
    }
    let slot_world = |tile: usize, k: usize, n: usize, jailed: bool| {
        let (p, s) = pawn_slot(tile, k, n, jailed);
        (to_world(p, BOARD_TOP + STAND_H * PAWN_SCALE * s), s)
    };
    let standing_yaw = |tile: usize, jailed: bool| {
        if jailed {
            yaw_of(Vec2::Y)
        } else {
            yaw_of(travel_dir(tile))
        }
    };

    for p in &state.players {
        if p.bankrupt {
            if let Some(e) = scene.pawns.remove(&p.id) {
                let target = pawns.get(e).map(|(t, _)| *t).unwrap_or_default();
                commands.entity(e).insert(Piece { target, grow: 1.0, vanish: true }).remove::<Pawn>();
            }
            continue;
        }
        scene.paints.entry(p.id).or_insert_with(|| player_paint(&mut materials, p.rgb()));
        let e = match scene.pawns.get(&p.id) {
            Some(e) => *e,
            None => {
                let e = spawn_pawn(&mut commands, &assets, &scene.paints[&p.id], p.token, Transform::from_scale(Vec3::ZERO));
                commands.entity(root).add_child(e);
                scene.pawns.insert(p.id, e);
                continue; // placed from the next frame on
            }
        };
        let Ok((mut t, mut pawn)) = pawns.get_mut(e) else { continue };
        let moving = ui.moving == Some(p.id);
        let (pos, yaw, scale, snap) = if moving && ui.dice_anim <= 0.0 && !ui.path.is_empty() {
            // A hop from the tile it stands on to the next one.
            let from_tile = ui.pawn_pos.get(&p.id).copied().unwrap_or(0.0).round() as usize % BOARD_SIZE;
            let to_tile = ui.path[0];
            let jail_end = ui.path.len() == 1 && to_tile == JAIL_INDEX && p.in_jail;
            let (a, _) = slot_world(from_tile, 0, 1, false);
            let (b, _) = slot_world(to_tile, 0, 1, jail_end);
            let k = smoothstep(ui.hop);
            let dist = a.distance(b);
            let arc = (0.16 + dist * 0.14) * (PI * ui.hop.clamp(0.0, 1.0)).sin();
            let dir = Vec2::new(b.x - a.x, b.z - a.z);
            let yaw = if dir.length() > 1e-4 { yaw_of(dir) } else { pawn.yaw };
            (a.lerp(b, k) + Vec3::Y * arc, yaw, 1.0, true)
        } else if moving {
            let tile = ui.pawn_pos.get(&p.id).copied().unwrap_or(0.0).round() as usize % BOARD_SIZE;
            let (pos, s) = slot_world(tile, 0, 1, false);
            (pos, standing_yaw(tile, false), s, false)
        } else {
            let shown = ui.pawn_pos.get(&p.id).copied().unwrap_or(p.position as f32).round() as usize % BOARD_SIZE;
            let jailed = p.in_jail && shown == JAIL_INDEX;
            let group = groups.get(&(shown, jailed)).map(Vec::as_slice).unwrap_or(&[]);
            let k = group.iter().position(|id| *id == p.id).unwrap_or(0);
            let (pos, s) = slot_world(shown, k, group.len().max(1), jailed);
            (pos, standing_yaw(shown, jailed), s, false)
        };
        // The flat star always shows its face to the camera.
        let yaw = if TOKENS.get(p.token).is_some_and(|t| t.id == "star") { 0.0 } else { yaw };
        let first = pawn.scale == 0.0;
        if first {
            // Just spawned: appear in place.
            t.translation = pos;
            pawn.yaw = yaw;
        }
        if snap {
            t.translation = pos;
            pawn.yaw += shortest_angle(pawn.yaw, yaw) * (1.0 - (-18.0 * dt).exp());
        } else {
            t.translation = t.translation.lerp(pos, follow);
            pawn.yaw += shortest_angle(pawn.yaw, yaw) * follow;
        }
        pawn.scale += (scale - pawn.scale) * if first { 0.15 } else { follow };
        t.rotation = Quat::from_rotation_y(pawn.yaw);
        t.scale = Vec3::splat(PAWN_SCALE * pawn.scale);
    }

    // The ring under the current pawn and a bobbing pointer above it.
    let now = time.elapsed_secs();
    let current = state.current_player().filter(|p| !p.bankrupt && state.phase == Phase::Playing);
    let pawn_t = current.and_then(|p| scene.pawns.get(&p.id)).and_then(|e| pawns.get(*e).ok()).map(|(t, _)| *t);
    if let (Ok((mut rt, mut rv)), Ok((mut pt, mut pv))) = (ring.single_mut(), pointer.single_mut()) {
        match pawn_t {
            Some(t) => {
                let s = t.scale.x / PAWN_SCALE;
                let pulse = 1.0 + (now * 4.0).sin() * 0.06;
                rt.translation = Vec3::new(t.translation.x, BOARD_TOP + 0.004, t.translation.z);
                rt.scale = Vec3::new(s * pulse, 0.25, s * pulse);
                pt.translation = t.translation + Vec3::Y * (PAWN_SCALE * s * 1.0 + 0.13 + (now * 3.0).sin() * 0.025);
                // Points down the screen, so it reads as an arrow from above.
                let down = Vec3::new(0.0, -PITCH.cos(), PITCH.sin());
                pt.rotation = Quat::from_rotation_arc(Vec3::Y, down) * Quat::from_rotation_y(now * 2.0);
                *rv = Visibility::Inherited;
                *pv = Visibility::Inherited;
            }
            None => {
                *rv = Visibility::Hidden;
                *pv = Visibility::Hidden;
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Property: owners, houses, mortgages
// ---------------------------------------------------------------------------

fn marker_transform(i: usize) -> Transform {
    let (r, side) = tile_rect(i);
    let c = r.center() + outward(side) * (LONG / 2.0 - 6.0);
    Transform::from_translation(to_world(c, BOARD_TOP + 0.012)).with_rotation(Quat::from_rotation_y(side_yaw(side)))
}

/// Where the houses of a street stand on its colour stripe.
fn building_transforms(i: usize, level: u8) -> Vec<Transform> {
    let (r, side) = tile_rect(i);
    let Some(stripe) = stripe_rect(r, side) else { return Vec::new() };
    let along = travel_dir(i);
    let inward = -outward(side);
    let yaw = Quat::from_rotation_y(yaw_of(inward));
    if level == HOTEL_LEVEL {
        return vec![Transform::from_translation(to_world(stripe.center(), BOARD_TOP)).with_rotation(yaw)];
    }
    (0..level)
        .map(|k| {
            let off = (k as f32 - (level as f32 - 1.0) / 2.0) * 20.0;
            Transform::from_translation(to_world(stripe.center() + along * off, BOARD_TOP)).with_rotation(yaw)
        })
        .collect()
}

fn mortgage_transform(i: usize) -> Transform {
    let (r, side) = tile_rect(i);
    Transform::from_translation(to_world(r.center(), BOARD_TOP + 0.004))
        .with_rotation(Quat::from_rotation_y(side_yaw(side)) * Quat::from_rotation_x(-FRAC_PI_2))
}

fn vanish(commands: &mut Commands, e: Entity) {
    commands.queue(move |world: &mut World| {
        if let Some(mut piece) = world.get_mut::<Piece>(e) {
            piece.vanish = true;
        }
    });
}

#[allow(clippy::type_complexity)]
pub fn sync_property(
    mut commands: Commands,
    session: Res<Session>,
    assets: Res<MonoAssets>,
    mut scene: ResMut<MonoScene>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut pieces: Query<&mut Piece>,
) {
    let Some(state) = session.monopoly.as_ref() else { return };
    let Some(root) = scene.root else { return };
    let spawn_piece = |commands: &mut Commands, mesh: &Handle<Mesh>, material: Handle<StandardMaterial>, target: Transform, shadow: bool| {
        let mut e = commands.spawn((
            Piece::new(target),
            Mesh3d(mesh.clone()),
            MeshMaterial3d(material),
            target.with_scale(Vec3::ZERO),
        ));
        if !shadow {
            e.insert(NotShadowCaster);
        }
        let e = e.id();
        commands.entity(root).add_child(e);
        e
    };

    for i in 0..BOARD_SIZE {
        if !tile(i).is_purchasable() {
            continue;
        }
        // Owner's bar along the outer edge of the tile.
        let owner = state.owner_of(i).map(|p| (p.id, p.rgb()));
        let shown = scene.markers.get(&i).map(|(id, _)| *id);
        if owner.map(|o| o.0) != shown {
            if let Some((_, e)) = scene.markers.remove(&i) {
                vanish(&mut commands, e);
            }
            if let Some((id, rgb)) = owner {
                scene.paints.entry(id).or_insert_with(|| player_paint(&mut materials, rgb));
                let material = scene.paints[&id].marker.clone();
                let e = spawn_piece(&mut commands, &assets.marker_mesh, material, marker_transform(i), false);
                scene.markers.insert(i, (id, e));
            }
        }

        // Mortgage veil.
        let mortgaged = state.is_mortgaged(i);
        match (mortgaged, scene.mortgages.get(&i).copied()) {
            (true, None) => {
                let e = spawn_piece(&mut commands, &assets.mortgage_mesh, assets.mortgage.clone(), mortgage_transform(i), false);
                scene.mortgages.insert(i, e);
            }
            (false, Some(e)) => {
                vanish(&mut commands, e);
                scene.mortgages.remove(&i);
            }
            _ => {}
        }

        // Houses and hotels.
        let level = state.building_level(i);
        let (shown_level, entities) = scene.buildings.remove(&i).unwrap_or((0, Vec::new()));
        if level == shown_level {
            scene.buildings.insert(i, (shown_level, entities));
            continue;
        }
        let places = building_transforms(i, level);
        let hotel_change = (level == HOTEL_LEVEL) != (shown_level == HOTEL_LEVEL);
        let mut kept = Vec::new();
        for (k, e) in entities.into_iter().enumerate() {
            // Houses slide over to their new places; a hotel replaces them all.
            if !hotel_change && k < places.len() {
                if let Ok(mut piece) = pieces.get_mut(e) {
                    piece.target = places[k];
                }
                kept.push(e);
            } else {
                vanish(&mut commands, e);
            }
        }
        for place in places.iter().skip(kept.len()) {
            let (mesh, material) = if level == HOTEL_LEVEL {
                (&assets.hotel_mesh, assets.hotel.clone())
            } else {
                (&assets.house_mesh, assets.house.clone())
            };
            kept.push(spawn_piece(&mut commands, mesh, material, *place, true));
        }
        scene.buildings.insert(i, (level, kept));
    }
}

/// Glides pieces to their places and pops them in and out.
pub fn animate_pieces(mut commands: Commands, time: Res<Time>, mut q: Query<(Entity, &mut Transform, &mut Piece)>) {
    let dt = time.delta_secs();
    let follow = 1.0 - (-12.0 * dt).exp();
    for (e, mut t, mut piece) in &mut q {
        if piece.vanish {
            piece.grow -= dt / 0.25;
            if piece.grow <= 0.0 {
                commands.entity(e).despawn();
                continue;
            }
            t.scale = piece.target.scale * piece.grow.max(0.0);
            continue;
        }
        piece.grow = (piece.grow + dt / 0.35).min(1.0);
        // Ease out with a little overshoot.
        let x = piece.grow - 1.0;
        let pop = 1.0 + 2.7 * x * x * x + 1.7 * x * x;
        t.translation = t.translation.lerp(piece.target.translation, follow);
        t.rotation = t.rotation.slerp(piece.target.rotation, follow);
        t.scale = piece.target.scale * pop;
    }
}

// ---------------------------------------------------------------------------
// Dice
// ---------------------------------------------------------------------------

#[allow(clippy::type_complexity)]
pub fn animate_dice(
    session: Res<Session>,
    mut scene: ResMut<MonoScene>,
    mut dice: Query<(&DieEntity, &mut Transform)>,
    time: Res<Time>,
) {
    let Some(state) = session.monopoly.as_ref() else { return };
    let seq = state.roll_seq;
    let roll = state.last_roll.map(|r| [r.a, r.b]);
    match scene.last_roll_seq {
        None => {
            // Entering a game in progress: the dice show the last roll.
            scene.last_roll_seq = Some(seq);
            if let Some(values) = roll {
                for (d, mut t) in &mut dice {
                    let (pos, rot) = dice_rest(d.0, seq, values[d.0]);
                    *t = Transform::from_translation(pos).with_rotation(rot);
                }
            }
        }
        Some(last) if last != seq => {
            scene.last_roll_seq = Some(seq);
            if let Some(values) = roll {
                let mut throw = DiceThrow {
                    t: 0.0,
                    from: [Vec3::ZERO; 2],
                    to: [Vec3::ZERO; 2],
                    axis: [Vec3::X; 2],
                    spin: [0.0; 2],
                    end: [Quat::IDENTITY; 2],
                };
                for (i, value) in values.into_iter().enumerate() {
                    let seed = seq as u64 * 977 + i as u64 * 13;
                    let (to, end) = dice_rest(i, seq, value);
                    throw.to[i] = to;
                    throw.end[i] = end;
                    // Thrown from the near edge of the board, high above it.
                    throw.from[i] = to + Vec3::new((hash(seed) - 0.5) * 1.2, 1.3 + hash(seed + 1) * 0.4, 2.4 + hash(seed + 2) * 0.5);
                    throw.axis[i] = Vec3::new(hash(seed + 3) - 0.5, hash(seed + 4) - 0.5, hash(seed + 5) - 0.5).normalize_or(Vec3::X);
                    throw.spin[i] = 9.0 + hash(seed + 6) * 6.0;
                }
                scene.throw = Some(throw);
            }
        }
        _ => {}
    }
    let Some(throw) = &mut scene.throw else { return };
    throw.t = (throw.t + time.delta_secs() / DICE_ROLL_SECS).min(1.0);
    let t = throw.t;
    let glide = 1.0 - (1.0 - t).powf(2.2);
    let settle = 1.0 - (1.0 - t).powi(3);
    let bounce = (2.5 * PI * t).cos().abs() * (1.0 - t).powi(2);
    for (d, mut tr) in &mut dice {
        let i = d.0.min(1);
        let mut p = throw.from[i].lerp(throw.to[i], glide);
        p.y = throw.to[i].y + (throw.from[i].y - throw.to[i].y) * bounce;
        tr.translation = p;
        tr.rotation = throw.end[i] * Quat::from_axis_angle(throw.axis[i], throw.spin[i] * (1.0 - settle));
    }
    if throw.t >= 1.0 {
        scene.throw = None;
    }
}

// ---------------------------------------------------------------------------
// Drawn cards
// ---------------------------------------------------------------------------

fn deck_top(deck: Deck) -> Transform {
    let spot = DECK_SPOTS.iter().find(|(d, _)| *d == deck).map(|(_, s)| *s).unwrap_or(Vec2::splat(VIEW / 2.0));
    let (pos, yaw) = deck_place(spot);
    // Face down: the front looks at the table.
    // Face down with the back the same way round as the deck's top card.
    Transform::from_translation(pos + Vec3::Y * (DECK_H + 0.006))
        .with_rotation(Quat::from_rotation_y(yaw) * Quat::from_rotation_x(-FRAC_PI_2) * Quat::from_rotation_y(PI))
}

/// The card held up in front of the camera, over the middle of the board.
fn card_show(camera: &Transform, fov: f32) -> Transform {
    // A little above the middle of the board, leaving the dice in view.
    let centre = to_world(Vec2::new(VIEW / 2.0 + 30.0, VIEW / 2.0 - 70.0), BOARD_TOP);
    let dir = (centre - camera.translation).normalize_or(camera.forward().as_vec3());
    let dist = 2.4;
    let scale = 0.34 * 2.0 * dist * (fov / 2.0).tan() / CARD_D;
    Transform::from_translation(camera.translation + dir * dist)
        .with_rotation(camera.rotation)
        .with_scale(Vec3::splat(scale))
}

fn flight(from: &Transform, to: &Transform, t: f32, lift: f32) -> Transform {
    let k = smoothstep(t);
    Transform {
        translation: from.translation.lerp(to.translation, k) + Vec3::Y * lift * (PI * k).sin(),
        rotation: from.rotation.slerp(to.rotation, k),
        scale: from.scale.lerp(to.scale, k),
    }
}

#[allow(clippy::too_many_arguments, clippy::type_complexity)]
pub fn animate_card(
    mut commands: Commands,
    ui: Res<MonoUi>,
    assets: Res<MonoAssets>,
    mut scene: ResMut<MonoScene>,
    mut images: ResMut<Assets<Image>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    camera: Query<(&Transform, &Projection), (With<MainCamera>, Without<CardMarker>)>,
    mut cards: Query<&mut Transform, With<CardMarker>>,
    time: Res<Time>,
) {
    let Some(root) = scene.root else { return };
    let Ok((cam, projection)) = camera.single() else { return };
    let fov = match projection {
        Projection::Perspective(p) => p.fov,
        _ => 42f32.to_radians(),
    };
    let show = card_show(cam, fov);
    let wanted = ui.card_popup.as_ref().map(|c| c.seq);

    // A new card: the previous one goes back, the new one rises.
    if let Some(popup) = &ui.card_popup {
        if scene.card.as_ref().map(|c| c.seq) != Some(popup.seq) {
            if let Some(old) = scene.card.take() {
                commands.entity(old.entity).despawn();
            }
            let face = art::paint_card_face(&assets.fonts, popup.deck, &popup.text, &popup.who);
            let material = materials.add(StandardMaterial {
                base_color_texture: Some(images.add(to_image(&face))),
                alpha_mode: AlphaMode::Mask(0.5),
                // Held up to the camera like a dialog: always crisp and
                // readable. Brighter than white, or the tonemapper greys it.
                base_color: Color::linear_rgb(1.45, 1.45, 1.45),
                unlit: true,
                ..default()
            });
            let from = deck_top(popup.deck);
            let entity = commands
                .spawn((CardMarker, from, Visibility::default(), NotShadowCaster))
                .with_children(|p| {
                    p.spawn((Mesh3d(assets.card_mesh.clone()), MeshMaterial3d(material), Transform::default(), NotShadowCaster));
                    p.spawn((
                        Mesh3d(assets.card_mesh.clone()),
                        MeshMaterial3d(assets.deck_backs[&popup.deck].clone()),
                        Transform::from_rotation(Quat::from_rotation_y(PI)),
                        NotShadowCaster,
                    ));
                })
                .id();
            commands.entity(root).add_child(entity);
            scene.card = Some(DrawnCard {
                entity,
                deck: popup.deck,
                seq: popup.seq,
                phase: CardPhase::Rising,
                t: 0.0,
                from,
            });
            return;
        }
    }

    let Some(card) = &mut scene.card else { return };
    let Ok(mut t) = cards.get_mut(card.entity) else { return };
    let dt = time.delta_secs();
    if wanted != Some(card.seq) && !matches!(card.phase, CardPhase::Returning) {
        card.phase = CardPhase::Returning;
        card.from = *t;
        card.t = 0.0;
    }
    match card.phase {
        CardPhase::Rising => {
            card.t = (card.t + dt / 0.6).min(1.0);
            *t = flight(&card.from, &show, card.t, 0.5);
            if card.t >= 1.0 {
                card.phase = CardPhase::Showing;
            }
        }
        CardPhase::Showing => *t = show,
        CardPhase::Returning => {
            card.t = (card.t + dt / 0.5).min(1.0);
            *t = flight(&card.from, &deck_top(card.deck), card.t, 0.3);
            if card.t >= 1.0 {
                commands.entity(card.entity).despawn();
                scene.card = None;
            }
        }
    }
}

#[derive(Component)]
pub struct CardMarker;

// ---------------------------------------------------------------------------
// Hover
// ---------------------------------------------------------------------------

/// Outlines the tile under the mouse.
pub fn highlight_tile(
    ui: Res<MonoUi>,
    mut scene: ResMut<MonoScene>,
    mut outline: Query<(&mut Visibility, &Children), With<Highlight>>,
    mut bars: Query<&mut Transform, Without<Highlight>>,
) {
    if scene.highlighted == ui.hovered_tile {
        return;
    }
    scene.highlighted = ui.hovered_tile;
    let Ok((mut vis, children)) = outline.single_mut() else { return };
    let Some(i) = ui.hovered_tile else {
        *vis = Visibility::Hidden;
        return;
    };
    *vis = Visibility::Inherited;
    let (r, _) = tile_rect(i);
    let r = VRect { min: r.min + Vec2::splat(1.5), max: r.max - Vec2::splat(1.5) };
    let w = 4.0;
    let y = BOARD_TOP + 0.004;
    let edges = [
        (Vec2::new(r.center().x, r.min.y), Vec2::new(r.size().x + w, w)),
        (Vec2::new(r.center().x, r.max.y), Vec2::new(r.size().x + w, w)),
        (Vec2::new(r.min.x, r.center().y), Vec2::new(w, r.size().y + w)),
        (Vec2::new(r.max.x, r.center().y), Vec2::new(w, r.size().y + w)),
    ];
    for (child, (c, size)) in children.iter().zip(edges) {
        if let Ok(mut t) = bars.get_mut(child) {
            *t = Transform::from_translation(to_world(c, y)).with_scale(Vec3::new(size.x * UNIT, 0.006, size.y * UNIT));
        }
    }
}

/// Tile of the board under a window position (logical pixels).
pub fn pick_tile(camera: &Camera, gt: &GlobalTransform, at: Vec2) -> Option<usize> {
    let ray = camera.viewport_to_world(gt, at).ok()?;
    if ray.direction.y.abs() < 1e-5 {
        return None;
    }
    let t = (BOARD_TOP - ray.origin.y) / ray.direction.y;
    if t <= 0.0 {
        return None;
    }
    let hit = ray.origin + ray.direction * t;
    super::geom::tile_at(super::geom::from_world(hit))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn camera_frames_the_whole_board_in_the_free_area() {
        let window = Vec2::new(1440.0, 900.0);
        let free = Rect::new(0.0, 0.0, 1030.0, 900.0);
        let fov = FOV;
        let cam = frame_board(window, free, fov);
        let tan = (fov / 2.0).tan();
        let h = BOARD_SIDE / 2.0;
        let mut lo = Vec2::splat(f32::MAX);
        let mut hi = Vec2::splat(f32::MIN);
        for c in [Vec3::new(-h, 0.0, -h), Vec3::new(h, 0.0, -h), Vec3::new(h, 0.0, h), Vec3::new(-h, 0.0, h)] {
            let v = cam.rotation.inverse() * (c - cam.translation);
            let ndc = Vec2::new(v.x / (-v.z * tan * window.x / window.y), v.y / (-v.z * tan));
            let px = Vec2::new((ndc.x * 0.5 + 0.5) * window.x, (0.5 - ndc.y * 0.5) * window.y);
            lo = lo.min(px);
            hi = hi.max(px);
        }
        assert!(lo.x >= free.min.x && hi.x <= free.max.x && lo.y >= 0.0 && hi.y <= window.y, "{lo:?} {hi:?}");
        // Fills the free area: touches its margins on at least one axis.
        let fill = ((hi - lo) / free.size()).max_element();
        assert!(fill > 0.9, "{fill}");
        // Centred in the free area.
        assert!(((lo + hi) / 2.0 - free.center()).length() < 3.0);
    }

    #[test]
    fn houses_sit_on_the_stripe() {
        for i in [1usize, 11, 21, 31, 39] {
            let (r, side) = tile_rect(i);
            let stripe = stripe_rect(r, side).unwrap();
            for level in 1..=5u8 {
                for t in building_transforms(i, level) {
                    assert!(stripe.contains(super::super::geom::from_world(t.translation)));
                }
            }
        }
    }

    #[test]
    fn deck_colours_differ() {
        assert_ne!(art::deck_color(Deck::Chance), art::deck_color(Deck::Chest));
    }
}
