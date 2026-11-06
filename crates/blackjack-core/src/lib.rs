//! Blackjack («21») rules engine: a six-deck shoe, bets, insurance, doubling,
//! splitting, the dealer's play and bots.
//!
//! Like the other cores it is pure and deterministic (all randomness comes from
//! the seeded [`Rng`] in the state), so the host runs it and sends every client
//! a view ([`BjGame::view_for`]) without the shoe and the hole card.

pub mod bot;
pub mod card;
pub mod game;
pub mod rng;

pub use bot::{basic_strategy, bot_action, bot_bet};
pub use card::{full_shoe, hand_value, Card, HandValue, Rank, Suit, DECKS, SHOE_SIZE};
pub use game::*;
pub use rng::Rng;

#[cfg(test)]
mod tests {
    use super::*;

    fn names(n: usize) -> Vec<(String, bool)> {
        (0..n).map(|i| (format!("P{i}"), true)).collect()
    }

    /// Makes the next cards drawn from the shoe have these ranks, in order.
    fn stack(g: &mut BjGame, ranks: &[Rank]) {
        for (i, r) in ranks.iter().enumerate() {
            // Take a matching card out of the shoe and put it on top.
            let from_top = g.shoe.len() - 1 - i;
            let pos = g.shoe[..=from_top]
                .iter()
                .position(|c| c.rank == *r)
                .expect("the shoe has this rank");
            let card = g.shoe.remove(pos);
            g.shoe.insert(from_top, card);
        }
    }

    /// Deal order for one player: P, D(up), P, D(hole).
    fn one_player(ranks: &[Rank], bet: u32) -> BjGame {
        let mut g = BjGame::new(names(1), 7, 1000, 0);
        stack(&mut g, ranks);
        g.apply(0, BjAction::Bet(bet)).unwrap();
        g
    }

    fn run_dealer(g: &mut BjGame) {
        while g.dealer_step() {}
    }

    #[test]
    fn bets_are_validated() {
        let mut g = BjGame::new(names(2), 1, 100, 0);
        assert_eq!(g.apply(0, BjAction::Bet(5)), Err(BjError::BadBet));
        assert_eq!(g.apply(0, BjAction::Bet(15)), Err(BjError::BadBet));
        assert_eq!(g.apply(0, BjAction::Bet(510)), Err(BjError::BadBet));
        assert_eq!(g.apply(0, BjAction::Bet(200)), Err(BjError::NotEnoughChips));
        assert_eq!(g.apply(0, BjAction::Hit), Err(BjError::WrongPhase));
        g.apply(0, BjAction::Bet(100)).unwrap();
        assert_eq!(g.players[0].chips, 0);
        assert_eq!(g.apply(0, BjAction::Bet(10)), Err(BjError::AlreadyBet));
        // Nothing is dealt until everyone has bet.
        assert_eq!(g.phase, Phase::Betting);
        assert!(g.dealer.is_empty());
        g.apply(1, BjAction::Bet(10)).unwrap();
        assert_eq!(g.dealer.len(), 2);
        assert_eq!(g.players[0].hands[0].cards.len(), 2);
    }

    #[test]
    fn natural_pays_three_to_two() {
        use Rank::*;
        let mut g = one_player(&[Ace, Nine, King, Seven], 100);
        assert!(g.players[0].hands[0].is_blackjack());
        assert_eq!(g.phase, Phase::DealerTurn);
        run_dealer(&mut g);
        assert_eq!(g.players[0].hands[0].outcome, Some(Outcome::Blackjack));
        assert_eq!(g.players[0].chips, 1150);
        assert_eq!(g.players[0].round_net(), 150);
        assert_eq!(g.phase, Phase::RoundOver);
    }

    #[test]
    fn dealer_blackjack_is_found_by_the_peek() {
        use Rank::*;
        let g = one_player(&[Ten, King, Nine, Ace], 50);
        // No turn for the player: the round is over at once.
        assert!(g.hole_revealed);
        assert_eq!(g.phase, Phase::RoundOver);
        assert_eq!(g.players[0].hands[0].outcome, Some(Outcome::Lose));
        assert_eq!(g.players[0].chips, 950);
    }

    #[test]
    fn insurance_pays_two_to_one() {
        use Rank::*;
        let mut g = one_player(&[Ten, Ace, Nine, Queen], 100);
        assert_eq!(g.phase, Phase::Insurance);
        assert_eq!(g.insurance_cost(0), 50);
        g.apply(0, BjAction::Insurance(true)).unwrap();
        assert_eq!(g.phase, Phase::RoundOver);
        // Lost 100 on the hand, insurance 50 returned with 100 profit: even.
        assert_eq!(g.players[0].chips, 1000);
    }

    #[test]
    fn insurance_is_lost_without_dealer_blackjack() {
        use Rank::*;
        let mut g = one_player(&[Ten, Ace, Nine, Six], 100);
        g.apply(0, BjAction::Insurance(true)).unwrap();
        assert_eq!(g.phase, Phase::PlayerTurn { seat: 0, hand: 0 });
        g.apply(0, BjAction::Stand).unwrap();
        run_dealer(&mut g); // dealer A+6 = soft 17 stands
        assert_eq!(g.dealer.len(), 2);
        assert_eq!(g.players[0].hands[0].outcome, Some(Outcome::Win));
        assert_eq!(g.players[0].chips, 1000 - 50 + 100);
    }

    #[test]
    fn dealer_hits_to_seventeen_and_busts() {
        use Rank::*;
        let mut g = one_player(&[Ten, Six, Eight, Ten, Nine], 20);
        g.apply(0, BjAction::Stand).unwrap();
        assert!(g.dealer_step()); // reveal
        assert_eq!(g.dealer_value().total, 16);
        assert!(g.dealer_step()); // draws the nine
        assert_eq!(g.dealer.len(), 3);
        run_dealer(&mut g);
        assert_eq!(g.players[0].hands[0].outcome, Some(Outcome::Win));
        assert_eq!(g.players[0].chips, 1020);
    }

    #[test]
    fn bust_loses_even_if_dealer_busts() {
        use Rank::*;
        let mut g = one_player(&[Ten, Six, Six, Ten, King, Nine], 20);
        g.apply(0, BjAction::Hit).unwrap();
        assert_eq!(g.players[0].hands[0].outcome, Some(Outcome::Bust));
        // Nobody left to play against: the dealer only turns the hole card.
        run_dealer(&mut g);
        assert_eq!(g.dealer.len(), 2);
        assert_eq!(g.players[0].chips, 980);
    }

    #[test]
    fn double_takes_one_card_and_doubles_the_bet() {
        use Rank::*;
        let mut g = one_player(&[Six, Ten, Five, Seven, Ten], 100);
        assert!(g.can_double(0));
        g.apply(0, BjAction::Double).unwrap();
        let h = &g.players[0].hands[0];
        assert_eq!((h.cards.len(), h.bet, h.doubled), (3, 200, true));
        assert_eq!(g.players[0].chips, 800);
        assert_eq!(g.phase, Phase::DealerTurn);
        run_dealer(&mut g); // 21 against 17
        assert_eq!(g.players[0].chips, 1200);
    }

    #[test]
    fn split_makes_two_hands() {
        use Rank::*;
        // P 8, D 10, P 8, D 7 (hole), then 3 to the first half, 10 to the second.
        let mut g = one_player(&[Eight, Ten, Eight, Seven, Three, Ten, Ten], 50);
        assert!(g.can_split(0));
        assert!(!g.can_split(1));
        g.apply(0, BjAction::Split).unwrap();
        let p = &g.players[0];
        assert_eq!(p.hands.len(), 2);
        assert_eq!(p.chips, 900);
        assert_eq!(p.hands[0].value().total, 11);
        assert_eq!(p.hands[1].value().total, 18);
        assert!(p.hands.iter().all(|h| h.from_split && h.bet == 50));
        // Double after split.
        g.apply(0, BjAction::Double).unwrap();
        assert_eq!(g.players[0].hands[0].value().total, 21);
        assert_eq!(g.phase, Phase::PlayerTurn { seat: 0, hand: 1 });
        g.apply(0, BjAction::Stand).unwrap();
        run_dealer(&mut g); // dealer 17
        let p = &g.players[0];
        assert_eq!(p.hands[0].outcome, Some(Outcome::Win));
        assert_eq!(p.hands[1].outcome, Some(Outcome::Win));
        assert_eq!(p.chips, 1000 + 100 + 50);
    }

    #[test]
    fn split_aces_get_one_card_and_21_is_not_blackjack() {
        use Rank::*;
        let mut g = one_player(&[Ace, Nine, Ace, Eight, King, Queen], 10);
        g.apply(0, BjAction::Split).unwrap();
        let p = &g.players[0];
        assert!(p.hands.iter().all(|h| h.done && h.cards.len() == 2));
        assert!(!p.hands[0].is_blackjack());
        assert_eq!(g.phase, Phase::DealerTurn);
        run_dealer(&mut g); // dealer 17
        assert_eq!(g.players[0].hands[0].outcome, Some(Outcome::Win));
        assert_eq!(g.players[0].hands[1].outcome, Some(Outcome::Win));
        assert_eq!(g.players[0].chips, 1000 + 10 + 10);
    }

    #[test]
    fn turns_go_seat_by_seat() {
        let mut g = BjGame::new(names(3), 11, 1000, 0);
        for s in 0..3 {
            g.apply(s, BjAction::Bet(10)).unwrap();
        }
        if g.phase == Phase::Insurance {
            for s in 0..3 {
                if g.needs_insurance(s) {
                    g.apply(s, BjAction::Insurance(false)).unwrap();
                }
            }
        }
        let mut seen = Vec::new();
        while let Phase::PlayerTurn { seat, .. } = g.phase {
            assert!(seen.last().is_none_or(|s| *s <= seat));
            seen.push(seat);
            let other = (seat + 1) % 3;
            assert_eq!(g.apply(other, BjAction::Stand), Err(BjError::NotYourTurn));
            g.apply(seat, BjAction::Stand).unwrap();
        }
    }

    #[test]
    fn view_hides_shoe_and_hole_card() {
        use Rank::*;
        let mut g = one_player(&[Ten, Six, Eight, Ten], 20);
        let v = g.view_for(Some(0));
        assert!(v.shoe.iter().all(Card::is_hidden));
        assert!(v.dealer[1].is_hidden());
        assert!(!v.dealer[0].is_hidden());
        assert_eq!(v.dealer_value().total, 6);
        // The hole card is dealt as a hidden card in the event log too.
        assert!(g.events.iter().all(|e| match &e.event {
            BjEvent::Dealt { to: Spot::Dealer, card } => card.is_hidden() || card.id == g.dealer[0].id,
            _ => true,
        }));
        g.apply(0, BjAction::Stand).unwrap();
        g.dealer_step();
        assert_eq!(g.view_for(Some(0)).dealer[1], g.dealer[1]);
    }

    #[test]
    fn match_ends_after_the_round_limit() {
        let mut g = BjGame::new(names(2), 5, 1000, 2);
        let mut rng = Rng::new(9);
        for _ in 0..2000 {
            if matches!(g.phase, Phase::GameOver { .. }) {
                break;
            }
            step(&mut g, &mut rng);
        }
        assert_eq!(g.round, 2);
        let Phase::GameOver { winner } = g.phase else { panic!("not over: {:?}", g.phase) };
        let best = g.players.iter().map(|p| p.chips).max().unwrap();
        assert_eq!(winner.map(|w| g.players[w].chips), Some(best));
    }

    /// One action by whoever is due (bots on every seat).
    fn step(g: &mut BjGame, rng: &mut Rng) {
        match g.phase {
            Phase::RoundOver => g.apply(0, BjAction::NextRound).unwrap(),
            Phase::DealerTurn => assert!(g.dealer_step()),
            _ => {
                let seat = (0..g.player_count())
                    .find(|s| g.is_waiting_for(*s))
                    .expect("someone must act");
                let action = bot_action(g, seat, rng).expect("bot has a move");
                g.apply(seat, action).unwrap_or_else(|e| panic!("{action:?} by {seat}: {e}"));
            }
        }
    }

    #[test]
    fn bot_games_keep_every_card_and_chip_sane() {
        for seed in 0..40u64 {
            let n = 1 + (seed as usize % MAX_PLAYERS);
            let mut g = BjGame::new(names(n), seed, 300, 0);
            let mut rng = Rng::new(seed ^ 0xABCD);
            let mut steps = 0;
            while !matches!(g.phase, Phase::GameOver { .. }) && steps < 20_000 {
                step(&mut g, &mut rng);
                steps += 1;
                assert_eq!(g.total_cards(), SHOE_SIZE, "seed {seed}");
                for p in &g.players {
                    assert!(p.hands.len() <= MAX_HANDS);
                    for h in &p.hands {
                        if h.outcome.is_none() {
                            assert!(h.value().total <= 21 || h.done);
                        }
                    }
                }
                if g.phase == Phase::RoundOver {
                    for p in &g.players {
                        assert!(p.hands.iter().all(|h| h.outcome.is_some()));
                    }
                }
            }
            // Every round must make progress; long games just stop here.
            assert!(g.round >= 1);
        }
    }

    #[test]
    fn broke_players_sit_out() {
        let mut g = BjGame::new(names(2), 3, 10, 0);
        let mut rng = Rng::new(1);
        while !matches!(g.phase, Phase::GameOver { .. }) {
            if g.phase == Phase::Betting {
                for s in 0..2 {
                    if g.players[s].out {
                        assert!(!g.needs_bet(s));
                        assert_eq!(g.apply(s, BjAction::Bet(10)), Err(BjError::OutOfGame));
                    }
                }
            }
            step(&mut g, &mut rng);
        }
        assert!(g.players.iter().all(|p| p.chips < MIN_BET));
        assert_eq!(g.phase, Phase::GameOver { winner: None });
    }

    #[test]
    fn state_round_trips_through_json() {
        let g = BjGame::new(names(3), 42, 1000, 10);
        let json = serde_json::to_string(&g.view_for(Some(1))).unwrap();
        let back: BjGame = serde_json::from_str(&json).unwrap();
        assert_eq!(back.players.len(), 3);
        assert_eq!(back.phase, Phase::Betting);
    }
}
