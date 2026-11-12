//! Sidebar (turn controls, players, property management, log) and dialogs
//! (purchase, offer, auction, trade, debt, game over).

use bevy_egui::egui::{self, Align2, Color32, RichText, ScrollArea, Vec2};
use monopoly_core::{
    tile, Action, GameState, LogKind, Phase, Player, PlayerId, TileIndex, TradeBundle, JAIL_FINE,
    MAX_JAIL_TURNS, MIN_BID_INCREMENT,
};

use super::{viewer, MonoUi};
use crate::net::Session;
use crate::theme::{self, action_button, bold, rgb, semibold, stroke};

pub struct TradeDraft {
    pub partner: Option<PlayerId>,
    pub give_tiles: Vec<TileIndex>,
    pub get_tiles: Vec<TileIndex>,
    pub give_money: i32,
    pub get_money: i32,
    pub give_cards: u32,
    pub get_cards: u32,
}

pub struct OfferDraft {
    pub to: Option<PlayerId>,
    pub price: i32,
}

fn player_chip(ui: &mut egui::Ui, p: &Player, size: f32) {
    let (rect, _) = ui.allocate_exact_size(Vec2::splat(size), egui::Sense::hover());
    ui.painter().circle_filled(rect.center(), size / 2.0, rgb(p.rgb()));
    ui.painter().circle_stroke(rect.center(), size / 2.0, stroke(1.5, Color32::WHITE));
    ui.painter().text(rect.center(), Align2::CENTER_CENTER, p.symbol(), egui::FontId::proportional(size * 0.55), Color32::WHITE);
}

/// Little green houses, or a red hotel, as on the board.
fn building_icons(ui: &mut egui::Ui, level: u8) {
    if level == 0 {
        return;
    }
    let hotel = level >= 5;
    let count = if hotel { 1 } else { level as usize };
    let (w, h) = if hotel { (22.0, 13.0) } else { (10.0, 12.0) };
    let (rect, _) = ui.allocate_exact_size(Vec2::new(count as f32 * (w + 3.0), 16.0), egui::Sense::hover());
    let color = if hotel { Color32::from_rgb(0xd8, 0x22, 0x22) } else { Color32::from_rgb(0x2f, 0xb8, 0x5a) };
    for k in 0..count {
        let x = rect.left() + k as f32 * (w + 3.0);
        let base = rect.center().y + h / 2.0;
        let roof = h * 0.42;
        let points = vec![
            egui::pos2(x, base),
            egui::pos2(x + w, base),
            egui::pos2(x + w, base - h + roof),
            egui::pos2(x + w / 2.0, base - h),
            egui::pos2(x, base - h + roof),
        ];
        ui.painter().add(egui::Shape::convex_polygon(points, color, stroke(1.0, theme::darken(color, 0.6))));
    }
}

fn group_swatch(ui: &mut egui::Ui, t: TileIndex) {
    let (rect, _) = ui.allocate_exact_size(Vec2::new(10.0, 18.0), egui::Sense::hover());
    let color = tile(t).group().map(|g| rgb(g.rgb())).unwrap_or(Color32::from_rgb(140, 140, 150));
    ui.painter().rect_filled(rect, 3, color);
}

/// Right-hand panel. Returns true when "leave" was pressed.
pub fn sidebar(ctx: &egui::Context, state: &GameState, session: &mut Session, ui: &mut MonoUi) -> bool {
    let mut leave = false;
    let me = viewer(state, session);
    let locals = session.local_monopoly_ids();
    let current = state.current_player().cloned();
    let my_turn = current.as_ref().is_some_and(|c| Some(c.id) == me);
    egui::SidePanel::right("mono-side")
        .exact_width(390.0)
        .resizable(false)
        .frame(egui::Frame::new().fill(theme::PANEL).inner_margin(egui::Margin::same(16)))
        .show(ctx, |side| {
            side.horizontal(|ui_| {
                ui_.label(RichText::new("Монополия").font(bold(22.0)));
                ui_.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui_| {
                    if ui_.button("✖ Выйти").clicked() {
                        leave = true;
                    }
                    if session.is_host && ui_.button("↩ В лобби").clicked() {
                        session.back_to_lobby();
                    }
                });
            });
            side.add_space(4.0);

            if state.phase == Phase::Finished {
                if let Some(w) = state.winner() {
                    egui::Frame::new().fill(theme::with_alpha(theme::GOLD, 40)).corner_radius(12).inner_margin(egui::Margin::same(12)).show(side, |ui_| {
                        ui_.set_width(ui_.available_width());
                        ui_.label(RichText::new("Игра окончена").font(bold(20.0)));
                        ui_.label(RichText::new(format!("🏆 Победил {} {}", w.symbol(), w.name)).font(semibold(17.0)).color(theme::GOLD));
                    });
                }
            } else if let Some(cur) = &current {
                // ---- turn header & main actions -----------------------------
                egui::Frame::new().fill(theme::PANEL_HI).corner_radius(12).inner_margin(egui::Margin::same(12)).show(side, |ui_| {
                    // Span the panel whatever the content (it used to shrink to
                    // the text when there was no button to show).
                    ui_.set_width(ui_.available_width());
                    ui_.horizontal(|ui_| {
                        player_chip(ui_, cur, 30.0);
                        ui_.vertical(|ui_| {
                            ui_.spacing_mut().item_spacing.y = 0.0;
                            ui_.label(RichText::new(format!("Ход {} · {}", state.turn, cur.name)).font(semibold(16.0)));
                            let status = if cur.in_jail { "в тюрьме" } else { tile(cur.position).name };
                            ui_.label(RichText::new(status).color(theme::TEXT_DIM));
                        });
                    });
                    ui_.add_space(6.0);
                    let busy = ui.busy();
                    let blocked = state.pending_purchase.is_some()
                        || state.pending_offer.is_some()
                        || state.pending_auction.is_some()
                        || state.pending_debt.is_some();
                    if !my_turn {
                        let who = if locals.contains(&cur.id) { cur.name.clone() } else { format!("{}…", cur.name) };
                        ui_.label(RichText::new(format!("Ходит {who}")).color(theme::TEXT_DIM));
                    } else if cur.in_jail && !state.pending_end_turn && !blocked {
                        ui_.label(RichText::new(format!("Вы в тюрьме · попытка {}/{}", cur.jail_turns + 1, MAX_JAIL_TURNS)).color(theme::WARN));
                        if action_button(ui_, "Бросить кубики (нужен дубль)", theme::ACCENT, !busy, 350.0).clicked() {
                            session.submit_monopoly(Action::JailRoll);
                        }
                        if action_button(ui_, &format!("Заплатить ₽{JAIL_FINE}"), theme::PANEL, cur.money >= JAIL_FINE && !busy, 350.0).clicked() {
                            session.submit_monopoly(Action::JailPayFine);
                        }
                        if cur.jail_free_cards > 0 && action_button(ui_, "Использовать карту освобождения", theme::GOOD, !busy, 350.0).clicked() {
                            session.submit_monopoly(Action::JailUseCard);
                        }
                    } else if !state.pending_end_turn && !blocked {
                        let label = if state.doubles_this_turn > 0 { "Дубль! Бросить ещё раз" } else { "🎲 Бросить кубики" };
                        if action_button(ui_, label, theme::ACCENT, !busy, 350.0).clicked() {
                            session.submit_monopoly(Action::RollAndMove);
                        }
                    } else if state.pending_end_turn && !blocked && state.pending_trade.is_none() {
                        if action_button(ui_, "Завершить ход", theme::GOOD, !busy, 350.0).clicked() {
                            session.submit_monopoly(Action::EndTurn);
                        }
                    } else if state.pending_trade.is_some() {
                        ui_.label(RichText::new("Ожидается ответ на предложение обмена").color(theme::TEXT_DIM));
                    }
                });

                // ---- debt ------------------------------------------------------
                if let Some(debt) = state.pending_debt {
                    if locals.contains(&debt.debtor) {
                        let debtor = state.player(debt.debtor).cloned();
                        egui::Frame::new().fill(theme::with_alpha(theme::BAD, 40)).stroke(stroke(1.0, theme::BAD)).corner_radius(12).inner_margin(egui::Margin::same(12)).show(side, |ui_| {
                            ui_.set_width(ui_.available_width());
                            ui_.label(RichText::new("Не хватает денег!").font(bold(18.0)).color(theme::BAD));
                            let to = debt.creditor.and_then(|c| state.player(c)).map(|p| p.name.clone()).unwrap_or_else(|| "банку".into());
                            ui_.label(format!("Вы должны ₽{} ({to}).", debt.amount));
                            ui_.label(RichText::new("Продайте дома или заложите имущество ниже, чтобы собрать сумму.").color(theme::TEXT_DIM));
                            if let Some(d) = debtor {
                                ui_.label(format!("Сейчас у вас: ₽{}", d.money));
                                let can = d.money >= debt.amount;
                                if action_button(ui_, &format!("Оплатить ₽{}", debt.amount), theme::GOOD, can, 350.0).clicked() {
                                    session.submit_monopoly(Action::DebtPay);
                                }
                                if !can {
                                    ui_.label(RichText::new(format!("Нужно ещё ₽{}", debt.amount - d.money)).color(theme::WARN));
                                }
                                if action_button(ui_, "Объявить банкротство", theme::BAD, true, 350.0).clicked() {
                                    session.submit_monopoly(Action::DebtDeclareBankruptcy);
                                }
                            }
                        });
                    }
                }
            }

            side.add_space(6.0);
            // ---- players ---------------------------------------------------------
            side.label(RichText::new("Игроки").font(semibold(15.0)).color(theme::TEXT_DIM));
            for p in &state.players {
                let is_current = current.as_ref().is_some_and(|c| c.id == p.id);
                egui::Frame::new()
                    .fill(if is_current { theme::PANEL_HI } else { Color32::TRANSPARENT })
                    .stroke(if is_current { stroke(1.0, theme::with_alpha(theme::GOLD, 120)) } else { stroke(0.0, Color32::TRANSPARENT) })
                    .corner_radius(10)
                    .inner_margin(egui::Margin::symmetric(8, 4))
                    .show(side, |ui_| {
                        ui_.horizontal(|ui_| {
                            player_chip(ui_, p, 24.0);
                            let mut name = RichText::new(&p.name).font(semibold(14.5));
                            if p.bankrupt {
                                name = name.strikethrough().color(theme::TEXT_DIM);
                            }
                            ui_.label(name);
                            if locals.contains(&p.id) {
                                ui_.label(RichText::new("вы").font(theme::regular(11.0)).color(theme::ACCENT));
                            }
                            if p.in_jail {
                                ui_.label("🔒");
                            }
                            if p.jail_free_cards > 0 {
                                ui_.label(RichText::new(format!("карт: {}", p.jail_free_cards)).font(theme::regular(12.0)));
                            }
                            ui_.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui_| {
                                if p.bankrupt {
                                    ui_.label(RichText::new("БАНКРОТ").color(theme::BAD));
                                } else {
                                    ui_.label(RichText::new(format!("₽{}", p.money)).font(semibold(15.0)).color(theme::GOOD));
                                    ui_.label(RichText::new(format!("{} уч.", p.owned.len())).color(theme::TEXT_DIM));
                                }
                            });
                        });
                    });
            }

            let can_trade = state.phase == Phase::Playing
                && state.pending_purchase.is_none()
                && state.pending_auction.is_none()
                && state.pending_offer.is_none()
                && state.pending_trade.is_none()
                && state.pending_debt.is_none()
                && me.and_then(|id| state.player(id)).is_some_and(|p| !p.bankrupt);
            side.add_space(4.0);
            if action_button(side, "Торговля…", theme::PANEL_HI, can_trade, 358.0).clicked() {
                ui.trade = Some(TradeDraft {
                    partner: None,
                    give_tiles: Vec::new(),
                    get_tiles: Vec::new(),
                    give_money: 0,
                    get_money: 0,
                    give_cards: 0,
                    get_cards: 0,
                });
            }

            // ---- my property ---------------------------------------------------------
            side.add_space(6.0);
            if let Some(me_p) = me.and_then(|id| state.player(id)).cloned() {
                if !me_p.bankrupt {
                    egui::CollapsingHeader::new(RichText::new(format!("Имущество: {}", me_p.name)).font(semibold(15.0)))
                        .default_open(true)
                        .show(side, |ui_| {
                            properties(ui_, state, session, &me_p, my_turn);
                        });
                }
            }

            // ---- log ---------------------------------------------------------------
            side.add_space(6.0);
            side.label(RichText::new("События").font(semibold(15.0)).color(theme::TEXT_DIM));
            let log = ScrollArea::vertical().stick_to_bottom(true).auto_shrink([false, false]).show(side, |ui_| {
                for e in &state.log {
                    let color = match e.kind {
                        LogKind::Rent | LogKind::Tax | LogKind::Bankrupt | LogKind::GoToJail | LogKind::ThirdDoubleJail => theme::WARN,
                        LogKind::Bought | LogKind::AuctionWon | LogKind::OfferAccepted | LogKind::TradeAccepted => theme::GOOD,
                        LogKind::GameWon => theme::GOLD,
                        LogKind::DrewCard => Color32::from_rgb(140, 180, 255),
                        _ => theme::TEXT_DIM,
                    };
                    ui_.label(RichText::new(&e.text).font(theme::regular(13.0)).color(color));
                }
            });
            // Older entries fade out under the heading instead of being cut
            // in half at the top edge of the list.
            let top = log.inner_rect;
            side.painter().rect_filled(
                // egui lets a scroll area paint a few pixels past its top edge.
                egui::Rect::from_min_size(egui::pos2(top.left(), top.top() - 5.0), Vec2::new(top.width(), 12.0)),
                0,
                theme::PANEL,
            );
            let steps = 12;
            for k in 0..steps {
                let band = egui::Rect::from_min_size(
                    egui::pos2(top.left(), top.top() + 7.0 + k as f32 * 2.0),
                    Vec2::new(top.width(), 2.0),
                );
                let alpha = (255.0 * (1.0 - k as f32 / steps as f32)) as u8;
                side.painter().rect_filled(band, 0, theme::with_alpha(theme::PANEL, alpha));
            }
        });
    leave
}

fn properties(ui: &mut egui::Ui, state: &GameState, session: &mut Session, me: &Player, my_turn: bool) {
    if me.owned.is_empty() {
        ui.label(RichText::new("Пока ничего нет").color(theme::TEXT_DIM));
        return;
    }
    let mut owned = me.owned.clone();
    owned.sort();
    if !my_turn {
        ui.label(RichText::new("Управлять имуществом можно в свой ход").font(theme::regular(12.0)).color(theme::TEXT_DIM));
    }
    ScrollArea::vertical().id_salt("props").max_height(210.0).show(ui, |ui| {
        for t in owned {
            ui.horizontal(|ui| {
                group_swatch(ui, t);
                let tl = tile(t);
                let mut name = RichText::new(tl.name).font(theme::regular(13.5));
                if state.is_mortgaged(t) {
                    name = name.color(theme::TEXT_DIM).italics();
                }
                ui.label(name);
                building_icons(ui, state.building_level(t));
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.spacing_mut().button_padding = Vec2::new(6.0, 2.0);
                    if state.is_mortgaged(t) {
                        let ok = my_turn && state.can_unmortgage(me, t).is_ok() && state.pending_debt.is_none();
                        if ui.add_enabled(ok, egui::Button::new("Выкупить")).on_hover_text(format!("−₽{}", monopoly_core::unmortgage_cost(t))).clicked() {
                            session.submit_monopoly(Action::Unmortgage(t));
                        }
                    } else {
                        if let Ok(v) = state.can_mortgage(me, t) {
                            if ui.add_enabled(my_turn, egui::Button::new("Залог")).on_hover_text(format!("+₽{v}")).clicked() {
                                session.submit_monopoly(Action::Mortgage(t));
                            }
                        }
                        if let Ok(r) = state.can_sell_house(me, t) {
                            if ui.add_enabled(my_turn, egui::Button::new("−")).on_hover_text(format!("Продать постройку (+₽{r})")).clicked() {
                                session.submit_monopoly(Action::SellHouse(t));
                            }
                        }
                        if let Ok(c) = state.can_buy_house(me, t) {
                            let ok = my_turn && state.pending_debt.is_none();
                            if ui.add_enabled(ok, egui::Button::new("+ дом")).on_hover_text(format!("Построить (₽{c})")).clicked() {
                                session.submit_monopoly(Action::BuyHouse(t));
                            }
                        }
                    }
                });
            });
        }
    });
}

fn window(ctx: &egui::Context, title: &str, width: f32, add: impl FnOnce(&mut egui::Ui)) {
    // Centred over the board, i.e. the part of the screen left of the panel.
    let offset = ctx.available_rect().center() - ctx.screen_rect().center();
    egui::Window::new(title)
        .collapsible(false)
        .resizable(false)
        .anchor(Align2::CENTER_CENTER, offset)
        .frame(theme::card_frame().inner_margin(egui::Margin::same(18)))
        .title_bar(false)
        .show(ctx, |ui| {
            ui.set_width(width);
            ui.label(RichText::new(title).font(bold(22.0)));
            ui.add_space(4.0);
            add(ui);
        });
}

fn tile_heading(ui: &mut egui::Ui, t: TileIndex) {
    let tl = tile(t);
    let color = tl.group().map(|g| rgb(g.rgb())).unwrap_or(theme::PANEL_HI);
    let (rect, _) = ui.allocate_exact_size(Vec2::new(ui.available_width(), 40.0), egui::Sense::hover());
    ui.painter().rect_filled(rect, 8, color);
    ui.painter().text(
        rect.center(),
        Align2::CENTER_CENTER,
        tl.name.to_uppercase(),
        bold(17.0),
        if theme::is_light(color) { Color32::from_rgb(25, 25, 30) } else { Color32::WHITE },
    );
}

pub fn dialogs(ctx: &egui::Context, state: &GameState, session: &mut Session, ui: &mut MonoUi) {
    if state.phase != Phase::Playing || ui.busy() {
        return;
    }
    let locals = session.local_monopoly_ids();
    let current = state.current_player().cloned();
    let cur_local = current.as_ref().is_some_and(|c| locals.contains(&c.id));

    // ---- purchase ----------------------------------------------------------------
    if let (Some(purchase), Some(cur)) = (state.pending_purchase, &current) {
        if cur_local && ui.offer.is_none() {
            window(ctx, "Купить участок?", 380.0, |w| {
                tile_heading(w, purchase.tile);
                w.label(format!("Цена: ₽{} · у вас ₽{}", purchase.price, cur.money));
                w.add_space(6.0);
                let afford = cur.money >= purchase.price;
                if action_button(w, &format!("Купить за ₽{}", purchase.price), theme::GOOD, afford, 380.0).clicked() {
                    session.submit_monopoly(Action::BuyCurrent);
                }
                w.horizontal(|w| {
                    if action_button(w, "На аукцион", theme::PANEL_HI, true, 186.0).clicked() {
                        session.submit_monopoly(Action::AuctionCurrent);
                    }
                    let others = state.players.iter().any(|p| !p.bankrupt && p.id != cur.id);
                    if action_button(w, "Предложить другому", theme::PANEL_HI, others, 186.0).clicked() {
                        ui.offer = Some(OfferDraft { to: None, price: purchase.price });
                    }
                });
                if w.button("Отказаться").clicked() {
                    session.submit_monopoly(Action::DeclinePurchase);
                }
            });
        }
    }

    // ---- offer composer ---------------------------------------------------------------
    if let (Some(purchase), Some(cur), Some(draft)) = (state.pending_purchase, &current, ui.offer.as_mut()) {
        let mut close = false;
        window(ctx, "Предложить покупку", 380.0, |w| {
            tile_heading(w, purchase.tile);
            w.label("Кому предложить:");
            for p in state.players.iter().filter(|p| !p.bankrupt && p.id != cur.id) {
                w.radio_value(&mut draft.to, Some(p.id), format!("{} {} (₽{})", p.symbol(), p.name, p.money));
            }
            w.horizontal(|w| {
                w.label("Цена ₽");
                w.add(egui::DragValue::new(&mut draft.price).range(purchase.price..=50_000).speed(5));
            });
            w.label(RichText::new(format!("Не ниже цены участка: ₽{}. Наценка достанется вам.", purchase.price)).color(theme::TEXT_DIM));
            w.horizontal(|w| {
                if action_button(w, &format!("Предложить за ₽{}", draft.price), theme::GOOD, draft.to.is_some(), 230.0).clicked() {
                    if let Some(to) = draft.to {
                        session.submit_monopoly(Action::OfferPurchase { to, price: draft.price });
                    }
                    close = true;
                }
                if w.button("Отмена").clicked() {
                    close = true;
                }
            });
        });
        if close {
            ui.offer = None;
        }
    } else if state.pending_purchase.is_none() {
        ui.offer = None;
    }

    // ---- offer review --------------------------------------------------------------------
    if let Some(offer) = state.pending_offer {
        if locals.contains(&offer.to) {
            let from = state.player(offer.from).map(|p| p.name.clone()).unwrap_or_default();
            let buyer = state.player(offer.to).cloned();
            window(ctx, "Вам предлагают купить", 380.0, |w| {
                tile_heading(w, offer.tile);
                w.label(format!("{from} предлагает купить «{}» за ₽{}", tile(offer.tile).name, offer.price));
                w.label(RichText::new(format!("Обычная цена: ₽{}", offer.original_price)).color(theme::TEXT_DIM));
                let afford = buyer.as_ref().is_some_and(|b| b.money >= offer.price);
                w.horizontal(|w| {
                    if action_button(w, &format!("Купить за ₽{}", offer.price), theme::GOOD, afford, 200.0).clicked() {
                        session.submit_monopoly(Action::OfferAccept);
                    }
                    if action_button(w, "Отказаться", theme::PANEL_HI, true, 160.0).clicked() {
                        session.submit_monopoly(Action::OfferDecline);
                    }
                });
            });
        } else if locals.contains(&offer.from) {
            let to = state.player(offer.to).map(|p| p.name.clone()).unwrap_or_default();
            window(ctx, "Предложение отправлено", 340.0, |w| {
                w.label(format!("Ждём ответа от {to}…"));
            });
        }
    }

    // ---- auction ------------------------------------------------------------------------------
    if let Some(a) = &state.pending_auction {
        let bidder = a.turn_player().and_then(|id| state.player(id)).cloned();
        let high = a.high_bidder.and_then(|id| state.player(id)).map(|p| p.name.clone());
        window(ctx, "Аукцион", 400.0, |w| {
            tile_heading(w, a.tile);
            w.label(format!(
                "Текущая ставка: ₽{}{}",
                a.current_bid,
                high.map(|h| format!(" ({h})")).unwrap_or_default()
            ));
            let names: Vec<String> = a.active.iter().filter_map(|id| state.player(*id)).map(|p| p.name.clone()).collect();
            w.label(RichText::new(format!("Участвуют: {}", names.join(", "))).color(theme::TEXT_DIM));
            if let Some(b) = bidder {
                if locals.contains(&b.id) {
                    let min = a.current_bid + MIN_BID_INCREMENT;
                    if ui.bid < min {
                        ui.bid = min;
                    }
                    w.label(RichText::new(format!("Ставка за {}", b.name)).font(semibold(16.0)).color(theme::GOLD));
                    w.horizontal(|w| {
                        for inc in [10, 50, 100] {
                            if w.button(format!("+{inc}")).clicked() {
                                ui.bid = (ui.bid.max(a.current_bid) + inc).max(min);
                            }
                        }
                        w.add(egui::DragValue::new(&mut ui.bid).range(min..=b.money.max(min)).speed(5).prefix("₽"));
                    });
                    w.horizontal(|w| {
                        let ok = ui.bid >= min && ui.bid <= b.money;
                        if action_button(w, &format!("Поставить ₽{}", ui.bid), theme::GOOD, ok, 200.0).clicked() {
                            session.submit_monopoly(Action::AuctionBid { player: b.id, amount: ui.bid });
                        }
                        if action_button(w, "Пас", theme::PANEL_HI, true, 160.0).clicked() {
                            session.submit_monopoly(Action::AuctionPass { player: b.id });
                        }
                    });
                } else {
                    w.label(format!("Ставку делает {}…", b.name));
                }
            }
        });
    }

    // ---- trade review ------------------------------------------------------------------------
    if let Some(trade) = &state.pending_trade {
        let from = state.player(trade.from).cloned();
        let to = state.player(trade.to).cloned();
        if let (Some(from), Some(to)) = (from, to) {
            if locals.contains(&trade.to) {
                window(ctx, "Предложение обмена", 460.0, |w| {
                    w.label(format!("{} предлагает обмен игроку {}", from.name, to.name));
                    w.columns(2, |c| {
                        c[0].label(RichText::new("Вы получаете").font(semibold(15.0)).color(theme::GOOD));
                        bundle_view(&mut c[0], &trade.from_offer);
                        c[1].label(RichText::new("Вы отдаёте").font(semibold(15.0)).color(theme::WARN));
                        bundle_view(&mut c[1], &trade.to_offer);
                    });
                    w.horizontal(|w| {
                        if action_button(w, "Принять", theme::GOOD, true, 200.0).clicked() {
                            session.submit_monopoly(Action::TradeAccept);
                        }
                        if action_button(w, "Отказаться", theme::BAD, true, 200.0).clicked() {
                            session.submit_monopoly(Action::TradeDecline);
                        }
                    });
                });
            } else if locals.contains(&trade.from) || cur_local {
                window(ctx, "Обмен", 360.0, |w| {
                    w.label(format!("Ждём ответа от {}…", to.name));
                    if w.button("Отменить предложение").clicked() {
                        session.submit_monopoly(Action::TradeCancel);
                    }
                });
            }
        }
    }

    // ---- trade composer ----------------------------------------------------------------------------
    let me = viewer(state, session);
    if let (Some(draft), Some(me)) = (ui.trade.as_mut(), me.and_then(|id| state.player(id)).cloned()) {
        let mut close = false;
        window(ctx, "Торговля", 560.0, |w| {
            w.horizontal(|w| {
                w.label("С кем:");
                for p in state.players.iter().filter(|p| !p.bankrupt && p.id != me.id) {
                    if w.selectable_label(draft.partner == Some(p.id), format!("{} {}", p.symbol(), p.name)).clicked() {
                        draft.partner = Some(p.id);
                        draft.get_tiles.clear();
                        draft.get_money = 0;
                        draft.get_cards = 0;
                    }
                }
            });
            let partner = draft.partner.and_then(|id| state.player(id)).cloned();
            w.columns(2, |c| {
                c[0].label(RichText::new("Вы отдаёте").font(semibold(15.0)).color(theme::WARN));
                bundle_editor(&mut c[0], state, &me, &mut draft.give_tiles, &mut draft.give_money, &mut draft.give_cards, "give");
                c[1].label(RichText::new("Вы получаете").font(semibold(15.0)).color(theme::GOOD));
                match &partner {
                    Some(p) => bundle_editor(&mut c[1], state, p, &mut draft.get_tiles, &mut draft.get_money, &mut draft.get_cards, "get"),
                    None => {
                        c[1].label(RichText::new("Выберите игрока").color(theme::TEXT_DIM));
                    }
                }
            });
            let from_offer = TradeBundle { tiles: draft.give_tiles.clone(), money: draft.give_money, jail_free_cards: draft.give_cards };
            let to_offer = TradeBundle { tiles: draft.get_tiles.clone(), money: draft.get_money, jail_free_cards: draft.get_cards };
            let valid = partner.as_ref().is_some_and(|p| {
                state.trade_bundle_valid(&me, &from_offer).is_ok()
                    && state.trade_bundle_valid(p, &to_offer).is_ok()
                    && !(from_offer.is_empty() && to_offer.is_empty())
            });
            w.horizontal(|w| {
                if action_button(w, "Предложить", theme::GOOD, valid, 200.0).clicked() {
                    if let Some(p) = &partner {
                        session.submit_monopoly(Action::TradePropose { from: me.id, to: p.id, from_offer, to_offer });
                    }
                    close = true;
                }
                if w.button("Отмена").clicked() {
                    close = true;
                }
            });
            w.label(RichText::new("Участки с постройками в группе обменять нельзя — сначала продайте дома.").font(theme::regular(12.0)).color(theme::TEXT_DIM));
        });
        if close {
            ui.trade = None;
        }
    }
}

fn bundle_view(ui: &mut egui::Ui, b: &TradeBundle) {
    if b.is_empty() {
        ui.label(RichText::new("ничего").color(theme::TEXT_DIM));
    }
    for t in &b.tiles {
        ui.horizontal(|ui| {
            group_swatch(ui, *t);
            ui.label(tile(*t).name);
        });
    }
    if b.money > 0 {
        ui.label(format!("₽{}", b.money));
    }
    if b.jail_free_cards > 0 {
        ui.label(format!("карт освобождения: {}", b.jail_free_cards));
    }
}

fn bundle_editor(
    ui: &mut egui::Ui,
    state: &GameState,
    owner: &Player,
    tiles: &mut Vec<TileIndex>,
    money: &mut i32,
    cards: &mut u32,
    salt: &str,
) {
    ScrollArea::vertical().id_salt(("bundle", salt)).max_height(220.0).show(ui, |ui| {
        let mut owned = owner.owned.clone();
        owned.sort();
        if owned.is_empty() {
            ui.label(RichText::new("нет участков").color(theme::TEXT_DIM));
        }
        for t in owned {
            let blocked = tile(t).group().is_some_and(|g| monopoly_core::tiles_in_group(g).any(|i| state.building_level(i) > 0));
            ui.horizontal(|ui| {
                group_swatch(ui, t);
                let mut on = tiles.contains(&t);
                let label = if state.is_mortgaged(t) { format!("{} (залог)", tile(t).name) } else { tile(t).name.to_string() };
                if ui.add_enabled(!blocked, egui::Checkbox::new(&mut on, label)).changed() {
                    if on {
                        tiles.push(t);
                    } else {
                        tiles.retain(|x| *x != t);
                    }
                }
            });
        }
    });
    ui.horizontal(|ui| {
        ui.label("₽");
        ui.add(egui::DragValue::new(money).range(0..=owner.money.max(0)).speed(10));
        ui.label(RichText::new(format!("из {}", owner.money)).color(theme::TEXT_DIM));
    });
    if owner.jail_free_cards > 0 {
        ui.horizontal(|ui| {
            ui.label("Карты");
            ui.add(egui::DragValue::new(cards).range(0..=owner.jail_free_cards));
        });
    }
}

/// Returns true when the player confirmed leaving.
pub fn confirm_leave(ctx: &egui::Context, session: &Session, ui: &mut MonoUi) -> bool {
    let mut confirmed = false;
    let offset = ctx.available_rect().center() - ctx.screen_rect().center();
    egui::Window::new("leave")
        .title_bar(false)
        .resizable(false)
        .collapsible(false)
        .anchor(Align2::CENTER_CENTER, offset)
        .frame(theme::card_frame().inner_margin(egui::Margin::same(20)))
        .show(ctx, |w| {
            w.label(RichText::new("Выйти из игры?").font(bold(22.0)));
            w.label(if session.is_host && session.mode.is_online() { "Вы хост — игра закончится для всех." } else { "Вы вернётесь в главное меню." });
            w.horizontal(|w| {
                if action_button(w, "Выйти", theme::BAD, true, 140.0).clicked() {
                    confirmed = true;
                    ui.confirm_leave = false;
                }
                if action_button(w, "Остаться", theme::PANEL_HI, true, 140.0).clicked() {
                    ui.confirm_leave = false;
                }
            });
        });
    confirmed
}
