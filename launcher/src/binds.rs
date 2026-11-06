//! Keyboard bindings of the card tables. Each game has its own list of
//! actions ([`BindAction`]); only the keys that differ from the defaults are
//! stored in the profile, so an untouched profile keeps working when new
//! actions are added later.

use std::collections::BTreeMap;
use std::marker::PhantomData;

use bevy::prelude::*;
use bevy_egui::egui::{self, RichText, Vec2};
use serde::{Deserialize, Serialize};

use crate::theme::{self, semibold};

/// One game's list of keyboard actions.
pub trait BindAction: Copy + Eq + std::fmt::Debug + 'static {
    /// Every action, in the order the settings dialog lists them.
    const ALL: &'static [Self];
    /// Stable key used in the profile file.
    fn id(self) -> &'static str;
    /// Name shown in the settings dialog.
    fn label(self) -> &'static str;
    fn default_key(self) -> KeyCode;
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

/// A player's bindings for one game. Stores only the overrides from the defaults.
#[derive(Serialize, Deserialize)]
#[serde(transparent, bound = "")]
pub struct Binds<A> {
    overrides: BTreeMap<String, String>,
    #[serde(skip)]
    _actions: PhantomData<fn() -> A>,
}

impl<A> Default for Binds<A> {
    fn default() -> Self {
        Binds {
            overrides: BTreeMap::new(),
            _actions: PhantomData,
        }
    }
}

impl<A> Clone for Binds<A> {
    fn clone(&self) -> Self {
        Binds {
            overrides: self.overrides.clone(),
            _actions: PhantomData,
        }
    }
}

impl<A> PartialEq for Binds<A> {
    fn eq(&self, other: &Self) -> bool {
        self.overrides == other.overrides
    }
}

impl<A> std::fmt::Debug for Binds<A> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.overrides.fmt(f)
    }
}

impl<A: BindAction> Binds<A> {
    /// The key currently bound to `action`.
    pub fn key(&self, action: A) -> KeyCode {
        self.overrides
            .get(action.id())
            .and_then(|name| key_from_name(name))
            .unwrap_or_else(|| action.default_key())
    }

    /// Whether the action was pressed this frame.
    pub fn pressed(&self, keys: &ButtonInput<KeyCode>, action: A) -> bool {
        keys.just_pressed(self.key(action))
    }

    pub fn label(&self, action: A) -> String {
        key_label(self.key(action))
    }

    /// Binds `action` to `key`. If another action already uses that key, the
    /// two actions swap keys, so no key ever triggers two actions.
    pub fn set(&mut self, action: A, key: KeyCode) {
        let old = self.key(action);
        if let Some(other) = A::ALL.iter().copied().find(|o| *o != action && self.key(*o) == key) {
            self.assign(other, old);
        }
        self.assign(action, key);
    }

    /// Back to the default keys.
    pub fn reset(&mut self) {
        self.overrides.clear();
    }

    fn assign(&mut self, action: A, key: KeyCode) {
        if key == action.default_key() {
            self.overrides.remove(action.id());
        } else if let Some(name) = key_name(key) {
            self.overrides.insert(action.id().to_string(), name.to_string());
        }
    }
}

/// What happened to a key press while the settings dialog waits for one.
pub enum Capture {
    /// No key was pressed this frame.
    Waiting,
    /// Escape: the old key stays.
    Cancelled,
    /// The key became the binding; the profile should be saved.
    Bound,
    /// This key can't be bound.
    Rejected,
}

/// Turns the first key pressed this frame into the binding for `action`.
pub fn capture_key<A: BindAction>(keys: &ButtonInput<KeyCode>, action: A, binds: &mut Binds<A>) -> Capture {
    let Some(&code) = keys.get_just_pressed().next() else { return Capture::Waiting };
    if code == KeyCode::Escape {
        Capture::Cancelled
    } else if is_bindable(code) {
        binds.set(action, code);
        Capture::Bound
    } else {
        Capture::Rejected
    }
}

/// The rows of the controls dialog: an action and the button with its key.
/// Clicking a key button starts waiting for a new key (`capture`).
pub fn binds_grid<A: BindAction>(ui: &mut egui::Ui, id: &str, binds: &Binds<A>, capture: &mut Option<A>) {
    egui::Grid::new(id).num_columns(2).spacing([20.0, 5.0]).show(ui, |ui| {
        for &action in A::ALL {
            ui.label(action.label());
            let waiting = *capture == Some(action);
            let text = if waiting { "Нажмите клавишу…".to_string() } else { binds.label(action) };
            let color = if waiting { theme::GOLD } else { theme::TEXT };
            let key = egui::Button::new(RichText::new(text).font(semibold(14.0)).color(color)).min_size(Vec2::new(170.0, 28.0));
            if ui.add(key).clicked() {
                *capture = Some(action);
            }
            ui.end_row();
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn key_names_round_trip() {
        for (name, code) in KEYS {
            assert_eq!(key_from_name(name), Some(*code));
            assert_eq!(key_name(*code), Some(*name));
        }
    }
}
