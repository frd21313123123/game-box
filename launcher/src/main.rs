//! Game Box — a launcher with table games: «Монополия», «Уно» and «Блэкджек».

// Hide the console window on Windows release builds.
#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

mod binds;
mod blackjack;
mod debug;
mod lobby;
mod menu;
mod monopoly;
mod net;
mod profile;
mod sfx;
mod steam;
mod table3d;
mod theme;
mod uno;
mod update;

use bevy::asset::AssetEvents;
use bevy::prelude::*;
use bevy::window::WindowResolution;
use bevy_egui::{EguiContexts, EguiPlugin, EguiPostUpdateSet, EguiPrimaryContextPass};

/// Top-level screens of the launcher.
#[derive(States, Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum AppState {
    #[default]
    Menu,
    Lobby,
    Uno,
    Monopoly,
    Blackjack,
}

/// The single camera of the app. Menus and Monopoly draw with egui on top of
/// it; the card games move it to look at their 3D tables.
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
            blackjack::BlackjackPlugin,
            monopoly::MonopolyPlugin,
            debug::DebugPlugin,
            update::UpdatePlugin,
        ))
        // Whenever egui rasterises new glyphs (new text, a new font size), bevy_egui
        // replaces the font atlas with a new `Image`. Unless that happens before
        // `AssetEvents`, the render world only learns about the new image a frame
        // later, finds no GPU texture for it and skips every egui draw: the whole
        // HUD blinked out for a frame each time something new appeared on it.
        .configure_sets(PostUpdate, EguiPostUpdateSet::PostProcessOutput.before(AssetEvents))
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
