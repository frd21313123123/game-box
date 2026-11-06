//! Pieces shared by the 3D card tables: the room with the round table and
//! the lamp, seated figures, and the motion of things that fly across the
//! table (cards, chips).

use std::f32::consts::{FRAC_PI_2, PI};

use bevy::pbr::{DistanceFog, FogFalloff, NotShadowCaster};
use bevy::prelude::*;

use crate::MainCamera;

pub const TABLE_R: f32 = 3.0;

/// Where an entity wants to be, and an optional flight that gets it there.
#[derive(Component)]
pub struct Motion {
    pub target: Transform,
    pub flight: Option<Flight>,
    pub despawn_on_arrival: bool,
}

impl Motion {
    /// Flies from `from` to `to` after `delay` seconds.
    pub fn flying(from: Transform, to: Transform, delay: f32, duration: f32, height: f32) -> Self {
        Motion {
            target: to,
            flight: Some(Flight::new(from, delay, duration, height)),
            despawn_on_arrival: false,
        }
    }
}

pub struct Flight {
    pub from: Transform,
    pub elapsed: f32,
    pub delay: f32,
    pub duration: f32,
    /// Height of the arc above the straight line.
    pub height: f32,
}

impl Flight {
    pub fn new(from: Transform, delay: f32, duration: f32, height: f32) -> Self {
        Flight {
            from,
            elapsed: 0.0,
            delay,
            duration,
            height,
        }
    }
}

/// Starts a flight from `from` to the motion's current target.
pub fn fly(motion: &mut Motion, from: Transform, delay: f32, duration: f32, height: f32) {
    motion.flight = Some(Flight::new(from, delay, duration, height));
}

fn ease_in_out(t: f32) -> f32 {
    if t < 0.5 {
        4.0 * t * t * t
    } else {
        1.0 - (-2.0 * t + 2.0).powi(3) / 2.0
    }
}

/// Moves every [`Motion`]: flights follow an arc, everything else glides
/// towards its target.
pub fn animate_motion(mut commands: Commands, time: Res<Time>, mut q: Query<(Entity, &mut Transform, &mut Motion)>) {
    let dt = time.delta_secs();
    let follow = 1.0 - (-14.0 * dt).exp();
    for (e, mut t, mut m) in &mut q {
        let target = m.target;
        if let Some(f) = &mut m.flight {
            f.elapsed += dt;
            if f.elapsed < f.delay {
                *t = f.from;
                continue;
            }
            let raw = ((f.elapsed - f.delay) / f.duration).clamp(0.0, 1.0);
            let k = ease_in_out(raw);
            t.translation = f.from.translation.lerp(target.translation, k) + Vec3::Y * f.height * (PI * k).sin();
            t.rotation = f.from.rotation.slerp(target.rotation, k);
            t.scale = f.from.scale.lerp(target.scale, k);
            if raw >= 1.0 {
                m.flight = None;
                if m.despawn_on_arrival {
                    commands.entity(e).despawn();
                }
            }
            continue;
        }
        if m.despawn_on_arrival {
            commands.entity(e).despawn();
            continue;
        }
        t.translation = t.translation.lerp(target.translation, follow);
        t.rotation = t.rotation.slerp(target.rotation, follow);
        t.scale = t.scale.lerp(target.scale, follow);
    }
}

/// Cheap deterministic hash to a float in `[0, 1)`.
pub fn hash(a: u64) -> f32 {
    let mut z = a.wrapping_add(0x9E37_79B9_7F4A_7C15);
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    ((z ^ (z >> 31)) >> 40) as f32 / (1u64 << 24) as f32
}

/// A card lying on the table face up, its top pointing away from the camera
/// when `yaw` is 0.
pub fn face_up(yaw: f32) -> Quat {
    Quat::from_rotation_y(yaw) * Quat::from_rotation_x(-FRAC_PI_2)
}

pub fn face_down(yaw: f32) -> Quat {
    Quat::from_rotation_y(yaw) * Quat::from_rotation_x(FRAC_PI_2)
}

/// Points the camera at the table and dims the room.
pub fn enter_room(commands: &mut Commands, camera: (Entity, &mut Transform), pos: Vec3, look: Vec3, ambient: &mut AmbientLight) {
    let (cam, t) = camera;
    *t = Transform::from_translation(pos).looking_at(look, Vec3::Y);
    commands.entity(cam).insert(DistanceFog {
        color: Color::srgb(0.045, 0.04, 0.05),
        falloff: FogFalloff::Linear {
            start: 11.0,
            end: 24.0,
        },
        ..default()
    });
    ambient.brightness = 260.0;
    ambient.color = Color::srgb(1.0, 0.93, 0.85);
}

/// Undoes [`enter_room`].
pub fn leave_room(commands: &mut Commands, camera: &Query<Entity, With<MainCamera>>, ambient: &mut AmbientLight) {
    if let Ok(cam) = camera.single() {
        commands.entity(cam).remove::<DistanceFog>();
    }
    *ambient = AmbientLight::default();
}

/// The round table with felt of the given colour, the floor, a rug, the lamp
/// and the fill lights, all as children of `root`.
pub fn spawn_room(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
    root: Entity,
    felt: Color,
) {
    let felt = materials.add(StandardMaterial {
        base_color: felt,
        perceptual_roughness: 0.96,
        reflectance: 0.1,
        ..default()
    });
    let wood = materials.add(StandardMaterial {
        base_color: Color::srgb(0.36, 0.2, 0.11),
        perceptual_roughness: 0.38,
        reflectance: 0.45,
        ..default()
    });
    let dark_wood = materials.add(StandardMaterial {
        base_color: Color::srgb(0.16, 0.1, 0.07),
        perceptual_roughness: 0.6,
        ..default()
    });
    let floor = materials.add(StandardMaterial {
        base_color: Color::srgb(0.17, 0.13, 0.12),
        perceptual_roughness: 0.92,
        ..default()
    });
    let rug = materials.add(StandardMaterial {
        base_color: Color::srgb(0.32, 0.12, 0.12),
        perceptual_roughness: 1.0,
        ..default()
    });

    let spawn = |commands: &mut Commands, bundle: (Mesh3d, MeshMaterial3d<StandardMaterial>, Transform)| {
        let e = commands.spawn(bundle).id();
        commands.entity(root).add_child(e);
    };
    spawn(commands, (
        Mesh3d(meshes.add(Cylinder::new(TABLE_R, 0.12).mesh().resolution(96))),
        MeshMaterial3d(felt),
        Transform::from_xyz(0.0, -0.06, 0.0),
    ));
    spawn(commands, (
        Mesh3d(meshes.add(Torus::new(TABLE_R - 0.02, TABLE_R + 0.26).mesh().minor_resolution(24).major_resolution(128))),
        MeshMaterial3d(wood.clone()),
        Transform::from_xyz(0.0, -0.02, 0.0).with_scale(Vec3::new(1.0, 0.55, 1.0)),
    ));
    spawn(commands, (
        Mesh3d(meshes.add(Cylinder::new(TABLE_R + 0.2, 0.22).mesh().resolution(96))),
        MeshMaterial3d(wood),
        Transform::from_xyz(0.0, -0.2, 0.0),
    ));
    spawn(commands, (
        Mesh3d(meshes.add(Cylinder::new(0.55, 1.6))),
        MeshMaterial3d(dark_wood.clone()),
        Transform::from_xyz(0.0, -1.1, 0.0),
    ));
    spawn(commands, (
        Mesh3d(meshes.add(Cylinder::new(1.4, 0.12))),
        MeshMaterial3d(dark_wood),
        Transform::from_xyz(0.0, -1.86, 0.0),
    ));
    spawn(commands, (
        Mesh3d(meshes.add(Plane3d::default().mesh().size(60.0, 60.0))),
        MeshMaterial3d(floor),
        Transform::from_xyz(0.0, -1.92, 0.0),
    ));
    // A round rug under the table.
    spawn(commands, (
        Mesh3d(meshes.add(Cylinder::new(6.5, 0.02).mesh().resolution(96))),
        MeshMaterial3d(rug),
        Transform::from_xyz(0.0, -1.91, 0.0),
    ));

    // Lights: a warm lamp above the table plus soft fill.
    let lamp = commands
        .spawn((
            SpotLight {
                intensity: 9_000_000.0,
                color: Color::srgb(1.0, 0.9, 0.75),
                range: 30.0,
                radius: 0.3,
                shadows_enabled: true,
                outer_angle: 0.95,
                inner_angle: 0.55,
                ..default()
            },
            Transform::from_xyz(0.0, 7.5, 0.4).looking_at(Vec3::ZERO, Vec3::Z),
        ))
        .id();
    commands.entity(root).add_child(lamp);
    let shade_mat = materials.add(StandardMaterial {
        base_color: Color::srgb(0.12, 0.1, 0.08),
        emissive: LinearRgba::rgb(0.6, 0.45, 0.25),
        perceptual_roughness: 0.5,
        cull_mode: None,
        ..default()
    });
    let shade = commands
        .spawn((
            Mesh3d(meshes.add(ConicalFrustum {
                radius_top: 0.25,
                radius_bottom: 0.9,
                height: 0.6,
            })),
            MeshMaterial3d(shade_mat),
            Transform::from_xyz(0.0, 7.9, 0.4),
            NotShadowCaster,
        ))
        .id();
    commands.entity(root).add_child(shade);
    for (color, intensity, pos) in [
        (Color::srgb(0.65, 0.75, 1.0), 1_400_000.0, Vec3::new(-6.0, 5.0, 6.0)),
        (Color::srgb(1.0, 0.7, 0.5), 900_000.0, Vec3::new(5.0, 3.0, -7.0)),
    ] {
        let light = commands
            .spawn((
                PointLight {
                    intensity,
                    color,
                    range: 40.0,
                    ..default()
                },
                Transform::from_translation(pos),
            ))
            .id();
        commands.entity(root).add_child(light);
    }
}

/// A person sitting at the table (`seat` is the game seat, or `usize::MAX`
/// for the dealer). `base` is the chair; its local -Z faces the table.
#[derive(Component)]
pub struct Figure {
    pub seat: usize,
    pub base: Transform,
}

#[derive(Component)]
pub struct FigureHead;

/// Meshes and materials shared by all figures.
pub struct FigureKit {
    body: Handle<Mesh>,
    head: Handle<Mesh>,
    eye: Handle<Mesh>,
    arm: Handle<Mesh>,
    seat: Handle<Mesh>,
    back: Handle<Mesh>,
    leg: Handle<Mesh>,
    skin: Handle<StandardMaterial>,
    eye_mat: Handle<StandardMaterial>,
    chair: Handle<StandardMaterial>,
}

/// Where the head sits relative to the figure's base.
pub const HEAD_OFFSET: Vec3 = Vec3::new(0.0, 1.62, 0.22);

impl FigureKit {
    pub fn new(meshes: &mut Assets<Mesh>, materials: &mut Assets<StandardMaterial>) -> Self {
        FigureKit {
            body: meshes.add(Capsule3d::new(0.42, 0.75)),
            head: meshes.add(Sphere::new(0.36).mesh().uv(32, 18)),
            eye: meshes.add(Sphere::new(0.05).mesh().uv(12, 8)),
            arm: meshes.add(Capsule3d::new(0.11, 0.55)),
            seat: meshes.add(Cuboid::new(1.1, 0.12, 0.95)),
            back: meshes.add(Cuboid::new(1.1, 1.3, 0.12)),
            leg: meshes.add(Cuboid::new(0.09, 1.0, 0.09)),
            skin: materials.add(StandardMaterial {
                base_color: Color::srgb(0.96, 0.8, 0.66),
                perceptual_roughness: 0.7,
                ..default()
            }),
            eye_mat: materials.add(StandardMaterial {
                base_color: Color::srgb(0.05, 0.05, 0.07),
                perceptual_roughness: 0.2,
                ..default()
            }),
            chair: materials.add(StandardMaterial {
                base_color: Color::srgb(0.24, 0.14, 0.09),
                perceptual_roughness: 0.55,
                ..default()
            }),
        }
    }

    /// Spawns a seated figure at `pos` (on the floor ring around the table)
    /// facing the table centre, wearing `shirt`. With `chair: false` the
    /// figure stands (the dealer).
    pub fn spawn(&self, commands: &mut Commands, pos: Vec3, seat: usize, shirt: Handle<StandardMaterial>, chair: bool) -> Entity {
        let facing = Transform::from_translation(pos).looking_at(Vec3::new(0.0, 0.0, 0.0), Vec3::Y);
        let base = Transform::from_translation(pos + Vec3::Y * -0.95).with_rotation(facing.rotation);
        commands
            .spawn((Figure { seat, base }, base, Visibility::default()))
            .with_children(|p| {
                if chair {
                    p.spawn((Mesh3d(self.seat.clone()), MeshMaterial3d(self.chair.clone()), Transform::from_xyz(0.0, -0.1, 0.25)));
                    p.spawn((Mesh3d(self.back.clone()), MeshMaterial3d(self.chair.clone()), Transform::from_xyz(0.0, 0.55, 0.72)));
                    for (x, z) in [(-0.48, -0.15), (0.48, -0.15), (-0.48, 0.65), (0.48, 0.65)] {
                        p.spawn((Mesh3d(self.leg.clone()), MeshMaterial3d(self.chair.clone()), Transform::from_xyz(x, -0.66, z)));
                    }
                }
                p.spawn((Mesh3d(self.body.clone()), MeshMaterial3d(shirt.clone()), Transform::from_xyz(0.0, 0.62, 0.25)));
                for side in [-1.0f32, 1.0] {
                    p.spawn((
                        Mesh3d(self.arm.clone()),
                        MeshMaterial3d(shirt.clone()),
                        Transform::from_xyz(side * 0.48, 0.72, -0.05)
                            .with_rotation(Quat::from_rotation_x(1.05) * Quat::from_rotation_z(side * -0.25)),
                    ));
                }
                p.spawn((
                    FigureHead,
                    Mesh3d(self.head.clone()),
                    MeshMaterial3d(self.skin.clone()),
                    Transform::from_translation(HEAD_OFFSET),
                ))
                .with_children(|h| {
                    for side in [-1.0f32, 1.0] {
                        h.spawn((
                            Mesh3d(self.eye.clone()),
                            MeshMaterial3d(self.eye_mat.clone()),
                            Transform::from_xyz(side * 0.12, 0.05, -0.32),
                        ));
                    }
                });
            })
            .id()
    }
}

/// Head rotation (relative to the figure) that looks at `target`.
pub fn head_look(base: &Transform, target: Vec3) -> Quat {
    let world_head = base.transform_point(HEAD_OFFSET);
    let to = (target - world_head).normalize_or_zero();
    let local = base.rotation.inverse() * to;
    let yaw = (-local.x).atan2(-local.z).clamp(-0.9, 0.9);
    let pitch = local.y.asin().clamp(-0.5, 0.3);
    Quat::from_rotation_y(yaw) * Quat::from_rotation_x(pitch)
}
