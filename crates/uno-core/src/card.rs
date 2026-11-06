use serde::{Deserialize, Serialize};

/// One of the four card colours. Wild cards have no colour of their own.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum CardColor {
    Red,
    Yellow,
    Green,
    Blue,
}

impl CardColor {
    pub const ALL: [CardColor; 4] = [
        CardColor::Red,
        CardColor::Yellow,
        CardColor::Green,
        CardColor::Blue,
    ];

    pub fn name_ru(self) -> &'static str {
        match self {
            CardColor::Red => "Красный",
            CardColor::Yellow => "Жёлтый",
            CardColor::Green => "Зелёный",
            CardColor::Blue => "Синий",
        }
    }

    /// sRGB colour used by every renderer, so the 3D cards, the UI swatches and
    /// the table glow agree on the exact shade.
    pub fn rgb(self) -> [u8; 3] {
        match self {
            CardColor::Red => [0xE5, 0x2D, 0x27],
            CardColor::Yellow => [0xF7, 0xC3, 0x1A],
            CardColor::Green => [0x2F, 0xA8, 0x4F],
            CardColor::Blue => [0x1F, 0x6F, 0xD6],
        }
    }

    pub fn index(self) -> usize {
        match self {
            CardColor::Red => 0,
            CardColor::Yellow => 1,
            CardColor::Green => 2,
            CardColor::Blue => 3,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Face {
    Number(u8),
    Skip,
    Reverse,
    DrawTwo,
    Wild,
    WildDrawFour,
}

impl Face {
    pub fn name_ru(self) -> String {
        match self {
            Face::Number(n) => n.to_string(),
            Face::Skip => "Пропуск хода".into(),
            Face::Reverse => "Смена направления".into(),
            Face::DrawTwo => "Возьми две".into(),
            Face::Wild => "Заказ цвета".into(),
            Face::WildDrawFour => "Возьми четыре".into(),
        }
    }
}

/// A physical card. `id` is unique within the 108-card deck so the renderer can
/// follow a card from hand to the discard pile.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Card {
    pub id: u16,
    pub color: Option<CardColor>,
    pub face: Face,
}

impl Card {
    /// Id used for face-down cards in a redacted (per-player) view.
    pub const HIDDEN_ID: u16 = u16::MAX;

    pub fn hidden() -> Card {
        Card {
            id: Self::HIDDEN_ID,
            color: None,
            face: Face::Wild,
        }
    }

    pub fn is_hidden(&self) -> bool {
        self.id == Self::HIDDEN_ID
    }

    pub fn is_wild(&self) -> bool {
        matches!(self.face, Face::Wild | Face::WildDrawFour)
    }

    /// Points this card is worth to the round winner when left in a hand.
    pub fn points(&self) -> u32 {
        match self.face {
            Face::Number(n) => n as u32,
            Face::Skip | Face::Reverse | Face::DrawTwo => 20,
            Face::Wild | Face::WildDrawFour => 50,
        }
    }

    /// Whether this card may be played on `top` while `active` is the colour in play.
    pub fn matches(&self, top: &Card, active: CardColor) -> bool {
        if self.is_wild() {
            return true;
        }
        if self.color == Some(active) {
            return true;
        }
        !top.is_wild() && self.face == top.face
    }

    pub fn describe_ru(&self) -> String {
        match self.color {
            Some(c) => format!("{} {}", c.name_ru(), self.face.name_ru()),
            None => self.face.name_ru(),
        }
    }
}

/// The standard 108-card deck: per colour one 0, two of each 1–9, two Skip,
/// two Reverse, two Draw Two; plus four Wild and four Wild Draw Four.
pub fn full_deck() -> Vec<Card> {
    let mut cards = Vec::with_capacity(108);
    let mut id = 0u16;
    let mut push = |cards: &mut Vec<Card>, color: Option<CardColor>, face: Face| {
        cards.push(Card { id, color, face });
        id += 1;
    };
    for color in CardColor::ALL {
        push(&mut cards, Some(color), Face::Number(0));
        for n in 1..=9 {
            push(&mut cards, Some(color), Face::Number(n));
            push(&mut cards, Some(color), Face::Number(n));
        }
        for face in [Face::Skip, Face::Reverse, Face::DrawTwo] {
            push(&mut cards, Some(color), face);
            push(&mut cards, Some(color), face);
        }
    }
    for _ in 0..4 {
        push(&mut cards, None, Face::Wild);
    }
    for _ in 0..4 {
        push(&mut cards, None, Face::WildDrawFour);
    }
    cards
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn deck_has_108_unique_cards() {
        let deck = full_deck();
        assert_eq!(deck.len(), 108);
        let mut ids: Vec<u16> = deck.iter().map(|c| c.id).collect();
        ids.sort();
        ids.dedup();
        assert_eq!(ids.len(), 108);
        assert_eq!(deck.iter().filter(|c| c.face == Face::Number(0)).count(), 4);
        assert_eq!(deck.iter().filter(|c| c.face == Face::Number(7)).count(), 8);
        assert_eq!(deck.iter().filter(|c| c.face == Face::WildDrawFour).count(), 4);
        assert_eq!(deck.iter().filter(|c| c.face == Face::Skip).count(), 8);
    }

    #[test]
    fn matching_rules() {
        let red5 = Card { id: 0, color: Some(CardColor::Red), face: Face::Number(5) };
        let blue5 = Card { id: 1, color: Some(CardColor::Blue), face: Face::Number(5) };
        let blue7 = Card { id: 2, color: Some(CardColor::Blue), face: Face::Number(7) };
        let wild = Card { id: 3, color: None, face: Face::Wild };
        assert!(blue5.matches(&red5, CardColor::Red));
        assert!(!blue7.matches(&red5, CardColor::Red));
        assert!(blue7.matches(&red5, CardColor::Blue));
        assert!(wild.matches(&red5, CardColor::Red));
        // On a wild only the chosen colour counts.
        assert!(blue7.matches(&wild, CardColor::Blue));
        assert!(!red5.matches(&wild, CardColor::Blue));
    }
}
