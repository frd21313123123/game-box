//! The 3D table: room, players sitting around it, and every card on it.
//!
//! Cards are real entities. The view of the game (from the [`Session`]) is
//! reconciled with them every frame: new cards fly in from the deck, played
//! cards fly from the player's hand to the discard pile, and the pile only
//! keeps the last few cards — older ones slide away and fade out.

use std::collections::VecDeque;
use std::f32::consts::{FRAC_PI_2, PI, TAU};

use bevy::pbr::NotShadowCaster;
use bevy::prelude::*;
use bevy::window::PrimaryWindow;
use bevy_egui::input::EguiWantsInput;
use uno_core::{Card, CardColor, Direction, Phase, UnoAction, UnoEvent, UnoGame};

use super::art::{build_card_art, face_index, CardArt, CARD_H, CARD_W};
use super::UnoUi;
use crate::net::Session;
use crate::sfx::Sfx;
use crate::table3d::{
    enter_room, face_down, face_up, fly, hash, head_look, leave_room, spawn_room, Figure, FigureHead, FigureKit,
    Flight, Motion, TABLE_R,
};
use crate::theme::SEAT_COLORS;
use crate::MainCamera;

const SEAT_R: f32 = 4.05;
const OPP_HAND_R: f32 = 3.45;
const DECK_POS: Vec3 = Vec3::new(-0.75, 0.0, -0.15);
const PILE_POS: Vec3 = Vec3::new(0.55, 0.0, -0.1);
/// How many cards stay visible on the discard pile.
pub const PILE_LIMIT: usize = 9;
const CARD_THICK: f32 = 0.0035;
const CAMERA_POS: Vec3 = Vec3::new(0.0, 5.3, 7.7);
const CAMERA_LOOK: Vec3 = Vec3::new(0.0, 0.25, 0.35);

/// Root of everything spawned for the card table.
#[derive(Component)]
pub struct UnoRoot;

#[derive(Component)]
pub struct CardEntity {
    front: Entity,
    back: Entity,
}

#[derive(Component)]
pub struct Fading {
    elapsed: f32,
    material: Handle<StandardMaterial>,
}

#[derive(Component)]
pub struct DeckStack;

#[derive(Component)]
pub struct DeckTop;

#[derive(Component)]
pub struct ColorRing;

#[derive(Component)]
pub struct DirectionArrow {
    phase: f32,
}

#[derive(Component)]
pub struct TurnMarker;

/// Book-keeping of which entity represents which card.
#[derive(Resource, Default)]
pub struct Table {
    seats: usize,
    pub(super) me: usize,
    pub(super) my_hand: Vec<(u16, Entity)>,
    opp_hands: Vec<Vec<Entity>>,
    pile: VecDeque<(Entity, Transform)>,
    last_seq: u64,
    pile_height: u32,
    initialized: bool,
}

#[derive(Resource)]
pub struct TableAssets {
    art: CardArt,
    ring_material: Handle<StandardMaterial>,
    arrow_material: Handle<StandardMaterial>,
    marker_material: Handle<StandardMaterial>,
}

pub fn setup_assets(
    mut commands: Commands,
    mut images: ResMut<Assets<Image>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut meshes: ResMut<Assets<Mesh>>,
) {
    let art = build_card_art(&mut images, &mut materials, &mut meshes);
    let glow = |materials: &mut Assets<StandardMaterial>| {
        materials.add(StandardMaterial {
            base_color: Color::srgb(0.9, 0.2, 0.2),
            emissive: LinearRgba::rgb(2.0, 0.4, 0.4),
            unlit: false,
            ..default()
        })
    };
    let ring_material = glow(&mut materials);
    let arrow_material = glow(&mut materials);
    let marker_material = materials.add(StandardMaterial {
        base_color: Color::srgba(1.0, 0.85, 0.3, 0.35),
        emissive: LinearRgba::rgb(1.6, 1.2, 0.3),
        alpha_mode: AlphaMode::Blend,
        unlit: true,
        ..default()
    });
    commands.insert_resource(TableAssets {
        art,
        ring_material,
        arrow_material,
        marker_material,
    });
}

/// Angle around the table of a seat, relative to the local player (who sits
/// at angle 0, nearest to the camera). Opponents spread over the far side.
pub fn seat_angle(rel: usize, n: usize) -> f32 {
    if rel == 0 {
        return 0.0;
    }
    if n == 2 {
        return PI;
    }
    let t = (rel - 1) as f32 / (n - 2) as f32;
    (85f32 + t * 190.0).to_radians()
}

fn seat_dir(angle: f32) -> Vec3 {
    Vec3::new(-angle.sin(), 0.0, angle.cos())
}

/// World position just above a player's head (for name plates).
pub fn head_top(rel: usize, n: usize) -> Vec3 {
    seat_dir(seat_angle(rel, n)) * SEAT_R + Vec3::Y * 1.2
}

pub fn rel_seat(seat: usize, me: usize, n: usize) -> usize {
    (seat + n - me) % n
}

fn deck_top_transform(count: usize) -> Transform {
    Transform::from_translation(DECK_POS + Vec3::Y * (count as f32 * 0.0024 + 0.01))
        .with_rotation(face_down(0.12))
}

pub fn spawn_table(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    assets: Res<TableAssets>,
    session: Res<Session>,
    mut camera: Query<(Entity, &mut Transform), With<MainCamera>>,
    mut ambient: ResMut<AmbientLight>,
) {
    commands.insert_resource(Table::default());
    if let Ok((cam, mut t)) = camera.single_mut() {
        enter_room(&mut commands, (cam, &mut t), CAMERA_POS, CAMERA_LOOK, &mut ambient);
    }
    let card_edge = materials.add(StandardMaterial {
        base_color: Color::srgb(0.93, 0.91, 0.86),
        perceptual_roughness: 0.8,
        ..default()
    });
    let root = commands
        .spawn((UnoRoot, Transform::default(), Visibility::default()))
        .id();
    spawn_room(&mut commands, &mut meshes, &mut materials, root, Color::srgb(0.09, 0.36, 0.24));

    // Draw pile: a block of card edges with a card back on top.
    let deck_block = commands
        .spawn((
            DeckStack,
            Mesh3d(meshes.add(Cuboid::new(CARD_W * 0.98, 1.0, CARD_H * 0.98))),
            MeshMaterial3d(card_edge),
            Transform::from_translation(DECK_POS).with_rotation(Quat::from_rotation_y(0.12)),
        ))
        .id();
    commands.entity(root).add_child(deck_block);
    let deck_top = commands
        .spawn((
            DeckTop,
            Mesh3d(assets.art.mesh.clone()),
            MeshMaterial3d(assets.art.back.clone()),
            Transform::from_translation(DECK_POS).with_rotation(face_up(0.12)),
        ))
        .id();
    commands.entity(root).add_child(deck_top);

    // Active colour ring around the discard pile and direction arrows.
    let ring = commands
        .spawn((
            ColorRing,
            Mesh3d(meshes.add(Torus::new(0.78, 0.83).mesh().major_resolution(96))),
            MeshMaterial3d(assets.ring_material.clone()),
            Transform::from_translation(PILE_POS + Vec3::Y * 0.005),
            NotShadowCaster,
        ))
        .id();
    commands.entity(root).add_child(ring);
    let arrow_mesh = meshes.add(Cone::new(0.11, 0.3));
    for i in 0..5 {
        let e = commands
            .spawn((
                DirectionArrow {
                    phase: i as f32 / 5.0 * TAU,
                },
                Mesh3d(arrow_mesh.clone()),
                MeshMaterial3d(assets.arrow_material.clone()),
                Transform::default(),
                NotShadowCaster,
            ))
            .id();
        commands.entity(root).add_child(e);
    }
    let marker = commands
        .spawn((
            TurnMarker,
            Mesh3d(meshes.add(Annulus::new(0.0, 0.85))),
            MeshMaterial3d(assets.marker_material.clone()),
            Transform::from_xyz(0.0, 0.004, TABLE_R - 0.7).with_rotation(Quat::from_rotation_x(-FRAC_PI_2)),
            NotShadowCaster,
        ))
        .id();
    commands.entity(root).add_child(marker);

    // Opponents in their chairs.
    let n = session.lobby.seats.len().max(2);
    let me = session.my_index().unwrap_or(0);
    let kit = FigureKit::new(&mut meshes, &mut materials);
    for seat in 0..n {
        let rel = rel_seat(seat, me, n);
        if rel == 0 {
            continue;
        }
        let pos = seat_dir(seat_angle(rel, n)) * SEAT_R;
        let c = SEAT_COLORS[seat % SEAT_COLORS.len()];
        let shirt = materials.add(StandardMaterial {
            base_color: Color::srgb_u8(c[0], c[1], c[2]),
            perceptual_roughness: 0.75,
            ..default()
        });
        let figure = kit.spawn(&mut commands, pos, seat, shirt, true);
        commands.entity(root).add_child(figure);
    }
}

pub fn despawn_table(
    mut commands: Commands,
    roots: Query<Entity, With<UnoRoot>>,
    cards: Query<Entity, With<CardEntity>>,
    camera: Query<Entity, With<MainCamera>>,
    mut ambient: ResMut<AmbientLight>,
) {
    for e in roots.iter().chain(cards.iter()) {
        commands.entity(e).despawn();
    }
    leave_room(&mut commands, &camera, &mut ambient);
    commands.remove_resource::<Table>();
}

fn spawn_card(commands: &mut Commands, assets: &TableAssets, at: Transform, face: Option<&Card>) -> Entity {
    let front_mat = match face {
        Some(c) if !c.is_hidden() => assets.art.faces[face_index(c)].clone(),
        _ => assets.art.back.clone(),
    };
    let front = commands
        .spawn((Mesh3d(assets.art.mesh.clone()), MeshMaterial3d(front_mat), Transform::default()))
        .id();
    let back = commands
        .spawn((
            Mesh3d(assets.art.mesh.clone()),
            MeshMaterial3d(assets.art.back.clone()),
            Transform::from_rotation(Quat::from_rotation_y(PI)),
        ))
        .id();
    commands
        .spawn((
            CardEntity { front, back },
            Motion {
                target: at,
                flight: None,
                despawn_on_arrival: false,
            },
            at,
            Visibility::default(),
        ))
        .add_children(&[front, back])
        .id()
}

/// Sort key so the local hand is grouped by colour and value.
fn hand_order(card: &Card) -> (usize, u8) {
    let color = card.color.map(|c| c.index()).unwrap_or(4);
    let face = match card.face {
        uno_core::Face::Number(n) => n,
        uno_core::Face::Skip => 10,
        uno_core::Face::Reverse => 11,
        uno_core::Face::DrawTwo => 12,
        uno_core::Face::Wild => 13,
        uno_core::Face::WildDrawFour => 14,
    };
    (color, face)
}

fn pile_transform(card: &Card, height: u32) -> Transform {
    let seed = card.id as u64 * 7919 + height as u64;
    let offset = Vec3::new((hash(seed) - 0.5) * 0.26, 0.0, (hash(seed + 1) - 0.5) * 0.26);
    let yaw = (hash(seed + 2) - 0.5) * 1.3;
    Transform::from_translation(PILE_POS + offset + Vec3::Y * (0.006 + height as f32 * CARD_THICK))
        .with_rotation(face_up(yaw))
}

fn opp_hand_slot(rel: usize, n: usize, i: usize, count: usize) -> Transform {
    let angle = seat_angle(rel, n);
    let dir = seat_dir(angle);
    let center = dir * OPP_HAND_R + Vec3::Y * 0.62;
    // Front faces the player holding the cards; we see the backs.
    let base = Transform::from_translation(center).looking_at(center - dir, Vec3::Y);
    let spread = (count.max(1) - 1) as f32;
    let k = i as f32 - spread / 2.0;
    let step = (0.16f32).min(1.6 / count.max(1) as f32);
    let fan = Quat::from_rotation_z(-k * 0.09);
    let right = base.rotation * Vec3::X;
    let back_dir = base.rotation * Vec3::Z;
    Transform {
        translation: center + right * k * step - Vec3::Y * (k * k) * 0.004 + back_dir * i as f32 * 0.004,
        rotation: base.rotation * fan * Quat::from_rotation_x(0.18),
        scale: Vec3::splat(0.82),
    }
}

/// Positions for the local player's hand, fanned in front of the camera.
fn my_hand_slot(cam: &Transform, i: usize, count: usize, lift: f32) -> Transform {
    let fwd = cam.forward().as_vec3();
    let right = cam.right().as_vec3();
    let up = cam.up().as_vec3();
    let base = cam.translation + fwd * 2.35 - up * 0.66;
    let spread = (count.max(1) - 1) as f32;
    let k = i as f32 - spread / 2.0;
    let step = (0.23f32).min(2.3 / count.max(1) as f32);
    let curve = -(k * step).powi(2) * 0.09;
    let tilt = Quat::from_rotation_x(-0.32);
    let fan = Quat::from_rotation_z(-k * step * 0.2);
    Transform {
        translation: base + right * k * step + up * (curve + lift) + fwd * (-0.002 * i as f32 - lift * 0.2),
        rotation: cam.rotation * tilt * fan,
        scale: Vec3::splat(0.42),
    }
}

#[allow(clippy::too_many_arguments)]
pub fn sync_table(
    mut commands: Commands,
    session: Res<Session>,
    assets: Res<TableAssets>,
    mut table: ResMut<Table>,
    mut motions: Query<(&mut Motion, &Transform)>,
    cards: Query<&CardEntity>,
    camera: Query<&Transform, (With<MainCamera>, Without<Motion>)>,
    mut ui: ResMut<UnoUi>,
    mut sfx: EventWriter<Sfx>,
) {
    let Some(view) = session.uno_view() else { return };
    let n = view.players.len();
    let me = session.my_index().unwrap_or(0).min(n - 1);
    if !table.initialized || table.seats != n {
        *table = Table {
            seats: n,
            me,
            opp_hands: vec![Vec::new(); n],
            initialized: true,
            ..default()
        };
    }

    if view.event_seq < table.last_seq {
        table.last_seq = 0;
    }
    let mut dealing = false;
    let mut played_sound = false;
    let new_events: Vec<UnoEvent> = view
        .events
        .iter()
        .filter(|e| e.seq > table.last_seq)
        .map(|e| e.event.clone())
        .collect();
    table.last_seq = view.event_seq;

    for event in new_events {
        match event {
            UnoEvent::RoundStarted { .. } => {
                // Clear the table; the new hands are dealt below.
                let mut all: Vec<Entity> = table.my_hand.drain(..).map(|(_, e)| e).collect();
                for h in &mut table.opp_hands {
                    all.extend(h.drain(..));
                }
                all.extend(table.pile.drain(..).map(|(e, _)| e));
                for e in all {
                    if let Ok((mut m, t)) = motions.get_mut(e) {
                        m.target = deck_top_transform(60);
                        fly(&mut m, *t, 0.0, 0.5, 0.6);
                        m.despawn_on_arrival = true;
                    }
                }
                table.pile_height = 0;
                dealing = true;
                sfx.write(Sfx::Shuffle);
            }
            UnoEvent::FirstCard { card } => {
                let from = deck_top_transform(view.draw_pile.len());
                let e = spawn_card(&mut commands, &assets, from, Some(&card));
                let target = pile_transform(&card, table.pile_height);
                table.pile_height += 1;
                commands.entity(e).insert(Motion {
                    target,
                    flight: Some(Flight { from, elapsed: 0.0, delay: 1.6, duration: 0.6, height: 0.8 }),
                    despawn_on_arrival: false,
                });
                table.pile.push_back((e, target));
            }
            UnoEvent::Played { seat, card } => {
                let rel = rel_seat(seat, me, n);
                let entity = if seat == me {
                    table
                        .my_hand
                        .iter()
                        .position(|(id, _)| *id == card.id)
                        .map(|i| table.my_hand.remove(i).1)
                } else {
                    table.opp_hands[seat].pop()
                };
                let entity = entity.unwrap_or_else(|| {
                    let at = if rel == 0 {
                        camera.single().map(|c| my_hand_slot(c, 0, 1, 0.0)).unwrap_or_default()
                    } else {
                        opp_hand_slot(rel, n, 0, 1)
                    };
                    spawn_card(&mut commands, &assets, at, Some(&card))
                });
                if let Ok(c) = cards.get(entity) {
                    commands
                        .entity(c.front)
                        .insert(MeshMaterial3d(assets.art.faces[face_index(&card)].clone()));
                }
                let target = pile_transform(&card, table.pile_height);
                table.pile_height += 1;
                if let Ok((mut m, t)) = motions.get_mut(entity) {
                    m.target = target;
                    fly(&mut m, *t, 0.0, 0.48, 0.55);
                } else {
                    // Spawned this frame: its Motion was inserted with the spawn.
                    commands.entity(entity).insert(Motion {
                        target,
                        flight: None,
                        despawn_on_arrival: false,
                    });
                }
                table.pile.push_back((entity, target));
                if !played_sound {
                    sfx.write(Sfx::CardPlay);
                    played_sound = true;
                }
                ui.last_played_by = Some(seat);
            }
            UnoEvent::Reshuffled => {
                // Everything but the top card goes back into the deck.
                while table.pile.len() > 1 {
                    let (e, _) = table.pile.pop_front().expect("len > 1");
                    if let Ok((mut m, t)) = motions.get_mut(e) {
                        m.target = deck_top_transform(20);
                        fly(&mut m, *t, 0.0, 0.6, 0.5);
                        m.despawn_on_arrival = true;
                    }
                }
                table.pile_height = 1;
                sfx.write(Sfx::Shuffle);
            }
            _ => {}
        }
    }

    // Retire old pile cards: slide them off and fade out.
    while table.pile.len() > PILE_LIMIT {
        let (e, _) = table.pile.pop_front().expect("len > limit");
        commands.entity(e).insert(Fading {
            elapsed: 0.0,
            material: Handle::default(),
        });
    }

    // Reconcile the local hand.
    let deck_from = deck_top_transform(view.draw_pile.len());
    let mut wanted: Vec<Card> = view.players[me].hand.clone();
    wanted.sort_by_key(hand_order);
    let mut drew = 0usize;
    let mut kept: Vec<(u16, Entity)> = Vec::with_capacity(wanted.len());
    for (order, card) in wanted.iter().enumerate() {
        if let Some(pos) = table.my_hand.iter().position(|(id, _)| *id == card.id) {
            kept.push(table.my_hand.remove(pos));
        } else {
            let e = spawn_card(&mut commands, &assets, deck_from, Some(card));
            let delay = if dealing {
                0.15 + (order * n) as f32 * 0.05
            } else {
                drew as f32 * 0.14
            };
            commands.entity(e).insert(Motion {
                target: deck_from,
                flight: Some(Flight { from: deck_from, elapsed: 0.0, delay, duration: 0.55, height: 0.5 }),
                despawn_on_arrival: false,
            });
            kept.push((card.id, e));
            drew += 1;
        }
    }
    // Cards that vanished without being played (e.g. a new round) go back.
    for (_, e) in table.my_hand.drain(..) {
        if let Ok((mut m, t)) = motions.get_mut(e) {
            m.target = deck_from;
            fly(&mut m, *t, 0.0, 0.4, 0.3);
            m.despawn_on_arrival = true;
        }
    }
    table.my_hand = kept;

    // Reconcile opponents' hand sizes.
    for seat in 0..n {
        if seat == me {
            continue;
        }
        let want = view.players[seat].hand.len();
        let rel = rel_seat(seat, me, n);
        let mut i = 0;
        while table.opp_hands[seat].len() < want {
            let e = spawn_card(&mut commands, &assets, deck_from, None);
            let delay = if dealing {
                0.15 + (table.opp_hands[seat].len() * n + rel) as f32 * 0.05
            } else {
                i as f32 * 0.14
            };
            commands.entity(e).insert(Motion {
                target: deck_from,
                flight: Some(Flight { from: deck_from, elapsed: 0.0, delay, duration: 0.55, height: 0.5 }),
                despawn_on_arrival: false,
            });
            table.opp_hands[seat].push(e);
            drew += 1;
            i += 1;
        }
        while table.opp_hands[seat].len() > want {
            let e = table.opp_hands[seat].pop().expect("len > want");
            if let Ok((mut m, t)) = motions.get_mut(e) {
                m.target = deck_from;
                fly(&mut m, *t, 0.0, 0.4, 0.3);
                m.despawn_on_arrival = true;
            }
        }
    }
    if drew > 0 && !dealing {
        sfx.write(Sfx::CardDraw);
    }
    if dealing {
        sfx.write(Sfx::Deal);
    }
    table.me = me;
}

/// Updates every card's target slot from the current layout.
pub fn layout_cards(
    table: Res<Table>,
    session: Res<Session>,
    ui: Res<UnoUi>,
    camera: Query<&Transform, (With<MainCamera>, Without<Motion>)>,
    mut motions: Query<&mut Motion>,
) {
    let Ok(cam) = camera.single() else { return };
    let Some(game) = session.uno.as_ref() else { return };
    let n = table.seats;
    if n == 0 {
        return;
    }
    let my_turn = game.acting_seat() == Some(table.me)
        && matches!(game.phase, Phase::Turn | Phase::AfterDraw { .. });
    let count = table.my_hand.len();
    let playable: Vec<u16> = if my_turn {
        playable_ids(game, table.me, &session)
    } else {
        Vec::new()
    };
    for (i, (id, e)) in table.my_hand.iter().enumerate() {
        // The card under the mouse or the keyboard selection rises.
        let focused = ui.hovered == Some(*id) || ui.cursor == Some(*id);
        let lift = if focused {
            0.14
        } else if playable.contains(id) {
            0.05
        } else {
            0.0
        };
        if let Ok(mut m) = motions.get_mut(*e) {
            let mut slot = my_hand_slot(cam, i, count, lift);
            if focused {
                slot.scale *= 1.08;
            }
            m.target = slot;
        }
    }
    for seat in 0..n {
        if seat == table.me {
            continue;
        }
        let rel = rel_seat(seat, table.me, n);
        let hand = &table.opp_hands[seat];
        for (i, e) in hand.iter().enumerate() {
            if let Ok(mut m) = motions.get_mut(*e) {
                m.target = opp_hand_slot(rel, n, i, hand.len());
            }
        }
    }
    for (e, t) in &table.pile {
        if let Ok(mut m) = motions.get_mut(*e) {
            m.target = *t;
        }
    }
}

/// Ids of the local player's cards that may be played right now. Works on the
/// view the player has (the host's full game is redacted the same way).
fn playable_ids(_game: &UnoGame, me: usize, session: &Session) -> Vec<u16> {
    session
        .uno_view()
        .map(|v| v.playable_cards(me).iter().map(|c| c.id).collect())
        .unwrap_or_default()
}

/// Old pile cards slide outwards and fade away.
pub fn fade_old_cards(
    mut commands: Commands,
    time: Res<Time>,
    mut q: Query<(Entity, &CardEntity, &mut Fading, &mut Motion, &Transform)>,
    mats: Query<&MeshMaterial3d<StandardMaterial>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    const DURATION: f32 = 1.1;
    for (e, card, mut fading, mut motion, t) in &mut q {
        if fading.elapsed == 0.0 {
            // Give this card its own translucent copy of its material.
            if let Ok(mat) = mats.get(card.front) {
                if let Some(m) = materials.get(&mat.0).cloned() {
                    let mut m = m;
                    m.alpha_mode = AlphaMode::Blend;
                    let h = materials.add(m);
                    commands.entity(card.front).insert(MeshMaterial3d(h.clone()));
                    commands.entity(card.back).insert(Visibility::Hidden);
                    fading.material = h;
                }
            }
            let outward = (t.translation - PILE_POS).with_y(0.0).normalize_or(Vec3::X);
            let mut target = *t;
            target.translation += outward * 0.35 - Vec3::Y * 0.002;
            motion.target = target;
        }
        fading.elapsed += time.delta_secs();
        let alpha = (1.0 - fading.elapsed / DURATION).clamp(0.0, 1.0);
        if let Some(m) = materials.get_mut(&fading.material) {
            m.base_color.set_alpha(alpha);
        }
        if fading.elapsed >= DURATION {
            materials.remove(&fading.material);
            commands.entity(e).despawn();
        }
    }
}

/// Deck height, the active-colour ring, direction arrows, turn marker and
/// the idle animation of the players.
#[allow(clippy::too_many_arguments, clippy::type_complexity)]
pub fn animate_table(
    time: Res<Time>,
    session: Res<Session>,
    table: Res<Table>,
    assets: Res<TableAssets>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut deck: Query<
        (&mut Transform, Has<DeckTop>),
        (Or<(With<DeckStack>, With<DeckTop>)>, Without<DirectionArrow>, Without<TurnMarker>, Without<Figure>, Without<FigureHead>),
    >,
    mut arrows: Query<
        (&DirectionArrow, &mut Transform),
        (Without<DeckStack>, Without<TurnMarker>, Without<Figure>, Without<FigureHead>),
    >,
    mut marker: Query<
        &mut Transform,
        (With<TurnMarker>, Without<DeckStack>, Without<DirectionArrow>, Without<Figure>, Without<FigureHead>),
    >,
    mut figures: Query<
        (&Figure, &mut Transform, &Children),
        (Without<FigureHead>, Without<DeckStack>, Without<DirectionArrow>, Without<TurnMarker>),
    >,
    mut heads: Query<
        &mut Transform,
        (With<FigureHead>, Without<Figure>, Without<DeckStack>, Without<DirectionArrow>, Without<TurnMarker>),
    >,
) {
    let Some(view) = session.uno_view() else { return };
    let t = time.elapsed_secs();
    let n = view.players.len();
    let me = table.me;

    let h = (view.draw_pile.len() as f32 * 0.0024).max(0.004);
    for (mut d, top) in &mut deck {
        if top {
            d.translation.y = h + 0.0015;
        } else {
            d.translation.y = h / 2.0;
            d.scale.y = h;
        }
    }

    let color = view.active_color.rgb();
    let lin = Color::srgb_u8(color[0], color[1], color[2]).to_linear();
    let pulse = 1.6 + (t * 3.0).sin() * 0.4;
    for h in [&assets.ring_material, &assets.arrow_material] {
        if let Some(m) = materials.get_mut(h) {
            m.base_color = Color::srgb_u8(color[0], color[1], color[2]);
            m.emissive = lin * pulse;
        }
    }

    let sign = match view.direction {
        Direction::Clockwise => 1.0,
        Direction::CounterClockwise => -1.0,
    };
    // Visual order of seats runs counter-clockwise on screen; arrows follow play.
    for (arrow, mut tr) in &mut arrows {
        let a = arrow.phase + t * 0.6 * sign;
        let r = 1.55;
        let pos = Vec3::new(-a.sin() * r, 0.03, a.cos() * r) + Vec3::new(0.0, 0.0, -0.1);
        let tangent = Vec3::new(-a.cos(), 0.0, -a.sin()) * sign;
        tr.translation = pos;
        tr.rotation = Quat::from_rotation_arc(Vec3::Y, tangent);
        tr.scale = Vec3::new(1.0, 1.0, 0.35);
    }

    if let (Ok(mut m), Some(acting)) = (marker.single_mut(), view.acting_seat()) {
        let rel = rel_seat(acting, me, n);
        let dir = seat_dir(seat_angle(rel, n));
        let target = dir * (TABLE_R - 0.75) + Vec3::Y * 0.004;
        m.translation = m.translation.lerp(target, (time.delta_secs() * 8.0).min(1.0));
        let s = 1.0 + (t * 4.0).sin() * 0.05;
        m.scale = Vec3::splat(s);
    }

    let acting = view.acting_seat();
    for (fig, mut tr, children) in &mut figures {
        let breathe = (t * 1.6 + fig.seat as f32).sin() * 0.025;
        let mut base = fig.base;
        base.translation.y += breathe;
        if acting == Some(fig.seat) {
            // Lean in on your turn.
            let fwd = base.forward().as_vec3();
            base.translation += fwd * 0.12 + Vec3::Y * ((t * 5.0).sin() * 0.02);
        }
        *tr = base;
        // Heads look at whoever is acting (or the pile).
        let look_at = match acting {
            Some(a) if a != fig.seat => {
                let rel = rel_seat(a, me, n);
                seat_dir(seat_angle(rel, n)) * SEAT_R
            }
            _ => PILE_POS,
        };
        for child in children.iter() {
            if let Ok(mut head) = heads.get_mut(child) {
                let goal = head_look(&fig.base, look_at + Vec3::Y * 0.6);
                head.rotation = head.rotation.slerp(goal, 0.08);
            }
        }
    }
}

/// Hovering and clicking the local hand and the deck.
#[allow(clippy::too_many_arguments)]
pub fn hand_input(
    window: Query<&Window, With<PrimaryWindow>>,
    camera: Query<(&Camera, &GlobalTransform), With<MainCamera>>,
    buttons: Res<ButtonInput<MouseButton>>,
    egui_input: Res<EguiWantsInput>,
    table: Res<Table>,
    transforms: Query<&GlobalTransform, With<CardEntity>>,
    mut session: ResMut<Session>,
    mut ui: ResMut<UnoUi>,
) {
    ui.hovered = None;
    ui.deck_hovered = false;
    let Ok(window) = window.single() else { return };
    let Ok((cam, cam_t)) = camera.single() else { return };
    let Some(cursor) = window.cursor_position() else { return };
    if egui_input.wants_any_pointer_input() || ui.modal_open {
        return;
    }
    let Ok(ray) = cam.viewport_to_world(cam_t, cursor) else { return };
    let Some(view) = session.uno_view() else { return };
    let me = table.me;

    // Topmost card under the cursor (later cards are drawn above earlier ones).
    let mut best: Option<(f32, usize, u16)> = None;
    for (i, (id, e)) in table.my_hand.iter().enumerate() {
        let Ok(gt) = transforms.get(*e) else { continue };
        let (scale, rot, pos) = gt.to_scale_rotation_translation();
        let normal = rot * Vec3::Z;
        let denom = ray.direction.dot(normal);
        if denom.abs() < 1e-4 {
            continue;
        }
        let dist = (pos - ray.origin).dot(normal) / denom;
        if dist <= 0.0 {
            continue;
        }
        let hit = ray.origin + ray.direction * dist;
        let local = rot.inverse() * (hit - pos);
        if local.x.abs() <= CARD_W * scale.x / 2.0 && local.y.abs() <= CARD_H * scale.y / 2.0 {
            let better = best.is_none_or(|(_, bi, _)| i > bi);
            if better {
                best = Some((dist, i, *id));
            }
        }
    }
    ui.hovered = best.map(|(_, _, id)| id);

    // The deck: intersect with the table plane around the deck position.
    if ui.hovered.is_none() {
        if let Some(dist) = ray.intersect_plane(Vec3::Y * 0.1, InfinitePlane3d::new(Vec3::Y)) {
            let hit = ray.get_point(dist);
            let d = hit - DECK_POS;
            ui.deck_hovered = d.x.abs() < CARD_W * 0.7 && d.z.abs() < CARD_H * 0.65;
        }
    }

    if !buttons.just_pressed(MouseButton::Left) {
        return;
    }
    let my_turn = view.acting_seat() == Some(me);
    if let Some(id) = ui.hovered {
        super::keys::play_card(&mut session, &mut ui, &view, me, id);
    } else if ui.deck_hovered && my_turn && view.phase == Phase::Turn {
        session.submit_uno(UnoAction::Draw);
    }
}

/// Colour to show for the active colour in the HUD.
pub fn color32(c: CardColor) -> bevy_egui::egui::Color32 {
    let [r, g, b] = c.rgb();
    bevy_egui::egui::Color32::from_rgb(r, g, b)
}
