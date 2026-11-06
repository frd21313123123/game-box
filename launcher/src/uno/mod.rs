//! «Уно» — a shedding card game in the classic four-colour style,
//! played at a 3D table.

pub mod binds;
mod art;
mod hud;
mod keys;
mod scene;

use std::collections::VecDeque;

use bevy::prelude::*;
use bevy_egui::EguiPrimaryContextPass;
use uno_core::{bot_action, bot_catch, Phase, Rng, UnoAction};

use crate::net::Session;
use crate::AppState;

pub use scene::color32;

/// Client-side UI state of the card game.
#[derive(Resource, Default)]
pub struct UnoUi {
    pub hovered: Option<u16>,
    pub deck_hovered: bool,
    /// "Last card!" pressed before playing the second-to-last card.
    pub uno_armed: bool,
    pub modal_open: bool,
    pub last_played_by: Option<usize>,
    pub confirm_leave: bool,
    pub show_scores: bool,
    /// Settings dialog with the keyboard controls.
    pub show_settings: bool,
    /// Action waiting for a key press in the settings dialog.
    pub capture: Option<binds::Action>,
    /// Card selected with the keyboard (highlighted in the hand).
    pub cursor: Option<u16>,
    hud_seq: u64,
    feed: VecDeque<hud::FeedItem>,
    bubbles: Vec<hud::Bubble>,
    banner: Option<hud::Banner>,
}

/// Paces the bots on the host so humans can follow what happens.
#[derive(Resource)]
struct BotClock {
    act: f32,
    catch: f32,
    round: u32,
    acting: Option<(usize, u64)>,
    /// The open «last card!» window: who forgot, and at which game event.
    vuln: Option<(usize, u64)>,
    /// Seconds the current window has been open.
    vuln_open: f32,
    /// Seconds a bot waits before it notices the window (chosen per window).
    vuln_delay: f32,
    rng: Rng,
}

/// Bots notice a forgotten «last card!» only after this many seconds...
const CATCH_DELAY_MIN: f32 = 2.5;
/// ...plus up to this many more, picked at random for each window.
const CATCH_DELAY_SPREAD: f32 = 1.5;
/// Chance per check (every 0.45 s) that a bot who has noticed actually catches.
const CATCH_CHANCE: f32 = 0.2;

impl Default for BotClock {
    fn default() -> Self {
        BotClock {
            act: 0.0,
            catch: 0.0,
            round: 0,
            acting: None,
            vuln: None,
            vuln_open: 0.0,
            vuln_delay: 0.0,
            rng: Rng::new(
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map(|d| d.as_nanos() as u64)
                    .unwrap_or(7),
            ),
        }
    }
}

pub struct UnoPlugin;

impl Plugin for UnoPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Startup, scene::setup_assets)
            .add_systems(OnEnter(AppState::Uno), (scene::spawn_table, reset_ui))
            .add_systems(OnExit(AppState::Uno), scene::despawn_table)
            .add_systems(
                Update,
                (
                    drive_bots,
                    keys::keyboard_input,
                    scene::hand_input,
                    scene::sync_table,
                    scene::layout_cards,
                    scene::animate_cards,
                    scene::fade_old_cards,
                    scene::animate_table,
                )
                    .chain()
                    .run_if(in_state(AppState::Uno)),
            )
            .add_systems(EguiPrimaryContextPass, hud::draw_hud.run_if(in_state(AppState::Uno)).run_if(resource_exists::<crate::ThemeReady>));
    }
}

fn reset_ui(mut commands: Commands) {
    commands.insert_resource(UnoUi::default());
    commands.insert_resource(BotClock::default());
}

fn drive_bots(time: Res<Time>, mut session: ResMut<Session>, mut clock: ResMut<BotClock>) {
    if !session.is_host {
        return;
    }
    let Some(game) = session.uno.as_ref() else { return };
    let dt = time.delta_secs();
    // Developer aid: GAMEBOX_AUTOPLAY lets a bot play the local seat too.
    let autoplay = std::env::var_os("GAMEBOX_AUTOPLAY").is_some();
    let is_bot = |session: &Session, seat: usize| autoplay || session.seat_is_bot(seat);

    // Give the deal animation time to finish at the start of each round.
    if game.round != clock.round {
        clock.round = game.round;
        clock.act = -2.2;
        clock.acting = None;
    }

    // Time the «last card!» window: bots react slowly to a forgotten call.
    let window = game.uno_vulnerable.map(|t| (t, game.event_seq));
    if window != clock.vuln {
        clock.vuln = window;
        clock.vuln_open = 0.0;
        clock.vuln_delay = CATCH_DELAY_MIN + clock.rng.next_f32() * CATCH_DELAY_SPREAD;
    }
    clock.vuln_open += dt;

    // Catching a forgotten "last card!" call.
    clock.catch += dt;
    if clock.catch >= 0.45 {
        clock.catch = 0.0;
        if let Some(target) = game.uno_vulnerable {
            let n = game.players.len();
            let mut action = None;
            if is_bot(&session, target) && clock.rng.chance(0.3) {
                // The bot remembers to call it itself.
                action = Some((target, UnoAction::CallUno));
            } else if clock.vuln_open >= clock.vuln_delay {
                for seat in 0..n {
                    if seat != target && is_bot(&session, seat) {
                        let mut rng = clock.rng;
                        if let Some(a) = bot_catch(game, seat, &mut rng, CATCH_CHANCE) {
                            action = Some((seat, a));
                        }
                        clock.rng = rng;
                        if action.is_some() {
                            break;
                        }
                    }
                }
            }
            if let Some((seat, a)) = action {
                session.host_apply_uno(seat, a);
                return;
            }
        }
    }

    let Some(game) = session.uno.as_ref() else { return };
    if matches!(game.phase, Phase::RoundOver { .. }) && autoplay {
        session.host_apply_uno(0, UnoAction::NextRound);
        return;
    }
    if matches!(game.phase, Phase::RoundOver { .. } | Phase::GameOver { .. }) {
        return;
    }
    let Some(seat) = game.acting_seat() else { return };
    if !is_bot(&session, seat) {
        clock.acting = None;
        return;
    }
    // Restart the think timer whenever the situation changes.
    let key = (seat, game.event_seq);
    if clock.acting != Some(key) {
        clock.acting = Some(key);
        let think = match game.phase {
            Phase::ChooseColor { .. } => 0.7,
            Phase::Challenge { .. } => 1.1,
            Phase::AfterDraw { .. } => 0.8,
            _ => 1.0 + clock.rng.next_f32() * 0.7,
        };
        clock.act = clock.act.min(0.0) - think;
    }
    clock.act += dt;
    if clock.act < 0.0 {
        return;
    }
    let mut rng = clock.rng;
    let action = bot_action(game, seat, &mut rng);
    clock.rng = rng;
    clock.act = 0.0;
    clock.acting = None;
    if let Some(action) = action {
        session.host_apply_uno(seat, action);
    }
}
