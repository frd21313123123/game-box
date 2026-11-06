//! «Монополия» — 2D board drawn with egui, ported from the user's
//! TypeScript/React version (board layout, colours, rules and dialogs).

mod board;
mod panels;

use std::collections::HashMap;

use bevy::prelude::*;
use bevy_egui::{EguiContexts, EguiPrimaryContextPass};
use monopoly_core::{bot_action, GameState, LogKind, Phase, PlayerId, BOARD_SIZE};

use crate::net::{SeatKind, Session};
use crate::sfx::Sfx;
use crate::steam::SteamApi;
use crate::AppState;

pub struct MonopolyPlugin;

impl Plugin for MonopolyPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(OnEnter(AppState::Monopoly), reset)
            .add_systems(Update, (drive_bots, animate).chain().run_if(in_state(AppState::Monopoly)))
            .add_systems(
                EguiPrimaryContextPass,
                draw.run_if(in_state(AppState::Monopoly)).run_if(resource_exists::<crate::ThemeReady>),
            );
    }
}

pub const DICE_ROLL_SECS: f32 = 0.75;
const STEP_SECS: f32 = 0.16;

/// Client-side animation and dialog state.
#[derive(Resource, Default)]
pub struct MonoUi {
    /// Displayed (possibly mid-animation) tile of every pawn.
    pub pawn_pos: HashMap<PlayerId, f32>,
    /// Pending hops for the moving pawn: tile indices to visit.
    pub path: Vec<usize>,
    pub moving: Option<PlayerId>,
    pub hop: f32,
    pub last_move_seq: u32,
    pub roll_seq: u32,
    /// Time left on the dice animation.
    pub dice_anim: f32,
    pub dice_faces: (u8, u8),
    pub log_seq: u32,
    /// Card drawn (deck name, text, who) shown in the centre of the board.
    pub card_popup: Option<(String, String, String, f32)>,
    pub hovered_tile: Option<usize>,
    pub trade: Option<panels::TradeDraft>,
    pub offer: Option<panels::OfferDraft>,
    pub bid: i32,
    pub confirm_leave: bool,
    pub last_turn_key: Option<(u32, usize)>,
    pub last_money: HashMap<PlayerId, i32>,
    initialized: bool,
}

impl MonoUi {
    pub fn busy(&self) -> bool {
        self.dice_anim > 0.0 || self.moving.is_some()
    }
}

#[derive(Resource, Default)]
struct BotTimer(f32);

fn reset(mut commands: Commands) {
    commands.insert_resource(MonoUi::default());
    commands.insert_resource(BotTimer::default());
}

fn drive_bots(
    time: Res<Time>,
    mut session: ResMut<Session>,
    mut timer: ResMut<BotTimer>,
    ui: Res<MonoUi>,
) {
    if !session.is_host {
        return;
    }
    let Some(state) = session.monopoly.as_ref() else { return };
    if state.phase != Phase::Playing {
        return;
    }
    // Let the dice and pawns finish before the next bot move.
    if ui.busy() || ui.card_popup.as_ref().is_some_and(|c| c.3 > 2.0) {
        timer.0 = 0.0;
        return;
    }
    timer.0 += time.delta_secs();
    if timer.0 < 0.75 {
        return;
    }
    let autoplay = std::env::var_os("GAMEBOX_AUTOPLAY").is_some();
    let bots: Vec<PlayerId> = session
        .lobby
        .seats
        .iter()
        .enumerate()
        .filter(|(_, s)| autoplay || s.kind == SeatKind::Bot)
        .map(|(i, _)| session.monopoly_player_id(i))
        .collect();
    let mut acted = false;
    for id in bots {
        let Some(state) = session.monopoly.as_ref() else { return };
        if let Some(action) = bot_action(state, id) {
            if session.host_apply_monopoly(id, &action) {
                acted = true;
                break;
            }
            warn!("Бот {id}: действие {action:?} отклонено");
        }
    }
    if acted {
        timer.0 = 0.0;
    }
}

/// Drives dice and pawn animations from the latest state.
fn animate(time: Res<Time>, session: Res<Session>, mut ui: ResMut<MonoUi>, mut sfx: EventWriter<Sfx>) {
    let Some(state) = session.monopoly.as_ref() else { return };
    let dt = time.delta_secs();
    if !ui.initialized {
        ui.initialized = true;
        ui.roll_seq = state.roll_seq;
        ui.last_move_seq = state.last_move.as_ref().map(|m| m.seq).unwrap_or(0);
        ui.log_seq = state.log_seq;
        for p in &state.players {
            ui.pawn_pos.insert(p.id, p.position as f32);
            ui.last_money.insert(p.id, p.money);
        }
    }

    if state.roll_seq != ui.roll_seq {
        ui.roll_seq = state.roll_seq;
        ui.dice_anim = DICE_ROLL_SECS;
        sfx.write(Sfx::Dice);
    }
    if ui.dice_anim > 0.0 {
        ui.dice_anim -= dt;
        let k = (time.elapsed_secs() * 14.0) as u32;
        ui.dice_faces = ((k * 7 % 6 + 1) as u8, (k * 13 % 6 + 1) as u8);
    }

    // A new move: queue every tile the pawn walks through.
    if let Some(m) = &state.last_move {
        if m.seq != ui.last_move_seq {
            ui.last_move_seq = m.seq;
            let mut cur = ui.pawn_pos.get(&m.player).copied().unwrap_or(0.0).round() as usize;
            let mut path = Vec::new();
            for &wp in &m.path {
                let forward = (wp + BOARD_SIZE - cur) % BOARD_SIZE;
                if forward > 0 && forward <= 12 {
                    for k in 1..=forward {
                        path.push((cur + k) % BOARD_SIZE);
                    }
                } else if wp != cur {
                    path.push(wp); // jump (to jail, or a card sending you back)
                }
                cur = wp;
            }
            if let Some(&current) = ui.pawn_pos.get(&m.player) {
                ui.pawn_pos.insert(m.player, current.round());
            }
            ui.path = path;
            ui.moving = Some(m.player);
            ui.hop = 0.0;
        }
    }
    if let Some(id) = ui.moving {
        if ui.dice_anim <= 0.0 {
            ui.hop += dt / STEP_SECS;
            if ui.hop >= 1.0 {
                ui.hop = 0.0;
                if !ui.path.is_empty() {
                    let next = ui.path.remove(0);
                    ui.pawn_pos.insert(id, next as f32);
                    sfx.write(Sfx::Step);
                }
                if ui.path.is_empty() {
                    ui.moving = None;
                }
            }
        }
    }
    // Everyone not animating sits on their real tile.
    for p in &state.players {
        if ui.moving != Some(p.id) {
            ui.pawn_pos.insert(p.id, p.position as f32);
        }
    }

    // Card popups and sounds from new log entries.
    if state.log_seq != ui.log_seq {
        let new = (state.log_seq - ui.log_seq) as usize;
        ui.log_seq = state.log_seq;
        let start = state.log.len().saturating_sub(new);
        for e in &state.log[start..] {
            match e.kind {
                LogKind::DrewCard => {
                    if let Some((deck, text)) = &e.card {
                        let who = e.player.and_then(|id| state.player(id)).map(|p| p.name.clone()).unwrap_or_default();
                        ui.card_popup = Some((deck.name().to_string(), text.clone(), who, 0.0));
                    }
                }
                LogKind::Bought | LogKind::AuctionWon | LogKind::OfferAccepted => {
                    sfx.write(Sfx::Coin);
                }
                LogKind::GameWon => {
                    sfx.write(Sfx::Win);
                }
                LogKind::GoToJail | LogKind::ThirdDoubleJail | LogKind::Bankrupt => {
                    sfx.write(Sfx::Caught);
                }
                _ => {}
            }
        }
    }
    if let Some(c) = &mut ui.card_popup {
        c.3 += dt;
        if c.3 > 6.0 {
            ui.card_popup = None;
        }
    }

    // Turn chime for players at this computer.
    let key = (state.turn, state.current);
    if ui.last_turn_key != Some(key) {
        let first = ui.last_turn_key.is_none();
        ui.last_turn_key = Some(key);
        let locals = session.local_monopoly_ids();
        if !first && state.current_player().is_some_and(|p| locals.contains(&p.id)) {
            sfx.write(Sfx::Turn);
        }
    }
}

/// Which player's perspective the screen shows: the current player when they
/// sit at this computer (hot-seat), otherwise this computer's own player.
pub fn viewer(state: &GameState, session: &Session) -> Option<PlayerId> {
    let locals = session.local_monopoly_ids();
    if let Some(cur) = state.current_player() {
        if locals.contains(&cur.id) {
            return Some(cur.id);
        }
    }
    locals.first().copied()
}

#[allow(clippy::too_many_arguments)]
fn draw(
    mut contexts: EguiContexts,
    mut session: ResMut<Session>,
    mut ui: ResMut<MonoUi>,
    steam: Res<SteamApi>,
    mut next: ResMut<NextState<AppState>>,
    time: Res<Time>,
) -> Result {
    let ctx = contexts.ctx_mut()?.clone();
    let Some(state) = session.monopoly.clone() else {
        bevy_egui::egui::CentralPanel::default().show(&ctx, |ui| {
            ui.centered_and_justified(|ui| ui.label("Ожидаем данные от хоста…"));
        });
        return Ok(());
    };
    let leave = panels::sidebar(&ctx, &state, &mut session, &mut ui);
    board::central(&ctx, &state, &session, &mut ui, time.elapsed_secs());
    panels::dialogs(&ctx, &state, &mut session, &mut ui);
    if leave || ui.confirm_leave {
        ui.confirm_leave = true;
        if panels::confirm_leave(&ctx, &session, &mut ui) {
            session.leave(&steam);
            next.set(AppState::Menu);
        }
    }
    Ok(())
}
