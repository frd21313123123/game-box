//! The game session: lobby seats, who is host, and the network protocol.
//!
//! The model is host-authoritative. The host runs the rules engine (and the
//! bots), clients only send the actions they want to take and receive state
//! snapshots back. In the card game every client gets a redacted view, so other
//! players' hands never leave the host.
//!
//! A purely local game against bots is just a host without a transport.

use std::collections::HashMap;
use std::sync::Mutex;

use bevy::prelude::*;
use monopoly_core as mono;
use netcode::{NetEvent, PeerId, TcpClient, TcpHost, Transport, DEFAULT_PORT, HOST_PEER};
use serde::{Deserialize, Serialize};
use uno_core::{UnoAction, UnoGame};

use crate::profile::Profile;
use crate::steam::{SteamApi, SteamEvent};
use crate::AppState;

pub const PROTOCOL_VERSION: u32 = 1;
pub const MAX_SEATS: usize = 8;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum GameKind {
    Uno,
    Monopoly,
}

impl GameKind {
    pub fn title(self) -> &'static str {
        match self {
            GameKind::Uno => "Последняя карта",
            GameKind::Monopoly => "Монополия",
        }
    }

    pub fn state(self) -> AppState {
        match self {
            GameKind::Uno => AppState::Uno,
            GameKind::Monopoly => AppState::Monopoly,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum SeatKind {
    /// The player hosting the game.
    Host,
    /// A player connected over the network.
    Remote,
    Bot,
    /// Another human at the host's computer (hot-seat, Monopoly only).
    Local,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Seat {
    pub id: u32,
    pub name: String,
    pub kind: SeatKind,
    /// Monopoly pawn index.
    pub token: usize,
    /// Monopoly pawn colour index.
    pub color: usize,
    /// The remote player left during the game and a bot took over.
    pub replaced_by_bot: bool,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct LobbyInfo {
    pub game: GameKind,
    pub seats: Vec<Seat>,
    /// Card game: points to win the match (0 = a single round).
    pub uno_target: u32,
    pub started: bool,
    pub join_hint: Option<String>,
}

impl Default for LobbyInfo {
    fn default() -> Self {
        LobbyInfo {
            game: GameKind::Uno,
            seats: Vec::new(),
            uno_target: uno_core::DEFAULT_TARGET_SCORE,
            started: false,
            join_hint: None,
        }
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub enum ClientMsg {
    Hello { name: String, version: u32, game: GameKind },
    SetToken(usize),
    SetColor(usize),
    Uno(UnoAction),
    Monopoly(mono::Action),
    Leave,
}

#[derive(Debug, Serialize, Deserialize)]
pub enum ServerMsg {
    Welcome { seat: u32 },
    Rejected(String),
    Lobby(LobbyInfo),
    Start,
    BackToLobby,
    Uno(Box<UnoGame>),
    Monopoly(Box<mono::GameState>),
    Notice(String),
    Closed(String),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum NetMode {
    #[default]
    Offline,
    TcpHost,
    TcpClient,
    SteamHost,
    SteamClient,
}

impl NetMode {
    pub fn is_online(self) -> bool {
        self != NetMode::Offline
    }
}

#[derive(Resource, Default)]
pub struct Session {
    pub active: bool,
    pub is_host: bool,
    pub mode: NetMode,
    transport: Option<Mutex<Box<dyn Transport>>>,
    peers: HashMap<PeerId, u32>,
    pub lobby: LobbyInfo,
    pub my_seat: u32,
    /// Host: the full game. Client: the latest view received.
    pub uno: Option<UnoGame>,
    pub monopoly: Option<mono::GameState>,
    /// Waiting for the host's welcome.
    pub connecting: bool,
    next_seat_id: u32,
    dirty: bool,
    /// A screen change requested by the session (game start, disconnect...).
    pub go_to: Option<AppState>,
    /// Messages for the player (shown as toasts).
    pub notices: Vec<String>,
}

fn encode<T: Serialize>(msg: &T) -> Vec<u8> {
    serde_json::to_vec(msg).expect("protocol messages always serialize")
}

impl Session {
    fn reset(&mut self) {
        let notices = std::mem::take(&mut self.notices);
        *self = Session::default();
        self.notices = notices;
    }

    pub fn notify(&mut self, text: impl Into<String>) {
        self.notices.push(text.into());
    }

    // ----- seats ----------------------------------------------------------

    pub fn seat_index(&self, seat_id: u32) -> Option<usize> {
        self.lobby.seats.iter().position(|s| s.id == seat_id)
    }

    pub fn my_index(&self) -> Option<usize> {
        self.seat_index(self.my_seat)
    }

    pub fn seat(&self, seat_id: u32) -> Option<&Seat> {
        self.lobby.seats.iter().find(|s| s.id == seat_id)
    }

    fn new_seat(&mut self, name: String, kind: SeatKind) -> Option<u32> {
        if self.lobby.seats.len() >= MAX_SEATS {
            return None;
        }
        self.next_seat_id += 1;
        let used_tokens: Vec<usize> = self.lobby.seats.iter().map(|s| s.token).collect();
        let used_colors: Vec<usize> = self.lobby.seats.iter().map(|s| s.color).collect();
        let seat = Seat {
            id: self.next_seat_id,
            name: unique_name(&self.lobby.seats, &name),
            kind,
            token: mono::next_free_token(&used_tokens).unwrap_or(0),
            color: mono::next_distinct_color(&used_colors, None),
            replaced_by_bot: false,
        };
        let id = seat.id;
        self.lobby.seats.push(seat);
        self.dirty = true;
        Some(id)
    }

    pub fn can_edit_lobby(&self) -> bool {
        self.is_host && !self.lobby.started
    }

    pub fn add_bot(&mut self) {
        if !self.can_edit_lobby() {
            return;
        }
        let n = self.lobby.seats.iter().filter(|s| s.kind == SeatKind::Bot).count();
        let name = BOT_NAMES[n % BOT_NAMES.len()].to_string();
        self.new_seat(name, SeatKind::Bot);
    }

    pub fn add_local_player(&mut self) {
        if !self.can_edit_lobby() {
            return;
        }
        let n = self.lobby.seats.len() + 1;
        self.new_seat(format!("Игрок {n}"), SeatKind::Local);
    }

    pub fn remove_seat(&mut self, seat_id: u32) {
        if !self.can_edit_lobby() || seat_id == self.my_seat {
            return;
        }
        if let Some(peer) = self.peers.iter().find(|(_, s)| **s == seat_id).map(|(p, _)| *p) {
            self.send_to(peer, &ServerMsg::Rejected("Хост убрал вас из лобби".into()));
            self.peers.remove(&peer);
            if let Some(t) = &mut self.transport {
                t.get_mut().unwrap().disconnect(peer);
            }
        }
        self.lobby.seats.retain(|s| s.id != seat_id);
        self.dirty = true;
    }

    pub fn rename_seat(&mut self, seat_id: u32, name: &str) {
        if !self.can_edit_lobby() {
            return;
        }
        let name = name.trim();
        if name.is_empty() {
            return;
        }
        if let Some(s) = self.lobby.seats.iter_mut().find(|s| s.id == seat_id) {
            if matches!(s.kind, SeatKind::Local | SeatKind::Bot | SeatKind::Host) {
                s.name = name.chars().take(20).collect();
                self.dirty = true;
            }
        }
    }

    /// Changes a seat's pawn; ignored if another seat already has it.
    pub fn set_token(&mut self, seat_id: u32, token: usize) {
        if self.lobby.started || token >= mono::TOKENS.len() {
            return;
        }
        if !self.is_host {
            self.send_to_host(&ClientMsg::SetToken(token));
            return;
        }
        if self.lobby.seats.iter().any(|s| s.token == token && s.id != seat_id) {
            return;
        }
        if let Some(s) = self.lobby.seats.iter_mut().find(|s| s.id == seat_id) {
            s.token = token;
            self.dirty = true;
        }
    }

    pub fn set_color(&mut self, seat_id: u32, color: usize) {
        if self.lobby.started || color >= mono::TOKEN_COLORS.len() {
            return;
        }
        if !self.is_host {
            self.send_to_host(&ClientMsg::SetColor(color));
            return;
        }
        if self.lobby.seats.iter().any(|s| s.color == color && s.id != seat_id) {
            return;
        }
        if let Some(s) = self.lobby.seats.iter_mut().find(|s| s.id == seat_id) {
            s.color = color;
            self.dirty = true;
        }
    }

    pub fn set_uno_target(&mut self, target: u32) {
        if self.can_edit_lobby() {
            self.lobby.uno_target = target;
            self.dirty = true;
        }
    }

    pub fn seat_is_bot(&self, index: usize) -> bool {
        self.lobby
            .seats
            .get(index)
            .is_some_and(|s| s.kind == SeatKind::Bot)
    }

    // ----- lifecycle ------------------------------------------------------

    /// Creates a lobby hosted on this machine (offline until opened to the network).
    pub fn host(&mut self, game: GameKind, profile: &Profile) {
        self.reset();
        self.active = true;
        self.is_host = true;
        self.lobby.game = game;
        let me = self
            .new_seat(profile.display_name(), SeatKind::Host)
            .expect("empty lobby has room");
        self.my_seat = me;
        let bots = match game {
            GameKind::Uno => 3,
            GameKind::Monopoly => 2,
        };
        for _ in 0..bots {
            self.add_bot();
        }
    }

    pub fn open_tcp(&mut self) -> Result<String, String> {
        if !self.is_host || self.lobby.started {
            return Err("Открыть игру может только хост".into());
        }
        let host = TcpHost::bind(DEFAULT_PORT)
            .or_else(|_| TcpHost::bind(0))
            .map_err(|e| format!("Не удалось открыть порт: {e}"))?;
        let hint = host.join_hint().unwrap_or_default();
        self.transport = Some(Mutex::new(Box::new(host)));
        self.mode = NetMode::TcpHost;
        self.lobby.join_hint = Some(hint.clone());
        self.dirty = true;
        Ok(hint)
    }

    pub fn open_steam(&mut self, steam: &SteamApi) -> Result<(), String> {
        if !self.is_host || self.lobby.started {
            return Err("Открыть игру может только хост".into());
        }
        let transport = steam
            .host_transport()
            .ok_or_else(|| "Steam не запущен".to_string())?;
        steam.create_lobby(MAX_SEATS as u32);
        self.transport = Some(Mutex::new(transport));
        self.mode = NetMode::SteamHost;
        self.lobby.join_hint = Some("Создаём лобби Steam…".into());
        self.dirty = true;
        Ok(())
    }

    pub fn join_tcp(&mut self, address: &str, game: GameKind, profile: &Profile) -> Result<(), String> {
        let client = TcpClient::connect(address.trim())
            .map_err(|e| format!("Не удалось подключиться к {address}: {e}"))?;
        self.join_with(Box::new(client), NetMode::TcpClient, game, profile);
        Ok(())
    }

    pub fn join_steam(&mut self, host: u64, profile: &Profile, steam: &SteamApi) -> Result<(), String> {
        let transport = steam
            .client_transport(host)
            .ok_or_else(|| "Steam не запущен".to_string())?;
        // The host's lobby decides the game; Uno is only a placeholder until
        // the first lobby snapshot arrives.
        self.join_with(transport, NetMode::SteamClient, GameKind::Uno, profile);
        Ok(())
    }

    fn join_with(&mut self, transport: Box<dyn Transport>, mode: NetMode, game: GameKind, profile: &Profile) {
        self.reset();
        self.active = true;
        self.is_host = false;
        self.mode = mode;
        self.connecting = true;
        self.lobby.game = game;
        self.transport = Some(Mutex::new(transport));
        self.send_to_host(&ClientMsg::Hello {
            name: profile.display_name(),
            version: PROTOCOL_VERSION,
            game,
        });
    }

    /// Leaves the session (closing it for everyone when we are the host).
    pub fn leave(&mut self, steam: &SteamApi) {
        if self.is_host {
            let peers: Vec<PeerId> = self.peers.keys().copied().collect();
            for p in peers {
                self.send_to(p, &ServerMsg::Closed("Хост закрыл игру".into()));
            }
        } else if self.transport.is_some() {
            self.send_to_host(&ClientMsg::Leave);
        }
        if matches!(self.mode, NetMode::SteamHost | NetMode::SteamClient) {
            steam.leave_lobby();
        }
        self.reset();
    }

    pub fn start_game(&mut self) -> Result<(), String> {
        if !self.can_edit_lobby() {
            return Err("Начать игру может только хост".into());
        }
        let n = self.lobby.seats.len();
        if n < 2 {
            return Err("Нужно минимум 2 игрока".into());
        }
        let seed = seed_now();
        match self.lobby.game {
            GameKind::Uno => {
                let players = self
                    .lobby
                    .seats
                    .iter()
                    .map(|s| (s.name.clone(), s.kind == SeatKind::Bot))
                    .collect();
                self.uno = Some(UnoGame::new(players, seed, self.lobby.uno_target));
            }
            GameKind::Monopoly => {
                let mut state = mono::GameState::new(seed as u32);
                for s in &self.lobby.seats {
                    let ok = state.apply(&mono::Action::AddPlayer {
                        name: s.name.clone(),
                        token: s.token,
                        color: Some(s.color),
                    });
                    if !ok {
                        return Err(format!("Не удалось посадить игрока «{}»", s.name));
                    }
                }
                state.apply(&mono::Action::StartGame);
                self.monopoly = Some(state);
            }
        }
        self.lobby.started = true;
        self.dirty = true;
        let peers: Vec<PeerId> = self.peers.keys().copied().collect();
        for p in peers {
            self.send_to(p, &ServerMsg::Start);
        }
        self.go_to = Some(self.lobby.game.state());
        Ok(())
    }

    /// Host: finish the current game and gather everyone back in the lobby.
    pub fn back_to_lobby(&mut self) {
        if !self.is_host {
            return;
        }
        self.lobby.started = false;
        self.uno = None;
        self.monopoly = None;
        // Seats whose players left mid-game are removed now.
        self.lobby.seats.retain(|s| !s.replaced_by_bot);
        self.dirty = true;
        let peers: Vec<PeerId> = self.peers.keys().copied().collect();
        for p in peers {
            self.send_to(p, &ServerMsg::BackToLobby);
        }
        self.go_to = Some(AppState::Lobby);
    }

    // ----- card game --------------------------------------------------------

    /// What this player is allowed to see of the card game.
    pub fn uno_view(&self) -> Option<UnoGame> {
        let game = self.uno.as_ref()?;
        if self.is_host {
            Some(game.view_for(self.my_index()))
        } else {
            Some(game.clone())
        }
    }

    pub fn submit_uno(&mut self, action: UnoAction) {
        if self.is_host {
            if let Some(i) = self.my_index() {
                self.host_apply_uno(i, action);
            }
        } else {
            self.send_to_host(&ClientMsg::Uno(action));
        }
    }

    /// Host only: applies an action on behalf of seat `index` (used by bots too).
    pub fn host_apply_uno(&mut self, index: usize, action: UnoAction) -> bool {
        let Some(game) = &mut self.uno else { return false };
        match game.apply(index, action) {
            Ok(()) => {
                self.dirty = true;
                true
            }
            Err(e) => {
                if Some(index) == self.my_index() {
                    self.notify(e.to_string());
                }
                false
            }
        }
    }

    // ----- Monopoly -----------------------------------------------------------

    pub fn monopoly_player_id(&self, seat_index: usize) -> mono::PlayerId {
        seat_index as mono::PlayerId + 1
    }

    /// Monopoly players controlled from this computer.
    pub fn local_monopoly_ids(&self) -> Vec<mono::PlayerId> {
        if self.is_host {
            self.lobby
                .seats
                .iter()
                .enumerate()
                .filter(|(_, s)| matches!(s.kind, SeatKind::Host | SeatKind::Local))
                .map(|(i, _)| self.monopoly_player_id(i))
                .collect()
        } else {
            self.my_index()
                .map(|i| vec![self.monopoly_player_id(i)])
                .unwrap_or_default()
        }
    }

    pub fn submit_monopoly(&mut self, action: mono::Action) {
        if self.is_host {
            let locals = self.local_monopoly_ids();
            let Some(state) = &mut self.monopoly else { return };
            let Some(actor) = locals.into_iter().find(|id| state.can_submit(*id, &action)) else {
                self.notify("Сейчас это действие недоступно");
                return;
            };
            if state.apply(&action) {
                self.dirty = true;
            } else {
                let _ = actor;
                self.notify("Действие отклонено правилами");
            }
        } else {
            self.send_to_host(&ClientMsg::Monopoly(action));
        }
    }

    /// Host only: applies a bot's action after authorizing it.
    pub fn host_apply_monopoly(&mut self, actor: mono::PlayerId, action: &mono::Action) -> bool {
        let Some(state) = &mut self.monopoly else { return false };
        if state.can_submit(actor, action) && state.apply(action) {
            self.dirty = true;
            true
        } else {
            false
        }
    }

    // ----- transport ------------------------------------------------------------

    fn send_to(&mut self, peer: PeerId, msg: &ServerMsg) {
        let bytes = encode(msg);
        if let Some(t) = &mut self.transport {
            t.get_mut().unwrap().send(peer, &bytes);
        }
    }

    fn send_to_host(&mut self, msg: &ClientMsg) {
        let bytes = encode(msg);
        if let Some(t) = &mut self.transport {
            t.get_mut().unwrap().send(HOST_PEER, &bytes);
        }
    }

    fn broadcast(&mut self) {
        let peers: Vec<(PeerId, u32)> = self.peers.iter().map(|(p, s)| (*p, *s)).collect();
        if peers.is_empty() {
            return;
        }
        let lobby = encode(&ServerMsg::Lobby(self.lobby.clone()));
        let monopoly = self
            .monopoly
            .as_ref()
            .map(|s| encode(&ServerMsg::Monopoly(Box::new(s.clone()))));
        for (peer, seat) in peers {
            let uno = self.uno.as_ref().map(|g| {
                let idx = self.seat_index(seat);
                encode(&ServerMsg::Uno(Box::new(g.view_for(idx))))
            });
            let Some(t) = &mut self.transport else { return };
            let t = t.get_mut().unwrap();
            t.send(peer, &lobby);
            if let Some(m) = &monopoly {
                t.send(peer, m);
            }
            if let Some(u) = &uno {
                t.send(peer, u);
            }
        }
    }

    pub fn poll(&mut self) {
        let events = match &mut self.transport {
            Some(t) => t.get_mut().unwrap().poll(),
            None => Vec::new(),
        };
        for event in events {
            if self.is_host {
                self.host_event(event);
            } else {
                self.client_event(event);
            }
            if !self.active {
                return;
            }
        }
        if self.is_host && self.dirty {
            self.dirty = false;
            self.broadcast();
        }
    }

    fn host_event(&mut self, event: NetEvent) {
        match event {
            NetEvent::Connected(_) => {}
            NetEvent::Disconnected(peer) => self.peer_left(peer),
            NetEvent::Message(peer, bytes) => {
                let Ok(msg) = serde_json::from_slice::<ClientMsg>(&bytes) else {
                    warn!("Некорректное сообщение от клиента {peer}");
                    return;
                };
                self.host_message(peer, msg);
            }
        }
    }

    fn host_message(&mut self, peer: PeerId, msg: ClientMsg) {
        let seat = self.peers.get(&peer).copied();
        match msg {
            ClientMsg::Hello { name, version, game: _ } => {
                if seat.is_some() {
                    return;
                }
                if version != PROTOCOL_VERSION {
                    self.send_to(peer, &ServerMsg::Rejected("Версии игры не совпадают — обновите лаунчер".into()));
                    return;
                }
                if self.lobby.started {
                    self.send_to(peer, &ServerMsg::Rejected("Игра уже началась".into()));
                    return;
                }
                let name: String = name.trim().chars().take(20).collect();
                let name = if name.is_empty() { "Гость".to_string() } else { name };
                match self.new_seat(name.clone(), SeatKind::Remote) {
                    Some(id) => {
                        self.peers.insert(peer, id);
                        self.send_to(peer, &ServerMsg::Welcome { seat: id });
                        self.notify(format!("{name} присоединился"));
                    }
                    None => self.send_to(peer, &ServerMsg::Rejected("Все места заняты".into())),
                }
            }
            ClientMsg::SetToken(t) => {
                if let Some(seat) = seat {
                    self.set_token(seat, t);
                }
            }
            ClientMsg::SetColor(c) => {
                if let Some(seat) = seat {
                    self.set_color(seat, c);
                }
            }
            ClientMsg::Uno(action) => {
                let Some(idx) = seat.and_then(|s| self.seat_index(s)) else { return };
                let Some(game) = &mut self.uno else { return };
                match game.apply(idx, action) {
                    Ok(()) => self.dirty = true,
                    Err(e) => self.send_to(peer, &ServerMsg::Notice(e.to_string())),
                }
            }
            ClientMsg::Monopoly(action) => {
                let Some(idx) = seat.and_then(|s| self.seat_index(s)) else { return };
                let actor = self.monopoly_player_id(idx);
                if !self.host_apply_monopoly(actor, &action) {
                    self.send_to(peer, &ServerMsg::Notice("Сейчас это действие недоступно".into()));
                }
            }
            ClientMsg::Leave => self.peer_left(peer),
        }
    }

    fn peer_left(&mut self, peer: PeerId) {
        let Some(seat_id) = self.peers.remove(&peer) else { return };
        if let Some(t) = &mut self.transport {
            t.get_mut().unwrap().disconnect(peer);
        }
        let Some(idx) = self.seat_index(seat_id) else { return };
        let name = self.lobby.seats[idx].name.clone();
        if self.lobby.started {
            let seat = &mut self.lobby.seats[idx];
            seat.kind = SeatKind::Bot;
            seat.replaced_by_bot = true;
            if let Some(game) = &mut self.uno {
                game.players[idx].is_bot = true;
            }
            self.notify(format!("{name} отключился — за него играет бот"));
        } else {
            self.lobby.seats.remove(idx);
            self.notify(format!("{name} покинул лобби"));
        }
        self.dirty = true;
    }

    fn client_event(&mut self, event: NetEvent) {
        match event {
            NetEvent::Connected(_) => {}
            NetEvent::Disconnected(_) => {
                self.notify("Соединение с хостом потеряно");
                self.reset();
                self.go_to = Some(AppState::Menu);
            }
            NetEvent::Message(_, bytes) => {
                let Ok(msg) = serde_json::from_slice::<ServerMsg>(&bytes) else {
                    warn!("Некорректное сообщение от хоста");
                    return;
                };
                match msg {
                    ServerMsg::Welcome { seat } => {
                        self.my_seat = seat;
                        self.connecting = false;
                    }
                    ServerMsg::Rejected(reason) | ServerMsg::Closed(reason) => {
                        self.notify(reason);
                        self.reset();
                        self.go_to = Some(AppState::Menu);
                    }
                    ServerMsg::Lobby(info) => self.lobby = info,
                    ServerMsg::Start => self.go_to = Some(self.lobby.game.state()),
                    ServerMsg::BackToLobby => {
                        self.uno = None;
                        self.monopoly = None;
                        self.go_to = Some(AppState::Lobby);
                    }
                    ServerMsg::Uno(view) => self.uno = Some(*view),
                    ServerMsg::Monopoly(state) => self.monopoly = Some(*state),
                    ServerMsg::Notice(text) => self.notify(text),
                }
            }
        }
    }
}

const BOT_NAMES: [&str; 8] = [
    "Бот Арсений",
    "Бот Вера",
    "Бот Гоша",
    "Бот Дуня",
    "Бот Ефим",
    "Бот Жанна",
    "Бот Захар",
    "Бот Ирина",
];

fn unique_name(seats: &[Seat], name: &str) -> String {
    if !seats.iter().any(|s| s.name == name) {
        return name.to_string();
    }
    (2..)
        .map(|i| format!("{name} ({i})"))
        .find(|candidate| !seats.iter().any(|s| &s.name == candidate))
        .expect("infinite iterator")
}

fn seed_now() -> u64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos() as u64)
        .unwrap_or(0x5eed)
}

pub struct NetPlugin;

impl Plugin for NetPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Session>()
            .add_systems(PreUpdate, poll_session)
            .add_systems(Update, (steam_events, follow_session_navigation));
    }
}

fn poll_session(mut session: ResMut<Session>) {
    if session.active {
        session.poll();
    }
}

fn steam_events(
    steam: Res<SteamApi>,
    mut session: ResMut<Session>,
    profile: Res<Profile>,
) {
    for event in steam.poll_events() {
        match event {
            SteamEvent::LobbyCreated(Ok(_)) => {
                if session.mode == NetMode::SteamHost {
                    steam.set_lobby_data("game", session.lobby.game.title());
                    session.lobby.join_hint =
                        Some("Лобби Steam открыто — пригласите друзей".into());
                    session.dirty = true;
                }
            }
            SteamEvent::LobbyCreated(Err(e)) => {
                session.notify(format!("Не удалось создать лобби Steam: {e}"));
            }
            SteamEvent::JoinRequested(lobby) => {
                if session.active && session.lobby.started {
                    session.notify("Сначала выйдите из текущей игры");
                } else {
                    steam.join_lobby(lobby);
                }
            }
            SteamEvent::LobbyJoined(Ok((_lobby, owner))) => {
                if Some(owner) == profile.steam_id {
                    continue;
                }
                match session.join_steam(owner, &profile, &steam) {
                    Ok(()) => session.go_to = Some(AppState::Lobby),
                    Err(e) => session.notify(e),
                }
            }
            SteamEvent::LobbyJoined(Err(e)) => session.notify(e),
        }
    }
}

fn follow_session_navigation(
    mut session: ResMut<Session>,
    state: Res<State<AppState>>,
    mut next: ResMut<NextState<AppState>>,
) {
    if let Some(target) = session.go_to.take() {
        if *state.get() != target {
            next.set(target);
        }
    }
}
