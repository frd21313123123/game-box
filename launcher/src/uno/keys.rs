//! Keyboard control of the card table: move a selection through the hand,
//! play, draw, call «last card!», and answer the dialogs. Keys come from the
//! player's bindings (see [`crate::binds`]).

use bevy::prelude::*;
use uno_core::{Phase, UnoAction, UnoGame};

use super::binds::Action;
use crate::binds::{capture_key, Capture};
use super::scene::Table;
use super::UnoUi;
use crate::net::Session;
use crate::profile::Profile;

/// Whether «last card!» can be pressed by seat `me` right now.
pub fn can_call_last(view: &UnoGame, me: usize) -> bool {
    let my = &view.players[me];
    let acting = view.acting_seat() == Some(me);
    !my.uno_called
        && ((my.hand.len() == 1 && view.uno_vulnerable == Some(me))
            || (my.hand.len() == 2 && acting && matches!(view.phase, Phase::Turn | Phase::AfterDraw { .. })))
}

/// Presses «last card!» for seat `me`; the call goes out before the card is
/// played, and the next play announces it too (see [`play_card`]).
pub fn call_last(session: &mut Session, ui: &mut UnoUi, view: &UnoGame, me: usize) {
    let my_turn = view.acting_seat() == Some(me);
    if view.players[me].hand.len() == 2 && my_turn {
        ui.uno_armed = true;
    }
    session.submit_uno(UnoAction::CallUno);
}

/// Plays card `id` from seat `me`'s hand if the rules allow it now, and tells
/// the player why not otherwise.
pub fn play_card(session: &mut Session, ui: &mut UnoUi, view: &UnoGame, me: usize, id: u16) {
    if view.acting_seat() != Some(me) {
        session.notify("Сейчас не ваш ход");
        return;
    }
    let Some(card) = view.players[me].hand.iter().find(|c| c.id == id).copied() else { return };
    if !view.playable_cards(me).iter().any(|c| c.id == id) {
        if matches!(view.phase, Phase::AfterDraw { .. }) {
            session.notify("После добора можно сыграть только взятую карту");
        } else {
            session.notify(format!("«{}» сюда не подходит", card.describe_ru()));
        }
        return;
    }
    // Trust the flag only while it still matches the table: the server clears
    // the call when cards are drawn, so a stale local flag must not resurrect it.
    let call_uno = ui.uno_armed && view.players[me].hand.len() == 2 && view.players[me].uno_called;
    ui.uno_armed = false;
    session.submit_uno(UnoAction::Play { card_id: id, call_uno });
}

/// Keyboard controls. Runs before the mouse input, so both share one state.
#[allow(clippy::too_many_arguments)]
pub fn keyboard_input(
    keys: Res<ButtonInput<KeyCode>>,
    mut profile: ResMut<Profile>,
    table: Res<Table>,
    mut session: ResMut<Session>,
    mut ui: ResMut<UnoUi>,
) {
    let Some(view) = session.uno_view() else { return };
    let me = table.me;

    // While the settings dialog waits for a key, that key becomes the binding.
    if let Some(action) = ui.capture {
        match capture_key(&keys, action, &mut profile.data.uno_binds) {
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

    let binds = &profile.data.uno_binds;
    let pressed = |a: Action| binds.pressed(&keys, a);

    if pressed(Action::Menu) {
        // Escape closes the topmost window, or toggles the leave dialog.
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
        } else if !(ui.show_settings || ui.confirm_leave || ui.modal_open) {
            ui.show_scores = true;
        }
        return;
    }
    if ui.show_settings || ui.confirm_leave || ui.show_scores {
        return;
    }

    // Round and game end: the same buttons as on screen.
    match view.phase {
        Phase::RoundOver { .. } if pressed(Action::Confirm) => {
            if session.is_host {
                if let Some(i) = session.my_index() {
                    session.host_apply_uno(i, UnoAction::NextRound);
                }
            }
            return;
        }
        Phase::GameOver { .. } if pressed(Action::Confirm) => {
            if session.is_host {
                session.back_to_lobby();
            }
            return;
        }
        _ => {}
    }

    // Dialogs that ask a question of this player.
    match view.phase {
        Phase::ChooseColor { seat, .. } if seat == me => {
            for (action, color) in [
                (Action::Red, uno_core::CardColor::Red),
                (Action::Yellow, uno_core::CardColor::Yellow),
                (Action::Green, uno_core::CardColor::Green),
                (Action::Blue, uno_core::CardColor::Blue),
            ] {
                if pressed(action) {
                    session.submit_uno(UnoAction::ChooseColor(color));
                    return;
                }
            }
        }
        _ => {}
    }

    if ui.modal_open {
        return;
    }

    // ---- the hand and the table ------------------------------------------------
    let my_turn = view.acting_seat() == Some(me);
    let hand: Vec<u16> = table.my_hand.iter().map(|(id, _)| *id).collect();

    // After drawing, the drawn card is the only one that can be played: select it.
    if my_turn {
        if let Phase::AfterDraw { card_id } = view.phase {
            ui.cursor = Some(card_id);
        }
    }
    if ui.cursor.is_some_and(|c| !hand.contains(&c)) {
        ui.cursor = None;
    }

    let playable: Vec<u16> = if my_turn {
        view.playable_cards(me).iter().map(|c| c.id).collect()
    } else {
        Vec::new()
    };

    if !hand.is_empty() && (pressed(Action::Left) || pressed(Action::Right)) {
        let n = hand.len();
        let step: i32 = if pressed(Action::Right) { 1 } else { -1 };
        let current = ui.cursor.and_then(|c| hand.iter().position(|x| *x == c));
        ui.cursor = Some(match current {
            // First press: start from the first playable card, or from the edge.
            None => hand
                .iter()
                .copied()
                .find(|id| playable.contains(id))
                .unwrap_or(if step > 0 { hand[0] } else { hand[n - 1] }),
            Some(i) => hand[(i as i32 + step).clamp(0, n as i32 - 1) as usize],
        });
    }

    if pressed(Action::Play) {
        match ui.cursor {
            // Enter first picks a card; a second Enter plays it.
            None => {
                if let Some(id) = hand.iter().copied().find(|id| playable.contains(id)) {
                    ui.cursor = Some(id);
                }
            }
            Some(id) => play_card(&mut session, &mut ui, &view, me, id),
        }
    }

    if pressed(Action::Draw) && my_turn && view.phase == Phase::Turn {
        session.submit_uno(UnoAction::Draw);
    }
    if pressed(Action::Pass) && my_turn && matches!(view.phase, Phase::AfterDraw { .. }) {
        session.submit_uno(UnoAction::Pass);
    }
    if pressed(Action::LastCard) && can_call_last(&view, me) {
        call_last(&mut session, &mut ui, &view, me);
    }
    if pressed(Action::Catch) {
        if let Some(target) = view.uno_vulnerable.filter(|t| *t != me) {
            session.submit_uno(UnoAction::CatchUno { target });
        }
    }
}
