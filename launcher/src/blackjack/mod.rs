//! «Блэкджек» — the casino card game «21» against the dealer, at the same 3D
//! table room as Uno.

pub mod art;
pub mod binds;
mod hud;
mod keys;
mod scene;

use std::collections::VecDeque;

use bevy::prelude::*;
use bevy_egui::EguiPrimaryContextPass;
use blackjack_core::{bot_action, BjAction, BjEvent, Phase, Rng};

use crate::net::Session;
use crate::AppState;

/// Client-side UI state of the Blackjack table.
#[derive(Resource, Default)]
pub struct BjUi {
    /// The bet being prepared in the betting phase (0 until first shown).
    pub bet: u32,
    pub confirm_leave: bool,
    pub show_scores: bool,
    pub show_settings: bool,
    /// Action waiting for a key press in the settings dialog.
    pub capture: Option<binds::Action>,
    hud_seq: u64,
    /// The dealer's hole card was turned (later dealer cards are draws).
    hole_seen: bool,
    feed: VecDeque<hud::FeedItem>,
    bubbles: Vec<hud::Bubble>,
    banner: Option<hud::Banner>,
}

/// Paces the bots and the dealer on the host so people can follow the cards.
#[derive(Resource)]
struct BjClock {
    seq: u64,
    wait: f32,
    rng: Rng,
}

impl Default for BjClock {
    fn default() -> Self {
        BjClock {
            seq: 0,
            wait: 1.0,
            rng: Rng::new(
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map(|d| d.as_nanos() as u64)
                    .unwrap_or(21),
            ),
        }
    }
}

pub struct BlackjackPlugin;

impl Plugin for BlackjackPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Startup, scene::setup_assets)
            .add_systems(OnEnter(AppState::Blackjack), (scene::spawn_table, reset_ui))
            .add_systems(OnExit(AppState::Blackjack), scene::despawn_table)
            .add_systems(
                Update,
                (
                    drive_table,
                    keys::keyboard_input,
                    scene::sync_table,
                    crate::table3d::animate_motion,
                    scene::animate_table,
                )
                    .chain()
                    .run_if(in_state(AppState::Blackjack)),
            )
            .add_systems(
                EguiPrimaryContextPass,
                hud::draw_hud
                    .run_if(in_state(AppState::Blackjack))
                    .run_if(resource_exists::<crate::ThemeReady>),
            );
    }
}

fn reset_ui(mut commands: Commands) {
    commands.insert_resource(BjUi::default());
    commands.insert_resource(BjClock::default());
}

/// Host: plays the bots' seats and the dealer, each after a short pause that
/// grows with the number of cards still flying across the table.
fn drive_table(time: Res<Time>, mut session: ResMut<Session>, mut clock: ResMut<BjClock>) {
    if !session.is_host {
        return;
    }
    let Some(game) = session.blackjack.as_ref() else { return };
    // Developer aid: GAMEBOX_AUTOPLAY lets a bot play the local seat too.
    let autoplay = std::env::var_os("GAMEBOX_AUTOPLAY").is_some();

    if game.event_seq != clock.seq {
        let dealt = game
            .events_after(clock.seq)
            .filter(|e| matches!(e.event, BjEvent::Dealt { .. }))
            .count();
        clock.seq = game.event_seq;
        let think = match game.phase {
            Phase::DealerTurn => 0.85,
            Phase::Betting => 0.35 + clock.rng.next_f32() * 0.5,
            Phase::RoundOver => 2.5,
            _ => 0.7 + clock.rng.next_f32() * 0.6,
        };
        clock.wait = think + dealt as f32 * 0.22;
    }
    clock.wait -= time.delta_secs();
    if clock.wait > 0.0 {
        return;
    }
    clock.wait = 0.25;

    match game.phase {
        Phase::DealerTurn => {
            session.host_dealer_step();
        }
        Phase::RoundOver => {
            if autoplay {
                if let Some(i) = session.my_index() {
                    session.host_apply_blackjack(i, BjAction::NextRound);
                }
            }
        }
        Phase::GameOver { .. } => {}
        _ => {
            let me = session.my_index();
            let bot = (0..game.player_count())
                .find(|s| (session.seat_is_bot(*s) || (autoplay && Some(*s) == me)) && game.is_waiting_for(*s));
            let Some(seat) = bot else { return };
            let mut rng = clock.rng;
            let action = bot_action(game, seat, &mut rng);
            clock.rng = rng;
            if let Some(action) = action {
                session.host_apply_blackjack(seat, action);
            }
        }
    }
}
