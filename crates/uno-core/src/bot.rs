//! A simple but sensible Uno opponent.

use crate::card::{Card, CardColor, Face};
use crate::game::{Phase, UnoAction, UnoGame};
use crate::rng::Rng;

/// What the bot in `seat` wants to do right now, if it is its move.
pub fn bot_action(game: &UnoGame, seat: usize, rng: &mut Rng) -> Option<UnoAction> {
    match &game.phase {
        Phase::ChooseColor { seat: chooser, .. } if *chooser == seat => {
            Some(UnoAction::ChooseColor(best_color(&game.players[seat].hand, rng)))
        }
        Phase::AfterDraw { card_id } if game.current == seat => {
            let hand = &game.players[seat].hand;
            let drawn = hand.iter().find(|c| c.id == *card_id)?;
            if game.can_play(drawn) {
                Some(UnoAction::Play {
                    card_id: drawn.id,
                    call_uno: hand.len() == 2 && rng.chance(0.85),
                })
            } else {
                Some(UnoAction::Pass)
            }
        }
        Phase::Turn if game.current == seat => {
            let hand = &game.players[seat].hand;
            match choose_card(game, seat, rng) {
                Some(card) => Some(UnoAction::Play {
                    card_id: card.id,
                    call_uno: hand.len() == 2 && rng.chance(0.85),
                }),
                None => Some(UnoAction::Draw),
            }
        }
        _ => None,
    }
}

/// Whether the bot in `seat` notices an opponent who forgot to call UNO.
/// Called periodically by the host; `p` is the per-check probability.
pub fn bot_catch(game: &UnoGame, seat: usize, rng: &mut Rng, p: f32) -> Option<UnoAction> {
    let target = game.uno_vulnerable?;
    if target == seat || !rng.chance(p) {
        return None;
    }
    Some(UnoAction::CatchUno { target })
}

fn choose_card(game: &UnoGame, seat: usize, rng: &mut Rng) -> Option<Card> {
    let hand = &game.players[seat].hand;
    let playable: Vec<Card> = hand.iter().copied().filter(|c| game.can_play(c)).collect();
    if playable.is_empty() {
        return None;
    }
    let next = game.next_seat(seat);
    let next_is_threat = game.players[next].hand.len() <= 2;
    let counts = color_counts(hand);

    let score = |c: &Card| -> f32 {
        let mut s = 0.0;
        match c.face {
            Face::Number(n) => s += n as f32 * 0.6,
            Face::Skip | Face::Reverse => s += if next_is_threat { 30.0 } else { 6.0 },
            Face::DrawTwo => s += if next_is_threat { 40.0 } else { 8.0 },
            // Keep wild cards for emergencies.
            Face::Wild => s -= if hand.len() <= 2 { -5.0 } else { 15.0 },
            Face::WildDrawFour => {
                s -= if next_is_threat { -20.0 } else { 25.0 };
            }
        }
        if let Some(color) = c.color {
            // Prefer staying in the colour we hold most of.
            s += counts[color.index()] as f32 * 1.5;
        }
        s + rng_jitter(c.id)
    };

    let mut best = playable[0];
    let mut best_score = f32::MIN;
    for c in &playable {
        let s = score(c);
        if s > best_score {
            best_score = s;
            best = *c;
        }
    }
    // A sprinkle of unpredictability.
    if playable.len() > 1 && rng.chance(0.1) {
        best = playable[rng.below(playable.len())];
    }
    Some(best)
}

/// Deterministic tiny tie-breaker so equal options don't always resolve the same way.
fn rng_jitter(id: u16) -> f32 {
    ((id as u32).wrapping_mul(2_654_435_761) >> 28) as f32 * 0.01
}

fn color_counts(hand: &[Card]) -> [usize; 4] {
    let mut counts = [0usize; 4];
    for c in hand {
        if let Some(color) = c.color {
            counts[color.index()] += 1;
        }
    }
    counts
}

pub fn best_color(hand: &[Card], rng: &mut Rng) -> CardColor {
    let counts = color_counts(hand);
    let max = counts.iter().copied().max().unwrap_or(0);
    if max == 0 {
        return CardColor::ALL[rng.below(4)];
    }
    let best: Vec<CardColor> = CardColor::ALL
        .into_iter()
        .filter(|c| counts[c.index()] == max)
        .collect();
    best[rng.below(best.len())]
}
