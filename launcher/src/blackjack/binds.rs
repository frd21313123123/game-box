//! Keyboard actions of the Blackjack table (the keys themselves are handled
//! by [`crate::binds`]).

use bevy::prelude::*;

use crate::binds::{BindAction, Binds};

/// The player's Blackjack bindings, stored in the profile.
pub type BjBinds = Binds<Action>;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Action {
    BetUp,
    BetDown,
    PlaceBet,
    RepeatBet,
    Hit,
    Stand,
    Double,
    Split,
    Insure,
    Decline,
    Scores,
    Menu,
    Confirm,
}

pub const ACTIONS: [Action; 13] = [
    Action::BetUp,
    Action::BetDown,
    Action::PlaceBet,
    Action::RepeatBet,
    Action::Hit,
    Action::Stand,
    Action::Double,
    Action::Split,
    Action::Insure,
    Action::Decline,
    Action::Scores,
    Action::Menu,
    Action::Confirm,
];

impl BindAction for Action {
    const ALL: &'static [Action] = &ACTIONS;

    fn id(self) -> &'static str {
        match self {
            Action::BetUp => "bet_up",
            Action::BetDown => "bet_down",
            Action::PlaceBet => "place_bet",
            Action::RepeatBet => "repeat_bet",
            Action::Hit => "hit",
            Action::Stand => "stand",
            Action::Double => "double",
            Action::Split => "split",
            Action::Insure => "insure",
            Action::Decline => "decline",
            Action::Scores => "scores",
            Action::Menu => "menu",
            Action::Confirm => "confirm",
        }
    }

    fn label(self) -> &'static str {
        match self {
            Action::BetUp => "Ставка +10",
            Action::BetDown => "Ставка −10",
            Action::PlaceBet => "Сделать ставку",
            Action::RepeatBet => "Повторить прошлую ставку",
            Action::Hit => "Ещё карту",
            Action::Stand => "Хватит",
            Action::Double => "Удвоить",
            Action::Split => "Разделить пару",
            Action::Insure => "Взять страховку",
            Action::Decline => "Без страховки",
            Action::Scores => "Таблица фишек",
            Action::Menu => "Закрыть окно / выйти",
            Action::Confirm => "Следующий раунд / в лобби",
        }
    }

    fn default_key(self) -> KeyCode {
        match self {
            Action::BetUp => KeyCode::ArrowUp,
            Action::BetDown => KeyCode::ArrowDown,
            Action::PlaceBet => KeyCode::Enter,
            Action::RepeatBet => KeyCode::KeyR,
            Action::Hit => KeyCode::KeyH,
            Action::Stand => KeyCode::KeyS,
            Action::Double => KeyCode::KeyD,
            Action::Split => KeyCode::KeyP,
            Action::Insure => KeyCode::KeyI,
            Action::Decline => KeyCode::KeyO,
            Action::Scores => KeyCode::Tab,
            Action::Menu => KeyCode::Escape,
            Action::Confirm => KeyCode::KeyN,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::binds::is_bindable;

    #[test]
    fn default_keys_are_unique_and_bindable() {
        for a in ACTIONS {
            assert!(is_bindable(a.default_key()), "{:?} has no name", a);
            for b in ACTIONS {
                if a != b {
                    assert_ne!(a.default_key(), b.default_key(), "{:?} and {:?} clash", a, b);
                }
            }
        }
    }

    #[test]
    fn rebinding_swaps_a_conflicting_key() {
        let mut b = BjBinds::default();
        b.set(Action::Hit, KeyCode::Space);
        b.set(Action::Stand, KeyCode::KeyH);
        assert_eq!(b.key(Action::Hit), KeyCode::Space);
        assert_eq!(b.key(Action::Stand), KeyCode::KeyH);
        b.set(Action::Stand, KeyCode::Space);
        assert_eq!(b.key(Action::Hit), KeyCode::KeyH);
    }
}
