//! Steam integration (cargo feature `steam`).
//!
//! With Steam running the launcher takes the player's nickname from Steam,
//! hosts games in Steam lobbies (friends join through the overlay invite or
//! "Join game" in the friends list) and sends game traffic over Steam
//! Networking Messages, so no ports need to be opened.
//!
//! Without the feature (or without a running Steam client) everything here is
//! a no-op and the launcher falls back to a local nickname and TCP play.

use bevy::prelude::*;
use netcode::Transport;

#[derive(Debug, Clone)]
#[cfg_attr(not(feature = "steam"), allow(dead_code))]
pub enum SteamEvent {
    LobbyCreated(Result<u64, String>),
    /// Joined a lobby: (lobby id, owner's Steam id).
    LobbyJoined(Result<(u64, u64), String>),
    /// The user accepted an invite / clicked "Join game" on a friend.
    JoinRequested(u64),
    /// A P2P session with this Steam user could not be established or broke.
    SessionFailed(u64, String),
}

pub struct SteamPlugin;

#[cfg(not(feature = "steam"))]
mod imp {
    use super::*;

    #[derive(Resource, Default)]
    pub struct SteamApi;

    impl SteamApi {
        pub fn available(&self) -> bool {
            false
        }
        pub fn persona_name(&self) -> Option<String> {
            None
        }
        pub fn steam_id(&self) -> Option<u64> {
            None
        }
        pub fn create_lobby(&self, _max_members: u32) {}
        pub fn join_lobby(&self, _lobby: u64) {}
        pub fn leave_lobby(&self) {}
        pub fn open_invite_overlay(&self) {}
        pub fn set_lobby_data(&self, _key: &str, _value: &str) {}
        pub fn poll_events(&self) -> Vec<SteamEvent> {
            Vec::new()
        }
        pub fn host_transport(&self) -> Option<Box<dyn Transport>> {
            None
        }
        pub fn client_transport(&self, _host: u64) -> Option<Box<dyn Transport>> {
            None
        }
    }

    impl Plugin for SteamPlugin {
        fn build(&self, app: &mut App) {
            app.init_resource::<SteamApi>();
        }
    }
}

#[cfg(feature = "steam")]
mod imp {
    use super::*;
    use std::collections::HashSet;
    use std::sync::mpsc::{channel, Receiver, Sender};
    use std::sync::{Arc, Mutex};

    use netcode::{NetEvent, PeerId, HOST_PEER};
    use steamworks::networking_messages::NetworkingMessagesSessionFailed;
    use steamworks::networking_types::{NetworkingIdentity, SendFlags};
    use steamworks::{
        sys, Callback, ChatMemberStateChange, Client, GameLobbyJoinRequested, LobbyChatUpdate,
        LobbyId, LobbyType, SteamId,
    };

    const CHANNEL: u32 = 0;

    /// An incoming P2P session request (SteamNetworkingMessagesSessionRequest_t).
    ///
    /// `NetworkingMessages::session_request_callback` in steamworks 0.12 throws
    /// away the handle of the callback it registers, and dropping a handle
    /// unregisters the callback. The host therefore never accepted a session,
    /// the client's `Hello` never arrived and joining hung on "Подключаемся к
    /// хосту…". We register the callback ourselves and keep its handle.
    struct SessionRequest(sys::SteamNetworkingIdentity);

    unsafe impl Callback for SessionRequest {
        const ID: i32 = sys::SteamNetworkingMessagesSessionRequest_t_k_iCallback as i32;

        unsafe fn from_raw(raw: *mut std::ffi::c_void) -> Self {
            let cb = raw
                .cast::<sys::SteamNetworkingMessagesSessionRequest_t>()
                .read_unaligned();
            SessionRequest(cb.m_identityRemote)
        }
    }

    fn accept_session(identity: &sys::SteamNetworkingIdentity) -> bool {
        // SAFETY: called on the main thread from run_callbacks while the
        // Steam client is alive; the identity comes straight from Steam.
        unsafe {
            let messages = sys::SteamAPI_SteamNetworkingMessages_SteamAPI_v002();
            !messages.is_null()
                && sys::SteamAPI_ISteamNetworkingMessages_AcceptSessionWithUser(messages, identity)
        }
    }
    /// Spacewar — Valve's public test AppID, used unless steam_appid.txt says otherwise.
    const FALLBACK_APP_ID: u32 = 480;

    #[derive(Resource)]
    pub struct SteamApi {
        client: Option<Client>,
        events_tx: Sender<SteamEvent>,
        events_rx: Mutex<Receiver<SteamEvent>>,
        lobby: Arc<Mutex<Option<LobbyId>>>,
        member_updates: Arc<Mutex<Vec<LobbyChatUpdate>>>,
        _callbacks: Vec<steamworks::CallbackHandle>,
    }

    impl SteamApi {
        fn init() -> Self {
            let (events_tx, events_rx) = channel();
            let lobby: Arc<Mutex<Option<LobbyId>>> = Arc::default();
            let member_updates: Arc<Mutex<Vec<LobbyChatUpdate>>> = Arc::default();
            let has_appid_file = std::path::Path::new("steam_appid.txt").exists();
            let client = if has_appid_file {
                Client::init()
            } else {
                Client::init_app(FALLBACK_APP_ID)
            };
            let client = match client {
                Ok(c) => Some(c),
                Err(e) => {
                    warn!("Steam недоступен ({e}); игра работает без Steam");
                    None
                }
            };
            let mut callbacks = Vec::new();
            if let Some(client) = &client {
                // Start connecting to the Steam relay network now, so the first
                // P2P connection does not have to wait for it.
                client.networking_utils().init_relay_network_access();
                // Accept P2P sessions; game messages are validated by the host anyway.
                callbacks.push(client.register_callback(|req: SessionRequest| {
                    let who = NetworkingIdentity::from(req.0).steam_id().map(|id| id.raw());
                    if accept_session(&req.0) {
                        info!("Steam: принято P2P-соединение от {who:?}");
                    } else {
                        warn!("Steam: не удалось принять P2P-соединение от {who:?}");
                    }
                }));
                let tx = events_tx.clone();
                callbacks.push(client.register_callback(
                    move |f: NetworkingMessagesSessionFailed| {
                        let Some(who) = f.info.identity_remote().and_then(|i| i.steam_id()) else {
                            return;
                        };
                        let reason = match f.info.end_reason() {
                            Some(r) => format!("{r:?}"),
                            None => "неизвестная ошибка".to_string(),
                        };
                        warn!("Steam: P2P-соединение с {} оборвалось: {reason}", who.raw());
                        let _ = tx.send(SteamEvent::SessionFailed(who.raw(), reason));
                    },
                ));
                let tx = events_tx.clone();
                callbacks.push(client.register_callback(move |r: GameLobbyJoinRequested| {
                    let _ = tx.send(SteamEvent::JoinRequested(r.lobby_steam_id.raw()));
                }));
                let updates = member_updates.clone();
                callbacks.push(client.register_callback(move |u: LobbyChatUpdate| {
                    updates.lock().unwrap().push(u);
                }));
                // Launched through an invite: "+connect_lobby <id>" on the command line.
                let args: Vec<String> = std::env::args().collect();
                if let Some(pos) = args.iter().position(|a| a == "+connect_lobby") {
                    if let Some(id) = args.get(pos + 1).and_then(|s| s.parse::<u64>().ok()) {
                        let _ = events_tx.send(SteamEvent::JoinRequested(id));
                    }
                }
            }
            SteamApi {
                client,
                events_tx,
                events_rx: Mutex::new(events_rx),
                lobby,
                member_updates,
                _callbacks: callbacks,
            }
        }

        pub fn available(&self) -> bool {
            self.client.is_some()
        }

        pub fn persona_name(&self) -> Option<String> {
            Some(self.client.as_ref()?.friends().name())
        }

        pub fn steam_id(&self) -> Option<u64> {
            Some(self.client.as_ref()?.user().steam_id().raw())
        }

        pub fn create_lobby(&self, max_members: u32) {
            let Some(client) = &self.client else { return };
            self.leave_lobby();
            let tx = self.events_tx.clone();
            let lobby = self.lobby.clone();
            client
                .matchmaking()
                .create_lobby(LobbyType::FriendsOnly, max_members, move |r| {
                    let r = r.map(|id| {
                        *lobby.lock().unwrap() = Some(id);
                        id.raw()
                    });
                    let _ = tx.send(SteamEvent::LobbyCreated(r.map_err(|e| e.to_string())));
                });
        }

        pub fn join_lobby(&self, lobby_id: u64) {
            let Some(client) = &self.client else { return };
            self.leave_lobby();
            let tx = self.events_tx.clone();
            let lobby = self.lobby.clone();
            let mm = client.matchmaking();
            let owner_client = client.clone();
            mm.join_lobby(LobbyId::from_raw(lobby_id), move |r| {
                let result = match r {
                    Ok(id) => {
                        *lobby.lock().unwrap() = Some(id);
                        let owner = owner_client.matchmaking().lobby_owner(id);
                        Ok((id.raw(), owner.raw()))
                    }
                    Err(()) => Err("не удалось войти в лобби Steam".to_string()),
                };
                let _ = tx.send(SteamEvent::LobbyJoined(result));
            });
        }

        pub fn leave_lobby(&self) {
            let Some(client) = &self.client else { return };
            if let Some(id) = self.lobby.lock().unwrap().take() {
                client.matchmaking().leave_lobby(id);
            }
        }

        pub fn open_invite_overlay(&self) {
            let Some(client) = &self.client else { return };
            if let Some(id) = *self.lobby.lock().unwrap() {
                client.friends().activate_invite_dialog(id);
            }
        }

        pub fn set_lobby_data(&self, key: &str, value: &str) {
            let Some(client) = &self.client else { return };
            if let Some(id) = *self.lobby.lock().unwrap() {
                client.matchmaking().set_lobby_data(id, key, value);
            }
        }

        pub fn poll_events(&self) -> Vec<SteamEvent> {
            self.events_rx.lock().unwrap().try_iter().collect()
        }

        pub fn host_transport(&self) -> Option<Box<dyn Transport>> {
            let client = self.client.clone()?;
            Some(Box::new(SteamTransport {
                client,
                host: None,
                known: HashSet::new(),
                member_updates: self.member_updates.clone(),
                announced: false,
            }))
        }

        pub fn client_transport(&self, host: u64) -> Option<Box<dyn Transport>> {
            let client = self.client.clone()?;
            Some(Box::new(SteamTransport {
                client,
                host: Some(SteamId::from_raw(host)),
                known: HashSet::new(),
                member_updates: self.member_updates.clone(),
                announced: false,
            }))
        }

        fn run_callbacks(&self) {
            if let Some(c) = &self.client {
                c.run_callbacks();
            }
        }
    }

    /// Game traffic over Steam Networking Messages. Peers are identified by
    /// their raw Steam ids; on a client the host is [`HOST_PEER`].
    struct SteamTransport {
        client: Client,
        host: Option<SteamId>,
        known: HashSet<u64>,
        member_updates: Arc<Mutex<Vec<LobbyChatUpdate>>>,
        announced: bool,
    }

    impl Transport for SteamTransport {
        fn poll(&mut self) -> Vec<NetEvent> {
            let mut events = Vec::new();
            if let (Some(_), false) = (self.host, self.announced) {
                self.announced = true;
                events.push(NetEvent::Connected(HOST_PEER));
            }
            for msg in self
                .client
                .networking_messages()
                .receive_messages_on_channel(CHANNEL, 64)
            {
                let Some(sender) = msg.identity_peer().steam_id() else { continue };
                let data = msg.data().to_vec();
                match self.host {
                    Some(host) => {
                        if sender == host {
                            events.push(NetEvent::Message(HOST_PEER, data));
                        }
                    }
                    None => {
                        let peer = sender.raw();
                        if self.known.insert(peer) {
                            events.push(NetEvent::Connected(peer));
                        }
                        events.push(NetEvent::Message(peer, data));
                    }
                }
            }
            for update in self.member_updates.lock().unwrap().drain(..) {
                let left = matches!(
                    update.member_state_change,
                    ChatMemberStateChange::Left
                        | ChatMemberStateChange::Disconnected
                        | ChatMemberStateChange::Kicked
                        | ChatMemberStateChange::Banned
                );
                if !left {
                    continue;
                }
                let who = update.user_changed;
                match self.host {
                    Some(host) if who == host => events.push(NetEvent::Disconnected(HOST_PEER)),
                    None if self.known.remove(&who.raw()) => {
                        events.push(NetEvent::Disconnected(who.raw()))
                    }
                    _ => {}
                }
            }
            events
        }

        fn send(&mut self, peer: PeerId, data: &[u8]) {
            let target = match self.host {
                Some(host) => host,
                None => SteamId::from_raw(peer),
            };
            let _ = self.client.networking_messages().send_message_to_user(
                NetworkingIdentity::new_steam_id(target),
                SendFlags::RELIABLE | SendFlags::AUTO_RESTART_BROKEN_SESSION,
                data,
                CHANNEL,
            );
        }

        fn disconnect(&mut self, peer: PeerId) {
            self.known.remove(&peer);
        }

        fn join_hint(&self) -> Option<String> {
            Some("Steam: пригласите друзей через оверлей".into())
        }
    }

    fn pump(steam: Res<SteamApi>) {
        steam.run_callbacks();
    }

    impl Plugin for SteamPlugin {
        fn build(&self, app: &mut App) {
            app.insert_resource(SteamApi::init())
                .add_systems(PreUpdate, pump);
        }
    }
}

pub use imp::SteamApi;
