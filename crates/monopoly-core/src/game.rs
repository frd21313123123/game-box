//! Game state and the reducer: every rule of the game lives here.
//!
//! Ported from the TypeScript `@monopoly/core` package. The reducer is
//! deterministic (seeded mulberry32), and an action that breaks a rule is
//! rejected without touching the state ([`GameState::apply`] returns `false`).

use serde::{Deserialize, Serialize};

use crate::board::{
    mortgage_value, tile, tiles_in_group, unmortgage_cost, ColorGroup, TileIndex, TileKind,
    BOARD_SIZE, JAIL_INDEX, STATION_INDICES, UTILITY_INDICES,
};
use crate::cards::{card, CardEffect, Deck, CHANCE_CARDS, CHEST_CARDS};
use crate::tokens::{MAX_PLAYERS, MIN_PLAYERS, TOKENS, TOKEN_COLORS};

pub type PlayerId = u32;

pub const STARTING_MONEY: i32 = 1500;
pub const GO_BONUS: i32 = 200;
pub const JAIL_FINE: i32 = 50;
pub const MAX_JAIL_TURNS: u32 = 3;
pub const HOTEL_LEVEL: u8 = 5;
pub const MIN_BID_INCREMENT: i32 = 10;
const LOG_CAP: usize = 100;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Player {
    pub id: PlayerId,
    pub name: String,
    /// Index into [`TOKENS`].
    pub token: usize,
    /// Index into [`TOKEN_COLORS`]; falls back to the token's own colour.
    pub color: Option<usize>,
    pub position: TileIndex,
    pub money: i32,
    pub in_jail: bool,
    pub jail_turns: u32,
    pub owned: Vec<TileIndex>,
    pub jail_free_cards: u32,
    pub bankrupt: bool,
}

impl Player {
    pub fn rgb(&self) -> [u8; 3] {
        self.color
            .and_then(|c| TOKEN_COLORS.get(c).copied())
            .unwrap_or(TOKENS[self.token].color)
    }

    pub fn symbol(&self) -> &'static str {
        TOKENS[self.token].symbol
    }

    pub fn owns(&self, t: TileIndex) -> bool {
        self.owned.contains(&t)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Phase {
    Lobby,
    Playing,
    Finished,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct DiceRoll {
    pub a: u8,
    pub b: u8,
}

impl DiceRoll {
    pub fn sum(&self) -> u32 {
        (self.a + self.b) as u32
    }
    pub fn is_double(&self) -> bool {
        self.a == self.b
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PendingPurchase {
    pub tile: TileIndex,
    pub price: i32,
}

/// The current player offers the unowned tile they landed on to another player.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PendingOffer {
    pub tile: TileIndex,
    pub from: PlayerId,
    pub to: PlayerId,
    pub price: i32,
    pub original_price: i32,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PendingAuction {
    pub tile: TileIndex,
    pub current_bid: i32,
    pub high_bidder: Option<PlayerId>,
    pub active: Vec<PlayerId>,
    pub turn_index: usize,
}

impl PendingAuction {
    pub fn turn_player(&self) -> Option<PlayerId> {
        self.active.get(self.turn_index).copied()
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct TradeBundle {
    pub tiles: Vec<TileIndex>,
    pub money: i32,
    pub jail_free_cards: u32,
}

impl TradeBundle {
    pub fn is_empty(&self) -> bool {
        self.tiles.is_empty() && self.money == 0 && self.jail_free_cards == 0
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PendingTrade {
    pub from: PlayerId,
    pub to: PlayerId,
    pub from_offer: TradeBundle,
    pub to_offer: TradeBundle,
}

/// A debt the current player must cover (by selling/mortgaging) or go bankrupt.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PendingDebt {
    pub debtor: PlayerId,
    /// `None` = owed to the bank.
    pub creditor: Option<PlayerId>,
    pub amount: i32,
    /// Set when the debt is the forced jail fine: once paid, the debtor leaves
    /// jail and moves by this dice sum.
    pub jail_move_sum: Option<u32>,
}

/// Waypoints of the latest move so the UI can animate leg by leg.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct LastMove {
    pub player: PlayerId,
    pub path: Vec<TileIndex>,
    pub seq: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum LogKind {
    GameStarted,
    Rolled,
    MovedTo,
    PassedGo,
    DoubleRoll,
    Bought,
    Declined,
    Rent,
    Tax,
    GoToJail,
    Bankrupt,
    GameWon,
    BuiltHouse,
    BuiltHotel,
    SoldHouse,
    DrewCard,
    ThirdDoubleJail,
    JailRoll,
    JailEscape,
    JailRollFailed,
    JailForcedFine,
    JailPaid,
    JailUsedCard,
    Mortgaged,
    Unmortgaged,
    AuctionStarted,
    AuctionBid,
    AuctionPass,
    AuctionWon,
    AuctionNoWinner,
    DebtPaid,
    TradeProposed,
    TradeAccepted,
    TradeDeclined,
    TradeCancelled,
    OfferProposed,
    OfferAccepted,
    OfferDeclined,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct LogEntry {
    pub turn: u32,
    pub player: Option<PlayerId>,
    pub kind: LogKind,
    pub text: String,
    /// For card draws: which deck and the card's text.
    pub card: Option<(Deck, String)>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Action {
    AddPlayer { name: String, token: usize, color: Option<usize> },
    RemovePlayer { id: PlayerId },
    StartGame,
    RollAndMove,
    BuyCurrent,
    DeclinePurchase,
    AuctionCurrent,
    OfferPurchase { to: PlayerId, price: i32 },
    OfferAccept,
    OfferDecline,
    EndTurn,
    /// Host-only: force the turn to pass (e.g. a player left).
    SkipTurn,
    BuyHouse(TileIndex),
    SellHouse(TileIndex),
    Mortgage(TileIndex),
    Unmortgage(TileIndex),
    JailRoll,
    JailPayFine,
    JailUseCard,
    DebtPay,
    DebtDeclareBankruptcy,
    AuctionBid { player: PlayerId, amount: i32 },
    AuctionPass { player: PlayerId },
    TradePropose { from: PlayerId, to: PlayerId, from_offer: TradeBundle, to_offer: TradeBundle },
    TradeAccept,
    TradeDecline,
    TradeCancel,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct GameState {
    pub phase: Phase,
    pub players: Vec<Player>,
    pub current: usize,
    pub turn: u32,
    pub rng_state: u32,
    pub last_roll: Option<DiceRoll>,
    /// Bumped on every real dice roll; the UI animates dice off this.
    pub roll_seq: u32,
    pub last_move: Option<LastMove>,
    pub doubles_this_turn: u32,
    pub pending_end_turn: bool,
    pub pending_purchase: Option<PendingPurchase>,
    pub pending_offer: Option<PendingOffer>,
    pub pending_auction: Option<PendingAuction>,
    pub pending_trade: Option<PendingTrade>,
    pub pending_debt: Option<PendingDebt>,
    /// Building level per tile: 0 none, 1–4 houses, 5 hotel.
    pub buildings: Vec<u8>,
    pub mortgaged: Vec<bool>,
    /// Remaining card order; top of the deck is element 0.
    pub chance_deck: Vec<usize>,
    pub chest_deck: Vec<usize>,
    pub log: Vec<LogEntry>,
    /// Total log entries ever appended (the log itself is capped).
    pub log_seq: u32,
}

// ---------------------------------------------------------------------------
// Randomness (mulberry32, same as the TypeScript original)
// ---------------------------------------------------------------------------

fn mulberry32_step(state: u32) -> (f64, u32) {
    let next = state.wrapping_add(0x6d2b_79f5);
    let mut t = next;
    t = (t ^ (t >> 15)).wrapping_mul(t | 1);
    t ^= t.wrapping_add((t ^ (t >> 7)).wrapping_mul(t | 61));
    ((t ^ (t >> 14)) as f64 / 4_294_967_296.0, next)
}

fn roll_dice(state: u32) -> (DiceRoll, u32) {
    let (v1, s1) = mulberry32_step(state);
    let (v2, s2) = mulberry32_step(s1);
    (
        DiceRoll {
            a: 1 + (v1 * 6.0) as u8,
            b: 1 + (v2 * 6.0) as u8,
        },
        s2,
    )
}

fn shuffle_deck(len: usize, mut state: u32) -> (Vec<usize>, u32) {
    let mut order: Vec<usize> = (0..len).collect();
    for i in (1..len).rev() {
        let (v, next) = mulberry32_step(state);
        state = next;
        let j = (v * (i + 1) as f64) as usize;
        order.swap(i, j);
    }
    (order, state)
}

// ---------------------------------------------------------------------------
// Queries
// ---------------------------------------------------------------------------

impl GameState {
    pub fn new(seed: u32) -> Self {
        GameState {
            phase: Phase::Lobby,
            players: Vec::new(),
            current: 0,
            turn: 0,
            rng_state: seed,
            last_roll: None,
            roll_seq: 0,
            last_move: None,
            doubles_this_turn: 0,
            pending_end_turn: false,
            pending_purchase: None,
            pending_offer: None,
            pending_auction: None,
            pending_trade: None,
            pending_debt: None,
            buildings: vec![0; BOARD_SIZE],
            mortgaged: vec![false; BOARD_SIZE],
            chance_deck: Vec::new(),
            chest_deck: Vec::new(),
            log: Vec::new(),
            log_seq: 0,
        }
    }

    pub fn current_player(&self) -> Option<&Player> {
        self.players.get(self.current)
    }

    pub fn player(&self, id: PlayerId) -> Option<&Player> {
        self.players.iter().find(|p| p.id == id)
    }

    pub fn player_index(&self, id: PlayerId) -> Option<usize> {
        self.players.iter().position(|p| p.id == id)
    }

    pub fn owner_of(&self, t: TileIndex) -> Option<&Player> {
        self.players.iter().find(|p| !p.bankrupt && p.owns(t))
    }

    pub fn building_level(&self, t: TileIndex) -> u8 {
        self.buildings[t]
    }

    pub fn is_mortgaged(&self, t: TileIndex) -> bool {
        self.mortgaged[t]
    }

    pub fn owns_full_group(&self, player: &Player, t: TileIndex) -> bool {
        match tile(t).group() {
            Some(g) => tiles_in_group(g).all(|i| player.owns(i)),
            None => false,
        }
    }

    fn group_min_level(&self, g: ColorGroup) -> u8 {
        tiles_in_group(g).map(|i| self.buildings[i]).min().unwrap_or(0)
    }

    fn group_max_level(&self, g: ColorGroup) -> u8 {
        tiles_in_group(g).map(|i| self.buildings[i]).max().unwrap_or(0)
    }

    fn group_has_buildings(&self, g: ColorGroup) -> bool {
        tiles_in_group(g).any(|i| self.buildings[i] > 0)
    }

    /// Rent the owner of `t` would charge right now (`None`: no rent).
    pub fn compute_rent(&self, t: TileIndex, dice_sum: u32) -> Option<(PlayerId, i32)> {
        let owner = self.owner_of(t)?;
        if self.mortgaged[t] {
            return None;
        }
        let amount = match tile(t).kind {
            TileKind::Street { rent, .. } => match self.buildings[t] {
                0 if self.owns_full_group(owner, t) => rent.mono,
                0 => rent.base,
                l @ 1..=4 => rent.houses[l as usize - 1],
                _ => rent.hotel,
            },
            TileKind::Station { .. } => {
                let n = STATION_INDICES.iter().filter(|i| owner.owns(**i)).count() as u32;
                if n == 0 {
                    0
                } else {
                    25 * 2i32.pow(n - 1)
                }
            }
            TileKind::Utility { .. } => {
                let n = UTILITY_INDICES.iter().filter(|i| owner.owns(**i)).count();
                dice_sum as i32 * if n >= 2 { 10 } else { 4 }
            }
            _ => return None,
        };
        Some((owner.id, amount))
    }

    /// `Ok(cost)` if `player` may build on `t` now.
    pub fn can_buy_house(&self, player: &Player, t: TileIndex) -> Result<i32, &'static str> {
        let TileKind::Street { group, .. } = tile(t).kind else {
            return Err("не улица");
        };
        if !player.owns(t) {
            return Err("не ваша");
        }
        if !self.owns_full_group(player, t) {
            return Err("нет монополии");
        }
        if tiles_in_group(group).any(|i| self.mortgaged[i]) {
            return Err("в группе есть заложенные");
        }
        let level = self.buildings[t];
        if level >= HOTEL_LEVEL {
            return Err("уже отель");
        }
        if level != self.group_min_level(group) {
            return Err("стройте равномерно");
        }
        let cost = group.house_cost();
        if player.money < cost {
            return Err("не хватает денег");
        }
        Ok(cost)
    }

    /// `Ok(refund)` if `player` may sell a building on `t` now.
    pub fn can_sell_house(&self, player: &Player, t: TileIndex) -> Result<i32, &'static str> {
        let TileKind::Street { group, .. } = tile(t).kind else {
            return Err("не улица");
        };
        if !player.owns(t) {
            return Err("не ваша");
        }
        let level = self.buildings[t];
        if level == 0 {
            return Err("нечего продавать");
        }
        if level != self.group_max_level(group) {
            return Err("продавайте равномерно");
        }
        Ok(group.house_cost() / 2)
    }

    pub fn can_mortgage(&self, player: &Player, t: TileIndex) -> Result<i32, &'static str> {
        if !player.owns(t) {
            return Err("не ваша");
        }
        if self.mortgaged[t] {
            return Err("уже заложена");
        }
        let tl = tile(t);
        if !tl.is_purchasable() {
            return Err("нельзя заложить");
        }
        if let Some(g) = tl.group() {
            if self.group_has_buildings(g) {
                return Err("в группе есть постройки");
            }
        }
        Ok(mortgage_value(t))
    }

    pub fn can_unmortgage(&self, player: &Player, t: TileIndex) -> Result<i32, &'static str> {
        if !player.owns(t) {
            return Err("не ваша");
        }
        if !self.mortgaged[t] {
            return Err("не заложена");
        }
        let cost = unmortgage_cost(t);
        if player.money < cost {
            return Err("не хватает денег");
        }
        Ok(cost)
    }

    pub fn trade_bundle_valid(&self, player: &Player, b: &TradeBundle) -> Result<(), &'static str> {
        if b.money < 0 {
            return Err("отрицательная сумма");
        }
        if player.money < b.money {
            return Err("не хватает денег");
        }
        if player.jail_free_cards < b.jail_free_cards {
            return Err("не хватает карт");
        }
        for &t in &b.tiles {
            if !player.owns(t) {
                return Err("участок не принадлежит игроку");
            }
            if let Some(g) = tile(t).group() {
                if self.group_has_buildings(g) {
                    return Err("сначала продайте постройки");
                }
            }
        }
        Ok(())
    }

    /// Value of everything a player owns (cash + property at mortgage value +
    /// buildings at sell value) — what they could raise to pay a debt.
    pub fn liquidation_value(&self, player: &Player) -> i32 {
        let mut total = player.money;
        for &t in &player.owned {
            if !self.mortgaged[t] {
                total += mortgage_value(t);
            }
            if let Some(g) = tile(t).group() {
                let level = self.buildings[t] as i32;
                total += level * g.house_cost() / 2;
            }
        }
        total
    }

    pub fn winner(&self) -> Option<&Player> {
        if self.phase == Phase::Finished {
            self.players.iter().find(|p| !p.bankrupt)
        } else {
            None
        }
    }

    /// Whether `actor` is allowed to submit `action` (server-side authorization).
    pub fn can_submit(&self, actor: PlayerId, action: &Action) -> bool {
        let current = self.current_player().map(|p| p.id);
        match action {
            Action::AddPlayer { .. } | Action::RemovePlayer { .. } | Action::StartGame => {
                self.players.first().map(|p| p.id) == Some(actor)
            }
            Action::RollAndMove
            | Action::BuyCurrent
            | Action::DeclinePurchase
            | Action::AuctionCurrent
            | Action::OfferPurchase { .. }
            | Action::EndTurn
            | Action::BuyHouse(_)
            | Action::SellHouse(_)
            | Action::Mortgage(_)
            | Action::Unmortgage(_)
            | Action::JailRoll
            | Action::JailPayFine
            | Action::JailUseCard
            | Action::DebtPay
            | Action::DebtDeclareBankruptcy => current == Some(actor),
            Action::AuctionBid { player, .. } | Action::AuctionPass { player } => {
                let expected = self.pending_auction.as_ref().and_then(|a| a.turn_player());
                expected == Some(actor) && *player == actor
            }
            Action::TradePropose { from, .. } => *from == actor,
            Action::TradeAccept | Action::TradeDecline => {
                self.pending_trade.as_ref().map(|t| t.to) == Some(actor)
            }
            Action::TradeCancel => {
                self.pending_trade.as_ref().map(|t| t.from) == Some(actor) || current == Some(actor)
            }
            Action::OfferAccept | Action::OfferDecline => {
                self.pending_offer.as_ref().map(|o| o.to) == Some(actor)
            }
            Action::SkipTurn => false,
        }
    }
}

// ---------------------------------------------------------------------------
// Reducer
// ---------------------------------------------------------------------------

impl GameState {
    /// Applies `action`. Returns `false` (leaving the state untouched) when the
    /// action is not allowed right now.
    pub fn apply(&mut self, action: &Action) -> bool {
        let mut next = self.clone();
        if next.reduce(action) {
            *self = next;
            true
        } else {
            false
        }
    }

    fn reduce(&mut self, action: &Action) -> bool {
        match action {
            Action::AddPlayer { name, token, color } => self.add_player(name, *token, *color),
            Action::RemovePlayer { id } => self.remove_player(*id),
            Action::StartGame => self.start_game(),
            Action::RollAndMove => self.roll_and_move(),
            Action::BuyCurrent => self.buy_current(),
            Action::DeclinePurchase => self.decline_purchase(),
            Action::AuctionCurrent => self.auction_current(),
            Action::OfferPurchase { to, price } => self.offer_purchase(*to, *price),
            Action::OfferAccept => self.offer_accept(),
            Action::OfferDecline => self.offer_decline(),
            Action::EndTurn => self.end_turn(),
            Action::SkipTurn => self.skip_turn(),
            Action::BuyHouse(t) => self.buy_house(*t),
            Action::SellHouse(t) => self.sell_house(*t),
            Action::Mortgage(t) => self.mortgage(*t),
            Action::Unmortgage(t) => self.unmortgage(*t),
            Action::JailRoll => self.jail_roll(),
            Action::JailPayFine => self.jail_pay_fine(),
            Action::JailUseCard => self.jail_use_card(),
            Action::DebtPay => self.pay_debt(),
            Action::DebtDeclareBankruptcy => self.debt_declare_bankruptcy(),
            Action::AuctionBid { player, amount } => self.auction_bid(*player, *amount),
            Action::AuctionPass { player } => self.auction_pass(*player),
            Action::TradePropose {
                from,
                to,
                from_offer,
                to_offer,
            } => self.trade_propose(*from, *to, from_offer, to_offer),
            Action::TradeAccept => self.trade_accept(),
            Action::TradeDecline => self.trade_decline(),
            Action::TradeCancel => self.trade_cancel(),
        }
    }

    fn log(&mut self, player: Option<PlayerId>, kind: LogKind, text: String) {
        self.log_with_card(player, kind, text, None);
    }

    fn log_with_card(
        &mut self,
        player: Option<PlayerId>,
        kind: LogKind,
        text: String,
        card: Option<(Deck, String)>,
    ) {
        self.log.push(LogEntry {
            turn: self.turn,
            player,
            kind,
            text,
            card,
        });
        if self.log.len() > LOG_CAP {
            let excess = self.log.len() - LOG_CAP;
            self.log.drain(..excess);
        }
        self.log_seq += 1;
    }

    fn name_of(&self, id: PlayerId) -> String {
        self.player(id).map(|p| p.name.clone()).unwrap_or_else(|| "—".into())
    }

    fn creditor_name(&self, creditor: Option<PlayerId>) -> String {
        match creditor {
            Some(id) => self.name_of(id),
            None => "банк".into(),
        }
    }

    fn begin_move(&mut self, player: PlayerId) {
        let seq = self.last_move.as_ref().map(|m| m.seq).unwrap_or(0) + 1;
        self.last_move = Some(LastMove {
            player,
            path: Vec::new(),
            seq,
        });
    }

    fn add_waypoint(&mut self, t: TileIndex) {
        if let Some(m) = &mut self.last_move {
            m.path.push(t);
        }
    }

    fn playing(&self) -> bool {
        self.phase == Phase::Playing
    }

    fn current_active(&self) -> Option<usize> {
        let p = self.players.get(self.current)?;
        (!p.bankrupt).then_some(self.current)
    }

    // --- lobby ------------------------------------------------------------

    fn add_player(&mut self, name: &str, token: usize, color: Option<usize>) -> bool {
        if self.phase != Phase::Lobby || self.players.len() >= MAX_PLAYERS {
            return false;
        }
        if token >= TOKENS.len() || self.players.iter().any(|p| p.token == token) {
            return false;
        }
        if matches!(color, Some(c) if c >= TOKEN_COLORS.len()) {
            return false;
        }
        let name = name.trim();
        if name.is_empty() {
            return false;
        }
        let id = self.players.iter().map(|p| p.id).max().unwrap_or(0) + 1;
        self.players.push(Player {
            id,
            name: name.chars().take(24).collect(),
            token,
            color,
            position: 0,
            money: STARTING_MONEY,
            in_jail: false,
            jail_turns: 0,
            owned: Vec::new(),
            jail_free_cards: 0,
            bankrupt: false,
        });
        true
    }

    fn remove_player(&mut self, id: PlayerId) -> bool {
        if self.phase != Phase::Lobby {
            return false;
        }
        let before = self.players.len();
        self.players.retain(|p| p.id != id);
        self.players.len() != before
    }

    fn start_game(&mut self) -> bool {
        if self.phase != Phase::Lobby || self.players.len() < MIN_PLAYERS {
            return false;
        }
        let (chance, s1) = shuffle_deck(CHANCE_CARDS.len(), self.rng_state);
        let (chest, s2) = shuffle_deck(CHEST_CARDS.len(), s1);
        self.phase = Phase::Playing;
        self.current = 0;
        self.turn = 1;
        self.rng_state = s2;
        self.chance_deck = chance;
        self.chest_deck = chest;
        let count = self.players.len();
        self.log(None, LogKind::GameStarted, format!("Игра началась ({count} игроков)"));
        true
    }

    // --- movement -----------------------------------------------------------

    fn roll_and_move(&mut self) -> bool {
        if !self.playing()
            || self.pending_end_turn
            || self.pending_purchase.is_some()
            || self.pending_offer.is_some()
            || self.pending_auction.is_some()
            || self.pending_debt.is_some()
        {
            return false;
        }
        let Some(idx) = self.current_active() else {
            return false;
        };
        if self.players[idx].in_jail {
            return false;
        }
        let (roll, next_rng) = roll_dice(self.rng_state);
        self.rng_state = next_rng;
        self.last_roll = Some(roll);
        self.roll_seq += 1;
        let id = self.players[idx].id;
        let name = self.players[idx].name.clone();

        // Third double in a row: straight to jail.
        if roll.is_double() && self.doubles_this_turn + 1 == 3 {
            let p = &mut self.players[idx];
            p.position = JAIL_INDEX;
            p.in_jail = true;
            p.jail_turns = 0;
            self.doubles_this_turn = 0;
            self.pending_end_turn = true;
            self.begin_move(id);
            self.add_waypoint(JAIL_INDEX);
            self.log(
                Some(id),
                LogKind::ThirdDoubleJail,
                format!("{name} выбросил 3-й дубль подряд — в тюрьму!"),
            );
            return true;
        }

        let old = self.players[idx].position;
        let new = (old + roll.sum() as usize) % BOARD_SIZE;
        let passed_go = new < old;
        self.players[idx].position = new;
        if passed_go {
            self.players[idx].money += GO_BONUS;
        }
        self.doubles_this_turn = if roll.is_double() {
            self.doubles_this_turn + 1
        } else {
            0
        };
        self.pending_end_turn = false;
        self.begin_move(id);
        self.add_waypoint(new);
        self.log(
            Some(id),
            LogKind::Rolled,
            format!("{name} бросает: {} + {} = {}", roll.a, roll.b, roll.sum()),
        );
        if passed_go {
            self.log(Some(id), LogKind::PassedGo, format!("{name} проходит СТАРТ (+₽{GO_BONUS})"));
        }
        self.log(
            Some(id),
            LogKind::MovedTo,
            format!("{name} переходит на «{}»", tile(new).name),
        );

        self.process_landing(id, new, roll.sum(), 1, false);

        let after = &self.players[idx];
        let sent_to_jail = after.in_jail;
        let went_bankrupt = after.bankrupt;
        let must_end = !roll.is_double() || sent_to_jail || went_bankrupt;
        if roll.is_double() && !sent_to_jail && !went_bankrupt {
            self.log(
                Some(id),
                LogKind::DoubleRoll,
                format!("{name} выбросил дубль — бросает ещё раз"),
            );
        }
        if self.phase == Phase::Playing {
            self.pending_end_turn = must_end;
        }
        true
    }

    fn move_after_jail(&mut self, id: PlayerId, sum: u32) {
        let Some(idx) = self.player_index(id) else {
            return;
        };
        let name = self.players[idx].name.clone();
        let old = self.players[idx].position;
        let new = (old + sum as usize) % BOARD_SIZE;
        let passed_go = new < old;
        self.players[idx].position = new;
        if passed_go {
            self.players[idx].money += GO_BONUS;
        }
        self.begin_move(id);
        self.add_waypoint(new);
        if passed_go {
            self.log(Some(id), LogKind::PassedGo, format!("{name} проходит СТАРТ (+₽{GO_BONUS})"));
        }
        self.log(
            Some(id),
            LogKind::MovedTo,
            format!("{name} переходит на «{}»", tile(new).name),
        );
        self.process_landing(id, new, sum, 1, false);
        if self.phase == Phase::Playing {
            self.pending_end_turn = true;
        }
        self.doubles_this_turn = 0;
    }

    fn process_landing(
        &mut self,
        id: PlayerId,
        t: TileIndex,
        dice_sum: u32,
        rent_multiplier: i32,
        force_utility_ten_x: bool,
    ) {
        let Some(idx) = self.player_index(id) else {
            return;
        };
        let name = self.players[idx].name.clone();
        let tl = tile(t);
        match tl.kind {
            TileKind::Street { .. } | TileKind::Station { .. } | TileKind::Utility { .. } => {
                let Some(owner) = self.owner_of(t).map(|o| (o.id, o.name.clone())) else {
                    if self.players[idx].money > 0 {
                        self.pending_purchase = Some(PendingPurchase {
                            tile: t,
                            price: tl.price().unwrap_or(0),
                        });
                    }
                    return;
                };
                if owner.0 == id {
                    return;
                }
                let Some((_, rent)) = self.compute_rent(t, dice_sum) else {
                    return;
                };
                let mut amount = rent * rent_multiplier;
                if force_utility_ten_x && matches!(tl.kind, TileKind::Utility { .. }) {
                    amount = dice_sum as i32 * 10;
                }
                self.log(
                    Some(id),
                    LogKind::Rent,
                    format!("{name} платит ренту ₽{amount} игроку {} за «{}»", owner.1, tl.name),
                );
                self.pay_or_bankrupt(id, Some(owner.0), amount);
            }
            TileKind::Tax { amount } => {
                self.log(Some(id), LogKind::Tax, format!("{name} платит налог ₽{amount}"));
                self.pay_or_bankrupt(id, None, amount);
            }
            TileKind::GoToJail => {
                let p = &mut self.players[idx];
                p.position = JAIL_INDEX;
                p.in_jail = true;
                p.jail_turns = 0;
                self.doubles_this_turn = 0;
                self.add_waypoint(JAIL_INDEX);
                self.log(Some(id), LogKind::GoToJail, format!("{name} отправляется в тюрьму"));
            }
            TileKind::Chance => self.draw_card(id, Deck::Chance, dice_sum),
            TileKind::Chest => self.draw_card(id, Deck::Chest, dice_sum),
            TileKind::Go | TileKind::Jail | TileKind::FreeParking => {}
        }
    }

    fn draw_card(&mut self, id: PlayerId, deck: Deck, dice_sum: u32) {
        let pile = match deck {
            Deck::Chance => &self.chance_deck,
            Deck::Chest => &self.chest_deck,
        };
        let Some(&top) = pile.first() else {
            return;
        };
        let c = card(deck, top);
        let name = self.name_of(id);
        self.log_with_card(
            Some(id),
            LogKind::DrewCard,
            format!("{name} тянет «{}»: {}", deck.name(), c.text),
            Some((deck, c.text.to_string())),
        );

        // Take the card off the deck first (a jail-free card is kept by the
        // player until used; everything else goes to the bottom).
        let pile = match deck {
            Deck::Chance => &mut self.chance_deck,
            Deck::Chest => &mut self.chest_deck,
        };
        pile.remove(0);
        if c.effect != CardEffect::GetOutOfJailFree {
            pile.push(top);
        }

        self.apply_card_effect(id, c.effect, dice_sum);
    }

    fn apply_card_effect(&mut self, id: PlayerId, effect: CardEffect, dice_sum: u32) {
        let Some(idx) = self.player_index(id) else {
            return;
        };
        let pos = self.players[idx].position;
        match effect {
            CardEffect::MoveTo { tile: target, collect_go } => {
                self.card_move_to(id, target, collect_go, dice_sum, 1, false)
            }
            CardEffect::MoveRelative { delta } => {
                let target = (pos as i32 + delta).rem_euclid(BOARD_SIZE as i32) as usize;
                self.card_move_to(id, target, false, dice_sum, 1, false)
            }
            CardEffect::MoveToNearestStation { pay_double } => {
                let target = nearest_ahead(pos, &STATION_INDICES);
                self.card_move_to(id, target, true, dice_sum, if pay_double { 2 } else { 1 }, false)
            }
            CardEffect::MoveToNearestUtility => {
                let target = nearest_ahead(pos, &UTILITY_INDICES);
                self.card_move_to(id, target, true, dice_sum, 1, true)
            }
            CardEffect::CollectBank(amount) => self.players[idx].money += amount,
            CardEffect::PayBank(amount) => self.pay_or_bankrupt(id, None, amount),
            CardEffect::CollectEach(amount) => {
                let others: Vec<PlayerId> = self
                    .players
                    .iter()
                    .filter(|p| p.id != id && !p.bankrupt)
                    .map(|p| p.id)
                    .collect();
                for other in others {
                    // A payer bankrupted mid-loop is skipped by pay_or_bankrupt.
                    if self.player(other).is_some_and(|p| !p.bankrupt) {
                        self.pay_or_bankrupt(other, Some(id), amount);
                    }
                }
            }
            CardEffect::PayEach(amount) => {
                let receivers: Vec<PlayerId> = self
                    .players
                    .iter()
                    .filter(|p| p.id != id && !p.bankrupt)
                    .map(|p| p.id)
                    .collect();
                let total = amount * receivers.len() as i32;
                if self.players[idx].money < total {
                    self.pay_or_bankrupt(id, None, total);
                } else {
                    for r in receivers {
                        self.pay_or_bankrupt(id, Some(r), amount);
                    }
                }
            }
            CardEffect::GoToJail => {
                let p = &mut self.players[idx];
                p.position = JAIL_INDEX;
                p.in_jail = true;
                p.jail_turns = 0;
                self.doubles_this_turn = 0;
                self.add_waypoint(JAIL_INDEX);
            }
            CardEffect::GetOutOfJailFree => self.players[idx].jail_free_cards += 1,
            CardEffect::PayRepairs {
                per_house,
                per_hotel,
            } => {
                let total: i32 = self.players[idx]
                    .owned
                    .iter()
                    .filter(|t| tile(**t).group().is_some())
                    .map(|t| match self.buildings[*t] {
                        HOTEL_LEVEL => per_hotel,
                        l => l as i32 * per_house,
                    })
                    .sum();
                if total > 0 {
                    self.pay_or_bankrupt(id, None, total);
                }
            }
        }
    }

    fn card_move_to(
        &mut self,
        id: PlayerId,
        target: TileIndex,
        collect_go: bool,
        dice_sum: u32,
        rent_multiplier: i32,
        force_utility_ten_x: bool,
    ) {
        let Some(idx) = self.player_index(id) else {
            return;
        };
        let name = self.players[idx].name.clone();
        let passed_go = collect_go && target < self.players[idx].position;
        self.players[idx].position = target;
        if passed_go {
            self.players[idx].money += GO_BONUS;
        }
        self.add_waypoint(target);
        if passed_go {
            self.log(Some(id), LogKind::PassedGo, format!("{name} проходит СТАРТ (+₽{GO_BONUS})"));
        }
        self.log(
            Some(id),
            LogKind::MovedTo,
            format!("{name} переходит на «{}»", tile(target).name),
        );
        self.process_landing(id, target, dice_sum, rent_multiplier, force_utility_ten_x);
    }

    // --- buying -----------------------------------------------------------

    fn buy_current(&mut self) -> bool {
        if !self.playing() {
            return false;
        }
        let Some(purchase) = self.pending_purchase else {
            return false;
        };
        let Some(idx) = self.current_active() else {
            return false;
        };
        if self.players[idx].money < purchase.price {
            return false;
        }
        let p = &mut self.players[idx];
        p.money -= purchase.price;
        p.owned.push(purchase.tile);
        let (id, name) = (p.id, p.name.clone());
        self.pending_purchase = None;
        self.log(
            Some(id),
            LogKind::Bought,
            format!("{name} купил «{}» за ₽{}", tile(purchase.tile).name, purchase.price),
        );
        true
    }

    fn decline_purchase(&mut self) -> bool {
        if !self.playing() {
            return false;
        }
        let Some(purchase) = self.pending_purchase.take() else {
            return false;
        };
        let Some(p) = self.players.get(self.current) else {
            return false;
        };
        let (id, name) = (p.id, p.name.clone());
        self.log(
            Some(id),
            LogKind::Declined,
            format!("{name} отказался от покупки «{}»", tile(purchase.tile).name),
        );
        true
    }

    fn auction_current(&mut self) -> bool {
        if !self.playing() {
            return false;
        }
        let Some(purchase) = self.pending_purchase.take() else {
            return false;
        };
        let Some(p) = self.players.get(self.current) else {
            return false;
        };
        let (id, name) = (p.id, p.name.clone());
        let tl = tile(purchase.tile);
        self.log(Some(id), LogKind::Declined, format!("{name} отказался от покупки «{}»", tl.name));
        let active: Vec<PlayerId> = self
            .players
            .iter()
            .filter(|p| !p.bankrupt)
            .map(|p| p.id)
            .collect();
        if active.len() >= 2 {
            // Bidding starts with the player after the one who declined.
            let start = active.iter().position(|a| *a == id).map(|i| (i + 1) % active.len()).unwrap_or(0);
            self.pending_auction = Some(PendingAuction {
                tile: purchase.tile,
                current_bid: 0,
                high_bidder: None,
                active,
                turn_index: start,
            });
            self.log(None, LogKind::AuctionStarted, format!("Аукцион на «{}» начался", tl.name));
        }
        true
    }

    fn offer_purchase(&mut self, to: PlayerId, price: i32) -> bool {
        if !self.playing() || self.pending_offer.is_some() {
            return false;
        }
        let Some(purchase) = self.pending_purchase else {
            return false;
        };
        let Some(idx) = self.current_active() else {
            return false;
        };
        let from = self.players[idx].id;
        if to == from {
            return false;
        }
        let Some(target) = self.player(to) else {
            return false;
        };
        if target.bankrupt || price < purchase.price {
            return false;
        }
        let to_name = target.name.clone();
        let from_name = self.players[idx].name.clone();
        self.pending_purchase = None;
        self.pending_offer = Some(PendingOffer {
            tile: purchase.tile,
            from,
            to,
            price,
            original_price: purchase.price,
        });
        self.log(
            Some(from),
            LogKind::OfferProposed,
            format!(
                "{from_name} предлагает {to_name} купить «{}» за ₽{price}",
                tile(purchase.tile).name
            ),
        );
        true
    }

    fn offer_accept(&mut self) -> bool {
        if !self.playing() {
            return false;
        }
        let Some(offer) = self.pending_offer else {
            return false;
        };
        let Some(buyer_idx) = self.player_index(offer.to) else {
            return false;
        };
        let Some(seller_idx) = self.player_index(offer.from) else {
            return false;
        };
        let buyer = &self.players[buyer_idx];
        if buyer.bankrupt || buyer.money < offer.price {
            return false;
        }
        // The base price goes to the bank; only the markup goes to the seller.
        let markup = offer.price - offer.original_price;
        self.players[buyer_idx].money -= offer.price;
        self.players[buyer_idx].owned.push(offer.tile);
        self.players[seller_idx].money += markup;
        self.pending_offer = None;
        let (to_name, from_name) = (
            self.players[buyer_idx].name.clone(),
            self.players[seller_idx].name.clone(),
        );
        self.log(
            Some(offer.to),
            LogKind::OfferAccepted,
            format!(
                "{to_name} купил «{}» у {from_name} за ₽{}",
                tile(offer.tile).name,
                offer.price
            ),
        );
        true
    }

    fn offer_decline(&mut self) -> bool {
        if !self.playing() {
            return false;
        }
        let Some(offer) = self.pending_offer.take() else {
            return false;
        };
        self.pending_purchase = Some(PendingPurchase {
            tile: offer.tile,
            price: offer.original_price,
        });
        let to_name = self.name_of(offer.to);
        self.log(
            Some(offer.to),
            LogKind::OfferDeclined,
            format!("{to_name} отказался купить «{}»", tile(offer.tile).name),
        );
        true
    }

    // --- turns ------------------------------------------------------------

    fn end_turn(&mut self) -> bool {
        if !self.playing()
            || !self.pending_end_turn
            || self.pending_purchase.is_some()
            || self.pending_offer.is_some()
            || self.pending_auction.is_some()
            || self.pending_trade.is_some()
            || self.pending_debt.is_some()
        {
            return false;
        }
        let total = self.players.len();
        let mut next = (self.current + 1) % total;
        let mut safety = 0;
        while self.players[next].bankrupt && safety < total {
            next = (next + 1) % total;
            safety += 1;
        }
        if next <= self.current {
            self.turn += 1;
        }
        self.current = next;
        self.last_roll = None;
        self.doubles_this_turn = 0;
        self.pending_end_turn = false;
        true
    }

    fn skip_turn(&mut self) -> bool {
        if !self.playing() {
            return false;
        }
        self.pending_purchase = None;
        self.pending_offer = None;
        self.pending_auction = None;
        self.pending_trade = None;
        self.pending_debt = None;
        self.pending_end_turn = true;
        self.end_turn()
    }

    // --- property management ------------------------------------------------

    fn buy_house(&mut self, t: TileIndex) -> bool {
        if !self.playing() || self.pending_debt.is_some() || t >= BOARD_SIZE {
            return false;
        }
        let Some(idx) = self.current_active() else {
            return false;
        };
        let Ok(cost) = self.can_buy_house(&self.players[idx], t) else {
            return false;
        };
        self.players[idx].money -= cost;
        self.buildings[t] += 1;
        let level = self.buildings[t];
        let (id, name) = (self.players[idx].id, self.players[idx].name.clone());
        let tl = tile(t);
        if level == HOTEL_LEVEL {
            self.log(Some(id), LogKind::BuiltHotel, format!("{name} построил отель на «{}» (₽{cost})", tl.name));
        } else {
            self.log(
                Some(id),
                LogKind::BuiltHouse,
                format!("{name} построил дом на «{}» (уровень {level}, ₽{cost})", tl.name),
            );
        }
        true
    }

    fn sell_house(&mut self, t: TileIndex) -> bool {
        if !self.playing() || t >= BOARD_SIZE {
            return false;
        }
        let Some(idx) = self.current_active() else {
            return false;
        };
        let Ok(refund) = self.can_sell_house(&self.players[idx], t) else {
            return false;
        };
        self.players[idx].money += refund;
        self.buildings[t] -= 1;
        let level = self.buildings[t];
        let (id, name) = (self.players[idx].id, self.players[idx].name.clone());
        self.log(
            Some(id),
            LogKind::SoldHouse,
            format!(
                "{name} продал постройку на «{}» (осталось {level}, +₽{refund})",
                tile(t).name
            ),
        );
        true
    }

    fn mortgage(&mut self, t: TileIndex) -> bool {
        if !self.playing() || t >= BOARD_SIZE {
            return false;
        }
        let Some(idx) = self.current_active() else {
            return false;
        };
        let Ok(refund) = self.can_mortgage(&self.players[idx], t) else {
            return false;
        };
        self.players[idx].money += refund;
        self.mortgaged[t] = true;
        let (id, name) = (self.players[idx].id, self.players[idx].name.clone());
        self.log(Some(id), LogKind::Mortgaged, format!("{name} заложил «{}» (+₽{refund})", tile(t).name));
        true
    }

    fn unmortgage(&mut self, t: TileIndex) -> bool {
        if !self.playing() || self.pending_debt.is_some() || t >= BOARD_SIZE {
            return false;
        }
        let Some(idx) = self.current_active() else {
            return false;
        };
        let Ok(cost) = self.can_unmortgage(&self.players[idx], t) else {
            return false;
        };
        self.players[idx].money -= cost;
        self.mortgaged[t] = false;
        let (id, name) = (self.players[idx].id, self.players[idx].name.clone());
        self.log(Some(id), LogKind::Unmortgaged, format!("{name} выкупил «{}» (−₽{cost})", tile(t).name));
        true
    }

    // --- jail -------------------------------------------------------------

    fn jail_blocked(&self) -> bool {
        !self.playing()
            || self.pending_end_turn
            || self.pending_purchase.is_some()
            || self.pending_debt.is_some()
    }

    fn jail_roll(&mut self) -> bool {
        if self.jail_blocked() {
            return false;
        }
        let Some(idx) = self.current_active() else {
            return false;
        };
        if !self.players[idx].in_jail {
            return false;
        }
        let (roll, next_rng) = roll_dice(self.rng_state);
        self.rng_state = next_rng;
        self.last_roll = Some(roll);
        self.roll_seq += 1;
        let (id, name) = (self.players[idx].id, self.players[idx].name.clone());
        self.log(
            Some(id),
            LogKind::JailRoll,
            format!("{name} бросает в тюрьме: {} + {} = {}", roll.a, roll.b, roll.sum()),
        );

        if roll.is_double() {
            self.players[idx].in_jail = false;
            self.players[idx].jail_turns = 0;
            self.log(Some(id), LogKind::JailEscape, format!("{name} выбросил дубль — освобождён"));
            self.move_after_jail(id, roll.sum());
            return true;
        }

        self.players[idx].jail_turns += 1;
        let attempts = self.players[idx].jail_turns;
        self.log(
            Some(id),
            LogKind::JailRollFailed,
            format!("{name} не выбросил дубль (попытка {attempts}/{MAX_JAIL_TURNS})"),
        );

        if attempts >= MAX_JAIL_TURNS {
            self.log(
                Some(id),
                LogKind::JailForcedFine,
                format!("{name}: 3 попытки исчерпаны — обязан заплатить ₽{JAIL_FINE}"),
            );
            self.pay_or_bankrupt(id, None, JAIL_FINE);
            if let Some(debt) = &mut self.pending_debt {
                debt.jail_move_sum = Some(roll.sum());
                self.doubles_this_turn = 0;
                return true;
            }
            if !self.players[idx].bankrupt {
                self.players[idx].in_jail = false;
                self.players[idx].jail_turns = 0;
                self.move_after_jail(id, roll.sum());
                return true;
            }
        }
        if self.phase == Phase::Playing {
            self.pending_end_turn = true;
        }
        self.doubles_this_turn = 0;
        true
    }

    fn jail_pay_fine(&mut self) -> bool {
        if self.jail_blocked() {
            return false;
        }
        let Some(idx) = self.current_active() else {
            return false;
        };
        let p = &mut self.players[idx];
        if !p.in_jail || p.money < JAIL_FINE {
            return false;
        }
        p.money -= JAIL_FINE;
        p.in_jail = false;
        p.jail_turns = 0;
        let (id, name) = (p.id, p.name.clone());
        self.log(
            Some(id),
            LogKind::JailPaid,
            format!("{name} заплатил ₽{JAIL_FINE} и вышел из тюрьмы"),
        );
        true
    }

    fn jail_use_card(&mut self) -> bool {
        if self.jail_blocked() {
            return false;
        }
        let Some(idx) = self.current_active() else {
            return false;
        };
        let p = &mut self.players[idx];
        if !p.in_jail || p.jail_free_cards == 0 {
            return false;
        }
        p.jail_free_cards -= 1;
        p.in_jail = false;
        p.jail_turns = 0;
        let (id, name) = (p.id, p.name.clone());
        self.log(Some(id), LogKind::JailUsedCard, format!("{name} использовал карту освобождения"));
        self.return_jail_card();
        true
    }

    /// Puts a used (or forfeited) jail-free card back under the deck it came from.
    fn return_jail_card(&mut self) {
        const CHANCE_JAIL_CARD: usize = 8;
        const CHEST_JAIL_CARD: usize = 2;
        if !self.chance_deck.contains(&CHANCE_JAIL_CARD) {
            self.chance_deck.push(CHANCE_JAIL_CARD);
        } else if !self.chest_deck.contains(&CHEST_JAIL_CARD) {
            self.chest_deck.push(CHEST_JAIL_CARD);
        }
    }

    // --- debts & bankruptcy -------------------------------------------------

    fn pay_debt(&mut self) -> bool {
        if !self.playing() {
            return false;
        }
        let Some(debt) = self.pending_debt else {
            return false;
        };
        let Some(idx) = self.player_index(debt.debtor) else {
            return false;
        };
        if self.players[idx].bankrupt || self.players[idx].money < debt.amount {
            return false;
        }
        self.players[idx].money -= debt.amount;
        if let Some(c) = debt.creditor.and_then(|c| self.player_index(c)) {
            self.players[c].money += debt.amount;
        }
        self.pending_debt = None;
        let name = self.players[idx].name.clone();
        let creditor = self.creditor_name(debt.creditor);
        self.log(
            Some(debt.debtor),
            LogKind::DebtPaid,
            format!("{name} погасил долг ₽{} ({creditor})", debt.amount),
        );
        if let Some(sum) = debt.jail_move_sum {
            self.players[idx].in_jail = false;
            self.players[idx].jail_turns = 0;
            self.move_after_jail(debt.debtor, sum);
        }
        true
    }

    fn debt_declare_bankruptcy(&mut self) -> bool {
        if !self.playing() {
            return false;
        }
        let Some(debt) = self.pending_debt.take() else {
            return false;
        };
        self.declare_bankruptcy(debt.debtor, debt.creditor, debt.amount);
        if self.phase == Phase::Playing {
            self.pending_purchase = None;
            self.pending_end_turn = true;
            self.end_turn();
        }
        true
    }

    fn pay_or_bankrupt(&mut self, payer: PlayerId, payee: Option<PlayerId>, amount: i32) {
        let Some(idx) = self.player_index(payer) else {
            return;
        };
        if self.players[idx].bankrupt {
            return;
        }
        if self.players[idx].money >= amount {
            self.players[idx].money -= amount;
            if let Some(p) = payee.and_then(|p| self.player_index(p)) {
                self.players[p].money += amount;
            }
            return;
        }
        // The current player gets a chance to raise money; anyone else (or a
        // second simultaneous debt) goes bankrupt immediately.
        let is_current = self.players.get(self.current).map(|p| p.id) == Some(payer);
        if is_current && self.pending_debt.is_none() {
            self.pending_debt = Some(PendingDebt {
                debtor: payer,
                creditor: payee,
                amount,
                jail_move_sum: None,
            });
            return;
        }
        self.declare_bankruptcy(payer, payee, amount);
    }

    fn declare_bankruptcy(&mut self, payer: PlayerId, creditor: Option<PlayerId>, owed: i32) {
        let Some(idx) = self.player_index(payer) else {
            return;
        };
        let transfer = self.players[idx].money;
        let released = std::mem::take(&mut self.players[idx].owned);
        for &t in &released {
            self.buildings[t] = 0;
            // Mortgages travel with the property; returned to the bank they clear.
            if creditor.is_none() {
                self.mortgaged[t] = false;
            }
        }
        let p = &mut self.players[idx];
        p.money = 0;
        p.bankrupt = true;
        p.in_jail = false;
        let name = p.name.clone();
        let jail_cards = std::mem::take(&mut p.jail_free_cards);
        let creditor_idx = creditor.and_then(|c| self.player_index(c));
        if let Some(c) = creditor_idx {
            self.players[c].money += transfer;
            self.players[c].owned.extend(released.iter().copied());
            self.players[c].jail_free_cards += jail_cards;
        } else {
            for _ in 0..jail_cards {
                self.return_jail_card();
            }
        }
        let creditor_name = self.creditor_name(creditor);
        self.log(
            Some(payer),
            LogKind::Bankrupt,
            format!("{name} обанкротился (должен был ₽{owed} → {creditor_name})"),
        );

        // Cancel anything the bankrupt player was part of.
        if let Some(a) = &mut self.pending_auction {
            if let Some(pos) = a.active.iter().position(|id| *id == payer) {
                a.active.remove(pos);
                if a.turn_index > pos || a.turn_index >= a.active.len() {
                    a.turn_index = a.turn_index.saturating_sub(1).min(a.active.len().saturating_sub(1));
                }
            }
        }
        if self
            .pending_trade
            .as_ref()
            .is_some_and(|t| t.from == payer || t.to == payer)
        {
            self.pending_trade = None;
        }
        if self
            .pending_offer
            .as_ref()
            .is_some_and(|o| o.from == payer || o.to == payer)
        {
            self.pending_offer = None;
        }

        let alive: Vec<(PlayerId, String)> = self
            .players
            .iter()
            .filter(|p| !p.bankrupt)
            .map(|p| (p.id, p.name.clone()))
            .collect();
        if alive.len() <= 1 {
            self.phase = Phase::Finished;
            self.pending_purchase = None;
            self.pending_end_turn = false;
            self.pending_auction = None;
            self.pending_trade = None;
            self.pending_offer = None;
            self.pending_debt = None;
            if let Some((id, name)) = alive.first() {
                self.log(Some(*id), LogKind::GameWon, format!("{name} побеждает!"));
            }
        }
    }

    // --- auctions -----------------------------------------------------------

    fn auction_bid(&mut self, player: PlayerId, amount: i32) -> bool {
        let Some(a) = &self.pending_auction else {
            return false;
        };
        if a.turn_player() != Some(player) || amount < a.current_bid + MIN_BID_INCREMENT {
            return false;
        }
        let Some(bidder) = self.player(player) else {
            return false;
        };
        if bidder.money < amount {
            return false;
        }
        let name = bidder.name.clone();
        let a = self.pending_auction.as_mut().expect("checked above");
        a.current_bid = amount;
        a.high_bidder = Some(player);
        a.turn_index = (a.turn_index + 1) % a.active.len();
        self.log(Some(player), LogKind::AuctionBid, format!("{name} делает ставку ₽{amount}"));
        self.maybe_finish_auction();
        true
    }

    fn auction_pass(&mut self, player: PlayerId) -> bool {
        let Some(a) = &self.pending_auction else {
            return false;
        };
        if a.turn_player() != Some(player) {
            return false;
        }
        let name = self.name_of(player);
        let a = self.pending_auction.as_mut().expect("checked above");
        a.active.retain(|id| *id != player);
        if a.turn_index >= a.active.len() {
            a.turn_index = 0;
        }
        self.log(Some(player), LogKind::AuctionPass, format!("{name} пасует"));
        self.maybe_finish_auction();
        true
    }

    fn maybe_finish_auction(&mut self) {
        let Some(a) = self.pending_auction.clone() else {
            return;
        };
        let tl = tile(a.tile);
        if a.active.is_empty() {
            self.pending_auction = None;
            self.pending_end_turn = self.pending_end_turn || self.doubles_this_turn == 0;
            self.log(None, LogKind::AuctionNoWinner, format!("Аукцион на «{}» завершён без победителя", tl.name));
            return;
        }
        if a.active.len() == 1 {
            match a.high_bidder {
                Some(winner) if a.active[0] == winner => {
                    let Some(idx) = self.player_index(winner) else {
                        return;
                    };
                    self.players[idx].money -= a.current_bid;
                    self.players[idx].owned.push(a.tile);
                    let name = self.players[idx].name.clone();
                    self.pending_auction = None;
                    self.pending_end_turn = self.pending_end_turn || self.doubles_this_turn == 0;
                    self.log(
                        Some(winner),
                        LogKind::AuctionWon,
                        format!("{name} выиграл «{}» за ₽{}", tl.name, a.current_bid),
                    );
                }
                _ => {}
            }
        }
    }

    // --- trades -------------------------------------------------------------

    fn trade_propose(
        &mut self,
        from: PlayerId,
        to: PlayerId,
        from_offer: &TradeBundle,
        to_offer: &TradeBundle,
    ) -> bool {
        if !self.playing()
            || self.pending_trade.is_some()
            || self.pending_debt.is_some()
            || from == to
        {
            return false;
        }
        let (Some(f), Some(t)) = (self.player(from), self.player(to)) else {
            return false;
        };
        if f.bankrupt || t.bankrupt {
            return false;
        }
        if from_offer.is_empty() && to_offer.is_empty() {
            return false;
        }
        if self.trade_bundle_valid(f, from_offer).is_err()
            || self.trade_bundle_valid(t, to_offer).is_err()
        {
            return false;
        }
        let (fname, tname) = (f.name.clone(), t.name.clone());
        self.pending_trade = Some(PendingTrade {
            from,
            to,
            from_offer: from_offer.clone(),
            to_offer: to_offer.clone(),
        });
        self.log(Some(from), LogKind::TradeProposed, format!("{fname} предлагает обмен игроку {tname}"));
        true
    }

    fn trade_accept(&mut self) -> bool {
        let Some(trade) = self.pending_trade.clone() else {
            return false;
        };
        let (Some(fi), Some(ti)) = (self.player_index(trade.from), self.player_index(trade.to)) else {
            return false;
        };
        if self.trade_bundle_valid(&self.players[fi], &trade.from_offer).is_err()
            || self.trade_bundle_valid(&self.players[ti], &trade.to_offer).is_err()
        {
            return false;
        }
        let (fo, to) = (&trade.from_offer, &trade.to_offer);
        {
            let f = &mut self.players[fi];
            f.money += to.money - fo.money;
            f.jail_free_cards = f.jail_free_cards - fo.jail_free_cards + to.jail_free_cards;
            f.owned.retain(|t| !fo.tiles.contains(t));
            f.owned.extend(to.tiles.iter().copied());
        }
        {
            let t = &mut self.players[ti];
            t.money += fo.money - to.money;
            t.jail_free_cards = t.jail_free_cards - to.jail_free_cards + fo.jail_free_cards;
            t.owned.retain(|x| !to.tiles.contains(x));
            t.owned.extend(fo.tiles.iter().copied());
        }
        self.pending_trade = None;
        let (fname, tname) = (self.players[fi].name.clone(), self.players[ti].name.clone());
        self.log(Some(trade.to), LogKind::TradeAccepted, format!("{tname} принял предложение от {fname}"));
        true
    }

    fn trade_decline(&mut self) -> bool {
        let Some(trade) = self.pending_trade.take() else {
            return false;
        };
        let (fname, tname) = (self.name_of(trade.from), self.name_of(trade.to));
        self.log(Some(trade.to), LogKind::TradeDeclined, format!("{tname} отклонил предложение от {fname}"));
        true
    }

    fn trade_cancel(&mut self) -> bool {
        let Some(trade) = self.pending_trade.take() else {
            return false;
        };
        let (fname, tname) = (self.name_of(trade.from), self.name_of(trade.to));
        self.log(Some(trade.from), LogKind::TradeCancelled, format!("{fname} отменил предложение игроку {tname}"));
        true
    }
}

fn nearest_ahead(from: TileIndex, candidates: &[TileIndex]) -> TileIndex {
    (1..=BOARD_SIZE)
        .map(|o| (from + o) % BOARD_SIZE)
        .find(|i| candidates.contains(i))
        .expect("candidate list is not empty")
}

#[cfg(test)]
impl GameState {
    pub(crate) fn process_landing_for_test(&mut self, id: PlayerId, t: TileIndex, dice_sum: u32) {
        self.process_landing(id, t, dice_sum, 1, false);
    }

    pub(crate) fn begin_move_for_test(&mut self, id: PlayerId) {
        self.begin_move(id);
    }
}
