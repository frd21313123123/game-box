use serde::{Deserialize, Serialize};

use crate::card::{full_shoe, hand_value, Card, HandValue, Rank, SHOE_SIZE};
use crate::rng::Rng;

/// Blackjack is fine with a single player against the dealer.
pub const MIN_PLAYERS: usize = 1;
/// Seven boxes, as on a casino table.
pub const MAX_PLAYERS: usize = 7;
pub const MIN_BET: u32 = 10;
pub const MAX_BET: u32 = 500;
/// Bets are made in steps of the smallest chip, so 3:2 and insurance stay whole.
pub const BET_STEP: u32 = 10;
pub const DEFAULT_CHIPS: u32 = 1000;
/// Default match length in rounds (0 = until everyone is broke).
pub const DEFAULT_ROUNDS: u32 = 10;
/// A player may split up to this many hands.
pub const MAX_HANDS: usize = 4;
/// The shoe is reshuffled before a round once fewer cards than this remain
/// (the cut card sits at 75% penetration).
pub const CUT_CARD: usize = SHOE_SIZE / 4;
const EVENT_HISTORY: usize = 192;

/// How a hand ended against the dealer.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Outcome {
    /// A natural 21 from the first two cards: pays 3:2.
    Blackjack,
    Win,
    Push,
    Lose,
    Bust,
}

impl Outcome {
    /// Chips returned to the player for a hand with stake `bet` (stake included).
    pub fn payout(self, bet: u32) -> u32 {
        match self {
            Outcome::Blackjack => bet + bet * 3 / 2,
            Outcome::Win => bet * 2,
            Outcome::Push => bet,
            Outcome::Lose | Outcome::Bust => 0,
        }
    }

    pub fn name_ru(self) -> &'static str {
        match self {
            Outcome::Blackjack => "Блэкджек",
            Outcome::Win => "Выигрыш",
            Outcome::Push => "Ничья",
            Outcome::Lose => "Проигрыш",
            Outcome::Bust => "Перебор",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Hand {
    pub cards: Vec<Card>,
    pub bet: u32,
    pub doubled: bool,
    /// The hand came from a split: 21 on two cards is not a blackjack then.
    pub from_split: bool,
    /// Split aces get one card each and no more.
    pub split_aces: bool,
    /// No more cards for this hand.
    pub done: bool,
    pub outcome: Option<Outcome>,
}

impl Hand {
    fn new(bet: u32) -> Self {
        Hand {
            cards: Vec::new(),
            bet,
            doubled: false,
            from_split: false,
            split_aces: false,
            done: false,
            outcome: None,
        }
    }

    pub fn value(&self) -> HandValue {
        hand_value(&self.cards)
    }

    /// A natural: 21 from the first two cards of an unsplit hand.
    pub fn is_blackjack(&self) -> bool {
        self.cards.len() == 2 && !self.from_split && self.value().total == 21
    }

    pub fn is_bust(&self) -> bool {
        self.value().is_bust()
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct BjPlayer {
    pub name: String,
    /// Chips not on the table.
    pub chips: u32,
    pub is_bot: bool,
    /// Empty while the player has not bet yet (or sits the round out).
    pub hands: Vec<Hand>,
    /// `None` until the player answered the insurance offer; `Some(0)` = declined.
    pub insurance: Option<u32>,
    /// The previous bet, offered again by the «same bet» button.
    pub last_bet: u32,
    /// Chips at the start of the current round, for the round result.
    pub round_start_chips: u32,
    /// Not enough chips for the minimum bet: watches until the match ends.
    pub out: bool,
}

impl BjPlayer {
    /// Chips won (positive) or lost in the current round. Bets still in play
    /// count as the player's; a bought insurance counts as spent.
    pub fn round_net(&self) -> i64 {
        self.total_chips() as i64 - self.round_start_chips as i64
    }

    /// Chips including the bets still on the table.
    pub fn total_chips(&self) -> u32 {
        self.chips
            + self
                .hands
                .iter()
                .filter(|h| h.outcome.is_none())
                .map(|h| h.bet)
                .sum::<u32>()
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Phase {
    /// Every player still in the game places a bet.
    Betting,
    /// The dealer shows an ace: each player decides on insurance.
    Insurance,
    /// `seat` plays hand number `hand`.
    PlayerTurn { seat: usize, hand: usize },
    /// The dealer reveals the hole card and draws to 17 (one step at a time).
    DealerTurn,
    RoundOver,
    /// `winner` has the most chips; `None` when everyone went broke.
    GameOver { winner: Option<usize> },
}

/// Where a card goes when it is dealt.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Spot {
    Dealer,
    Player { seat: usize, hand: usize },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Move {
    Hit,
    Stand,
    Double,
    Split,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum BjEvent {
    RoundStarted { round: u32 },
    Reshuffled,
    BetPlaced { seat: usize, amount: u32 },
    /// The dealer's hole card is dealt as a hidden card.
    Dealt { to: Spot, card: Card },
    Natural { seat: usize },
    InsuranceOffered,
    InsuranceTaken { seat: usize, amount: u32 },
    InsuranceDeclined { seat: usize },
    /// The dealer looked under a ten or an ace.
    DealerPeeked { blackjack: bool },
    Moved { seat: usize, hand: usize, action: Move },
    Busted { seat: usize, hand: usize },
    HoleRevealed { card: Card },
    DealerStands { total: u8 },
    DealerBusted { total: u8 },
    Settled { seat: usize, hand: usize, outcome: Outcome, payout: u32 },
    InsurancePaid { seat: usize, amount: u32 },
    PlayerOut { seat: usize },
    GameWon { seat: Option<usize> },
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SeqEvent {
    pub seq: u64,
    pub event: BjEvent,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum BjAction {
    Bet(u32),
    /// Take (`true`) or decline the insurance.
    Insurance(bool),
    Hit,
    Stand,
    Double,
    Split,
    NextRound,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum BjError {
    NotYourTurn,
    WrongPhase,
    BadBet,
    NotEnoughChips,
    AlreadyBet,
    CannotDouble,
    CannotSplit,
    OutOfGame,
}

impl std::fmt::Display for BjError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let s = match self {
            BjError::NotYourTurn => "Сейчас не ваш ход",
            BjError::WrongPhase => "Сейчас это действие недоступно",
            BjError::BadBet => "Ставка от 10 до 500, кратно 10",
            BjError::NotEnoughChips => "Не хватает фишек",
            BjError::AlreadyBet => "Ставка уже сделана",
            BjError::CannotDouble => "Удвоить можно только на первых двух картах",
            BjError::CannotSplit => "Разделить можно только пару одинаковых карт",
            BjError::OutOfGame => "У вас закончились фишки",
        };
        f.write_str(s)
    }
}

impl std::error::Error for BjError {}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct BjGame {
    pub players: Vec<BjPlayer>,
    /// The dealer's cards; the second one is the hole card.
    pub dealer: Vec<Card>,
    pub hole_revealed: bool,
    /// Face-down shoe. The next card is the last element.
    pub shoe: Vec<Card>,
    /// Used cards waiting for the next reshuffle.
    pub discard: Vec<Card>,
    pub phase: Phase,
    pub round: u32,
    /// Rounds in the match; 0 plays until everyone is broke.
    pub rounds_limit: u32,
    pub starting_chips: u32,
    pub rng: Rng,
    pub event_seq: u64,
    pub events: Vec<SeqEvent>,
}

impl BjGame {
    /// Creates a game and opens the first round for bets. `players` are `(name, is_bot)`.
    pub fn new(players: Vec<(String, bool)>, seed: u64, starting_chips: u32, rounds_limit: u32) -> Self {
        assert!(
            (MIN_PLAYERS..=MAX_PLAYERS).contains(&players.len()),
            "Blackjack needs {MIN_PLAYERS}..={MAX_PLAYERS} players"
        );
        let mut rng = Rng::new(seed);
        let mut shoe = full_shoe();
        rng.shuffle(&mut shoe);
        let starting_chips = starting_chips.max(MIN_BET);
        let mut game = BjGame {
            players: players
                .into_iter()
                .map(|(name, is_bot)| BjPlayer {
                    name,
                    chips: starting_chips,
                    is_bot,
                    hands: Vec::new(),
                    insurance: None,
                    last_bet: 0,
                    round_start_chips: starting_chips,
                    out: false,
                })
                .collect(),
            dealer: Vec::new(),
            hole_revealed: false,
            shoe,
            discard: Vec::new(),
            phase: Phase::Betting,
            round: 0,
            rounds_limit,
            starting_chips,
            rng,
            event_seq: 0,
            events: Vec::new(),
        };
        game.start_round();
        game
    }

    pub fn player_count(&self) -> usize {
        self.players.len()
    }

    /// The seat whose turn it is (only while players play their hands).
    pub fn acting_seat(&self) -> Option<usize> {
        match self.phase {
            Phase::PlayerTurn { seat, .. } => Some(seat),
            _ => None,
        }
    }

    /// Whether `seat` still has to place a bet this round.
    pub fn needs_bet(&self, seat: usize) -> bool {
        self.phase == Phase::Betting
            && self
                .players
                .get(seat)
                .is_some_and(|p| !p.out && p.hands.is_empty())
    }

    /// Whether `seat` still has to answer the insurance offer.
    pub fn needs_insurance(&self, seat: usize) -> bool {
        self.phase == Phase::Insurance
            && self
                .players
                .get(seat)
                .is_some_and(|p| !p.hands.is_empty() && p.insurance.is_none())
    }

    /// Whether `seat` has anything to decide right now.
    pub fn is_waiting_for(&self, seat: usize) -> bool {
        self.needs_bet(seat) || self.needs_insurance(seat) || self.acting_seat() == Some(seat)
    }

    /// The hand being played by `seat`, if it is their turn.
    pub fn active_hand(&self, seat: usize) -> Option<&Hand> {
        match self.phase {
            Phase::PlayerTurn { seat: s, hand } if s == seat => self.players[seat].hands.get(hand),
            _ => None,
        }
    }

    pub fn can_double(&self, seat: usize) -> bool {
        self.active_hand(seat).is_some_and(|h| {
            h.cards.len() == 2 && !h.split_aces && self.players[seat].chips >= h.bet
        })
    }

    pub fn can_split(&self, seat: usize) -> bool {
        self.active_hand(seat).is_some_and(|h| {
            h.cards.len() == 2
                && h.cards[0].rank == h.cards[1].rank
                && !h.split_aces
                && self.players[seat].hands.len() < MAX_HANDS
                && self.players[seat].chips >= h.bet
        })
    }

    /// Insurance costs half the bet.
    pub fn insurance_cost(&self, seat: usize) -> u32 {
        self.players
            .get(seat)
            .and_then(|p| p.hands.first())
            .map(|h| h.bet / 2)
            .unwrap_or(0)
    }

    /// The dealer's card facing up.
    pub fn dealer_up(&self) -> Option<&Card> {
        self.dealer.first()
    }

    /// Value of the dealer's cards that can be seen.
    pub fn dealer_value(&self) -> HandValue {
        if self.hole_revealed {
            hand_value(&self.dealer)
        } else {
            hand_value(&self.dealer[..self.dealer.len().min(1)])
        }
    }

    pub fn events_after(&self, seq: u64) -> impl Iterator<Item = &SeqEvent> {
        self.events.iter().filter(move |e| e.seq > seq)
    }

    fn emit(&mut self, event: BjEvent) {
        self.event_seq += 1;
        self.events.push(SeqEvent {
            seq: self.event_seq,
            event,
        });
        if self.events.len() > EVENT_HISTORY {
            let excess = self.events.len() - EVENT_HISTORY;
            self.events.drain(..excess);
        }
    }

    fn start_round(&mut self) {
        self.round += 1;
        // Everything on the table goes to the discard tray.
        let mut used: Vec<Card> = self.dealer.drain(..).collect();
        for p in &mut self.players {
            for h in p.hands.drain(..) {
                used.extend(h.cards);
            }
            p.insurance = None;
        }
        self.discard.extend(used);
        self.hole_revealed = false;
        self.phase = Phase::Betting;
        self.emit(BjEvent::RoundStarted { round: self.round });
        if self.shoe.len() < CUT_CARD {
            self.reshuffle();
        }
        for seat in 0..self.players.len() {
            let p = &mut self.players[seat];
            p.round_start_chips = p.chips;
            if !p.out && p.chips < MIN_BET {
                p.out = true;
                self.emit(BjEvent::PlayerOut { seat });
            }
        }
        if self.players.iter().all(|p| p.out) {
            self.finish_game();
        }
    }

    /// Puts the discard tray back into the shoe and shuffles it.
    fn reshuffle(&mut self) {
        if self.discard.is_empty() {
            return;
        }
        self.shoe.append(&mut self.discard);
        self.rng.shuffle(&mut self.shoe);
        self.emit(BjEvent::Reshuffled);
    }

    fn draw(&mut self) -> Card {
        if self.shoe.is_empty() {
            self.reshuffle();
        }
        self.shoe
            .pop()
            .expect("six decks never run out with seven boxes")
    }

    /// Applies an action performed by the player in `seat`.
    pub fn apply(&mut self, seat: usize, action: BjAction) -> Result<(), BjError> {
        if seat >= self.players.len() {
            return Err(BjError::NotYourTurn);
        }
        match action {
            BjAction::Bet(amount) => self.bet(seat, amount),
            BjAction::Insurance(take) => self.insurance(seat, take),
            BjAction::Hit => self.hit(seat),
            BjAction::Stand => self.stand(seat),
            BjAction::Double => self.double(seat),
            BjAction::Split => self.split(seat),
            BjAction::NextRound => self.next_round(),
        }
    }

    fn bet(&mut self, seat: usize, amount: u32) -> Result<(), BjError> {
        if self.phase != Phase::Betting {
            return Err(BjError::WrongPhase);
        }
        let p = &self.players[seat];
        if p.out {
            return Err(BjError::OutOfGame);
        }
        if !p.hands.is_empty() {
            return Err(BjError::AlreadyBet);
        }
        if !(MIN_BET..=MAX_BET).contains(&amount) || !amount.is_multiple_of(BET_STEP) {
            return Err(BjError::BadBet);
        }
        if amount > p.chips {
            return Err(BjError::NotEnoughChips);
        }
        let p = &mut self.players[seat];
        p.chips -= amount;
        p.last_bet = amount;
        p.hands.push(Hand::new(amount));
        self.emit(BjEvent::BetPlaced { seat, amount });
        if self.players.iter().all(|p| p.out || !p.hands.is_empty()) {
            self.deal();
        }
        Ok(())
    }

    fn deal(&mut self) {
        let order: Vec<usize> = (0..self.players.len())
            .filter(|s| !self.players[*s].hands.is_empty())
            .collect();
        for pass in 0..2 {
            for &seat in &order {
                let card = self.draw();
                self.players[seat].hands[0].cards.push(card);
                self.emit(BjEvent::Dealt {
                    to: Spot::Player { seat, hand: 0 },
                    card,
                });
            }
            let card = self.draw();
            self.dealer.push(card);
            let shown = if pass == 0 { card } else { Card::hidden() };
            self.emit(BjEvent::Dealt { to: Spot::Dealer, card: shown });
        }
        for &seat in &order {
            if self.players[seat].hands[0].is_blackjack() {
                self.players[seat].hands[0].done = true;
                self.emit(BjEvent::Natural { seat });
            }
        }
        if self.dealer[0].rank == Rank::Ace {
            let mut offered = false;
            for &seat in &order {
                let cost = self.insurance_cost(seat);
                if cost > 0 && self.players[seat].chips >= cost {
                    offered = true;
                } else {
                    self.players[seat].insurance = Some(0);
                }
            }
            if offered {
                self.phase = Phase::Insurance;
                self.emit(BjEvent::InsuranceOffered);
                return;
            }
        }
        self.peek();
    }

    fn insurance(&mut self, seat: usize, take: bool) -> Result<(), BjError> {
        if !self.needs_insurance(seat) {
            return Err(if self.phase == Phase::Insurance {
                BjError::NotYourTurn
            } else {
                BjError::WrongPhase
            });
        }
        let cost = self.insurance_cost(seat);
        if take {
            if self.players[seat].chips < cost {
                return Err(BjError::NotEnoughChips);
            }
            self.players[seat].chips -= cost;
            self.players[seat].insurance = Some(cost);
            self.emit(BjEvent::InsuranceTaken { seat, amount: cost });
        } else {
            self.players[seat].insurance = Some(0);
            self.emit(BjEvent::InsuranceDeclined { seat });
        }
        if self
            .players
            .iter()
            .all(|p| p.hands.is_empty() || p.insurance.is_some())
        {
            self.peek();
        }
        Ok(())
    }

    /// With a ten or an ace showing, the dealer checks for blackjack at once,
    /// so nobody doubles or splits into a hand that is already lost.
    fn peek(&mut self) {
        if self.dealer[0].rank.value() >= 10 {
            let blackjack = hand_value(&self.dealer).total == 21;
            self.emit(BjEvent::DealerPeeked { blackjack });
            if blackjack {
                self.reveal();
                self.settle();
                return;
            }
        }
        self.advance_from(0, 0);
    }

    /// Moves the turn to the first unfinished hand at or after (`seat`, `hand`).
    fn advance_from(&mut self, seat: usize, hand: usize) {
        for s in seat..self.players.len() {
            let start = if s == seat { hand } else { 0 };
            if let Some(h) = (start..self.players[s].hands.len()).find(|h| !self.players[s].hands[*h].done) {
                self.phase = Phase::PlayerTurn { seat: s, hand: h };
                return;
            }
        }
        self.phase = Phase::DealerTurn;
    }

    fn turn(&self, seat: usize) -> Result<usize, BjError> {
        match self.phase {
            Phase::PlayerTurn { seat: s, hand } if s == seat => Ok(hand),
            Phase::PlayerTurn { .. } => Err(BjError::NotYourTurn),
            _ => Err(BjError::WrongPhase),
        }
    }

    /// Deals one card to a player's hand; returns its new value.
    fn deal_to(&mut self, seat: usize, hand: usize) -> HandValue {
        let card = self.draw();
        self.players[seat].hands[hand].cards.push(card);
        self.emit(BjEvent::Dealt {
            to: Spot::Player { seat, hand },
            card,
        });
        self.players[seat].hands[hand].value()
    }

    /// Ends the hand and passes the turn on.
    fn finish_hand(&mut self, seat: usize, hand: usize) {
        let h = &mut self.players[seat].hands[hand];
        h.done = true;
        if h.is_bust() {
            h.outcome = Some(Outcome::Bust);
            self.emit(BjEvent::Busted { seat, hand });
        }
        self.advance_from(seat, hand + 1);
    }

    fn hit(&mut self, seat: usize) -> Result<(), BjError> {
        let hand = self.turn(seat)?;
        self.emit(BjEvent::Moved { seat, hand, action: Move::Hit });
        let value = self.deal_to(seat, hand);
        if value.total >= 21 {
            self.finish_hand(seat, hand);
        }
        Ok(())
    }

    fn stand(&mut self, seat: usize) -> Result<(), BjError> {
        let hand = self.turn(seat)?;
        self.emit(BjEvent::Moved { seat, hand, action: Move::Stand });
        self.finish_hand(seat, hand);
        Ok(())
    }

    fn double(&mut self, seat: usize) -> Result<(), BjError> {
        let hand = self.turn(seat)?;
        if !self.can_double(seat) {
            let h = &self.players[seat].hands[hand];
            return Err(if h.cards.len() == 2 && !h.split_aces {
                BjError::NotEnoughChips
            } else {
                BjError::CannotDouble
            });
        }
        let p = &mut self.players[seat];
        let bet = p.hands[hand].bet;
        p.chips -= bet;
        p.hands[hand].bet += bet;
        p.hands[hand].doubled = true;
        self.emit(BjEvent::Moved { seat, hand, action: Move::Double });
        self.deal_to(seat, hand);
        self.finish_hand(seat, hand);
        Ok(())
    }

    fn split(&mut self, seat: usize) -> Result<(), BjError> {
        let hand = self.turn(seat)?;
        if !self.can_split(seat) {
            let p = &self.players[seat];
            let h = &p.hands[hand];
            let pair = h.cards.len() == 2 && h.cards[0].rank == h.cards[1].rank && !h.split_aces;
            return Err(if pair && p.hands.len() < MAX_HANDS {
                BjError::NotEnoughChips
            } else {
                BjError::CannotSplit
            });
        }
        let p = &mut self.players[seat];
        let bet = p.hands[hand].bet;
        p.chips -= bet;
        let second = p.hands[hand].cards.pop().expect("a pair has two cards");
        let aces = second.rank == Rank::Ace;
        let first = &mut p.hands[hand];
        first.from_split = true;
        first.split_aces = aces;
        let mut new_hand = Hand::new(bet);
        new_hand.cards.push(second);
        new_hand.from_split = true;
        new_hand.split_aces = aces;
        p.hands.insert(hand + 1, new_hand);
        self.emit(BjEvent::Moved { seat, hand, action: Move::Split });
        // Each half gets its second card right away.
        for h in [hand, hand + 1] {
            let value = self.deal_to(seat, h);
            if aces || value.total == 21 {
                self.players[seat].hands[h].done = true;
            }
        }
        if self.players[seat].hands[hand].done {
            self.advance_from(seat, hand + 1);
        }
        Ok(())
    }

    /// Whether some hand still needs the dealer to play it out.
    fn has_live_hands(&self) -> bool {
        self.players
            .iter()
            .flat_map(|p| p.hands.iter())
            .any(|h| !h.is_bust() && !h.is_blackjack())
    }

    /// Host only: one step of the dealer's turn (reveal, draw, or settle).
    /// Returns whether anything happened. The host calls it on a timer so
    /// players can follow every card.
    pub fn dealer_step(&mut self) -> bool {
        if self.phase != Phase::DealerTurn {
            return false;
        }
        if !self.hole_revealed {
            self.reveal();
            return true;
        }
        let value = hand_value(&self.dealer);
        let live = self.has_live_hands();
        // The dealer stands on every 17, soft ones included.
        if live && value.total < 17 {
            let card = self.draw();
            self.dealer.push(card);
            self.emit(BjEvent::Dealt { to: Spot::Dealer, card });
            return true;
        }
        if value.is_bust() {
            self.emit(BjEvent::DealerBusted { total: value.total });
        } else if live {
            self.emit(BjEvent::DealerStands { total: value.total });
        }
        self.settle();
        true
    }

    fn reveal(&mut self) {
        self.hole_revealed = true;
        if let Some(card) = self.dealer.get(1).copied() {
            self.emit(BjEvent::HoleRevealed { card });
        }
    }

    fn settle(&mut self) {
        let dealer = hand_value(&self.dealer);
        let dealer_blackjack = self.dealer.len() == 2 && dealer.total == 21;
        for seat in 0..self.players.len() {
            for hand in 0..self.players[seat].hands.len() {
                let h = &self.players[seat].hands[hand];
                let value = h.value();
                let outcome = if value.is_bust() {
                    Outcome::Bust
                } else if h.is_blackjack() {
                    if dealer_blackjack {
                        Outcome::Push
                    } else {
                        Outcome::Blackjack
                    }
                } else if dealer_blackjack {
                    Outcome::Lose
                } else if dealer.is_bust() || value.total > dealer.total {
                    Outcome::Win
                } else if value.total == dealer.total {
                    Outcome::Push
                } else {
                    Outcome::Lose
                };
                let payout = outcome.payout(h.bet);
                let p = &mut self.players[seat];
                p.chips += payout;
                p.hands[hand].outcome = Some(outcome);
                p.hands[hand].done = true;
                self.emit(BjEvent::Settled { seat, hand, outcome, payout });
            }
            if let Some(cost) = self.players[seat].insurance.filter(|c| *c > 0) {
                if dealer_blackjack {
                    let amount = cost * 3;
                    self.players[seat].chips += amount;
                    self.emit(BjEvent::InsurancePaid { seat, amount });
                }
            }
        }
        self.phase = Phase::RoundOver;
        let last_round = self.rounds_limit > 0 && self.round >= self.rounds_limit;
        let all_broke = self.players.iter().all(|p| p.chips < MIN_BET);
        if last_round || all_broke {
            self.finish_game();
        }
    }

    fn finish_game(&mut self) {
        let best = self.players.iter().map(|p| p.chips).max().unwrap_or(0);
        let winner = if best == 0 {
            None
        } else {
            self.players.iter().position(|p| p.chips == best)
        };
        self.emit(BjEvent::GameWon { seat: winner });
        self.phase = Phase::GameOver { winner };
    }

    fn next_round(&mut self) -> Result<(), BjError> {
        if self.phase != Phase::RoundOver {
            return Err(BjError::WrongPhase);
        }
        self.start_round();
        Ok(())
    }

    /// A copy of the state as a player sees it: the shoe, the discard tray and
    /// the dealer's hole card (until it is turned) are hidden.
    pub fn view_for(&self, _viewer: Option<usize>) -> BjGame {
        let mut view = self.clone();
        view.shoe.iter_mut().for_each(|c| *c = Card::hidden());
        view.discard.iter_mut().for_each(|c| *c = Card::hidden());
        if !view.hole_revealed {
            if let Some(hole) = view.dealer.get_mut(1) {
                *hole = Card::hidden();
            }
        }
        view.rng = Rng::new(0);
        view
    }

    /// Every card in every place — tests check that none is lost or duplicated.
    pub fn total_cards(&self) -> usize {
        self.shoe.len()
            + self.discard.len()
            + self.dealer.len()
            + self
                .players
                .iter()
                .flat_map(|p| p.hands.iter())
                .map(|h| h.cards.len())
                .sum::<usize>()
    }
}
