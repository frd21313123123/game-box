//! Developer helpers driven by environment variables:
//! `GAMEBOX_AUTOSTART=uno|monopoly` starts a local game with bots right away;
//! `GAMEBOX_SCREENSHOT=path.png` (with `GAMEBOX_SCREENSHOT_AFTER=seconds`)
//! saves a screenshot and quits.

use bevy::prelude::*;
use bevy::render::view::screenshot::{save_to_disk, Screenshot};

use crate::net::{GameKind, Session};
use crate::profile::Profile;
use crate::AppState;

pub struct DebugPlugin;

impl Plugin for DebugPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Update, (autostart, screenshot));
    }
}

fn autostart(
    mut done: Local<bool>,
    mut session: ResMut<Session>,
    profile: Res<Profile>,
    mut next: ResMut<NextState<AppState>>,
) {
    if *done {
        return;
    }
    *done = true;
    let Ok(game) = std::env::var("GAMEBOX_AUTOSTART") else { return };
    let kind = match game.as_str() {
        "uno" => GameKind::Uno,
        "monopoly" => GameKind::Monopoly,
        "lobby-uno" | "lobby-monopoly" => {
            let kind = if game.ends_with("uno") { GameKind::Uno } else { GameKind::Monopoly };
            session.host(kind, &profile);
            next.set(AppState::Lobby);
            return;
        }
        _ => return,
    };
    session.host(kind, &profile);
    if let Ok(n) = std::env::var("GAMEBOX_BOTS").map(|s| s.parse::<usize>().unwrap_or(3)) {
        while session.lobby.seats.len() > 1 + n {
            let id = session.lobby.seats.last().unwrap().id;
            session.remove_seat(id);
        }
        while session.lobby.seats.len() < 1 + n {
            session.add_bot();
        }
    }
    if let Err(e) = session.start_game() {
        error!("autostart failed: {e}");
    }
}

fn screenshot(mut commands: Commands, time: Res<Time>, mut state: Local<u8>, mut exit: EventWriter<AppExit>) {
    let Ok(path) = std::env::var("GAMEBOX_SCREENSHOT") else { return };
    let after: f32 = std::env::var("GAMEBOX_SCREENSHOT_AFTER")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(4.0);
    let t = time.elapsed_secs();
    if *state == 0 && t > after {
        commands.spawn(Screenshot::primary_window()).observe(save_to_disk(path));
        *state = 1;
    } else if *state == 1 && t > after + 1.5 {
        exit.write(AppExit::Success);
        *state = 2;
    }
}
