//! Keyboard bindings of the card table. Only the keys that differ from the
//! defaults are stored in the profile, so an untouched profile keeps working
//! when new actions are added later.

use std::collections::BTreeMap;

use bevy::prelude::*;
use serde::{Deserialize, Serialize};

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

impl Action {
    /// Stable key used in the profile file.
    pub fn id(self) -> &'static str {
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

    /// Name shown in the settings dialog.
    pub fn label(self) -> &'static str {
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

    pub fn default_key(self) -> KeyCode {
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

/// Keys that can be bound, with the names they are stored under.
const KEYS: &[(&str, KeyCode)] = &[
    ("A", KeyCode::KeyA),
    ("B", KeyCode::KeyB),
    ("C", KeyCode::KeyC),
    ("D", KeyCode::KeyD),
    ("E", KeyCode::KeyE),
    ("F", KeyCode::KeyF),
    ("G", KeyCode::KeyG),
    ("H", KeyCode::KeyH),
    ("I", KeyCode::KeyI),
    ("J", KeyCode::KeyJ),
    ("K", KeyCode::KeyK),
    ("L", KeyCode::KeyL),
    ("M", KeyCode::KeyM),
    ("N", KeyCode::KeyN),
    ("O", KeyCode::KeyO),
    ("P", KeyCode::KeyP),
    ("Q", KeyCode::KeyQ),
    ("R", KeyCode::KeyR),
    ("S", KeyCode::KeyS),
    ("T", KeyCode::KeyT),
    ("U", KeyCode::KeyU),
    ("V", KeyCode::KeyV),
    ("W", KeyCode::KeyW),
    ("X", KeyCode::KeyX),
    ("Y", KeyCode::KeyY),
    ("Z", KeyCode::KeyZ),
    ("0", KeyCode::Digit0),
    ("1", KeyCode::Digit1),
    ("2", KeyCode::Digit2),
    ("3", KeyCode::Digit3),
    ("4", KeyCode::Digit4),
    ("5", KeyCode::Digit5),
    ("6", KeyCode::Digit6),
    ("7", KeyCode::Digit7),
    ("8", KeyCode::Digit8),
    ("9", KeyCode::Digit9),
    ("Space", KeyCode::Space),
    ("Enter", KeyCode::Enter),
    ("Tab", KeyCode::Tab),
    ("Backspace", KeyCode::Backspace),
    ("Left", KeyCode::ArrowLeft),
    ("Right", KeyCode::ArrowRight),
    ("Up", KeyCode::ArrowUp),
    ("Down", KeyCode::ArrowDown),
    ("Esc", KeyCode::Escape),
];

fn key_name(code: KeyCode) -> Option<&'static str> {
    KEYS.iter().find(|(_, k)| *k == code).map(|(name, _)| *name)
}

fn key_from_name(name: &str) -> Option<KeyCode> {
    KEYS.iter().find(|(n, _)| *n == name).map(|(_, k)| *k)
}

/// Whether a key can be used as a binding (the settings dialog rejects others).
pub fn is_bindable(code: KeyCode) -> bool {
    key_name(code).is_some()
}

/// Short text for a key, as shown on buttons and hints.
pub fn key_label(code: KeyCode) -> String {
    match code {
        KeyCode::Space => "Пробел".into(),
        KeyCode::ArrowLeft => "←".into(),
        KeyCode::ArrowRight => "→".into(),
        KeyCode::ArrowUp => "↑".into(),
        KeyCode::ArrowDown => "↓".into(),
        KeyCode::Escape => "Esc".into(),
        other => key_name(other).map(str::to_string).unwrap_or_else(|| format!("{other:?}")),
    }
}

/// Player's bindings. Stores only the overrides from the defaults.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct UnoBinds(BTreeMap<String, String>);

impl UnoBinds {
    /// The key currently bound to `action`.
    pub fn key(&self, action: Action) -> KeyCode {
        self.0
            .get(action.id())
            .and_then(|name| key_from_name(name))
            .unwrap_or_else(|| action.default_key())
    }

    /// Whether the action was pressed this frame.
    pub fn pressed(&self, keys: &ButtonInput<KeyCode>, action: Action) -> bool {
        keys.just_pressed(self.key(action))
    }

    pub fn label(&self, action: Action) -> String {
        key_label(self.key(action))
    }

    /// Binds `action` to `key`. If another action already uses that key, the
    /// two actions swap keys, so no key ever triggers two actions.
    pub fn set(&mut self, action: Action, key: KeyCode) {
        let old = self.key(action);
        if let Some(other) = ACTIONS.into_iter().find(|o| *o != action && self.key(*o) == key) {
            self.assign(other, old);
        }
        self.assign(action, key);
    }

    /// Back to the default keys.
    pub fn reset(&mut self) {
        self.0.clear();
    }

    fn assign(&mut self, action: Action, key: KeyCode) {
        if key == action.default_key() {
            self.0.remove(action.id());
        } else if let Some(name) = key_name(key) {
            self.0.insert(action.id().to_string(), name.to_string());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
    fn key_names_round_trip() {
        for (name, code) in KEYS {
            assert_eq!(key_from_name(name), Some(*code));
            assert_eq!(key_name(*code), Some(*name));
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
