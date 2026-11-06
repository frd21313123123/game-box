//! Developer helpers driven by environment variables:
//! `GAMEBOX_AUTOSTART=uno|monopoly|blackjack` starts a local game with bots right away;
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
        app.add_systems(Update, (autostart, screenshot, delayed_start));
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
    if let Ok(addr) = std::env::var("GAMEBOX_JOIN") {
        match session.join_tcp(&addr, GameKind::Uno, &profile) {
            Ok(()) => next.set(AppState::Lobby),
            Err(e) => error!("join failed: {e}"),
        }
        return;
    }
    let Ok(game) = std::env::var("GAMEBOX_AUTOSTART") else { return };
    let kind = match game.as_str() {
        "uno" => GameKind::Uno,
        "monopoly" => GameKind::Monopoly,
        "blackjack" => GameKind::Blackjack,
        "lobby-uno" | "lobby-monopoly" | "lobby-blackjack" => {
            let kind = match game.as_str() {
                "lobby-uno" => GameKind::Uno,
                "lobby-blackjack" => GameKind::Blackjack,
                _ => GameKind::Monopoly,
            };
            session.host(kind, &profile);
            if std::env::var_os("GAMEBOX_OPEN_TCP").is_some() {
                if let Err(e) = session.open_tcp() {
                    error!("open failed: {e}");
                }
            }
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

/// `GAMEBOX_START_AFTER=seconds`: the host starts the lobby's game after a delay.
fn delayed_start(time: Res<Time>, mut session: ResMut<Session>, mut done: Local<bool>) {
    if *done {
        return;
    }
    let Some(after) = std::env::var("GAMEBOX_START_AFTER").ok().and_then(|s| s.parse::<f32>().ok()) else {
        return;
    };
    if time.elapsed_secs() > after && session.is_host && !session.lobby.started {
        *done = true;
        if let Err(e) = session.start_game() {
            error!("start failed: {e}");
        }
    }
}
