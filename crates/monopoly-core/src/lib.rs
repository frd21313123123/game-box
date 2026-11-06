//! «Монополия» (Moscow edition) rules engine.
//!
//! A Rust port of the TypeScript `@monopoly/core` package: one deterministic
//! reducer ([`GameState::apply`]) runs both locally and on the network host.

pub mod board;
pub mod bot;
pub mod cards;
pub mod game;
pub mod tokens;

pub use board::*;
pub use bot::bot_action;
pub use cards::{Deck, CHANCE_CARDS, CHEST_CARDS};
pub use game::*;
pub use tokens::*;

#[cfg(test)]
mod tests {
    use super::*;

    fn lobby(n: usize) -> GameState {
        let mut s = GameState::new(42);
        for i in 0..n {
            assert!(s.apply(&Action::AddPlayer { name: format!("Игрок {i}"), token: i, color: None }));
        }
        s
    }

    fn started(n: usize) -> GameState {
        let mut s = lobby(n);
        assert!(s.apply(&Action::StartGame));
        s
    }

    #[test]
    fn lobby_rules() {
        let mut s = lobby(2);
        assert_eq!(s.players[0].money, STARTING_MONEY);
        assert!(!s.apply(&Action::AddPlayer { name: "Дубль".into(), token: 0, color: None }), "token taken");
        assert!(!s.apply(&Action::AddPlayer { name: "  ".into(), token: 5, color: None }), "empty name");
        assert!(!s.apply(&Action::AddPlayer { name: "X".into(), token: 99, color: None }), "unknown token");
        assert!(!s.apply(&Action::AddPlayer { name: "X".into(), token: 5, color: Some(42) }), "bad colour");
        let id = s.players[1].id;
        assert!(s.apply(&Action::RemovePlayer { id }));
        assert_eq!(s.players.len(), 1);
        assert!(!s.apply(&Action::StartGame), "needs two players");
        let mut full = lobby(8);
        assert!(!full.apply(&Action::AddPlayer { name: "9".into(), token: 0, color: None }));
        assert!(full.apply(&Action::StartGame));
        assert_eq!(full.phase, Phase::Playing);
        assert_eq!(full.chance_deck.len(), 16);
    }

    #[test]
    fn roll_moves_and_blocks_second_roll() {
        let mut s = started(2);
        assert!(s.apply(&Action::RollAndMove));
        let roll = s.last_roll.unwrap();
        // The roll may have sent the player elsewhere via a card; the first
        // waypoint is always the dice target.
        assert_eq!(s.last_move.as_ref().unwrap().path[0], roll.sum() as usize);
        assert_eq!(s.roll_seq, 1);
        if !roll.is_double() {
            assert!(s.pending_end_turn);
            assert!(!s.apply(&Action::RollAndMove));
        }
    }

    #[test]
    fn end_turn_requires_resolution() {
        let mut s = started(2);
        assert!(!s.apply(&Action::EndTurn), "must roll first");
        s.pending_end_turn = true;
        s.pending_purchase = Some(PendingPurchase { tile: 1, price: 60 });
        assert!(!s.apply(&Action::EndTurn));
        assert!(s.apply(&Action::DeclinePurchase));
        assert!(s.apply(&Action::EndTurn));
        assert_eq!(s.current, 1);
    }

    #[test]
    fn end_turn_skips_bankrupt_players() {
        let mut s = started(3);
        s.players[1].bankrupt = true;
        s.pending_end_turn = true;
        assert!(s.apply(&Action::EndTurn));
        assert_eq!(s.current, 2);
    }

    #[test]
    fn buying_property() {
        let mut s = started(2);
        s.pending_purchase = Some(PendingPurchase { tile: 1, price: 60 });
        assert!(s.apply(&Action::BuyCurrent));
        assert_eq!(s.players[0].money, 1440);
        assert!(s.players[0].owns(1));
        s.pending_purchase = Some(PendingPurchase { tile: 39, price: 400 });
        s.players[0].money = 100;
        assert!(!s.apply(&Action::BuyCurrent), "insufficient funds");
    }

    #[test]
    fn rent_rules() {
        let mut s = started(2);
        s.players[1].owned = vec![1];
        assert_eq!(s.compute_rent(1, 7), Some((s.players[1].id, 2)));
        s.players[1].owned = vec![1, 3];
        assert_eq!(s.compute_rent(1, 7).unwrap().1, 4, "monopoly doubles base rent");
        s.buildings[1] = 5;
        assert_eq!(s.compute_rent(1, 7).unwrap().1, 250);
        s.players[1].owned = vec![5, 15];
        assert_eq!(s.compute_rent(5, 7).unwrap().1, 50);
        s.players[1].owned = vec![12];
        assert_eq!(s.compute_rent(12, 7).unwrap().1, 28);
        s.players[1].owned = vec![12, 28];
        assert_eq!(s.compute_rent(12, 7).unwrap().1, 70);
        s.mortgaged[12] = true;
        assert_eq!(s.compute_rent(12, 7), None);
    }

    #[test]
    fn landing_on_owned_tile_pays_rent() {
        let mut s = started(2);
        s.players[1].owned = vec![6];
        s.players[0].position = 0;
        s.process_landing_for_test(s.players[0].id, 6, 6);
        assert_eq!(s.players[0].money, 1494);
        assert_eq!(s.players[1].money, 1506);
    }

    #[test]
    fn houses_must_be_built_evenly() {
        let mut s = started(2);
        s.players[0].owned = vec![1, 3];
        s.pending_end_turn = true;
        assert!(s.apply(&Action::BuyHouse(1)));
        assert!(!s.apply(&Action::BuyHouse(1)), "uneven");
        assert!(s.apply(&Action::BuyHouse(3)));
        assert!(s.apply(&Action::BuyHouse(1)));
        assert_eq!(s.players[0].money, 1500 - 150);
        assert!(!s.apply(&Action::SellHouse(3)), "must sell evenly");
        assert!(s.apply(&Action::SellHouse(1)));
        assert_eq!(s.players[0].money, 1500 - 150 + 25);
        assert!(!s.apply(&Action::Mortgage(1)), "buildings in group");
    }

    #[test]
    fn mortgage_and_unmortgage() {
        let mut s = started(2);
        s.players[0].owned = vec![39];
        assert!(s.apply(&Action::Mortgage(39)));
        assert_eq!(s.players[0].money, 1700);
        assert!(s.apply(&Action::Unmortgage(39)));
        assert_eq!(s.players[0].money, 1480);
    }

    #[test]
    fn debt_then_bankruptcy_ends_game() {
        let mut s = started(2);
        s.players[0].money = 10;
        s.players[1].owned = vec![39];
        s.buildings[39] = 0;
        s.process_landing_for_test(s.players[0].id, 39, 4);
        let debt = s.pending_debt.expect("debt instead of bankruptcy");
        assert_eq!(debt.amount, 50);
        assert!(!s.apply(&Action::DebtPay));
        assert!(s.apply(&Action::DebtDeclareBankruptcy));
        assert_eq!(s.phase, Phase::Finished);
        assert_eq!(s.winner().unwrap().id, s.players[1].id);
        assert_eq!(s.players[1].money, 1510);
    }

    #[test]
    fn debt_can_be_paid_after_mortgaging() {
        let mut s = started(2);
        s.players[0].money = 10;
        s.players[0].owned = vec![37];
        s.players[1].owned = vec![39];
        s.process_landing_for_test(s.players[0].id, 39, 4);
        assert!(s.pending_debt.is_some());
        assert!(!s.apply(&Action::EndTurn));
        assert!(s.apply(&Action::Mortgage(37)));
        assert!(s.apply(&Action::DebtPay));
        assert_eq!(s.players[0].money, 10 + 175 - 50);
        assert!(s.pending_debt.is_none());
    }

    #[test]
    fn offer_purchase_flow() {
        let mut s = started(2);
        s.pending_purchase = Some(PendingPurchase { tile: 1, price: 60 });
        let to = s.players[1].id;
        assert!(!s.apply(&Action::OfferPurchase { to, price: 50 }), "below list price");
        assert!(!s.apply(&Action::OfferPurchase { to: s.players[0].id, price: 80 }), "self");
        assert!(s.apply(&Action::OfferPurchase { to, price: 100 }));
        assert!(s.pending_purchase.is_none());
        assert!(s.apply(&Action::OfferAccept));
        assert_eq!(s.players[1].money, 1400);
        assert_eq!(s.players[0].money, 1540, "only the markup goes to the seller");
        assert!(s.players[1].owns(1));

        let mut s = started(2);
        s.pending_purchase = Some(PendingPurchase { tile: 1, price: 60 });
        assert!(s.apply(&Action::OfferPurchase { to, price: 70 }));
        assert!(s.apply(&Action::OfferDecline));
        assert_eq!(s.pending_purchase, Some(PendingPurchase { tile: 1, price: 60 }));
    }

    #[test]
    fn auction_flow() {
        let mut s = started(3);
        s.pending_purchase = Some(PendingPurchase { tile: 1, price: 60 });
        assert!(s.apply(&Action::AuctionCurrent));
        let a = s.pending_auction.clone().unwrap();
        let first = a.turn_player().unwrap();
        assert_eq!(first, s.players[1].id, "bidding starts after the decliner");
        assert!(!s.apply(&Action::AuctionBid { player: first, amount: 5 }), "min increment");
        assert!(s.apply(&Action::AuctionBid { player: first, amount: 20 }));
        let p2 = s.players[2].id;
        let p0 = s.players[0].id;
        assert!(s.apply(&Action::AuctionPass { player: p2 }));
        assert!(s.apply(&Action::AuctionPass { player: p0 }));
        assert!(s.pending_auction.is_none());
        assert!(s.players[1].owns(1));
        assert_eq!(s.players[1].money, 1480);
    }

    #[test]
    fn trade_flow() {
        let mut s = started(2);
        s.players[0].owned = vec![1];
        s.players[1].owned = vec![3];
        let (a, b) = (s.players[0].id, s.players[1].id);
        let propose = Action::TradePropose {
            from: a,
            to: b,
            from_offer: TradeBundle { tiles: vec![1], money: 50, jail_free_cards: 0 },
            to_offer: TradeBundle { tiles: vec![3], money: 0, jail_free_cards: 0 },
        };
        assert!(s.can_submit(a, &propose));
        assert!(!s.can_submit(b, &propose));
        assert!(s.apply(&propose));
        assert!(!s.can_submit(a, &Action::TradeAccept));
        assert!(s.apply(&Action::TradeAccept));
        assert!(s.players[0].owns(3) && s.players[1].owns(1));
        assert_eq!(s.players[0].money, 1450);
        assert_eq!(s.players[1].money, 1550);
    }

    #[test]
    fn jail_flow() {
        let mut s = started(2);
        s.players[0].in_jail = true;
        s.players[0].position = JAIL_INDEX;
        assert!(!s.apply(&Action::RollAndMove), "jailed players use jail actions");
        assert!(s.apply(&Action::JailPayFine));
        assert_eq!(s.players[0].money, 1450);
        assert!(!s.players[0].in_jail);

        let mut s = started(2);
        s.players[0].in_jail = true;
        s.players[0].jail_free_cards = 1;
        s.chance_deck.retain(|c| *c != 8);
        assert!(s.apply(&Action::JailUseCard));
        assert_eq!(s.players[0].jail_free_cards, 0);
        assert!(s.chance_deck.contains(&8), "used card returns to its deck");
    }

    #[test]
    fn go_to_jail_tile() {
        let mut s = started(2);
        s.begin_move_for_test(s.players[0].id);
        s.process_landing_for_test(s.players[0].id, GO_TO_JAIL_INDEX, 5);
        assert!(s.players[0].in_jail);
        assert_eq!(s.players[0].position, JAIL_INDEX);
    }

    #[test]
    fn state_round_trips_through_json() {
        let mut s = started(3);
        s.apply(&Action::RollAndMove);
        let json = serde_json::to_string(&s).unwrap();
        let back: GameState = serde_json::from_str(&json).unwrap();
        assert_eq!(back, s);
    }

    /// Bots play against each other; invariants hold and most games finish.
    #[test]
    fn bots_play_full_games() {
        let mut finished = 0;
        for seed in 0..20u32 {
            let mut s = GameState::new(seed.wrapping_mul(7919) + 1);
            let n = 2 + (seed as usize % 3);
            for i in 0..n {
                s.apply(&Action::AddPlayer { name: format!("Бот {i}"), token: i, color: None });
            }
            assert!(s.apply(&Action::StartGame));
            for _ in 0..20_000 {
                if s.phase == Phase::Finished {
                    break;
                }
                // Whoever has to act: auction bidder, trade/offer target, or current player.
                let actor = s
                    .players
                    .iter()
                    .filter(|p| !p.bankrupt)
                    .map(|p| p.id)
                    .find(|id| bot_action(&s, *id).is_some())
                    .expect("someone must be able to act");
                let action = bot_action(&s, actor).unwrap();
                assert!(s.can_submit(actor, &action), "{action:?} by {actor} not authorized");
                assert!(s.apply(&action), "bot action rejected: {action:?}\n{:?}", s.pending_debt);
                for p in &s.players {
                    assert!(p.money >= 0, "negative money");
                }
                let owned: usize = s.players.iter().map(|p| p.owned.len()).sum();
                let mut all: Vec<usize> = s.players.iter().flat_map(|p| p.owned.clone()).collect();
                all.sort();
                all.dedup();
                assert_eq!(all.len(), owned, "a tile has two owners");
            }
            if s.phase == Phase::Finished {
                finished += 1;
            }
        }
        assert!(finished >= 10, "only {finished}/20 bot games finished");
    }
}
