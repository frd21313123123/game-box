//! Uno rules engine: deck, turns, penalties, challenges, scoring and bots.
//!
//! The engine is pure and deterministic (all randomness comes from the seeded
//! [`Rng`] stored in the state) so the host can run it and send per-player
//! views ([`UnoGame::view_for`]) to clients.

pub mod bot;
pub mod card;
pub mod game;
pub mod rng;

pub use bot::{best_color, bot_action, bot_catch};
pub use card::{full_deck, Card, CardColor, Face};
pub use game::*;
pub use rng::Rng;

#[cfg(test)]
mod tests {
    use super::*;

    fn names(n: usize) -> Vec<(String, bool)> {
        (0..n).map(|i| (format!("P{i}"), true)).collect()
    }

    fn card(id: u16, color: Option<CardColor>, face: Face) -> Card {
        Card { id, color, face }
    }

    /// A game with hand-crafted hands and top card.
    fn rigged(hands: Vec<Vec<Card>>, top: Card, active: CardColor) -> UnoGame {
        let mut g = UnoGame::new(names(hands.len()), 1, 0);
        for (p, hand) in g.players.iter_mut().zip(hands) {
            p.hand = hand;
            p.uno_called = false;
        }
        g.discard = vec![top];
        g.draw_pile = (500..560)
            .map(|id| card(id, Some(CardColor::Green), Face::Number(3)))
            .collect();
        g.active_color = active;
        g.current = 0;
        g.direction = Direction::Clockwise;
        g.phase = Phase::Turn;
        g.uno_vulnerable = None;
        g
    }

    #[test]
    fn deals_seven_cards_each() {
        for seed in 0..50 {
            let g = UnoGame::new(names(4), seed, 500);
            assert_eq!(g.total_cards(), 108);
            // Players may have drawn 2 extra from a first-card Draw Two.
            for p in &g.players {
                assert!(p.hand.len() == 7 || p.hand.len() == 9);
            }
            assert_ne!(g.top_card().face, Face::WildDrawFour);
        }
    }

    #[test]
    fn skip_reverse_draw_two() {
        let red = Some(CardColor::Red);
        let mut g = rigged(
            vec![
                vec![card(1, red, Face::Skip), card(2, red, Face::Number(1))],
                vec![card(3, red, Face::Number(2)), card(4, red, Face::Number(4))],
                vec![card(5, red, Face::Number(3)), card(6, red, Face::Number(4))],
            ],
            card(0, red, Face::Number(9)),
            CardColor::Red,
        );
        g.apply(0, UnoAction::Play { card_id: 1, call_uno: true }).unwrap();
        assert_eq!(g.current, 2, "seat 1 is skipped");

        let mut g = rigged(
            vec![
                vec![card(1, red, Face::Reverse), card(2, red, Face::Number(1))],
                vec![card(3, red, Face::Number(2))],
                vec![card(5, red, Face::Number(3))],
            ],
            card(0, red, Face::Number(9)),
            CardColor::Red,
        );
        g.apply(0, UnoAction::Play { card_id: 1, call_uno: true }).unwrap();
        assert_eq!(g.direction, Direction::CounterClockwise);
        assert_eq!(g.current, 2);

        let mut g = rigged(
            vec![
                vec![card(1, red, Face::DrawTwo), card(2, red, Face::Number(1))],
                vec![card(3, red, Face::Number(2))],
                vec![card(5, red, Face::Number(3))],
            ],
            card(0, red, Face::Number(9)),
            CardColor::Red,
        );
        g.apply(0, UnoAction::Play { card_id: 1, call_uno: true }).unwrap();
        assert_eq!(g.players[1].hand.len(), 3);
        assert_eq!(g.current, 2);
    }

    #[test]
    fn reverse_with_two_players_acts_as_skip() {
        let red = Some(CardColor::Red);
        let mut g = rigged(
            vec![
                vec![card(1, red, Face::Reverse), card(2, red, Face::Number(1)), card(7, red, Face::Number(1))],
                vec![card(3, red, Face::Number(2))],
            ],
            card(0, red, Face::Number(9)),
            CardColor::Red,
        );
        g.apply(0, UnoAction::Play { card_id: 1, call_uno: false }).unwrap();
        assert_eq!(g.current, 0);
    }

    #[test]
    fn cannot_play_mismatched_card() {
        let mut g = rigged(
            vec![
                vec![card(1, Some(CardColor::Blue), Face::Number(2)), card(2, None, Face::Wild)],
                vec![card(3, Some(CardColor::Red), Face::Number(2))],
            ],
            card(0, Some(CardColor::Red), Face::Number(9)),
            CardColor::Red,
        );
        assert_eq!(
            g.apply(0, UnoAction::Play { card_id: 1, call_uno: false }),
            Err(UnoError::CardDoesNotMatch)
        );
        assert_eq!(
            g.apply(1, UnoAction::Play { card_id: 3, call_uno: false }),
            Err(UnoError::NotYourTurn)
        );
    }

    #[test]
    fn wild_requires_color_choice() {
        let mut g = rigged(
            vec![
                vec![card(1, None, Face::Wild), card(2, Some(CardColor::Blue), Face::Number(1))],
                vec![card(3, Some(CardColor::Red), Face::Number(2))],
            ],
            card(0, Some(CardColor::Red), Face::Number(9)),
            CardColor::Red,
        );
        g.apply(0, UnoAction::Play { card_id: 1, call_uno: true }).unwrap();
        assert!(matches!(g.phase, Phase::ChooseColor { seat: 0, .. }));
        assert_eq!(g.apply(1, UnoAction::Draw), Err(UnoError::NotYourTurn));
        g.apply(0, UnoAction::ChooseColor(CardColor::Blue)).unwrap();
        assert_eq!(g.active_color, CardColor::Blue);
        assert_eq!(g.current, 1);
        assert_eq!(g.phase, Phase::Turn);
    }

    #[test]
    fn draw_four_makes_next_player_draw_and_skip() {
        let red = Some(CardColor::Red);
        let mut g = rigged(
            vec![
                vec![card(1, None, Face::WildDrawFour), card(2, red, Face::Number(1)), card(9, Some(CardColor::Blue), Face::Number(1))],
                vec![card(3, red, Face::Number(2))],
                vec![card(4, red, Face::Number(2))],
            ],
            card(0, red, Face::Number(9)),
            CardColor::Red,
        );
        g.apply(0, UnoAction::Play { card_id: 1, call_uno: false }).unwrap();
        g.apply(0, UnoAction::ChooseColor(CardColor::Blue)).unwrap();
        assert_eq!(g.players[1].hand.len(), 5);
        assert_eq!(g.active_color, CardColor::Blue);
        assert_eq!(g.current, 2);
        assert_eq!(g.phase, Phase::Turn);
    }

    #[test]
    fn forgetting_uno_can_be_caught() {
        let red = Some(CardColor::Red);
        let mut g = rigged(
            vec![
                vec![card(1, red, Face::Number(1)), card(2, red, Face::Number(4))],
                vec![card(3, red, Face::Number(2)), card(5, red, Face::Number(2))],
            ],
            card(0, red, Face::Number(9)),
            CardColor::Red,
        );
        g.apply(0, UnoAction::Play { card_id: 1, call_uno: false }).unwrap();
        assert_eq!(g.uno_vulnerable, Some(0));
        assert_eq!(g.apply(0, UnoAction::CatchUno { target: 0 }), Err(UnoError::NothingToCatch));
        g.apply(1, UnoAction::CatchUno { target: 0 }).unwrap();
        assert_eq!(g.players[0].hand.len(), 3);
        assert_eq!(g.uno_vulnerable, None);
    }

    #[test]
    fn calling_uno_late_protects_you() {
        let red = Some(CardColor::Red);
        let mut g = rigged(
            vec![
                vec![card(1, red, Face::Number(1)), card(2, red, Face::Number(4))],
                vec![card(3, red, Face::Number(2)), card(5, red, Face::Number(2))],
            ],
            card(0, red, Face::Number(9)),
            CardColor::Red,
        );
        g.apply(0, UnoAction::Play { card_id: 1, call_uno: false }).unwrap();
        g.apply(0, UnoAction::CallUno).unwrap();
        assert_eq!(g.apply(1, UnoAction::CatchUno { target: 0 }), Err(UnoError::NothingToCatch));
    }

    #[test]
    fn next_player_acting_ends_catch_window() {
        let red = Some(CardColor::Red);
        let mut g = rigged(
            vec![
                vec![card(1, red, Face::Number(1)), card(2, red, Face::Number(4))],
                vec![card(3, red, Face::Number(2)), card(5, red, Face::Number(2))],
            ],
            card(0, red, Face::Number(9)),
            CardColor::Red,
        );
        g.apply(0, UnoAction::Play { card_id: 1, call_uno: false }).unwrap();
        g.apply(1, UnoAction::Play { card_id: 3, call_uno: false }).unwrap();
        assert_eq!(g.uno_vulnerable, Some(1));
        assert_eq!(g.apply(1, UnoAction::CatchUno { target: 0 }), Err(UnoError::NothingToCatch));
    }

    #[test]
    fn draw_then_play_or_pass() {
        let mut g = rigged(
            vec![
                vec![card(1, Some(CardColor::Blue), Face::Number(1)), card(2, Some(CardColor::Blue), Face::Number(4))],
                vec![card(3, Some(CardColor::Red), Face::Number(2))],
            ],
            card(0, Some(CardColor::Green), Face::Number(9)),
            CardColor::Green,
        );
        // Stock is all green 3s, so the drawn card is playable.
        g.apply(0, UnoAction::Draw).unwrap();
        let Phase::AfterDraw { card_id } = g.phase else { panic!("expected AfterDraw") };
        assert_eq!(
            g.apply(0, UnoAction::Play { card_id: 1, call_uno: false }),
            Err(UnoError::CardDoesNotMatch)
        );
        g.apply(0, UnoAction::Pass).unwrap();
        assert_eq!(g.current, 1);
        assert!(g.players[0].hand.iter().any(|c| c.id == card_id));
    }

    #[test]
    fn going_out_scores_points() {
        let red = Some(CardColor::Red);
        let mut g = rigged(
            vec![
                vec![card(1, red, Face::Number(1))],
                vec![card(3, red, Face::Number(7)), card(4, None, Face::Wild), card(5, red, Face::Skip)],
            ],
            card(0, red, Face::Number(9)),
            CardColor::Red,
        );
        g.target_score = 500;
        g.apply(0, UnoAction::Play { card_id: 1, call_uno: false }).unwrap();
        assert_eq!(g.phase, Phase::RoundOver { winner: 0, points: 77 });
        assert_eq!(g.players[0].score, 77);
        g.apply(1, UnoAction::NextRound).unwrap();
        assert_eq!(g.round, 2);
        assert_eq!(g.total_cards(), 108);
    }

    #[test]
    fn redacted_view_hides_other_hands() {
        let g = UnoGame::new(names(3), 7, 0);
        let v = g.view_for(Some(1));
        assert!(v.players[0].hand.iter().all(Card::is_hidden));
        assert!(v.players[1].hand.iter().all(|c| !c.is_hidden()));
        assert!(v.draw_pile.iter().all(Card::is_hidden));
        assert_eq!(v.players[0].hand.len(), g.players[0].hand.len());
        // Round-trips through JSON for the network layer.
        let json = serde_json::to_string(&v).unwrap();
        let back: UnoGame = serde_json::from_str(&json).unwrap();
        assert_eq!(back, v);
    }

    /// Bots play many complete matches; no card may be lost and every game ends.
    #[test]
    fn bots_finish_games_without_losing_cards() {
        for seed in 0..60u64 {
            let players = 2 + (seed as usize % 7);
            let mut g = UnoGame::new(names(players), seed, 200);
            let mut rng = Rng::new(seed ^ 0xABCD);
            let mut steps = 0;
            loop {
                steps += 1;
                assert!(steps < 20_000, "game {seed} did not finish");
                assert_eq!(g.total_cards(), 108, "seed {seed}");
                match g.phase {
                    Phase::GameOver { .. } => break,
                    Phase::RoundOver { .. } => {
                        g.apply(0, UnoAction::NextRound).unwrap();
                        continue;
                    }
                    _ => {}
                }
                for seat in 0..players {
                    if let Some(a) = bot_catch(&g, seat, &mut rng, 0.3) {
                        g.apply(seat, a).unwrap();
                    }
                }
                let seat = g.acting_seat().unwrap();
                let action = bot_action(&g, seat, &mut rng).expect("bot must act");
                g.apply(seat, action).unwrap();
            }
            let Phase::GameOver { winner } = g.phase else { unreachable!() };
            assert!(g.players[winner].score >= 200);
        }
    }
}
