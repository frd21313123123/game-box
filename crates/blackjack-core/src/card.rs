use serde::{Deserialize, Serialize};

/// Number of 52-card decks shuffled together into the shoe.
pub const DECKS: usize = 6;
pub const SHOE_SIZE: usize = DECKS * 52;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Suit {
    Spades,
    Hearts,
    Diamonds,
    Clubs,
}

impl Suit {
    pub const ALL: [Suit; 4] = [Suit::Spades, Suit::Hearts, Suit::Diamonds, Suit::Clubs];

    pub fn index(self) -> usize {
        match self {
            Suit::Spades => 0,
            Suit::Hearts => 1,
            Suit::Diamonds => 2,
            Suit::Clubs => 3,
        }
    }

    pub fn is_red(self) -> bool {
        matches!(self, Suit::Hearts | Suit::Diamonds)
    }

    pub fn symbol(self) -> &'static str {
        match self {
            Suit::Spades => "♠",
            Suit::Hearts => "♥",
            Suit::Diamonds => "♦",
            Suit::Clubs => "♣",
        }
    }

    /// Genitive plural, as in «туз пик».
    pub fn name_ru(self) -> &'static str {
        match self {
            Suit::Spades => "пик",
            Suit::Hearts => "червей",
            Suit::Diamonds => "бубен",
            Suit::Clubs => "треф",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Rank {
    Two,
    Three,
    Four,
    Five,
    Six,
    Seven,
    Eight,
    Nine,
    Ten,
    Jack,
    Queen,
    King,
    Ace,
}

impl Rank {
    pub const ALL: [Rank; 13] = [
        Rank::Two,
        Rank::Three,
        Rank::Four,
        Rank::Five,
        Rank::Six,
        Rank::Seven,
        Rank::Eight,
        Rank::Nine,
        Rank::Ten,
        Rank::Jack,
        Rank::Queen,
        Rank::King,
        Rank::Ace,
    ];

    pub fn index(self) -> usize {
        Rank::ALL.iter().position(|r| *r == self).expect("every rank is listed")
    }

    /// Points of the card; an ace counts 11 here (see [`hand_value`] for soft hands).
    pub fn value(self) -> u8 {
        match self {
            Rank::Two => 2,
            Rank::Three => 3,
            Rank::Four => 4,
            Rank::Five => 5,
            Rank::Six => 6,
            Rank::Seven => 7,
            Rank::Eight => 8,
            Rank::Nine => 9,
            Rank::Ten | Rank::Jack | Rank::Queen | Rank::King => 10,
            Rank::Ace => 11,
        }
    }

    /// Index printed in the card corner.
    pub fn label(self) -> &'static str {
        match self {
            Rank::Two => "2",
            Rank::Three => "3",
            Rank::Four => "4",
            Rank::Five => "5",
            Rank::Six => "6",
            Rank::Seven => "7",
            Rank::Eight => "8",
            Rank::Nine => "9",
            Rank::Ten => "10",
            Rank::Jack => "J",
            Rank::Queen => "Q",
            Rank::King => "K",
            Rank::Ace => "A",
        }
    }

    pub fn name_ru(self) -> &'static str {
        match self {
            Rank::Jack => "валет",
            Rank::Queen => "дама",
            Rank::King => "король",
            Rank::Ace => "туз",
            other => other.label(),
        }
    }
}

/// A physical card of the shoe. `id` is unique within the shoe so the
/// renderer can follow a card from the shoe to a hand and to the discard tray.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Card {
    pub id: u16,
    pub suit: Suit,
    pub rank: Rank,
}

impl Card {
    /// Id of a face-down card in a redacted view (the dealer's hole card).
    pub const HIDDEN_ID: u16 = u16::MAX;

    pub fn hidden() -> Card {
        Card {
            id: Self::HIDDEN_ID,
            suit: Suit::Spades,
            rank: Rank::Two,
        }
    }

    pub fn is_hidden(&self) -> bool {
        self.id == Self::HIDDEN_ID
    }

    /// Short form for the event feed, e.g. «10♥» or «K♠».
    pub fn short(&self) -> String {
        format!("{}{}", self.rank.label(), self.suit.symbol())
    }

    pub fn describe_ru(&self) -> String {
        format!("{} {}", self.rank.name_ru(), self.suit.name_ru())
    }
}

/// The shoe: [`DECKS`] standard 52-card decks, unshuffled.
pub fn full_shoe() -> Vec<Card> {
    let mut cards = Vec::with_capacity(SHOE_SIZE);
    let mut id = 0u16;
    for _ in 0..DECKS {
        for suit in Suit::ALL {
            for rank in Rank::ALL {
                cards.push(Card { id, suit, rank });
                id += 1;
            }
        }
    }
    cards
}

/// The total of a hand and whether an ace still counts as 11 in it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct HandValue {
    pub total: u8,
    pub soft: bool,
}

impl HandValue {
    pub fn is_bust(self) -> bool {
        self.total > 21
    }

    /// «17», «мягкие 17» or «перебор 23».
    pub fn describe_ru(self) -> String {
        if self.is_bust() {
            format!("перебор {}", self.total)
        } else if self.soft {
            format!("{} / {}", self.total - 10, self.total)
        } else {
            self.total.to_string()
        }
    }
}

/// Value of the visible cards of a hand (face-down cards are ignored). Aces
/// count 11 unless that would bust the hand, then 1.
pub fn hand_value(cards: &[Card]) -> HandValue {
    let mut total: u32 = 0;
    let mut aces = 0;
    for c in cards.iter().filter(|c| !c.is_hidden()) {
        total += c.rank.value() as u32;
        if c.rank == Rank::Ace {
            aces += 1;
        }
    }
    while total > 21 && aces > 0 {
        total -= 10;
        aces -= 1;
    }
    HandValue {
        total: total.min(u8::MAX as u32) as u8,
        soft: aces > 0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn c(rank: Rank) -> Card {
        Card { id: 0, suit: Suit::Hearts, rank }
    }

    #[test]
    fn shoe_has_unique_cards() {
        let shoe = full_shoe();
        assert_eq!(shoe.len(), SHOE_SIZE);
        let mut ids: Vec<u16> = shoe.iter().map(|c| c.id).collect();
        ids.sort();
        ids.dedup();
        assert_eq!(ids.len(), SHOE_SIZE);
        assert_eq!(shoe.iter().filter(|c| c.rank == Rank::Ace).count(), 4 * DECKS);
        assert_eq!(shoe.iter().filter(|c| c.rank.value() == 10).count(), 16 * DECKS);
    }

    #[test]
    fn values_and_soft_aces() {
        assert_eq!(hand_value(&[c(Rank::Ace), c(Rank::King)]), HandValue { total: 21, soft: true });
        assert_eq!(hand_value(&[c(Rank::Ace), c(Rank::Six)]), HandValue { total: 17, soft: true });
        assert_eq!(
            hand_value(&[c(Rank::Ace), c(Rank::Six), c(Rank::Nine)]),
            HandValue { total: 16, soft: false }
        );
        assert_eq!(hand_value(&[c(Rank::Ace), c(Rank::Ace)]), HandValue { total: 12, soft: true });
        assert_eq!(
            hand_value(&[c(Rank::King), c(Rank::Queen), c(Rank::Five)]),
            HandValue { total: 25, soft: false }
        );
        assert!(hand_value(&[c(Rank::King), c(Rank::Queen), c(Rank::Five)]).is_bust());
        // The hole card does not count while it is face down.
        assert_eq!(hand_value(&[c(Rank::Ten), Card::hidden()]).total, 10);
    }
}
