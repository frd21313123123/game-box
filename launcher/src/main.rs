//! Game Box — a launcher with two table games: «Монополия» and «Последняя карта».

// Hide the console window on Windows release builds.
#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

mod debug;
mod lobby;
mod menu;
mod monopoly;
mod net;
mod profile;
mod sfx;
mod steam;
mod theme;
mod uno;

use bevy::prelude::*;
use bevy::window::WindowResolution;
use bevy_egui::{EguiContexts, EguiPlugin, EguiPrimaryContextPass};

/// Top-level screens of the launcher.
#[derive(States, Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum AppState {
    #[default]
    Menu,
    Lobby,
    Uno,
    Monopoly,
}

/// The single camera of the app. Menus and Monopoly draw with egui on top of
/// it; the card game moves it to look at its 3D table.
#[derive(Component)]
pub struct MainCamera;

fn main() {
    App::new()
        .add_plugins(
            DefaultPlugins
                .set(WindowPlugin {
                    primary_window: Some(Window {
                        title: "Game Box".into(),
                        resolution: WindowResolution::new(1440.0, 900.0),
                        ..default()
                    }),
                    ..default()
                })
                .set(bevy::log::LogPlugin {
                    filter: "wgpu=error,naga=warn,bevy_render=warn,bevy_egui=warn".into(),
                    ..default()
                }),
        )
        .add_plugins(EguiPlugin::default())
        .init_state::<AppState>()
        .insert_resource(ClearColor(theme::BACKGROUND_3D))
        .add_plugins((
            steam::SteamPlugin,
            profile::ProfilePlugin,
            net::NetPlugin,
            sfx::SfxPlugin,
            menu::MenuPlugin,
            lobby::LobbyPlugin,
            uno::UnoPlugin,
            monopoly::MonopolyPlugin,
            debug::DebugPlugin,
        ))
        .add_systems(Startup, spawn_camera)
        .add_systems(EguiPrimaryContextPass, install_theme)
        .run();
}

fn spawn_camera(mut commands: Commands) {
    commands.spawn((
        MainCamera,
        Camera3d::default(),
        Projection::Perspective(PerspectiveProjection {
            fov: 42f32.to_radians(),
            ..default()
        }),
        Transform::from_xyz(0.0, 6.0, 8.0).looking_at(Vec3::ZERO, Vec3::Y),
    ));
}

/// Present once the custom fonts are active (egui applies new fonts on the
/// frame after they are set, so UI systems wait for this).
#[derive(Resource)]
pub struct ThemeReady;

/// Fonts and visuals are installed once, on the first egui frame.
fn install_theme(mut commands: Commands, mut contexts: EguiContexts, mut step: Local<u8>) -> Result {
    match *step {
        0 => {
            theme::install(contexts.ctx_mut()?);
            *step = 1;
        }
        1 => {
            commands.insert_resource(ThemeReady);
            *step = 2;
        }
        _ => {}
    }
    Ok(())
}
