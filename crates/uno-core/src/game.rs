use serde::{Deserialize, Serialize};

use crate::card::{full_deck, Card, CardColor, Face};
use crate::rng::Rng;

pub const HAND_SIZE: usize = 7;
pub const MIN_PLAYERS: usize = 2;
pub const MAX_PLAYERS: usize = 8;
/// Classic match length: first to 500 points wins.
pub const DEFAULT_TARGET_SCORE: u32 = 500;
const EVENT_HISTORY: usize = 128;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct UnoPlayer {
    pub name: String,
    pub hand: Vec<Card>,
    pub score: u32,
    /// The player has announced "UNO!" for their last card.
    pub uno_called: bool,
    pub is_bot: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Direction {
    Clockwise,
    CounterClockwise,
}

impl Direction {
    pub fn flipped(self) -> Self {
        match self {
            Direction::Clockwise => Direction::CounterClockwise,
            Direction::CounterClockwise => Direction::Clockwise,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum Phase {
    /// The current player must play a card or draw one.
    Turn,
    /// The current player drew a card that can be played: play it or pass.
    AfterDraw { card_id: u16 },
    /// `seat` must name the colour for the wild card on top of the pile.
    ChooseColor {
        seat: usize,
        draw_four: bool,
        /// Whether play advances after the colour is chosen (false for the
        /// wild flipped as the very first card: the first player then plays).
        advance: bool,
    },
    RoundOver { winner: usize, points: u32 },
    GameOver { winner: usize },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum DrawReason {
    Voluntary,
    DrawTwo,
    DrawFour,
    UnoPenalty,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum UnoEvent {
    RoundStarted { round: u32, dealer: usize },
    FirstCard { card: Card },
    Played { seat: usize, card: Card },
    Drew { seat: usize, count: usize, reason: DrawReason },
    Passed { seat: usize },
    ColorChosen { seat: usize, color: CardColor },
    Skipped { seat: usize },
    Reversed { direction: Direction },
    UnoCalled { seat: usize },
    UnoCaught { catcher: usize, target: usize },
    Reshuffled,
    RoundWon { seat: usize, points: u32 },
    GameWon { seat: usize },
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SeqEvent {
    pub seq: u64,
    pub event: UnoEvent,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum UnoAction {
    /// Play a card from hand; `call_uno` announces UNO together with the play.
    Play { card_id: u16, call_uno: bool },
    Draw,
    /// Keep the card just drawn and end the turn.
    Pass,
    ChooseColor(CardColor),
    CallUno,
    CatchUno { target: usize },
    NextRound,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum UnoError {
    NotYourTurn,
    WrongPhase,
    NoSuchCard,
    CardDoesNotMatch,
    NothingToCatch,
    CannotCallUno,
}

impl std::fmt::Display for UnoError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let s = match self {
            UnoError::NotYourTurn => "Сейчас не ваш ход",
            UnoError::WrongPhase => "Сейчас это действие недоступно",
            UnoError::NoSuchCard => "Такой карты нет на руке",
            UnoError::CardDoesNotMatch => "Эту карту нельзя положить",
            UnoError::NothingToCatch => "Ловить некого",
            UnoError::CannotCallUno => "Объявить УНО можно только с одной-двумя картами",
        };
        f.write_str(s)
    }
}

impl std::error::Error for UnoError {}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct UnoGame {
    pub players: Vec<UnoPlayer>,
    /// Face-down stock. The top card is the last element.
    pub draw_pile: Vec<Card>,
    /// Played cards. The top card is the last element.
    pub discard: Vec<Card>,
    pub current: usize,
    pub direction: Direction,
    pub active_color: CardColor,
    pub phase: Phase,
    pub dealer: usize,
    pub round: u32,
    /// Points needed to win the match; 0 means a single round decides it.
    pub target_score: u32,
    /// A player who went down to one card without calling UNO and can be caught
    /// until the next player acts.
    pub uno_vulnerable: Option<usize>,
    pub rng: Rng,
    /// Total number of events ever emitted.
    pub event_seq: u64,
    /// The most recent events (capped), each tagged with its sequence number.
    pub events: Vec<SeqEvent>,
}

impl UnoGame {
    /// Creates a game and deals the first round. `players` are `(name, is_bot)`.
    pub fn new(players: Vec<(String, bool)>, seed: u64, target_score: u32) -> Self {
        assert!(
            (MIN_PLAYERS..=MAX_PLAYERS).contains(&players.len()),
            "Uno needs {MIN_PLAYERS}..={MAX_PLAYERS} players"
        );
        let mut rng = Rng::new(seed);
        let dealer = rng.below(players.len());
        let mut game = UnoGame {
            players: players
                .into_iter()
                .map(|(name, is_bot)| UnoPlayer {
                    name,
                    hand: Vec::new(),
                    score: 0,
                    uno_called: false,
                    is_bot,
                })
                .collect(),
            draw_pile: Vec::new(),
            discard: Vec::new(),
            current: 0,
            direction: Direction::Clockwise,
            active_color: CardColor::Red,
            phase: Phase::Turn,
            dealer,
            round: 0,
            target_score,
            uno_vulnerable: None,
            rng,
            event_seq: 0,
            events: Vec::new(),
        };
        game.start_round();
        game
    }

    pub fn top_card(&self) -> &Card {
        self.discard.last().expect("discard pile is never empty during play")
    }

    pub fn player_count(&self) -> usize {
        self.players.len()
    }

    /// Seat that is next after `from` in the current direction.
    pub fn next_seat(&self, from: usize) -> usize {
        let n = self.players.len();
        match self.direction {
            Direction::Clockwise => (from + 1) % n,
            Direction::CounterClockwise => (from + n - 1) % n,
        }
    }

    /// The seat that has to act right now (handles colour choice and challenges).
    pub fn acting_seat(&self) -> Option<usize> {
        match &self.phase {
            Phase::Turn | Phase::AfterDraw { .. } => Some(self.current),
            Phase::ChooseColor { seat, .. } => Some(*seat),
            Phase::RoundOver { .. } | Phase::GameOver { .. } => None,
        }
    }

    pub fn can_play(&self, card: &Card) -> bool {
        card.matches(self.top_card(), self.active_color)
    }

    /// Cards `seat` could legally play right now.
    pub fn playable_cards(&self, seat: usize) -> Vec<Card> {
        if seat != self.current {
            return Vec::new();
        }
        match self.phase {
            Phase::Turn => self.players[seat]
                .hand
                .iter()
                .copied()
                .filter(|c| self.can_play(c))
                .collect(),
            Phase::AfterDraw { card_id } => self.players[seat]
                .hand
                .iter()
                .copied()
                .filter(|c| c.id == card_id && self.can_play(c))
                .collect(),
            _ => Vec::new(),
        }
    }

    pub fn events_after(&self, seq: u64) -> impl Iterator<Item = &SeqEvent> {
        self.events.iter().filter(move |e| e.seq > seq)
    }

    fn emit(&mut self, event: UnoEvent) {
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
        let mut deck = full_deck();
        self.rng.shuffle(&mut deck);
        self.draw_pile = deck;
        self.discard.clear();
        self.direction = Direction::Clockwise;
        self.uno_vulnerable = None;
        for p in &mut self.players {
            p.hand.clear();
            p.uno_called = false;
        }
        self.emit(UnoEvent::RoundStarted {
            round: self.round,
            dealer: self.dealer,
        });

        // Deal one card at a time, starting left of the dealer.
        let n = self.players.len();
        for _ in 0..HAND_SIZE {
            for k in 1..=n {
                let seat = (self.dealer + k) % n;
                if let Some(card) = self.draw_pile.pop() {
                    self.players[seat].hand.push(card);
                }
            }
        }

        // Flip the first card. A Wild Draw Four goes back into the deck.
        let first = loop {
            let card = self.draw_pile.pop().expect("deck has enough cards");
            if card.face == Face::WildDrawFour {
                let pos = self.rng.below(self.draw_pile.len() + 1);
                self.draw_pile.insert(pos, card);
                continue;
            }
            break card;
        };
        self.discard.push(first);
        self.emit(UnoEvent::FirstCard { card: first });

        let first_player = (self.dealer + 1) % n;
        self.current = first_player;
        self.phase = Phase::Turn;
        if let Some(color) = first.color {
            self.active_color = color;
        }
        match first.face {
            Face::Number(_) => {}
            Face::Skip => {
                self.emit(UnoEvent::Skipped { seat: first_player });
                self.current = self.next_seat(first_player);
            }
            Face::Reverse => {
                // The dealer starts and play goes the other way. With two
                // players this is the same as skipping the first player.
                self.direction = self.direction.flipped();
                self.emit(UnoEvent::Reversed {
                    direction: self.direction,
                });
                self.current = self.dealer;
            }
            Face::DrawTwo => {
                self.draw_cards(first_player, 2, DrawReason::DrawTwo);
                self.emit(UnoEvent::Skipped { seat: first_player });
                self.current = self.next_seat(first_player);
            }
            Face::Wild => {
                self.phase = Phase::ChooseColor {
                    seat: first_player,
                    draw_four: false,
                    advance: false,
                };
            }
            Face::WildDrawFour => unreachable!("re-drawn above"),
        }
    }

    /// Moves cards from the stock to `seat`, reshuffling the discard pile into
    /// the stock when it runs out. Returns the cards actually drawn.
    fn draw_cards(&mut self, seat: usize, count: usize, reason: DrawReason) -> Vec<Card> {
        let mut drawn = Vec::with_capacity(count);
        for _ in 0..count {
            if self.draw_pile.is_empty() {
                self.reshuffle();
            }
            match self.draw_pile.pop() {
                Some(card) => drawn.push(card),
                None => break, // every card is in someone's hand
            }
        }
        let player = &mut self.players[seat];
        player.hand.extend(drawn.iter().copied());
        // Any new card voids an earlier «last card!»: the call only covers the
        // hand it was made for (a one-card hand that draws back up to two must
        // call again before playing down to one).
        if !drawn.is_empty() {
            player.uno_called = false;
        }
        if player.hand.len() > 1 && self.uno_vulnerable == Some(seat) {
            self.uno_vulnerable = None;
        }
        if !drawn.is_empty() {
            self.emit(UnoEvent::Drew {
                seat,
                count: drawn.len(),
                reason,
            });
        }
        drawn
    }

    fn reshuffle(&mut self) {
        if self.discard.len() <= 1 {
            return;
        }
        let top = self.discard.pop().expect("checked above");
        let mut cards: Vec<Card> = self.discard.drain(..).collect();
        self.rng.shuffle(&mut cards);
        self.draw_pile = cards;
        self.discard.push(top);
        self.emit(UnoEvent::Reshuffled);
    }

    /// Advances the turn by `steps` seats from the current player.
    fn advance(&mut self, steps: usize) {
        for _ in 0..steps {
            self.current = self.next_seat(self.current);
        }
        self.phase = Phase::Turn;
    }

    /// Applies an action performed by the player in `seat`.
    pub fn apply(&mut self, seat: usize, action: UnoAction) -> Result<(), UnoError> {
        if seat >= self.players.len() {
            return Err(UnoError::NotYourTurn);
        }
        match action {
            UnoAction::Play { card_id, call_uno } => self.play(seat, card_id, call_uno),
            UnoAction::Draw => self.draw(seat),
            UnoAction::Pass => self.pass(seat),
            UnoAction::ChooseColor(color) => self.choose_color(seat, color),
            UnoAction::CallUno => self.call_uno(seat),
            UnoAction::CatchUno { target } => self.catch_uno(seat, target),
            UnoAction::NextRound => self.next_round(),
        }
    }

    fn play(&mut self, seat: usize, card_id: u16, call_uno: bool) -> Result<(), UnoError> {
        if seat != self.current {
            return Err(UnoError::NotYourTurn);
        }
        match self.phase {
            Phase::Turn => {}
            Phase::AfterDraw { card_id: drawn } if drawn == card_id => {}
            Phase::AfterDraw { .. } => return Err(UnoError::CardDoesNotMatch),
            _ => return Err(UnoError::WrongPhase),
        }
        let pos = self.players[seat]
            .hand
            .iter()
            .position(|c| c.id == card_id)
            .ok_or(UnoError::NoSuchCard)?;
        let card = self.players[seat].hand[pos];
        if !self.can_play(&card) {
            return Err(UnoError::CardDoesNotMatch);
        }

        // The previous player is safe from being caught once the next one acts.
        self.uno_vulnerable = None;

        self.players[seat].hand.remove(pos);
        self.discard.push(card);
        self.emit(UnoEvent::Played { seat, card });

        let remaining = self.players[seat].hand.len();
        if remaining == 1 {
            if call_uno || self.players[seat].uno_called {
                if !self.players[seat].uno_called || call_uno {
                    self.emit(UnoEvent::UnoCalled { seat });
                }
                self.players[seat].uno_called = true;
            } else {
                self.uno_vulnerable = Some(seat);
            }
        }

        if let Some(color) = card.color {
            self.active_color = color;
        }

        if remaining == 0 {
            // Going out on a draw card still makes the next player draw.
            let next = self.next_seat(seat);
            match card.face {
                Face::DrawTwo => {
                    self.draw_cards(next, 2, DrawReason::DrawTwo);
                }
                Face::WildDrawFour => {
                    self.draw_cards(next, 4, DrawReason::DrawFour);
                }
                _ => {}
            }
            self.end_round(seat);
            return Ok(());
        }

        match card.face {
            Face::Number(_) => self.advance(1),
            Face::Skip => {
                let skipped = self.next_seat(seat);
                self.emit(UnoEvent::Skipped { seat: skipped });
                self.advance(2);
            }
            Face::Reverse => {
                self.direction = self.direction.flipped();
                self.emit(UnoEvent::Reversed {
                    direction: self.direction,
                });
                if self.players.len() == 2 {
                    // Acts like Skip: the same player goes again.
                    let skipped = self.next_seat(seat);
                    self.emit(UnoEvent::Skipped { seat: skipped });
                    self.advance(2);
                } else {
                    self.advance(1);
                }
            }
            Face::DrawTwo => {
                let victim = self.next_seat(seat);
                self.draw_cards(victim, 2, DrawReason::DrawTwo);
                self.emit(UnoEvent::Skipped { seat: victim });
                self.advance(2);
            }
            Face::Wild => {
                self.phase = Phase::ChooseColor {
                    seat,
                    draw_four: false,
                    advance: true,
                };
            }
            Face::WildDrawFour => {
                self.phase = Phase::ChooseColor {
                    seat,
                    draw_four: true,
                    advance: true,
                };
            }
        }
        Ok(())
    }

    fn draw(&mut self, seat: usize) -> Result<(), UnoError> {
        if seat != self.current {
            return Err(UnoError::NotYourTurn);
        }
        if self.phase != Phase::Turn {
            return Err(UnoError::WrongPhase);
        }
        self.uno_vulnerable = None;
        let drawn = self.draw_cards(seat, 1, DrawReason::Voluntary);
        match drawn.first() {
            Some(card) if self.can_play(card) => {
                self.phase = Phase::AfterDraw { card_id: card.id };
            }
            _ => {
                self.emit(UnoEvent::Passed { seat });
                self.advance(1);
            }
        }
        Ok(())
    }

    fn pass(&mut self, seat: usize) -> Result<(), UnoError> {
        if seat != self.current {
            return Err(UnoError::NotYourTurn);
        }
        if !matches!(self.phase, Phase::AfterDraw { .. }) {
            return Err(UnoError::WrongPhase);
        }
        self.emit(UnoEvent::Passed { seat });
        self.advance(1);
        Ok(())
    }

    fn choose_color(&mut self, seat: usize, color: CardColor) -> Result<(), UnoError> {
        let Phase::ChooseColor {
            seat: chooser,
            draw_four,
            advance,
        } = self.phase
        else {
            return Err(UnoError::WrongPhase);
        };
        if seat != chooser {
            return Err(UnoError::NotYourTurn);
        }
        self.active_color = color;
        self.emit(UnoEvent::ColorChosen { seat, color });
        if draw_four {
            // No challenges: the next player simply draws four and is skipped.
            let victim = self.next_seat(seat);
            self.draw_cards(victim, 4, DrawReason::DrawFour);
            self.emit(UnoEvent::Skipped { seat: victim });
            self.phase = Phase::Turn;
            self.advance(2);
        } else if advance {
            self.advance(1);
        } else {
            self.phase = Phase::Turn;
        }
        Ok(())
    }

    fn call_uno(&mut self, seat: usize) -> Result<(), UnoError> {
        if matches!(self.phase, Phase::RoundOver { .. } | Phase::GameOver { .. }) {
            return Err(UnoError::WrongPhase);
        }
        let len = self.players[seat].hand.len();
        let on_turn_with_two = len == 2
            && seat == self.current
            && matches!(self.phase, Phase::Turn | Phase::AfterDraw { .. });
        if !(len == 1 || on_turn_with_two) {
            return Err(UnoError::CannotCallUno);
        }
        if self.players[seat].uno_called {
            return Ok(());
        }
        self.players[seat].uno_called = true;
        if self.uno_vulnerable == Some(seat) {
            self.uno_vulnerable = None;
        }
        self.emit(UnoEvent::UnoCalled { seat });
        Ok(())
    }

    fn catch_uno(&mut self, catcher: usize, target: usize) -> Result<(), UnoError> {
        if target >= self.players.len()
            || catcher == target
            || self.uno_vulnerable != Some(target)
            || self.players[target].uno_called
            || self.players[target].hand.len() != 1
        {
            return Err(UnoError::NothingToCatch);
        }
        self.uno_vulnerable = None;
        self.emit(UnoEvent::UnoCaught { catcher, target });
        self.draw_cards(target, 2, DrawReason::UnoPenalty);
        Ok(())
    }

    fn end_round(&mut self, winner: usize) {
        let points: u32 = self
            .players
            .iter()
            .enumerate()
            .filter(|(i, _)| *i != winner)
            .flat_map(|(_, p)| p.hand.iter())
            .map(Card::points)
            .sum();
        self.players[winner].score += points;
        self.uno_vulnerable = None;
        self.emit(UnoEvent::RoundWon {
            seat: winner,
            points,
        });
        if self.target_score == 0 || self.players[winner].score >= self.target_score {
            self.emit(UnoEvent::GameWon { seat: winner });
            self.phase = Phase::GameOver { winner };
        } else {
            self.phase = Phase::RoundOver { winner, points };
        }
    }

    fn next_round(&mut self) -> Result<(), UnoError> {
        if !matches!(self.phase, Phase::RoundOver { .. }) {
            return Err(UnoError::WrongPhase);
        }
        self.dealer = (self.dealer + 1) % self.players.len();
        self.start_round();
        Ok(())
    }

    /// A copy of the state as `viewer` may see it: other hands, the stock and
    /// any secret flags are hidden. `None` hides every hand (spectator).
    pub fn view_for(&self, viewer: Option<usize>) -> UnoGame {
        let mut view = self.clone();
        for (i, p) in view.players.iter_mut().enumerate() {
            if Some(i) != viewer {
                p.hand.iter_mut().for_each(|c| *c = Card::hidden());
            }
        }
        view.draw_pile.iter_mut().for_each(|c| *c = Card::hidden());
        match &mut view.phase {
            Phase::AfterDraw { card_id } if Some(view.current) != viewer => {
                *card_id = Card::HIDDEN_ID;
            }
            _ => {}
        }
        view.rng = Rng::new(0);
        view
    }

    /// Total of all cards in all places — used by tests to make sure no card
    /// is ever lost or duplicated.
    pub fn total_cards(&self) -> usize {
        self.draw_pile.len()
            + self.discard.len()
            + self.players.iter().map(|p| p.hand.len()).sum::<usize>()
    }
}
