//! Computer opponent. [`bot_action`] returns the next action a bot wants to
//! take (one at a time), or `None` when it has nothing to do right now.

use crate::board::{mortgage_value, tile, tiles_in_group, TileIndex, BOARD_SIZE};
use crate::game::{Action, GameState, Phase, Player, PlayerId, TradeBundle, JAIL_FINE, MIN_BID_INCREMENT};

/// Cash the bot tries to keep for rent.
fn reserve(state: &GameState) -> i32 {
    // Be thriftier once the board is developed.
    let houses: i32 = state.buildings.iter().map(|&b| b as i32).sum();
    150 + houses * 15
}

/// How badly `player` wants tile `t` (1.0 = list price).
fn desire(state: &GameState, player: &Player, t: TileIndex) -> f32 {
    let tl = tile(t);
    let Some(group) = tl.group() else {
        // Stations and utilities: more valuable when we already have some.
        let same_kind = player
            .owned
            .iter()
            .filter(|o| std::mem::discriminant(&tile(**o).kind) == std::mem::discriminant(&tl.kind))
            .count();
        return 1.0 + same_kind as f32 * 0.25;
    };
    let group_tiles: Vec<TileIndex> = tiles_in_group(group).collect();
    let mine = group_tiles.iter().filter(|g| player.owns(**g)).count();
    let others_own = group_tiles
        .iter()
        .filter(|g| state.owner_of(**g).is_some_and(|o| o.id != player.id))
        .count();
    if mine + 1 == group_tiles.len() {
        1.8 // completes a monopoly
    } else if others_own > 0 && mine == 0 {
        0.9
    } else {
        1.0 + mine as f32 * 0.2
    }
}

fn bundle_value(state: &GameState, receiver: &Player, b: &TradeBundle) -> f32 {
    let tiles: f32 = b
        .tiles
        .iter()
        .map(|t| tile(*t).price().unwrap_or(0) as f32 * desire(state, receiver, *t) * if state.is_mortgaged(*t) { 0.5 } else { 1.0 })
        .sum();
    tiles + b.money as f32 + b.jail_free_cards as f32 * 40.0
}

/// Would giving away `tiles` complete a colour group for `receiver`?
fn gives_monopoly(receiver: &Player, tiles: &[TileIndex]) -> bool {
    tiles.iter().any(|t| {
        tile(*t).group().is_some_and(|g| {
            tiles_in_group(g).all(|i| receiver.owns(i) || tiles.contains(&i))
        })
    })
}

pub fn bot_action(state: &GameState, me: PlayerId) -> Option<Action> {
    if state.phase != Phase::Playing {
        return None;
    }
    let player = state.player(me)?;
    if player.bankrupt {
        return None;
    }

    // Respond to things addressed to us, even outside our turn.
    if let Some(trade) = &state.pending_trade {
        if trade.to == me {
            let from = state.player(trade.from)?;
            let gain = bundle_value(state, player, &trade.from_offer);
            let loss = bundle_value(state, from, &trade.to_offer);
            let dangerous = gives_monopoly(from, &trade.to_offer.tiles)
                && !gives_monopoly(player, &trade.from_offer.tiles);
            let cash_after = player.money - trade.to_offer.money + trade.from_offer.money;
            let ok = gain >= loss * 1.15 && !dangerous && cash_after >= 0;
            return Some(if ok { Action::TradeAccept } else { Action::TradeDecline });
        }
    }
    if let Some(offer) = &state.pending_offer {
        if offer.to == me {
            let price = tile(offer.tile).price().unwrap_or(0) as f32;
            let worth = price * desire(state, player, offer.tile);
            let ok = player.money - offer.price >= reserve(state) && (offer.price as f32) <= worth * 1.1;
            return Some(if ok { Action::OfferAccept } else { Action::OfferDecline });
        }
        return None;
    }
    if let Some(auction) = &state.pending_auction {
        if auction.turn_player() != Some(me) {
            return None;
        }
        let price = tile(auction.tile).price().unwrap_or(0) as f32;
        let max = (price * desire(state, player, auction.tile) * 0.95) as i32;
        let max = max.min(player.money - reserve(state) / 2);
        let bid = auction.current_bid + MIN_BID_INCREMENT.max(((price as i32) / 20 / 10) * 10);
        return Some(if bid <= max {
            Action::AuctionBid { player: me, amount: bid }
        } else {
            Action::AuctionPass { player: me }
        });
    }

    let current = state.current_player()?;
    if current.id != me {
        return None;
    }
    if state.pending_trade.is_some() {
        // Our own proposal is waiting for an answer.
        return None;
    }

    if let Some(debt) = state.pending_debt {
        if debt.debtor != me {
            return None;
        }
        if player.money >= debt.amount {
            return Some(Action::DebtPay);
        }
        return Some(raise_money(state, player).unwrap_or(Action::DebtDeclareBankruptcy));
    }

    if let Some(purchase) = state.pending_purchase {
        let worth = purchase.price as f32 * desire(state, player, purchase.tile);
        let can_afford = player.money >= purchase.price;
        let comfortable = player.money - purchase.price >= reserve(state);
        let eager = worth >= purchase.price as f32 * 1.5;
        return Some(if can_afford && (comfortable || eager) {
            Action::BuyCurrent
        } else {
            Action::AuctionCurrent
        });
    }

    if player.in_jail && !state.pending_end_turn {
        if player.jail_free_cards > 0 {
            return Some(Action::JailUseCard);
        }
        // Early on, getting out to buy land is worth it; later, jail is a safe haven.
        let unowned = (0..BOARD_SIZE)
            .filter(|t| tile(*t).is_purchasable() && state.owner_of(*t).is_none())
            .count();
        if unowned > 8 && player.money >= JAIL_FINE + reserve(state) {
            return Some(Action::JailPayFine);
        }
        return Some(Action::JailRoll);
    }

    if !state.pending_end_turn {
        return Some(Action::RollAndMove);
    }

    // End of turn: develop property, then pass the dice.
    if let Some(a) = develop(state, player) {
        return Some(a);
    }
    Some(Action::EndTurn)
}

/// Sell a building or mortgage something to cover a debt.
fn raise_money(state: &GameState, player: &Player) -> Option<Action> {
    // Sell buildings from the most developed street first.
    let mut sellable: Vec<TileIndex> = player
        .owned
        .iter()
        .copied()
        .filter(|t| state.can_sell_house(player, *t).is_ok())
        .collect();
    sellable.sort_by_key(|t| std::cmp::Reverse(state.building_level(*t)));
    if let Some(t) = sellable.first() {
        return Some(Action::SellHouse(*t));
    }
    // Then mortgage, cheapest and least useful first.
    let mut mortgageable: Vec<TileIndex> = player
        .owned
        .iter()
        .copied()
        .filter(|t| state.can_mortgage(player, *t).is_ok())
        .collect();
    mortgageable.sort_by_key(|t| {
        let in_monopoly = state.owns_full_group(player, *t) as i32;
        (in_monopoly, mortgage_value(*t))
    });
    mortgageable.first().map(|t| Action::Mortgage(*t))
}

fn develop(state: &GameState, player: &Player) -> Option<Action> {
    let spare = player.money - reserve(state);
    // Lift mortgages on monopolies first so they can be built on.
    for &t in &player.owned {
        if state.is_mortgaged(t) {
            if let Ok(cost) = state.can_unmortgage(player, t) {
                if cost + 100 <= spare {
                    return Some(Action::Unmortgage(t));
                }
            }
        }
    }
    // Build on the most expensive buildable street we can afford.
    let mut best: Option<(TileIndex, i32)> = None;
    for &t in &player.owned {
        if let Ok(cost) = state.can_buy_house(player, t) {
            if cost <= spare && best.is_none_or(|(_, c)| cost > c) {
                best = Some((t, cost));
            }
        }
    }
    best.map(|(t, _)| Action::BuyHouse(t))
}
