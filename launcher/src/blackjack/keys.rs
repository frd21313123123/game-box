//! Keyboard control of the Blackjack table. Keys come from the player's
//! bindings (see [`super::binds`]).

use bevy::prelude::*;
use blackjack_core::{BjAction, BjGame, Phase, BET_STEP, MAX_BET, MIN_BET};

use super::binds::Action;
use super::BjUi;
use crate::binds::{capture_key, Capture};
use crate::net::Session;
use crate::profile::Profile;

/// The largest bet `seat` can make right now.
pub fn max_bet(view: &BjGame, seat: usize) -> u32 {
    let chips = view.players.get(seat).map(|p| p.chips).unwrap_or(0);
    MAX_BET.min(chips - chips % BET_STEP)
}

/// Keeps the prepared bet inside the table limits and the player's stack.
pub fn clamp_bet(view: &BjGame, seat: usize, bet: u32) -> u32 {
    let top = max_bet(view, seat).max(MIN_BET);
    (bet / BET_STEP * BET_STEP).clamp(MIN_BET, top)
}

pub fn keyboard_input(
    keys: Res<ButtonInput<KeyCode>>,
    mut profile: ResMut<Profile>,
    mut session: ResMut<Session>,
    mut ui: ResMut<BjUi>,
) {
    let Some(view) = session.bj_view() else { return };
    let me = session.my_index().unwrap_or(0).min(view.players.len() - 1);

    if let Some(action) = ui.capture {
        match capture_key(&keys, action, &mut profile.data.bj_binds) {
            Capture::Waiting => {}
            Capture::Cancelled => ui.capture = None,
            Capture::Bound => {
                ui.capture = None;
                profile.save();
            }
            Capture::Rejected => {
                ui.capture = None;
                session.notify("Эту клавишу назначить нельзя");
            }
        }
        return;
    }

    let binds = &profile.data.bj_binds;
    let pressed = |a: Action| binds.pressed(&keys, a);

    if pressed(Action::Menu) {
        if ui.show_settings {
            ui.show_settings = false;
        } else if ui.show_scores {
            ui.show_scores = false;
        } else {
            ui.confirm_leave = !ui.confirm_leave;
        }
        return;
    }
    if pressed(Action::Scores) {
        if ui.show_scores {
            ui.show_scores = false;
        } else if !(ui.show_settings || ui.confirm_leave) {
            ui.show_scores = true;
        }
        return;
    }
    if ui.show_settings || ui.confirm_leave || ui.show_scores {
        return;
    }

    match view.phase {
        Phase::RoundOver if pressed(Action::Confirm) => {
            if session.is_host {
                session.submit_blackjack(BjAction::NextRound);
            }
        }
        Phase::GameOver { .. } if pressed(Action::Confirm) => {
            if session.is_host {
                session.back_to_lobby();
            }
        }
        Phase::Betting if view.needs_bet(me) => {
            if pressed(Action::BetUp) {
                ui.bet = clamp_bet(&view, me, ui.bet + BET_STEP);
            }
            if pressed(Action::BetDown) {
                ui.bet = clamp_bet(&view, me, ui.bet.saturating_sub(BET_STEP));
            }
            if pressed(Action::PlaceBet) {
                let bet = clamp_bet(&view, me, ui.bet);
                session.submit_blackjack(BjAction::Bet(bet));
            }
            if pressed(Action::RepeatBet) {
                let last = view.players[me].last_bet;
                if last > 0 {
                    session.submit_blackjack(BjAction::Bet(last));
                }
            }
        }
        Phase::Insurance if view.needs_insurance(me) => {
            if pressed(Action::Insure) {
                session.submit_blackjack(BjAction::Insurance(true));
            } else if pressed(Action::Decline) {
                session.submit_blackjack(BjAction::Insurance(false));
            }
        }
        Phase::PlayerTurn { seat, .. } if seat == me => {
            if pressed(Action::Hit) {
                session.submit_blackjack(BjAction::Hit);
            } else if pressed(Action::Stand) {
                session.submit_blackjack(BjAction::Stand);
            } else if pressed(Action::Double) {
                session.submit_blackjack(BjAction::Double);
            } else if pressed(Action::Split) {
                session.submit_blackjack(BjAction::Split);
            }
        }
        _ => {}
    }
}
