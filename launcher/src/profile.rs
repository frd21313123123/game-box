//! The local player's profile: nickname (taken from Steam when available)
//! and a few settings, stored in the user's config directory.

use std::path::PathBuf;

use bevy::prelude::*;
use serde::{Deserialize, Serialize};

use crate::steam::SteamApi;
use crate::blackjack::binds::BjBinds;
use crate::uno::binds::UnoBinds;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ProfileData {
    pub name: String,
    pub sound: bool,
    pub volume: f32,
    pub last_address: String,
    /// Keyboard controls of the card game (only overrides are stored).
    #[serde(default)]
    pub uno_binds: UnoBinds,
    /// Keyboard controls of Blackjack (only overrides are stored).
    #[serde(default)]
    pub bj_binds: BjBinds,
}

impl Default for ProfileData {
    fn default() -> Self {
        let name = std::env::var("USERNAME")
            .or_else(|_| std::env::var("USER"))
            .ok()
            .filter(|n| !n.is_empty() && n != "root")
            .unwrap_or_else(|| "Игрок".to_string());
        ProfileData {
            name,
            sound: true,
            volume: 0.7,
            last_address: String::new(),
            uno_binds: UnoBinds::default(),
            bj_binds: BjBinds::default(),
        }
    }
}

#[derive(Resource)]
pub struct Profile {
    pub data: ProfileData,
    /// Set when the nickname comes from Steam (and can't be edited here).
    pub steam_name: Option<String>,
    pub steam_id: Option<u64>,
}

impl Profile {
    /// The nickname shown to other players.
    pub fn display_name(&self) -> String {
        self.steam_name.clone().unwrap_or_else(|| self.data.name.clone())
    }

    pub fn save(&self) {
        let Some(path) = config_path() else { return };
        if let Some(dir) = path.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        if let Ok(json) = serde_json::to_string_pretty(&self.data) {
            if let Err(e) = std::fs::write(&path, json) {
                warn!("Не удалось сохранить профиль: {e}");
            }
        }
    }
}

fn config_path() -> Option<PathBuf> {
    Some(dirs::config_dir()?.join("game-box").join("profile.json"))
}

fn load() -> ProfileData {
    config_path()
        .and_then(|p| std::fs::read_to_string(p).ok())
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

pub struct ProfilePlugin;

impl Plugin for ProfilePlugin {
    fn build(&self, app: &mut App) {
        let steam = app.world().get_resource::<SteamApi>();
        let steam_name = steam.and_then(|s| s.persona_name());
        let steam_id = steam.and_then(|s| s.steam_id());
        if let Some(name) = &steam_name {
            info!("Steam: вошли как «{name}»");
        }
        app.insert_resource(Profile {
            data: load(),
            steam_name,
            steam_id,
        });
    }
}
