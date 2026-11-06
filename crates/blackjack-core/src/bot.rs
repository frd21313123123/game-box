//! Bots play the textbook basic strategy (dealer stands on soft 17, double
//! after split allowed) and bet a modest share of their stack.

use crate::card::Rank;
use crate::game::{BjAction, BjGame, Hand, Move, Phase, BET_STEP, MAX_BET, MIN_BET};
use crate::rng::Rng;

/// What the bot in `seat` wants to do right now, if anything.
pub fn bot_action(game: &BjGame, seat: usize, rng: &mut Rng) -> Option<BjAction> {
    if game.needs_bet(seat) {
        return Some(BjAction::Bet(bot_bet(game.players[seat].chips, rng)));
    }
    if game.needs_insurance(seat) {
        // Basic strategy never insures; bots give in to the temptation now and then.
        return Some(BjAction::Insurance(rng.chance(0.08)));
    }
    let Phase::PlayerTurn { seat: s, .. } = game.phase else { return None };
    if s != seat {
        return None;
    }
    let hand = game.active_hand(seat)?;
    let up = game.dealer_up()?.rank;
    Some(match basic_strategy(hand, up, game.can_double(seat), game.can_split(seat)) {
        Move::Hit => BjAction::Hit,
        Move::Stand => BjAction::Stand,
        Move::Double => BjAction::Double,
        Move::Split => BjAction::Split,
    })
}

/// A bet of about 4% of the stack, sometimes doubled or halved.
pub fn bot_bet(chips: u32, rng: &mut Rng) -> u32 {
    let base = chips / 25;
    let pick = match rng.below(6) {
        0 => base * 2,
        1 => base / 2,
        _ => base,
    };
    let top = MAX_BET.min(chips - chips % BET_STEP).max(MIN_BET);
    (pick / BET_STEP * BET_STEP).clamp(MIN_BET, top)
}

/// The basic strategy move for `hand` against the dealer's up card.
pub fn basic_strategy(hand: &Hand, up: Rank, can_double: bool, can_split: bool) -> Move {
    let u = up.value(); // 2..=11
    let value = hand.value();
    let total = value.total;

    if can_split && hand.cards.len() == 2 {
        let split = match hand.cards[0].rank {
            Rank::Ace | Rank::Eight => true,
            Rank::Nine => matches!(u, 2..=6 | 8 | 9),
            Rank::Seven | Rank::Two | Rank::Three => u <= 7,
            Rank::Six => u <= 6,
            Rank::Four => u == 5 || u == 6,
            _ => false,
        };
        if split {
            return Move::Split;
        }
    }

    let double_or = |fallback: Move| if can_double { Move::Double } else { fallback };
    if value.soft && total <= 21 {
        return match total {
            13 | 14 if (5..=6).contains(&u) => double_or(Move::Hit),
            15 | 16 if (4..=6).contains(&u) => double_or(Move::Hit),
            17 if (3..=6).contains(&u) => double_or(Move::Hit),
            18 if (3..=6).contains(&u) => double_or(Move::Stand),
            18 if u >= 9 => Move::Hit,
            ..=17 => Move::Hit,
            _ => Move::Stand,
        };
    }
    match total {
        ..=8 => Move::Hit,
        9 if (3..=6).contains(&u) => double_or(Move::Hit),
        10 if u <= 9 => double_or(Move::Hit),
        11 if u <= 10 => double_or(Move::Hit),
        9..=11 => Move::Hit,
        12 if (4..=6).contains(&u) => Move::Stand,
        12 => Move::Hit,
        13..=16 if u <= 6 => Move::Stand,
        13..=16 => Move::Hit,
        _ => Move::Stand,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::card::{Card, Suit};

    fn hand(ranks: &[Rank]) -> Hand {
        let mut g = BjGame::new(vec![("P".into(), true)], 1, 1000, 0);
        g.apply(0, BjAction::Bet(10)).unwrap();
        let mut h = g.players[0].hands[0].clone();
        h.cards = ranks
            .iter()
            .enumerate()
            .map(|(i, r)| Card { id: i as u16, suit: Suit::Clubs, rank: *r })
            .collect();
        h.from_split = false;
        h
    }

    #[test]
    fn textbook_decisions() {
        use Rank::*;
        assert_eq!(basic_strategy(&hand(&[Eight, Eight]), Ten, true, true), Move::Split);
        assert_eq!(basic_strategy(&hand(&[Ten, Ten]), Six, true, true), Move::Stand);
        assert_eq!(basic_strategy(&hand(&[Six, Five]), Six, true, false), Move::Double);
        assert_eq!(basic_strategy(&hand(&[Six, Five]), Six, false, false), Move::Hit);
        assert_eq!(basic_strategy(&hand(&[Ten, Six]), Seven, true, false), Move::Hit);
        assert_eq!(basic_strategy(&hand(&[Ten, Six]), Six, true, false), Move::Stand);
        assert_eq!(basic_strategy(&hand(&[Ten, Two]), Two, true, false), Move::Hit);
        assert_eq!(basic_strategy(&hand(&[Ace, Seven]), Two, true, false), Move::Stand);
        assert_eq!(basic_strategy(&hand(&[Ace, Seven]), Ten, true, false), Move::Hit);
        assert_eq!(basic_strategy(&hand(&[Ace, Six]), Four, true, false), Move::Double);
        assert_eq!(basic_strategy(&hand(&[Ace, Eight]), Six, true, false), Move::Stand);
        assert_eq!(basic_strategy(&hand(&[Ten, Seven]), Ace, true, false), Move::Stand);
    }

    #[test]
    fn bets_stay_in_bounds() {
        let mut rng = Rng::new(3);
        for chips in [10, 15, 40, 100, 999, 1000, 5000, 100_000] {
            for _ in 0..50 {
                let b = bot_bet(chips, &mut rng);
                assert!(b >= MIN_BET && b <= MAX_BET && b <= chips && b % BET_STEP == 0, "{chips}: {b}");
            }
        }
    }
}
