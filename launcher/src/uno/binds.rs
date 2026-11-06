//! Keyboard actions of the Uno table (the keys themselves are handled by
//! [`crate::binds`]).

use bevy::prelude::*;

use crate::binds::{BindAction, Binds};

/// The player's Uno bindings, stored in the profile.
pub type UnoBinds = Binds<Action>;

/// Everything that can be done from the keyboard at the card table.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Action {
    Left,
    Right,
    Play,
    Draw,
    Pass,
    LastCard,
    Catch,
    Scores,
    Menu,
    Confirm,
    Red,
    Yellow,
    Green,
    Blue,
    Challenge,
    Accept,
}

/// Every action, in the order the settings dialog lists them.
pub const ACTIONS: [Action; 16] = [
    Action::Left,
    Action::Right,
    Action::Play,
    Action::Draw,
    Action::Pass,
    Action::LastCard,
    Action::Catch,
    Action::Scores,
    Action::Menu,
    Action::Confirm,
    Action::Red,
    Action::Yellow,
    Action::Green,
    Action::Blue,
    Action::Challenge,
    Action::Accept,
];

impl BindAction for Action {
    const ALL: &'static [Action] = &ACTIONS;

    fn id(self) -> &'static str {
        match self {
            Action::Left => "left",
            Action::Right => "right",
            Action::Play => "play",
            Action::Draw => "draw",
            Action::Pass => "pass",
            Action::LastCard => "last_card",
            Action::Catch => "catch",
            Action::Scores => "scores",
            Action::Menu => "menu",
            Action::Confirm => "confirm",
            Action::Red => "red",
            Action::Yellow => "yellow",
            Action::Green => "green",
            Action::Blue => "blue",
            Action::Challenge => "challenge",
            Action::Accept => "accept",
        }
    }

    fn label(self) -> &'static str {
        match self {
            Action::Left => "Выбрать карту левее",
            Action::Right => "Выбрать карту правее",
            Action::Play => "Сыграть выбранную карту",
            Action::Draw => "Взять карту",
            Action::Pass => "Оставить и пропустить",
            Action::LastCard => "«Последняя!»",
            Action::Catch => "Поймать без «Последней!»",
            Action::Scores => "Таблица счёта",
            Action::Menu => "Закрыть окно / выйти",
            Action::Confirm => "Следующий раунд / в лобби",
            Action::Red => "Заказать красный",
            Action::Yellow => "Заказать жёлтый",
            Action::Green => "Заказать зелёный",
            Action::Blue => "Заказать синий",
            Action::Challenge => "Оспорить «+4»",
            Action::Accept => "Взять 4 карты",
        }
    }

    fn default_key(self) -> KeyCode {
        match self {
            Action::Left => KeyCode::ArrowLeft,
            Action::Right => KeyCode::ArrowRight,
            Action::Play => KeyCode::Enter,
            Action::Draw => KeyCode::KeyD,
            Action::Pass => KeyCode::KeyS,
            Action::LastCard => KeyCode::Space,
            Action::Catch => KeyCode::KeyC,
            Action::Scores => KeyCode::Tab,
            Action::Menu => KeyCode::Escape,
            Action::Confirm => KeyCode::KeyN,
            Action::Red => KeyCode::Digit1,
            Action::Yellow => KeyCode::Digit2,
            Action::Green => KeyCode::Digit3,
            Action::Blue => KeyCode::Digit4,
            Action::Challenge => KeyCode::KeyX,
            Action::Accept => KeyCode::KeyA,
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
        let mut b = UnoBinds::default();
        b.set(Action::Draw, KeyCode::Space);
        assert_eq!(b.key(Action::Draw), KeyCode::Space);
        // Space used to play «Последняя!»: that action takes over D.
        assert_eq!(b.key(Action::LastCard), KeyCode::KeyD);
        assert_eq!(b.key(Action::Pass), KeyCode::KeyS);
    }

    #[test]
    fn overrides_survive_a_profile_round_trip() {
        let mut b = UnoBinds::default();
        b.set(Action::Catch, KeyCode::KeyJ);
        let json = serde_json::to_string(&b).unwrap();
        assert_eq!(json, r#"{"catch":"J"}"#);
        let back: UnoBinds = serde_json::from_str(&json).unwrap();
        assert_eq!(back, b);
        assert_eq!(back.key(Action::Catch), KeyCode::KeyJ);
        assert_eq!(back.key(Action::Draw), KeyCode::KeyD);
    }

    #[test]
    fn reset_restores_defaults() {
        let mut b = UnoBinds::default();
        b.set(Action::Play, KeyCode::KeyF);
        b.reset();
        assert_eq!(b, UnoBinds::default());
        assert_eq!(b.key(Action::Play), KeyCode::Enter);
    }
}
