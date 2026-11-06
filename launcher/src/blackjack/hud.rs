//! egui overlay for the Blackjack table: name plates at the boxes, hand
//! totals, the betting and playing buttons, the event feed and the dialogs.

use bevy::prelude::*;
use bevy_egui::egui::{self, Align2, Color32, CornerRadius, Pos2, RichText, Vec2};
use bevy_egui::EguiContexts;
use blackjack_core::{BjAction, BjEvent, BjGame, Card, Hand, Move, Outcome, Phase, BET_STEP, MIN_BET};

use super::binds::Action;
use super::keys::{clamp_bet, max_bet};
use super::scene::{dealer_label_pos, hand_label_pos, plate_pos};
use super::BjUi;
use crate::binds::binds_grid;
use crate::net::{SeatKind, Session};
use crate::profile::Profile;
use crate::sfx::Sfx;
use crate::steam::SteamApi;
use crate::theme::{self, action_button, bold, hud_frame, modal, rgb, semibold, with_alpha, SEAT_COLORS};
use crate::{AppState, MainCamera};

pub struct FeedItem {
    text: String,
    color: Color32,
    age: f32,
}

pub struct Bubble {
    seat: usize,
    text: String,
    color: Color32,
    age: f32,
}

pub struct Banner {
    text: String,
    color: Color32,
    age: f32,
}

const FEED_LIFE: f32 = 9.0;
const BUBBLE_LIFE: f32 = 2.2;
const BANNER_LIFE: f32 = 1.8;

fn name(view: &BjGame, seat: usize) -> String {
    view.players.get(seat).map(|p| p.name.clone()).unwrap_or_default()
}

fn card_color(card: &Card) -> Color32 {
    if card.suit.is_red() {
        Color32::from_rgb(255, 120, 120)
    } else {
        theme::TEXT
    }
}

fn outcome_color(o: Outcome) -> Color32 {
    match o {
        Outcome::Blackjack => theme::GOLD,
        Outcome::Win => theme::GOOD,
        Outcome::Push => theme::TEXT_DIM,
        Outcome::Lose | Outcome::Bust => theme::BAD,
    }
}

fn signed(v: i64) -> String {
    match v {
        0 => "±0".into(),
        v if v > 0 => format!("+{v}"),
        v => format!("−{}", -v),
    }
}

/// «17», «7 / 17», «Блэкджек» or «Перебор 24».
fn hand_text(hand: &Hand) -> String {
    if hand.is_blackjack() {
        return "Блэкджек".into();
    }
    let v = hand.value();
    if v.is_bust() {
        format!("Перебор {}", v.total)
    } else if v.soft && v.total < 21 {
        format!("{} / {}", v.total - 10, v.total)
    } else {
        v.total.to_string()
    }
}

fn process_events(view: &BjGame, me: usize, ui: &mut BjUi, sfx: &mut EventWriter<Sfx>) {
    if view.event_seq < ui.hud_seq {
        ui.hud_seq = 0;
    }
    let events: Vec<BjEvent> = view
        .events
        .iter()
        .filter(|e| e.seq > ui.hud_seq)
        .map(|e| e.event.clone())
        .collect();
    ui.hud_seq = view.event_seq;
    let feed = |ui: &mut BjUi, text: String, color: Color32| {
        ui.feed.push_back(FeedItem { text, color, age: 0.0 });
        while ui.feed.len() > 7 {
            ui.feed.pop_front();
        }
    };
    let bubble = |ui: &mut BjUi, seat: usize, text: String, color: Color32| {
        ui.bubbles.push(Bubble { seat, text, color, age: 0.0 });
    };
    let mut settled_me = false;
    for e in events {
        match e {
            BjEvent::RoundStarted { round } => {
                ui.banner = Some(Banner { text: format!("Раунд {round}"), color: theme::GOLD, age: 0.0 });
                feed(ui, "Делайте ставки".into(), theme::TEXT_DIM);
                ui.hole_seen = false;
            }
            BjEvent::Reshuffled => feed(ui, "Башмак перетасован".into(), theme::TEXT_DIM),
            BjEvent::BetPlaced { seat, amount } => {
                feed(ui, format!("{} ставит {amount}", name(view, seat)), theme::TEXT_DIM);
            }
            BjEvent::Dealt { to: blackjack_core::Spot::Dealer, card } if ui.hole_seen => {
                feed(ui, format!("Дилер берёт {}", card.short()), card_color(&card));
            }
            BjEvent::Dealt { .. } => {}
            BjEvent::Natural { seat } => {
                bubble(ui, seat, "Блэкджек!".into(), theme::GOLD);
                feed(ui, format!("{}: блэкджек!", name(view, seat)), theme::GOLD);
                sfx.write(Sfx::Special);
            }
            BjEvent::InsuranceOffered => {
                ui.banner = Some(Banner { text: "У дилера туз — страховка?".into(), color: theme::WARN, age: 0.0 });
            }
            BjEvent::InsuranceTaken { seat, amount } => {
                feed(ui, format!("{} страхуется на {amount}", name(view, seat)), theme::WARN);
            }
            BjEvent::InsuranceDeclined { .. } => {}
            BjEvent::DealerPeeked { blackjack } => {
                if blackjack {
                    ui.banner = Some(Banner { text: "У дилера блэкджек!".into(), color: theme::BAD, age: 0.0 });
                    sfx.write(Sfx::Caught);
                } else {
                    feed(ui, "Дилер проверил: блэкджека нет".into(), theme::TEXT_DIM);
                }
            }
            BjEvent::Moved { seat, action, .. } => {
                let who = name(view, seat);
                let (short, line, color) = match action {
                    Move::Hit => ("Ещё!", format!("{who}: ещё карту"), theme::TEXT),
                    Move::Stand => ("Хватит", format!("{who}: хватит"), theme::TEXT_DIM),
                    Move::Double => ("Удваиваю!", format!("{who} удваивает ставку"), theme::GOLD),
                    Move::Split => ("Делю!", format!("{who} делит пару"), theme::ACCENT),
                };
                bubble(ui, seat, short.into(), color);
                feed(ui, line, color);
            }
            BjEvent::Busted { seat, .. } => {
                bubble(ui, seat, "Перебор!".into(), theme::BAD);
                feed(ui, format!("{}: перебор", name(view, seat)), theme::BAD);
                sfx.write(Sfx::Caught);
            }
            BjEvent::HoleRevealed { card } => {
                ui.hole_seen = true;
                feed(ui, format!("Дилер открывает {}", card.short()), card_color(&card));
            }
            BjEvent::DealerStands { total } => {
                feed(ui, format!("У дилера {total}"), theme::TEXT);
            }
            BjEvent::DealerBusted { total } => {
                ui.banner = Some(Banner { text: format!("У дилера перебор ({total})!"), color: theme::GOOD, age: 0.0 });
                sfx.write(Sfx::Special);
            }
            BjEvent::Settled { seat, hand, outcome, payout } => {
                let bet = view.players[seat].hands.get(hand).map(|h| h.bet).unwrap_or(0);
                let text = match outcome {
                    Outcome::Win | Outcome::Blackjack => format!("+{}", payout - bet),
                    Outcome::Push => "Ничья".into(),
                    Outcome::Lose => format!("−{bet}"),
                    Outcome::Bust => String::new(),
                };
                if !text.is_empty() {
                    bubble(ui, seat, text, outcome_color(outcome));
                }
                settled_me |= seat == me;
            }
            BjEvent::InsurancePaid { seat, amount } => {
                feed(ui, format!("{}: страховка сыграла, +{amount}", name(view, seat)), theme::GOOD);
            }
            BjEvent::PlayerOut { seat } => {
                feed(ui, format!("{} выбывает: фишки кончились", name(view, seat)), theme::BAD);
            }
            BjEvent::GameWon { seat } => {
                let text = match seat {
                    Some(s) => format!("Победитель: {}", name(view, s)),
                    None => "Казино забрало все фишки".into(),
                };
                feed(ui, text, theme::GOLD);
            }
        }
    }
    if settled_me {
        let net = view.players[me].round_net();
        let (text, color) = match net {
            n if n > 0 => (format!("Вы выиграли {}", signed(n)), theme::GOOD),
            n if n < 0 => (format!("Вы проиграли {}", -n), theme::BAD),
            _ => ("При своих".to_string(), theme::TEXT),
        };
        ui.banner = Some(Banner { text, color, age: 0.0 });
        if net > 0 {
            sfx.write(Sfx::Win);
        }
    }
}

/// Short state of a seat under its name.
fn seat_status(view: &BjGame, seat: usize) -> (String, Color32) {
    let p = &view.players[seat];
    if p.out {
        return ("выбыл".into(), theme::BAD);
    }
    if view.needs_bet(seat) {
        return ("делает ставку…".into(), theme::WARN);
    }
    if view.needs_insurance(seat) {
        return ("думает о страховке…".into(), theme::WARN);
    }
    if matches!(view.phase, Phase::RoundOver | Phase::GameOver { .. }) && !p.hands.is_empty() {
        let net = p.round_net();
        let color = if net > 0 { theme::GOOD } else if net < 0 { theme::BAD } else { theme::TEXT_DIM };
        return (format!("за раунд {}", signed(net)), color);
    }
    let bet: u32 = p.hands.iter().map(|h| h.bet).sum();
    if bet > 0 {
        let ins = p.insurance.filter(|a| *a > 0).map(|a| format!(" + страховка {a}")).unwrap_or_default();
        (format!("ставка {bet}{ins}"), theme::TEXT_DIM)
    } else {
        (String::new(), theme::TEXT_DIM)
    }
}

#[allow(clippy::too_many_arguments)]
pub fn draw_hud(
    mut contexts: EguiContexts,
    mut session: ResMut<Session>,
    mut ui: ResMut<BjUi>,
    camera: Query<(&Camera, &GlobalTransform), With<MainCamera>>,
    time: Res<Time>,
    steam: Res<SteamApi>,
    mut next: ResMut<NextState<AppState>>,
    mut sfx: EventWriter<Sfx>,
    mut profile: ResMut<Profile>,
    mut prev_waiting: Local<bool>,
) -> Result {
    let ctx = contexts.ctx_mut()?.clone();
    // The keys belong to the table: a focused egui button must not also fire on Enter.
    if let Some(focused) = ctx.memory(|m| m.focused()) {
        ctx.memory_mut(|m| m.surrender_focus(focused));
    }
    let binds = profile.data.bj_binds.clone();
    let Some(view) = session.bj_view() else {
        egui::CentralPanel::default().frame(egui::Frame::NONE).show(&ctx, |ui| {
            ui.centered_and_justified(|ui| ui.label(RichText::new("Ожидаем данные от хоста…").font(semibold(20.0))));
        });
        return Ok(());
    };
    let n = view.players.len();
    let me = session.my_index().unwrap_or(0).min(n - 1);
    let dt = time.delta_secs();

    process_events(&view, me, &mut ui, &mut sfx);
    let waiting_me = view.is_waiting_for(me);
    if waiting_me && !*prev_waiting {
        sfx.write(Sfx::Turn);
    }
    *prev_waiting = waiting_me;
    for f in ui.feed.iter_mut() {
        f.age += dt;
    }
    ui.feed.retain(|f| f.age < FEED_LIFE);
    for b in ui.bubbles.iter_mut() {
        b.age += dt;
    }
    ui.bubbles.retain(|b| b.age < BUBBLE_LIFE);
    if let Some(b) = &mut ui.banner {
        b.age += dt;
        if b.age > BANNER_LIFE {
            ui.banner = None;
        }
    }
    // Keep the prepared bet valid as the stack changes.
    if view.needs_bet(me) {
        let last = view.players[me].last_bet;
        if ui.bet == 0 {
            ui.bet = if last > 0 { last } else { 50 };
        }
        ui.bet = clamp_bet(&view, me, ui.bet);
    }

    let screen = ctx.screen_rect();
    let cam = camera.single().ok();
    let project = |p: Vec3| -> Option<Pos2> {
        let (c, gt) = cam?;
        c.world_to_viewport(gt, p).ok().map(|v| Pos2::new(v.x, v.y))
    };

    // ---- name plates at the boxes --------------------------------------------------------
    for seat in 0..n {
        let Some(pos) = project(plate_pos(seat, n)) else { continue };
        let p = &view.players[seat];
        let waiting = view.is_waiting_for(seat);
        let is_me = seat == me;
        let kind = session.lobby.seats.get(seat).map(|s| s.kind);
        let (status, status_color) = seat_status(&view, seat);
        egui::Area::new(egui::Id::new(("bj-plate", seat)))
            .fixed_pos(Pos2::new(pos.x, pos.y.min(screen.bottom() - 70.0)))
            .pivot(Align2::CENTER_TOP)
            .interactable(false)
            .show(&ctx, |ui_| {
                let stroke = if waiting {
                    theme::stroke(2.0, theme::GOLD)
                } else if is_me {
                    theme::stroke(1.5, rgb(SEAT_COLORS[seat % 8]))
                } else {
                    theme::stroke(1.0, Color32::from_white_alpha(25))
                };
                hud_frame().inner_margin(egui::Margin::symmetric(9, 5)).stroke(stroke).show(ui_, |ui_| {
                    ui_.horizontal(|ui_| {
                        theme::avatar(ui_, &p.name, rgb(SEAT_COLORS[seat % 8]), 24.0);
                        ui_.vertical(|ui_| {
                            ui_.spacing_mut().item_spacing.y = 0.0;
                            ui_.horizontal(|ui_| {
                                ui_.add(egui::Label::new(RichText::new(&p.name).font(semibold(13.5))).extend());
                                if is_me {
                                    ui_.label(RichText::new("вы").font(theme::regular(11.0)).color(theme::ACCENT));
                                } else if kind == Some(SeatKind::Bot) {
                                    ui_.label(RichText::new("бот").font(theme::regular(11.0)).color(theme::TEXT_DIM));
                                }
                            });
                            ui_.add(egui::Label::new(RichText::new(format!("● {} фишек", p.chips)).font(theme::regular(12.0)).color(theme::GOLD)).extend());
                            if !status.is_empty() {
                                ui_.add(egui::Label::new(RichText::new(status).font(theme::regular(11.5)).color(status_color)).extend());
                            }
                        });
                    });
                });
            });
    }

    // ---- hand totals ---------------------------------------------------------------------------
    let active = match view.phase {
        Phase::PlayerTurn { seat, hand } => Some((seat, hand)),
        _ => None,
    };
    for (seat, p) in view.players.iter().enumerate() {
        for (h, hand) in p.hands.iter().enumerate() {
            if hand.cards.is_empty() {
                continue;
            }
            let Some(pos) = project(hand_label_pos(seat, n, h, p.hands.len(), hand.cards.len())) else { continue };
            let mut text = hand_text(hand);
            if hand.doubled {
                text.push_str(" · ×2");
            }
            let color = match hand.outcome {
                Some(o) => outcome_color(o),
                None if hand.is_blackjack() => theme::GOLD,
                None => theme::TEXT,
            };
            let is_active = active == Some((seat, h));
            pill(&ctx, ("bj-hand", seat * 8 + h), pos, &text, color, is_active);
        }
    }
    if !view.dealer.is_empty() {
        if let Some(pos) = project(dealer_label_pos()) {
            let v = view.dealer_value();
            let text = if view.hole_revealed && view.dealer.len() == 2 && v.total == 21 {
                "Дилер: блэкджек".to_string()
            } else if v.is_bust() {
                format!("Дилер: перебор {}", v.total)
            } else if view.hole_revealed {
                format!("Дилер: {}", v.total)
            } else {
                format!("Дилер: {} + ?", v.total)
            };
            let color = if v.is_bust() { theme::GOOD } else { theme::TEXT };
            pill(&ctx, ("bj-dealer", 0), pos, &text, color, view.phase == Phase::DealerTurn);
        }
    }

    // ---- speech bubbles ---------------------------------------------------------------------------
    let mut stacked: std::collections::HashMap<usize, f32> = Default::default();
    for (i, b) in ui.bubbles.iter().enumerate().rev() {
        let stack = stacked.entry(b.seat).or_insert(0.0);
        let offset = *stack;
        *stack += 34.0;
        let Some(anchor) = project(super::scene::spot_pos(b.seat, n) + Vec3::Y * 0.4) else { continue };
        let rise = b.age * 18.0;
        let alpha = ((BUBBLE_LIFE - b.age) / 0.4).clamp(0.0, 1.0);
        egui::Area::new(egui::Id::new(("bj-bubble", i)))
            .fixed_pos(Pos2::new(anchor.x, anchor.y - 30.0 - rise - offset))
            .pivot(Align2::CENTER_BOTTOM)
            .interactable(false)
            .order(egui::Order::Foreground)
            .show(&ctx, |ui_| {
                egui::Frame::new()
                    .fill(with_alpha(b.color, (230.0 * alpha) as u8))
                    .corner_radius(CornerRadius::same(16))
                    .inner_margin(egui::Margin::symmetric(14, 6))
                    .show(ui_, |ui_| {
                        let text_color = if theme::is_light(b.color) { Color32::from_rgb(25, 25, 30) } else { Color32::WHITE };
                        ui_.add(egui::Label::new(RichText::new(&b.text).font(bold(18.0)).color(with_alpha(text_color, (255.0 * alpha) as u8))).extend());
                    });
            });
    }

    // ---- top-left: game info --------------------------------------------------------------------
    egui::Area::new(egui::Id::new("bj-info"))
        .fixed_pos(Pos2::new(16.0, 16.0))
        .show(&ctx, |ui_| {
            hud_frame().show(ui_, |ui_| {
                ui_.set_min_width(240.0);
                ui_.label(RichText::new("Блэкджек").font(bold(20.0)));
                let rounds = if view.rounds_limit == 0 {
                    format!("Раунд {} · пока есть фишки", view.round)
                } else {
                    format!("Раунд {} из {}", view.round, view.rounds_limit)
                };
                ui_.label(RichText::new(rounds).color(theme::TEXT_DIM));
                ui_.add_space(4.0);
                ui_.label(RichText::new("Блэкджек 3:2 · страховка 2:1").font(semibold(14.0)));
                ui_.label(RichText::new("Дилер берёт до 17, на мягких 17 стоит").color(theme::TEXT_DIM));
                ui_.label(RichText::new(format!("Ставки {MIN_BET}–{} · в башмаке {} карт", blackjack_core::MAX_BET, view.shoe.len())).color(theme::TEXT_DIM));
            });
        });

    // ---- top-right: buttons ------------------------------------------------------------------------
    egui::Area::new(egui::Id::new("bj-menu"))
        .anchor(Align2::RIGHT_TOP, Vec2::new(-16.0, 16.0))
        .show(&ctx, |ui_| {
            ui_.horizontal(|ui_| {
                if ui_.button("⚙").on_hover_text("Управление: клавиши и их назначение").clicked() {
                    ui.show_settings = true;
                }
                if ui_.button("📊 Фишки").clicked() {
                    ui.show_scores = !ui.show_scores;
                }
                if session.is_host && ui_.button("↩ В лобби").clicked() {
                    session.back_to_lobby();
                }
                if ui_.button("✖ Выйти").clicked() {
                    ui.confirm_leave = true;
                }
            });
        });

    // ---- event feed ----------------------------------------------------------------------------------
    egui::Area::new(egui::Id::new("bj-feed"))
        .anchor(Align2::LEFT_BOTTOM, Vec2::new(16.0, -16.0))
        .interactable(false)
        .show(&ctx, |ui_| {
            ui_.set_max_width(300.0);
            for item in &ui.feed {
                let alpha = ((FEED_LIFE - item.age) / 1.5).clamp(0.0, 1.0);
                egui::Frame::new()
                    .fill(Color32::from_rgba_unmultiplied(15, 17, 21, (190.0 * alpha) as u8))
                    .corner_radius(CornerRadius::same(8))
                    .inner_margin(egui::Margin::symmetric(10, 4))
                    .show(ui_, |ui_| {
                        ui_.label(RichText::new(&item.text).color(with_alpha(item.color, (255.0 * alpha) as u8)));
                    });
            }
        });

    // ---- centre banner ----------------------------------------------------------------------------------
    if let Some(b) = &ui.banner {
        let t = b.age / BANNER_LIFE;
        let alpha = if t < 0.15 { t / 0.15 } else { ((1.0 - t) / 0.3).clamp(0.0, 1.0) };
        let scale = 1.0 + (1.0 - (t / 0.15).min(1.0)) * 0.3;
        let painter = ctx.layer_painter(egui::LayerId::new(egui::Order::Foreground, egui::Id::new("bj-banner")));
        let pos = Pos2::new(screen.center().x, screen.top() + screen.height() * 0.3);
        painter.text(pos + Vec2::new(2.0, 3.0), Align2::CENTER_CENTER, &b.text, bold(42.0 * scale), Color32::from_black_alpha((160.0 * alpha) as u8));
        painter.text(pos, Align2::CENTER_CENTER, &b.text, bold(42.0 * scale), with_alpha(b.color, (255.0 * alpha) as u8));
    }

    // ---- top: status line ---------------------------------------------------------------------------------
    let my = &view.players[me];
    let status = match view.phase {
        Phase::Betting if view.needs_bet(me) => "Ваша ставка: выберите сумму и поставьте".to_string(),
        Phase::Betting if my.out => "У вас кончились фишки — вы наблюдаете".to_string(),
        Phase::Betting => {
            let left: Vec<String> = (0..n).filter(|s| view.needs_bet(*s)).map(|s| name(&view, s)).collect();
            format!("Ждём ставки: {}", left.join(", "))
        }
        Phase::Insurance if view.needs_insurance(me) => {
            format!("У дилера туз. Страховка за {}?", view.insurance_cost(me))
        }
        Phase::Insurance => "Игроки решают насчёт страховки…".to_string(),
        Phase::PlayerTurn { seat, hand } if seat == me => {
            let h = &my.hands[hand];
            let which = if my.hands.len() > 1 { format!(" (рука {} из {})", hand + 1, my.hands.len()) } else { String::new() };
            format!("Ваш ход{which}: у вас {}", hand_text(h))
        }
        Phase::PlayerTurn { seat, .. } => format!("Ходит {}…", name(&view, seat)),
        Phase::DealerTurn => "Ход дилера".to_string(),
        Phase::RoundOver => "Раунд окончен".to_string(),
        Phase::GameOver { .. } => "Игра окончена".to_string(),
    };
    egui::Area::new(egui::Id::new("bj-status"))
        .anchor(Align2::CENTER_TOP, Vec2::new(0.0, 16.0))
        .interactable(false)
        .show(&ctx, |ui_| {
            hud_frame()
                .inner_margin(egui::Margin::symmetric(18, 8))
                .stroke(if waiting_me { theme::stroke(2.0, theme::GOLD) } else { theme::stroke(1.0, Color32::from_white_alpha(18)) })
                .show(ui_, |ui_| {
                    ui_.add(egui::Label::new(RichText::new(status).font(semibold(17.0)).color(if waiting_me { theme::GOLD } else { theme::TEXT })).extend());
                });
        });

    // ---- bottom-right: the player's controls -------------------------------------------------------------
    let hint = |action: Action| format!("  [{}]", binds.label(action));
    const W: f32 = 250.0;
    egui::Area::new(egui::Id::new("bj-actions"))
        .anchor(Align2::RIGHT_BOTTOM, Vec2::new(-20.0, -20.0))
        .show(&ctx, |ui_| {
            hud_frame().show(ui_, |ui_| {
                ui_.set_width(W);
                ui_.horizontal(|ui_| {
                    theme::avatar(ui_, &my.name, rgb(SEAT_COLORS[me % 8]), 34.0);
                    ui_.vertical(|ui_| {
                        ui_.spacing_mut().item_spacing.y = 0.0;
                        ui_.label(RichText::new(&my.name).font(semibold(15.0)));
                        ui_.label(RichText::new(format!("● {} фишек", my.chips)).color(theme::GOLD));
                    });
                });
                ui_.add_space(6.0);
                match view.phase {
                    Phase::Betting if view.needs_bet(me) => {
                        let top = max_bet(&view, me);
                        ui_.horizontal(|ui_| {
                            if ui_.add_enabled(ui.bet > MIN_BET, egui::Button::new(RichText::new("−").font(bold(18.0))).min_size(Vec2::new(36.0, 34.0))).clicked() {
                                ui.bet = clamp_bet(&view, me, ui.bet.saturating_sub(BET_STEP));
                            }
                            ui_.add_sized(
                                Vec2::new(W - 2.0 * 36.0 - 20.0, 34.0),
                                egui::Label::new(RichText::new(format!("Ставка: {}", ui.bet)).font(bold(20.0)).color(theme::GOLD)),
                            );
                            if ui_.add_enabled(ui.bet < top, egui::Button::new(RichText::new("+").font(bold(18.0))).min_size(Vec2::new(36.0, 34.0))).clicked() {
                                ui.bet = clamp_bet(&view, me, ui.bet + BET_STEP);
                            }
                        });
                        ui_.horizontal(|ui_| {
                            for (value, color) in super::art::CHIPS.iter().rev() {
                                let c = rgb(*color);
                                let enabled = ui.bet + value <= top;
                                let chip = egui::Button::new(RichText::new(format!("+{value}")).font(semibold(13.0)).color(Color32::WHITE))
                                    .fill(if enabled { c } else { with_alpha(c, 60) })
                                    .corner_radius(CornerRadius::same(16))
                                    .min_size(Vec2::new(54.0, 30.0));
                                if ui_.add_enabled(enabled, chip).clicked() {
                                    ui.bet = clamp_bet(&view, me, ui.bet + value);
                                }
                            }
                        });
                        ui_.horizontal(|ui_| {
                            if ui_.button("Мин.").clicked() {
                                ui.bet = MIN_BET;
                            }
                            if ui_.button("Макс.").clicked() {
                                ui.bet = clamp_bet(&view, me, top);
                            }
                        });
                        ui_.add_space(4.0);
                        if action_button(ui_, &format!("Поставить {}{}", ui.bet, hint(Action::PlaceBet)), theme::GOOD, true, W).clicked() {
                            let bet = clamp_bet(&view, me, ui.bet);
                            session.submit_blackjack(BjAction::Bet(bet));
                        }
                        let last = my.last_bet;
                        if last > 0 && action_button(ui_, &format!("Как в прошлый раз: {last}{}", hint(Action::RepeatBet)), theme::PANEL_HI, last <= my.chips, W).clicked() {
                            session.submit_blackjack(BjAction::Bet(last));
                        }
                    }
                    Phase::Insurance if view.needs_insurance(me) => {
                        let cost = view.insurance_cost(me);
                        ui_.label(RichText::new("Страховка платит 2:1, если у дилера блэкджек.").color(theme::TEXT_DIM));
                        if action_button(ui_, &format!("Страховка {cost}{}", hint(Action::Insure)), theme::WARN, my.chips >= cost, W).clicked() {
                            session.submit_blackjack(BjAction::Insurance(true));
                        }
                        if action_button(ui_, &format!("Без страховки{}", hint(Action::Decline)), theme::PANEL_HI, true, W).clicked() {
                            session.submit_blackjack(BjAction::Insurance(false));
                        }
                    }
                    Phase::PlayerTurn { seat, .. } if seat == me => {
                        if action_button(ui_, &format!("Ещё карту{}", hint(Action::Hit)), theme::ACCENT, true, W).clicked() {
                            session.submit_blackjack(BjAction::Hit);
                        }
                        if action_button(ui_, &format!("Хватит{}", hint(Action::Stand)), theme::PANEL_HI, true, W).clicked() {
                            session.submit_blackjack(BjAction::Stand);
                        }
                        if action_button(ui_, &format!("Удвоить{}", hint(Action::Double)), theme::GOLD, view.can_double(me), W).clicked() {
                            session.submit_blackjack(BjAction::Double);
                        }
                        if action_button(ui_, &format!("Разделить{}", hint(Action::Split)), theme::GOOD, view.can_split(me), W).clicked() {
                            session.submit_blackjack(BjAction::Split);
                        }
                    }
                    Phase::RoundOver => {
                        if !my.hands.is_empty() {
                            let net = my.round_net();
                            let color = if net > 0 { theme::GOOD } else if net < 0 { theme::BAD } else { theme::TEXT };
                            ui_.label(RichText::new(format!("За раунд: {}", signed(net))).font(semibold(16.0)).color(color));
                        }
                        if session.is_host {
                            if action_button(ui_, &format!("Следующий раунд{}", hint(Action::Confirm)), theme::GOOD, true, W).clicked() {
                                session.submit_blackjack(BjAction::NextRound);
                            }
                        } else {
                            ui_.label(RichText::new("Ждём, пока хост раздаст снова…").color(theme::TEXT_DIM));
                        }
                    }
                    _ => {
                        let text = if my.out {
                            "Фишки кончились — вы наблюдаете за игрой."
                        } else if !my.hands.is_empty() {
                            "Ставка сделана. Ждём остальных…"
                        } else {
                            "Ждём…"
                        };
                        ui_.label(RichText::new(text).color(theme::TEXT_DIM));
                    }
                }
                ui_.add_space(2.0);
                ui_.label(RichText::new("Цель — набрать больше дилера, не больше 21. ⚙ — клавиши").font(theme::regular(11.5)).color(theme::TEXT_DIM));
            });
        });

    // ---- results of the round (left, under the info) ----------------------------------------------------
    if view.phase == Phase::RoundOver {
        egui::Area::new(egui::Id::new("bj-results"))
            .fixed_pos(Pos2::new(16.0, 190.0))
            .interactable(false)
            .show(&ctx, |ui_| {
                hud_frame().show(ui_, |ui_| {
                    ui_.set_min_width(240.0);
                    ui_.label(RichText::new("Итоги раунда").font(bold(17.0)));
                    for (seat, p) in view.players.iter().enumerate() {
                        if p.hands.is_empty() {
                            continue;
                        }
                        let net = p.round_net();
                        let outcomes: Vec<&str> = p.hands.iter().filter_map(|h| h.outcome.map(Outcome::name_ru)).collect();
                        ui_.horizontal(|ui_| {
                            theme::avatar(ui_, &p.name, rgb(SEAT_COLORS[seat % 8]), 20.0);
                            ui_.label(&p.name);
                            ui_.label(RichText::new(outcomes.join(", ")).color(theme::TEXT_DIM));
                            let color = if net > 0 { theme::GOOD } else if net < 0 { theme::BAD } else { theme::TEXT_DIM };
                            ui_.label(RichText::new(signed(net)).font(semibold(14.0)).color(color));
                        });
                    }
                });
            });
    }

    // ---- dialogs ---------------------------------------------------------------------------------------------
    let over = matches!(view.phase, Phase::GameOver { .. });
    if over || ui.show_scores {
        let title = match view.phase {
            Phase::GameOver { winner: Some(w) } => format!("🏆 Победитель: {}", name(&view, w)),
            Phase::GameOver { winner: None } => "Казино забрало все фишки".to_string(),
            _ => "Фишки".to_string(),
        };
        let subtitle = if over {
            format!("Сыграно раундов: {}", view.round)
        } else {
            format!("Раунд {} · у всех было по {}", view.round, view.starting_chips)
        };
        modal(&ctx, "bj-scores", |ui_| {
            ui_.label(RichText::new(title).font(bold(26.0)));
            ui_.label(RichText::new(subtitle).color(theme::TEXT_DIM));
            ui_.add_space(8.0);
            let mut order: Vec<usize> = (0..n).collect();
            order.sort_by_key(|i| std::cmp::Reverse(view.players[*i].total_chips()));
            egui::Grid::new("bj-score-grid").striped(true).min_col_width(80.0).show(ui_, |ui_| {
                ui_.label(RichText::new("Игрок").color(theme::TEXT_DIM));
                ui_.label(RichText::new("Фишки").color(theme::TEXT_DIM));
                ui_.label(RichText::new("Итог").color(theme::TEXT_DIM));
                ui_.end_row();
                for i in order {
                    let p = &view.players[i];
                    ui_.horizontal(|ui_| {
                        theme::avatar(ui_, &p.name, rgb(SEAT_COLORS[i % 8]), 22.0);
                        ui_.label(&p.name);
                    });
                    ui_.label(RichText::new(p.total_chips().to_string()).font(semibold(15.0)));
                    let diff = p.total_chips() as i64 - view.starting_chips as i64;
                    let color = if diff > 0 { theme::GOOD } else if diff < 0 { theme::BAD } else { theme::TEXT_DIM };
                    ui_.label(RichText::new(signed(diff)).color(color));
                    ui_.end_row();
                }
            });
            ui_.add_space(10.0);
            ui_.horizontal(|ui_| {
                if over {
                    if session.is_host {
                        if action_button(ui_, &format!("Сыграть ещё (в лобби) [{}]", binds.label(Action::Confirm)), theme::GOOD, true, 260.0).clicked() {
                            session.back_to_lobby();
                        }
                    } else {
                        ui_.label(RichText::new("Хост может начать новую игру").color(theme::TEXT_DIM));
                    }
                    if action_button(ui_, "В меню", theme::PANEL_HI, true, 140.0).clicked() {
                        ui.confirm_leave = true;
                    }
                } else if action_button(ui_, "Закрыть", theme::PANEL_HI, true, 140.0).clicked() {
                    ui.show_scores = false;
                }
            });
        });
    }

    if ui.show_settings {
        modal(&ctx, "bj-settings", |ui_| {
            ui_.set_width(460.0);
            ui_.label(RichText::new("Управление").font(bold(24.0)));
            ui_.label(RichText::new("Клавиши привязаны к положению, как на латинской раскладке QWERTY.").color(theme::TEXT_DIM));
            ui_.add_space(8.0);
            binds_grid(ui_, "bj-binds", &binds, &mut ui.capture);
            ui_.add_space(8.0);
            ui_.label(
                RichText::new("Нажмите кнопку с клавишей, затем нужную клавишу. Esc — отмена. Если клавиша уже занята, действия меняются местами.")
                    .font(theme::regular(12.0))
                    .color(theme::TEXT_DIM),
            );
            ui_.add_space(10.0);
            ui_.horizontal(|ui_| {
                if action_button(ui_, "По умолчанию", theme::PANEL_HI, true, 200.0).clicked() {
                    profile.data.bj_binds.reset();
                    profile.save();
                    ui.capture = None;
                }
                if action_button(ui_, "Готово", theme::GOOD, true, 140.0).clicked() {
                    ui.show_settings = false;
                    ui.capture = None;
                }
            });
        });
    }

    if ui.confirm_leave {
        modal(&ctx, "bj-confirm-leave", |ui_| {
            ui_.label(RichText::new("Выйти из игры?").font(bold(22.0)));
            ui_.label(if session.is_host && session.mode.is_online() {
                "Вы хост — игра закончится для всех."
            } else {
                "Вы вернётесь в главное меню."
            });
            ui_.add_space(8.0);
            ui_.horizontal(|ui_| {
                if action_button(ui_, "Выйти", theme::BAD, true, 140.0).clicked() {
                    session.leave(&steam);
                    next.set(AppState::Menu);
                    ui.confirm_leave = false;
                }
                if action_button(ui_, "Остаться", theme::PANEL_HI, true, 140.0).clicked() {
                    ui.confirm_leave = false;
                }
            });
        });
    }
    Ok(())
}

/// A small rounded label pinned to a point on the table.
fn pill(ctx: &egui::Context, id: (&str, usize), pos: Pos2, text: &str, color: Color32, highlight: bool) {
    egui::Area::new(egui::Id::new(id))
        .fixed_pos(pos)
        .pivot(Align2::CENTER_BOTTOM)
        .interactable(false)
        .show(ctx, |ui_| {
            egui::Frame::new()
                .fill(Color32::from_rgba_unmultiplied(12, 14, 18, 215))
                .stroke(if highlight { theme::stroke(2.0, theme::GOLD) } else { theme::stroke(1.0, Color32::from_white_alpha(30)) })
                .corner_radius(CornerRadius::same(10))
                .inner_margin(egui::Margin::symmetric(9, 3))
                .show(ui_, |ui_| {
                    ui_.add(egui::Label::new(RichText::new(text).font(semibold(14.0)).color(color)).extend());
                });
        });
}
