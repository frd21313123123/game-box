//! The 3D Blackjack table: the dealer stands across the table, the players'
//! boxes run along the near edge, the shoe sits at the dealer's left and the
//! discard tray at the right.
//!
//! Every card and chip stack is an entity reconciled with the game view each
//! frame: dealt cards fly out of the shoe, the hole card flips over, bets slide
//! in from the players, lost bets go to the dealer's tray and winnings come out
//! of it.

use std::collections::HashMap;
use std::f32::consts::{FRAC_PI_2, PI};

use bevy::pbr::NotShadowCaster;
use bevy::prelude::*;
use blackjack_core::{hand_value, BjEvent, BjGame, Card, Outcome, Phase};

use super::art::{build_art, chip_breakdown, face_index, BjArt, CHIP_H};
use crate::net::Session;
use crate::sfx::Sfx;
use crate::table3d::{enter_room, face_down, face_up, hash, head_look, leave_room, spawn_room, Figure, FigureHead, FigureKit, Motion};
use crate::theme::SEAT_COLORS;
use crate::MainCamera;

const CAMERA_POS: Vec3 = Vec3::new(0.0, 6.1, 6.9);
const CAMERA_LOOK: Vec3 = Vec3::new(0.0, 0.0, 0.25);
const DEALER_POS: Vec3 = Vec3::new(0.0, 0.3, -4.05);
const DEALER_CARDS: Vec3 = Vec3::new(0.0, 0.0, -1.45);
const SHOE_POS: Vec3 = Vec3::new(1.85, 0.0, -1.25);
const DISCARD_POS: Vec3 = Vec3::new(-1.85, 0.0, -1.25);
const TRAY_POS: Vec3 = Vec3::new(0.0, 0.0, -2.35);
/// Radius of the arc the players' boxes sit on.
const SPOT_R: f32 = 2.05;
const CARD_SCALE: f32 = 0.68;
/// Height of one card in the shoe and the discard tray.
const CARD_THICK: f32 = 0.0011;
/// Key of the face-down hole card (real cards are keyed by their id).
const HOLE: u16 = Card::HIDDEN_ID;

#[derive(Component)]
pub struct BjRoot;

#[derive(Component)]
pub struct CardEntity {
    front: Entity,
}

#[derive(Component)]
pub struct ChipStack;

#[derive(Component)]
pub struct ShoeStack {
    discard: bool,
}

#[derive(Component)]
pub struct ShoeTop {
    discard: bool,
}

/// The betting circle of a seat.
#[derive(Component)]
pub struct SpotRing {
    seat: usize,
}

/// Bobbing arrow over the hand being played.
#[derive(Component)]
pub struct HandPointer;

/// Which bet a chip stack shows.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
enum StackKind {
    Bet,
    Winnings,
    Insurance,
    InsuranceWin,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
struct StackKey {
    seat: usize,
    hand: usize,
    kind: StackKind,
}

struct StackEntry {
    entity: Entity,
    amount: u32,
    /// The stack went to the dealer (a lost bet).
    at_dealer: bool,
}

/// Book-keeping of which entity shows which card and bet.
#[derive(Resource, Default)]
pub struct BjTable {
    seats: usize,
    cards: HashMap<u16, Entity>,
    stacks: HashMap<StackKey, StackEntry>,
    last_seq: u64,
    initialized: bool,
}

#[derive(Resource)]
pub struct BjAssets {
    art: BjArt,
    ring_materials: Vec<Handle<StandardMaterial>>,
    pointer_material: Handle<StandardMaterial>,
}

pub fn setup_assets(
    mut commands: Commands,
    mut images: ResMut<Assets<Image>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut meshes: ResMut<Assets<Mesh>>,
) {
    let art = build_art(&mut images, &mut materials, &mut meshes);
    let ring_materials = SEAT_COLORS
        .iter()
        .map(|c| {
            materials.add(StandardMaterial {
                base_color: Color::srgba_u8(c[0], c[1], c[2], 150),
                emissive: LinearRgba::BLACK,
                alpha_mode: AlphaMode::Blend,
                unlit: true,
                ..default()
            })
        })
        .collect();
    let pointer_material = materials.add(StandardMaterial {
        base_color: Color::srgb(1.0, 0.85, 0.3),
        emissive: LinearRgba::rgb(2.0, 1.5, 0.4),
        ..default()
    });
    commands.insert_resource(BjAssets {
        art,
        ring_materials,
        pointer_material,
    });
}

// ----- layout -------------------------------------------------------------------

/// Angle of a seat's box on the arc: seat 0 sits at the dealer's left (the
/// right side of the screen) and play goes right to left, like in a casino.
fn spot_angle(seat: usize, n: usize) -> f32 {
    if n <= 1 {
        return 0.0;
    }
    let span = (26.0f32 * (n - 1) as f32).min(132.0).to_radians();
    span / 2.0 - span * seat as f32 / (n - 1) as f32
}

/// Outward direction of a seat (from the table centre towards the player).
fn spot_dir(seat: usize, n: usize) -> Vec3 {
    let a = spot_angle(seat, n);
    Vec3::new(a.sin(), 0.0, a.cos())
}

/// Right-hand direction for a player looking at the dealer.
fn spot_tangent(seat: usize, n: usize) -> Vec3 {
    let a = spot_angle(seat, n);
    Vec3::new(a.cos(), 0.0, -a.sin())
}

/// Centre of a seat's betting circle on the felt.
pub fn spot_pos(seat: usize, n: usize) -> Vec3 {
    spot_dir(seat, n) * SPOT_R
}

/// Where the cards of hand `hand` (of `hands`) start, towards the dealer.
fn hand_base(seat: usize, n: usize, hand: usize, hands: usize) -> Vec3 {
    let lateral = (hand as f32 - (hands.max(1) - 1) as f32 / 2.0) * 0.52;
    spot_pos(seat, n) - spot_dir(seat, n) * 0.62 + spot_tangent(seat, n) * lateral
}

/// A point just above a hand's cards, for the HUD label with its total.
pub fn hand_label_pos(seat: usize, n: usize, hand: usize, hands: usize, cards: usize) -> Vec3 {
    hand_base(seat, n, hand, hands) - spot_dir(seat, n) * (0.3 + cards.saturating_sub(1) as f32 * 0.07) + Vec3::Y * 0.05
}

/// Where the seat's name plate hangs (near the table edge, in front of the bet).
pub fn plate_pos(seat: usize, n: usize) -> Vec3 {
    spot_pos(seat, n) + spot_dir(seat, n) * 0.45
}

/// Above the dealer's cards, for the dealer's total.
pub fn dealer_label_pos() -> Vec3 {
    DEALER_CARDS - Vec3::Z * 0.55 + Vec3::Y * 0.05
}

fn card_slot(seat: usize, n: usize, hand: usize, hands: usize, k: usize) -> Transform {
    let dir = spot_dir(seat, n);
    let tangent = spot_tangent(seat, n);
    let pos = hand_base(seat, n, hand, hands) + tangent * (k as f32 * 0.1) - dir * (k as f32 * 0.075)
        + Vec3::Y * (0.004 + k as f32 * 0.0025);
    Transform::from_translation(pos)
        .with_rotation(face_up(spot_angle(seat, n)))
        .with_scale(Vec3::splat(CARD_SCALE))
}

fn dealer_slot(k: usize, count: usize, hidden: bool) -> Transform {
    let x = (k as f32 - (count.max(2) - 1) as f32 / 2.0) * 0.36;
    let pos = DEALER_CARDS + Vec3::new(x, 0.004 + k as f32 * 0.0025, 0.0);
    let rot = if hidden { face_down(0.0) } else { face_up(0.0) };
    Transform::from_translation(pos).with_rotation(rot).with_scale(Vec3::splat(CARD_SCALE))
}

fn shoe_top(count: usize) -> Transform {
    Transform::from_translation(SHOE_POS + Vec3::Y * (count as f32 * CARD_THICK + 0.01))
        .with_rotation(face_down(-0.25))
        .with_scale(Vec3::splat(CARD_SCALE))
}

fn discard_top(count: usize) -> Transform {
    Transform::from_translation(DISCARD_POS + Vec3::Y * (count as f32 * CARD_THICK + 0.01))
        .with_rotation(face_down(0.25))
        .with_scale(Vec3::splat(CARD_SCALE))
}

fn stack_slot(key: StackKey, n: usize, hands: usize) -> Transform {
    let dir = spot_dir(key.seat, n);
    let tangent = spot_tangent(key.seat, n);
    let base = spot_pos(key.seat, n);
    let lateral = (key.hand as f32 - (hands.max(1) - 1) as f32 / 2.0) * 0.52;
    let pos = match key.kind {
        StackKind::Bet => base + tangent * lateral,
        StackKind::Winnings => base + tangent * (lateral + 0.2) + dir * 0.06,
        StackKind::Insurance => base - dir * 0.28 - tangent * 0.42,
        StackKind::InsuranceWin => base - dir * 0.28 - tangent * 0.22,
    };
    Transform::from_translation(pos)
}

/// A spot in the dealer's chip tray for a stack coming from or going to it.
fn tray_slot(key: StackKey) -> Transform {
    let k = key.seat * 5 + key.hand * 2 + key.kind as usize;
    let x = (hash(k as u64) - 0.5) * 1.2;
    let z = (hash(k as u64 + 99) - 0.5) * 0.2;
    Transform::from_translation(TRAY_POS + Vec3::new(x, 0.0, z))
}

/// Off the table edge on the player's side (bets come from and go back there).
fn player_side(seat: usize, n: usize) -> Transform {
    Transform::from_translation(spot_pos(seat, n) + spot_dir(seat, n) * 1.3 + Vec3::Y * 0.1)
}

// ----- spawning ------------------------------------------------------------------

pub fn spawn_table(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    assets: Res<BjAssets>,
    session: Res<Session>,
    mut camera: Query<(Entity, &mut Transform), With<MainCamera>>,
    mut ambient: ResMut<AmbientLight>,
) {
    commands.insert_resource(BjTable::default());
    if let Ok((cam, mut t)) = camera.single_mut() {
        enter_room(&mut commands, (cam, &mut t), CAMERA_POS, CAMERA_LOOK, &mut ambient);
    }
    let root = commands.spawn((BjRoot, Transform::default(), Visibility::default())).id();
    // Casino blue-green felt.
    spawn_room(&mut commands, &mut meshes, &mut materials, root, Color::srgb(0.05, 0.3, 0.27));

    let child = |commands: &mut Commands, bundle: (Mesh3d, MeshMaterial3d<StandardMaterial>, Transform)| {
        let e = commands.spawn((bundle, NotShadowCaster)).id();
        commands.entity(root).add_child(e);
        e
    };

    // Printed felt: the insurance line arc.
    let gold = materials.add(StandardMaterial {
        base_color: Color::srgba(0.95, 0.78, 0.35, 0.75),
        alpha_mode: AlphaMode::Blend,
        unlit: true,
        ..default()
    });
    child(&mut commands, (
        Mesh3d(meshes.add(Annulus::new(1.38, 1.41))),
        MeshMaterial3d(gold.clone()),
        Transform::from_xyz(0.0, 0.002, 0.0).with_rotation(Quat::from_rotation_x(-FRAC_PI_2)),
    ));

    // The shoe and the discard tray: blocks of card edges with a back on top.
    let edge = materials.add(StandardMaterial {
        base_color: Color::srgb(0.93, 0.91, 0.86),
        perceptual_roughness: 0.8,
        ..default()
    });
    let holder = materials.add(StandardMaterial {
        base_color: Color::srgb(0.12, 0.09, 0.07),
        perceptual_roughness: 0.4,
        reflectance: 0.5,
        ..default()
    });
    let card_box = meshes.add(Cuboid::new(super::art::CARD_W * CARD_SCALE * 0.98, 1.0, super::art::CARD_H * CARD_SCALE * 0.98));
    for (pos, yaw, discard) in [(SHOE_POS, -0.25f32, false), (DISCARD_POS, 0.25, true)] {
        let base = commands
            .spawn((
                Mesh3d(meshes.add(Cuboid::new(0.5, 0.03, 0.7))),
                MeshMaterial3d(holder.clone()),
                Transform::from_translation(pos + Vec3::Y * 0.0).with_rotation(Quat::from_rotation_y(yaw)),
            ))
            .id();
        commands.entity(root).add_child(base);
        let block = commands
            .spawn((
                ShoeStack { discard },
                Mesh3d(card_box.clone()),
                MeshMaterial3d(edge.clone()),
                Transform::from_translation(pos).with_rotation(Quat::from_rotation_y(yaw)),
            ))
            .id();
        commands.entity(root).add_child(block);
        let top = commands
            .spawn((
                ShoeTop { discard },
                Mesh3d(assets.art.card_mesh.clone()),
                MeshMaterial3d(assets.art.back.clone()),
                Transform::from_translation(pos).with_rotation(face_up(yaw)).with_scale(Vec3::splat(CARD_SCALE)),
            ))
            .id();
        commands.entity(root).add_child(top);
    }

    // The dealer's chip tray with a row of decorative stacks.
    let tray = commands
        .spawn((
            Mesh3d(meshes.add(Cuboid::new(1.9, 0.05, 0.42))),
            MeshMaterial3d(holder),
            Transform::from_translation(TRAY_POS + Vec3::new(0.0, 0.012, -0.05)),
        ))
        .id();
    commands.entity(root).add_child(tray);
    for i in 0..10 {
        let denom = i % assets.art.chips.len();
        let x = -0.8 + i as f32 * 0.178;
        let height = 6 + (hash(i as u64) * 8.0) as usize;
        for k in 0..height {
            let e = commands
                .spawn((
                    Mesh3d(assets.art.chip_mesh.clone()),
                    MeshMaterial3d(assets.art.chips[denom].clone()),
                    Transform::from_translation(TRAY_POS + Vec3::new(x, 0.04 + CHIP_H * (k as f32 + 0.5), -0.16))
                        .with_rotation(Quat::from_rotation_y(hash((i * 31 + k) as u64) * PI)),
                ))
                .id();
            commands.entity(root).add_child(e);
        }
    }

    // Betting circles, one per seat.
    let n = session.lobby.seats.len().max(1);
    let ring = meshes.add(Annulus::new(0.25, 0.29));
    for seat in 0..n {
        let e = commands
            .spawn((
                SpotRing { seat },
                Mesh3d(ring.clone()),
                MeshMaterial3d(assets.ring_materials[seat % assets.ring_materials.len()].clone()),
                Transform::from_translation(spot_pos(seat, n) + Vec3::Y * 0.003)
                    .with_rotation(Quat::from_rotation_x(-FRAC_PI_2)),
                NotShadowCaster,
            ))
            .id();
        commands.entity(root).add_child(e);
    }

    let pointer = commands
        .spawn((
            HandPointer,
            Mesh3d(meshes.add(Cone::new(0.07, 0.18))),
            MeshMaterial3d(assets.pointer_material.clone()),
            Transform::from_xyz(0.0, -5.0, 0.0).with_rotation(Quat::from_rotation_x(PI)),
            NotShadowCaster,
        ))
        .id();
    commands.entity(root).add_child(pointer);

    // The dealer in a waistcoat.
    let kit = FigureKit::new(&mut meshes, &mut materials);
    let vest = materials.add(StandardMaterial {
        base_color: Color::srgb(0.12, 0.12, 0.14),
        perceptual_roughness: 0.6,
        ..default()
    });
    let dealer = kit.spawn(&mut commands, DEALER_POS, usize::MAX, vest, false);
    commands.entity(root).add_child(dealer);
}

/// Everything despawned when leaving the table.
type TableEntities = Or<(With<BjRoot>, With<CardEntity>, With<ChipStack>)>;

pub fn despawn_table(
    mut commands: Commands,
    roots: Query<Entity, TableEntities>,
    camera: Query<Entity, With<MainCamera>>,
    mut ambient: ResMut<AmbientLight>,
) {
    for e in &roots {
        commands.entity(e).despawn();
    }
    leave_room(&mut commands, &camera, &mut ambient);
    commands.remove_resource::<BjTable>();
}

fn spawn_card(commands: &mut Commands, assets: &BjAssets, motion: Motion, face: Option<&Card>) -> Entity {
    let front_mat = match face {
        Some(c) if !c.is_hidden() => assets.art.faces[face_index(c)].clone(),
        _ => assets.art.back.clone(),
    };
    let front = commands
        .spawn((Mesh3d(assets.art.card_mesh.clone()), MeshMaterial3d(front_mat), Transform::default()))
        .id();
    let back = commands
        .spawn((
            Mesh3d(assets.art.card_mesh.clone()),
            MeshMaterial3d(assets.art.back.clone()),
            Transform::from_rotation(Quat::from_rotation_y(PI)),
        ))
        .id();
    let start = motion.flight.as_ref().map(|f| f.from).unwrap_or(motion.target);
    commands
        .spawn((CardEntity { front }, motion, start, Visibility::default()))
        .add_children(&[front, back])
        .id()
}

fn spawn_stack(commands: &mut Commands, assets: &BjAssets, amount: u32, motion: Motion, seed: u64) -> Entity {
    let start = motion.flight.as_ref().map(|f| f.from).unwrap_or(motion.target);
    let root = commands.spawn((ChipStack, motion, start, Visibility::default())).id();
    for (k, denom) in chip_breakdown(amount, 16).into_iter().enumerate() {
        let s = seed * 131 + k as u64;
        let jitter = Vec3::new((hash(s) - 0.5) * 0.012, 0.0, (hash(s + 7) - 0.5) * 0.012);
        let disc = commands
            .spawn((
                Mesh3d(assets.art.chip_mesh.clone()),
                MeshMaterial3d(assets.art.chips[denom].clone()),
                Transform::from_translation(Vec3::Y * (CHIP_H * (k as f32 + 0.5) + 0.002) + jitter)
                    .with_rotation(Quat::from_rotation_y(hash(s + 3) * PI)),
            ))
            .id();
        commands.entity(root).add_child(disc);
    }
    root
}

// ----- reconciliation -----------------------------------------------------------------

/// Whether the dealer showed a blackjack (the hole card is up and makes 21).
fn dealer_blackjack(view: &BjGame) -> bool {
    view.hole_revealed && view.dealer.len() == 2 && hand_value(&view.dealer).total == 21
}

#[allow(clippy::too_many_arguments)]
pub fn sync_table(
    mut commands: Commands,
    session: Res<Session>,
    assets: Res<BjAssets>,
    mut table: ResMut<BjTable>,
    mut motions: Query<(&mut Motion, &Transform)>,
    cards: Query<&CardEntity>,
    mut sfx: EventWriter<Sfx>,
) {
    let Some(view) = session.bj_view() else { return };
    let n = view.players.len();
    if !table.initialized || table.seats != n {
        *table = BjTable {
            seats: n,
            initialized: true,
            ..default()
        };
    }

    // ---- events: deal timing and sounds -------------------------------------------
    if view.event_seq < table.last_seq {
        table.last_seq = 0;
    }
    let mut delays: HashMap<u16, f32> = HashMap::new();
    let mut dealt = 0usize;
    let mut chips_moved = false;
    let mut collect = false;
    for e in view.events.iter().filter(|e| e.seq > table.last_seq) {
        match &e.event {
            BjEvent::Dealt { card, .. } => {
                let key = if card.is_hidden() { HOLE } else { card.id };
                delays.insert(key, dealt as f32 * 0.22);
                dealt += 1;
            }
            BjEvent::RoundStarted { .. } => collect = true,
            BjEvent::Reshuffled => {
                sfx.write(Sfx::Shuffle);
            }
            BjEvent::BetPlaced { .. }
            | BjEvent::InsuranceTaken { .. }
            | BjEvent::Settled { .. }
            | BjEvent::InsurancePaid { .. } => chips_moved = true,
            BjEvent::HoleRevealed { .. } => {
                sfx.write(Sfx::CardPlay);
            }
            _ => {}
        }
    }
    table.last_seq = view.event_seq;
    if dealt > 2 {
        sfx.write(Sfx::Deal);
    } else if dealt > 0 {
        sfx.write(Sfx::CardDraw);
    }
    if chips_moved {
        sfx.write(Sfx::Coin);
    }
    if collect {
        sfx.write(Sfx::Shuffle);
    }

    // ---- cards --------------------------------------------------------------------------
    let from_shoe = shoe_top(view.shoe.len());
    let mut wanted: Vec<(u16, Transform, Option<Card>)> = Vec::new();
    for (k, card) in view.dealer.iter().enumerate() {
        let hidden = card.is_hidden();
        let key = if hidden { HOLE } else { card.id };
        wanted.push((key, dealer_slot(k, view.dealer.len(), hidden), (!hidden).then_some(*card)));
    }
    for (seat, p) in view.players.iter().enumerate() {
        for (h, hand) in p.hands.iter().enumerate() {
            for (k, card) in hand.cards.iter().enumerate() {
                wanted.push((card.id, card_slot(seat, n, h, p.hands.len(), k), Some(*card)));
            }
        }
    }
    let mut keep: HashMap<u16, Entity> = HashMap::with_capacity(wanted.len());
    for (key, target, face) in wanted {
        let existing = table.cards.remove(&key).or_else(|| {
            // The hole card was turned over: it now has its real id.
            (key != HOLE && view.dealer.get(1).is_some_and(|c| c.id == key))
                .then(|| table.cards.remove(&HOLE))
                .flatten()
        });
        match existing {
            Some(e) => {
                if let Ok((mut m, t)) = motions.get_mut(e) {
                    let flipping = face.is_some() && m.target.rotation.dot(face_down(0.0)).abs() > 0.99;
                    if flipping {
                        let from = *t;
                        m.target = target;
                        crate::table3d::fly(&mut m, from, 0.0, 0.45, 0.25);
                        if let (Ok(c), Some(card)) = (cards.get(e), face) {
                            commands
                                .entity(c.front)
                                .insert(MeshMaterial3d(assets.art.faces[face_index(&card)].clone()));
                        }
                    } else {
                        m.target = target;
                    }
                }
                keep.insert(key, e);
            }
            None => {
                let delay = delays.get(&key).copied().unwrap_or(0.0);
                let e = spawn_card(&mut commands, &assets, Motion::flying(from_shoe, target, delay, 0.42, 0.35), face.as_ref());
                keep.insert(key, e);
            }
        }
    }
    // Cards that left the table go to the discard tray.
    let to_discard = discard_top(view.discard.len());
    for (i, (_, e)) in table.cards.drain().enumerate() {
        if let Ok((mut m, t)) = motions.get_mut(e) {
            let from = *t;
            m.target = to_discard;
            crate::table3d::fly(&mut m, from, i as f32 * 0.03, 0.5, 0.4);
            m.despawn_on_arrival = true;
        }
    }
    table.cards = keep;

    // ---- chips -----------------------------------------------------------------------------
    let dealer_bj = dealer_blackjack(&view);
    let insurance_settled = !matches!(view.phase, Phase::Betting | Phase::Insurance);
    let mut want_stacks: Vec<(StackKey, u32, Transform, bool)> = Vec::new();
    for (seat, p) in view.players.iter().enumerate() {
        let hands = p.hands.len();
        for (h, hand) in p.hands.iter().enumerate() {
            let bet_key = StackKey { seat, hand: h, kind: StackKind::Bet };
            let lost = matches!(hand.outcome, Some(Outcome::Lose | Outcome::Bust));
            let target = if lost { tray_slot(bet_key) } else { stack_slot(bet_key, n, hands) };
            want_stacks.push((bet_key, hand.bet, target, lost));
            if let Some(outcome @ (Outcome::Win | Outcome::Blackjack)) = hand.outcome {
                let key = StackKey { seat, hand: h, kind: StackKind::Winnings };
                want_stacks.push((key, outcome.payout(hand.bet) - hand.bet, stack_slot(key, n, hands), false));
            }
        }
        if let Some(ins) = p.insurance.filter(|a| *a > 0) {
            let key = StackKey { seat, hand: 0, kind: StackKind::Insurance };
            let lost = insurance_settled && !dealer_bj;
            let target = if lost { tray_slot(key) } else { stack_slot(key, n, hands) };
            want_stacks.push((key, ins, target, lost));
            if dealer_bj {
                let key = StackKey { seat, hand: 0, kind: StackKind::InsuranceWin };
                want_stacks.push((key, ins * 2, stack_slot(key, n, hands), false));
            }
        }
    }
    let mut keep_stacks = HashMap::new();
    for (key, amount, target, at_dealer) in want_stacks {
        let entry = match table.stacks.remove(&key) {
            Some(entry) if entry.amount == amount => {
                if let Ok((mut m, t)) = motions.get_mut(entry.entity) {
                    if entry.at_dealer != at_dealer {
                        let from = *t;
                        m.target = target;
                        crate::table3d::fly(&mut m, from, 0.1, 0.6, 0.4);
                    } else {
                        m.target = target;
                    }
                }
                StackEntry { at_dealer, ..entry }
            }
            old => {
                // New or resized (a doubled bet): build the stack again.
                let from = match &old {
                    Some(entry) => {
                        let at = motions.get(entry.entity).map(|(_, t)| *t).unwrap_or(target);
                        commands.entity(entry.entity).despawn();
                        at
                    }
                    None => match key.kind {
                        StackKind::Bet | StackKind::Insurance => player_side(key.seat, n),
                        StackKind::Winnings | StackKind::InsuranceWin => tray_slot(key),
                    },
                };
                let seed = (key.seat * 8 + key.hand) as u64 * 4 + key.kind as u64;
                let e = spawn_stack(&mut commands, &assets, amount, Motion::flying(from, target, 0.0, 0.5, 0.3), seed);
                StackEntry { entity: e, amount, at_dealer }
            }
        };
        keep_stacks.insert(key, entry);
    }
    // Stacks that are gone: lost bets sink into the tray, the rest go home.
    for (key, entry) in table.stacks.drain() {
        if let Ok((mut m, t)) = motions.get_mut(entry.entity) {
            let from = *t;
            m.target = if entry.at_dealer { tray_slot(key) } else { player_side(key.seat, n) };
            m.target.translation.y -= if entry.at_dealer { 0.1 } else { 0.0 };
            crate::table3d::fly(&mut m, from, 0.0, 0.5, 0.25);
            m.despawn_on_arrival = true;
        }
    }
    table.stacks = keep_stacks;
}

/// The shoe and tray heights, the betting circles, the pointer over the hand
/// in play and the dealer's idle animation.
#[allow(clippy::type_complexity, clippy::too_many_arguments)]
pub fn animate_table(
    time: Res<Time>,
    session: Res<Session>,
    assets: Res<BjAssets>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut blocks: Query<(&ShoeStack, &mut Transform), (Without<ShoeTop>, Without<HandPointer>, Without<Figure>, Without<FigureHead>)>,
    mut tops: Query<(&ShoeTop, &mut Transform), (Without<ShoeStack>, Without<HandPointer>, Without<Figure>, Without<FigureHead>)>,
    mut pointer: Query<&mut Transform, (With<HandPointer>, Without<ShoeStack>, Without<ShoeTop>, Without<Figure>, Without<FigureHead>)>,
    mut figures: Query<(&Figure, &mut Transform, &Children), (Without<FigureHead>, Without<ShoeStack>, Without<ShoeTop>, Without<HandPointer>)>,
    mut heads: Query<&mut Transform, (With<FigureHead>, Without<Figure>, Without<ShoeStack>, Without<ShoeTop>, Without<HandPointer>)>,
    rings: Query<&SpotRing>,
) {
    let Some(view) = session.bj_view() else { return };
    let t = time.elapsed_secs();
    let dt = time.delta_secs();
    let n = view.players.len();

    for (s, mut tr) in &mut blocks {
        let count = if s.discard { view.discard.len() } else { view.shoe.len() };
        let h = (count as f32 * CARD_THICK).max(0.001);
        tr.translation.y = h / 2.0;
        tr.scale.y = h;
    }
    for (s, mut tr) in &mut tops {
        let count = if s.discard { view.discard.len() } else { view.shoe.len() };
        tr.translation.y = count as f32 * CARD_THICK + 0.0015;
    }

    // Betting circles glow for whoever the table is waiting on.
    for ring in &rings {
        let p = &view.players[ring.seat.min(n - 1)];
        let waiting = view.is_waiting_for(ring.seat);
        let h = &assets.ring_materials[ring.seat % assets.ring_materials.len()];
        if let Some(m) = materials.get_mut(h) {
            let c = SEAT_COLORS[ring.seat % SEAT_COLORS.len()];
            let base = Color::srgb_u8(c[0], c[1], c[2]);
            let pulse = if waiting { 0.75 + (t * 4.0).sin() * 0.25 } else if p.out { 0.15 } else { 0.45 };
            m.base_color = base.with_alpha(pulse);
        }
    }

    if let Ok(mut tr) = pointer.single_mut() {
        if let Phase::PlayerTurn { seat, hand } = view.phase {
            let p = &view.players[seat];
            let cards = p.hands.get(hand).map(|h| h.cards.len()).unwrap_or(0);
            let target = hand_label_pos(seat, n, hand, p.hands.len(), cards) + Vec3::Y * (0.3 + (t * 5.0).sin() * 0.04);
            if tr.translation.y < -1.0 {
                tr.translation = target;
            }
            tr.translation = tr.translation.lerp(target, (dt * 10.0).min(1.0));
        } else {
            tr.translation = Vec3::new(0.0, -5.0, 0.0);
        }
    }

    let look_at = match view.phase {
        Phase::PlayerTurn { seat, .. } => spot_pos(seat, n),
        _ => DEALER_CARDS,
    };
    for (fig, mut tr, children) in &mut figures {
        let mut base = fig.base;
        base.translation.y += (t * 1.4).sin() * 0.02;
        if view.phase == Phase::DealerTurn {
            base.translation += base.forward().as_vec3() * 0.1;
        }
        *tr = base;
        for child in children.iter() {
            if let Ok(mut head) = heads.get_mut(child) {
                let goal = head_look(&fig.base, look_at + Vec3::Y * 0.3);
                head.rotation = head.rotation.slerp(goal, 0.08);
            }
        }
    }
}
