//! egui overlay for the card table: name plates over the players, the action
//! bar, colour picker, challenge prompt, event feed and round results.

use bevy::prelude::*;
use bevy_egui::egui::{self, Align2, Color32, CornerRadius, Pos2, RichText, StrokeKind, Vec2};
use bevy_egui::EguiContexts;
use uno_core::{Card, CardColor, DrawReason, Phase, UnoAction, UnoEvent, UnoGame};

use super::binds::Action;
use crate::binds::binds_grid;
use super::keys;
use super::scene::{head_top, rel_seat};
use super::{color32, UnoUi};
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
const BANNER_LIFE: f32 = 1.6;

fn name(view: &UnoGame, seat: usize) -> String {
    view.players.get(seat).map(|p| p.name.clone()).unwrap_or_default()
}

fn card_text(card: &Card) -> String {
    card.describe_ru()
}

fn card_color32(card: &Card) -> Color32 {
    card.color.map(color32).unwrap_or(theme::TEXT)
}

fn color_name_acc(c: CardColor) -> &'static str {
    match c {
        CardColor::Red => "красный",
        CardColor::Yellow => "жёлтый",
        CardColor::Green => "зелёный",
        CardColor::Blue => "синий",
    }
}

fn process_events(view: &UnoGame, me: usize, ui: &mut UnoUi, sfx: &mut EventWriter<Sfx>) {
    if view.event_seq < ui.hud_seq {
        // A brand-new game started on the same screen.
        ui.hud_seq = 0;
    }
    let events: Vec<UnoEvent> = view
        .events
        .iter()
        .filter(|e| e.seq > ui.hud_seq)
        .map(|e| e.event.clone())
        .collect();
    ui.hud_seq = view.event_seq;
    let feed = |ui: &mut UnoUi, text: String, color: Color32| {
        ui.feed.push_back(FeedItem { text, color, age: 0.0 });
        while ui.feed.len() > 7 {
            ui.feed.pop_front();
        }
    };
    for e in events {
        match e {
            UnoEvent::RoundStarted { round, dealer } => {
                ui.banner = Some(Banner { text: format!("Раунд {round}"), color: theme::GOLD, age: 0.0 });
                feed(ui, format!("Раздаёт {}", name(view, dealer)), theme::TEXT_DIM);
            }
            UnoEvent::FirstCard { card } => {
                feed(ui, format!("Первая карта: {}", card_text(&card)), card_color32(&card));
            }
            UnoEvent::Played { seat, card } => {
                feed(ui, format!("{}: {}", name(view, seat), card_text(&card)), card_color32(&card));
            }
            UnoEvent::Drew { seat, count, reason } => {
                let who = name(view, seat);
                let (text, color) = match reason {
                    DrawReason::Voluntary => (format!("{who} берёт карту"), theme::TEXT_DIM),
                    DrawReason::DrawTwo => (format!("{who} берёт +2"), theme::WARN),
                    DrawReason::DrawFour => (format!("{who} берёт +4"), theme::WARN),
                    DrawReason::UnoPenalty => (format!("{who}: штраф +{count}"), theme::BAD),
                    DrawReason::ChallengeFailed => (format!("{who} проиграл спор: +{count}"), theme::BAD),
                    DrawReason::ChallengeGuilty => (format!("{who} пойман на блефе: +{count}"), theme::BAD),
                };
                if reason != DrawReason::Voluntary {
                    ui.bubbles.push(Bubble { seat, text: format!("+{count}"), color: theme::BAD, age: 0.0 });
                }
                feed(ui, text, color);
            }
            UnoEvent::Passed { seat } => feed(ui, format!("{} пасует", name(view, seat)), theme::TEXT_DIM),
            UnoEvent::ColorChosen { seat, color } => {
                feed(ui, format!("{} заказывает {} цвет", name(view, seat), color_name_acc(color)), color32(color));
                ui.banner = Some(Banner {
                    text: format!("Цвет: {}", color.name_ru()),
                    color: color32(color),
                    age: 0.0,
                });
            }
            UnoEvent::Skipped { seat } => {
                ui.bubbles.push(Bubble { seat, text: "Пропуск!".into(), color: theme::WARN, age: 0.0 });
                if seat == me {
                    ui.banner = Some(Banner { text: "Вы пропускаете ход".into(), color: theme::WARN, age: 0.0 });
                }
                sfx.write(Sfx::Special);
            }
            UnoEvent::Reversed { .. } => {
                ui.banner = Some(Banner { text: "⟲ Смена направления".into(), color: theme::ACCENT, age: 0.0 });
                sfx.write(Sfx::Special);
            }
            UnoEvent::UnoCalled { seat } => {
                ui.bubbles.push(Bubble { seat, text: "Последняя!".into(), color: theme::GOLD, age: 0.0 });
                feed(ui, format!("{} объявляет: «Последняя!»", name(view, seat)), theme::GOLD);
                sfx.write(Sfx::LastCard);
            }
            UnoEvent::UnoCaught { catcher, target } => {
                ui.bubbles.push(Bubble { seat: catcher, text: "Попался!".into(), color: theme::BAD, age: 0.0 });
                feed(
                    ui,
                    format!("{} ловит {} без объявления!", name(view, catcher), name(view, target)),
                    theme::BAD,
                );
                sfx.write(Sfx::Caught);
            }
            UnoEvent::Challenged { challenger, offender, success } => {
                let text = if success {
                    format!("{} разоблачил блеф {}!", name(view, challenger), name(view, offender))
                } else {
                    format!("{} зря оспорил +4", name(view, challenger))
                };
                ui.banner = Some(Banner { text: text.clone(), color: theme::WARN, age: 0.0 });
                feed(ui, text, theme::WARN);
            }
            UnoEvent::Reshuffled => feed(ui, "Сброс перетасован в колоду".into(), theme::TEXT_DIM),
            UnoEvent::RoundWon { seat, points } => {
                ui.banner = Some(Banner {
                    text: format!("{} выигрывает раунд! +{points}", name(view, seat)),
                    color: theme::GOLD,
                    age: 0.0,
                });
                sfx.write(Sfx::Win);
            }
            UnoEvent::GameWon { .. } => {}
        }
    }
}

#[allow(clippy::too_many_arguments)]
pub fn draw_hud(
    mut contexts: EguiContexts,
    mut session: ResMut<Session>,
    mut ui: ResMut<UnoUi>,
    camera: Query<(&Camera, &GlobalTransform), With<MainCamera>>,
    time: Res<Time>,
    steam: Res<SteamApi>,
    mut next: ResMut<NextState<AppState>>,
    mut sfx: EventWriter<Sfx>,
    mut profile: ResMut<Profile>,
    mut prev_acting: Local<Option<usize>>,
) -> Result {
    let ctx = contexts.ctx_mut()?.clone();
    // The keys belong to the table: a focused egui button must not also
    // fire on Enter or Space.
    if let Some(focused) = ctx.memory(|m| m.focused()) {
        ctx.memory_mut(|m| m.surrender_focus(focused));
    }
    let binds = profile.data.uno_binds.clone();
    let Some(view) = session.uno_view() else {
        egui::CentralPanel::default().frame(egui::Frame::NONE).show(&ctx, |ui| {
            ui.centered_and_justified(|ui| ui.label(RichText::new("Ожидаем данные от хоста…").font(semibold(20.0))));
        });
        return Ok(());
    };
    let n = view.players.len();
    let me = session.my_index().unwrap_or(0).min(n - 1);
    let dt = time.delta_secs();

    process_events(&view, me, &mut ui, &mut sfx);
    let acting = view.acting_seat();
    if acting != *prev_acting {
        if acting == Some(me) {
            sfx.write(Sfx::Turn);
        }
        *prev_acting = acting;
    }
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

    let screen = ctx.screen_rect();
    let cam = camera.single().ok();

    // ---- name plates over the opponents --------------------------------------
    for seat in 0..n {
        if seat == me {
            continue;
        }
        let rel = rel_seat(seat, me, n);
        let Some((c, gt)) = cam else { break };
        let Ok(pos) = c.world_to_viewport(gt, head_top(rel, n)) else { continue };
        let p = &view.players[seat];
        let is_acting = acting == Some(seat);
        let seat_kind = session.lobby.seats.get(seat).map(|s| s.kind);
        egui::Area::new(egui::Id::new(("plate", seat)))
            .fixed_pos(Pos2::new(pos.x, pos.y.max(120.0)))
            .pivot(Align2::CENTER_BOTTOM)
            .interactable(false)
            .show(&ctx, |ui_| {
                let stroke = if is_acting {
                    theme::stroke(2.0, color32(view.active_color))
                } else {
                    theme::stroke(1.0, Color32::from_white_alpha(25))
                };
                hud_frame()
                    .inner_margin(egui::Margin::symmetric(10, 6))
                    .stroke(stroke)
                    .show(ui_, |ui_| {
                        ui_.horizontal(|ui_| {
                            theme::avatar(ui_, &p.name, rgb(SEAT_COLORS[seat % 8]), 26.0);
                            ui_.vertical(|ui_| {
                                ui_.spacing_mut().item_spacing.y = 0.0;
                                ui_.horizontal(|ui_| {
                                    ui_.add(egui::Label::new(RichText::new(&p.name).font(semibold(14.0))).extend());
                                    if seat_kind == Some(SeatKind::Bot) {
                                        ui_.label(RichText::new("бот").font(theme::regular(11.0)).color(theme::TEXT_DIM));
                                    }
                                });
                                let mut info = format!("{} карт · {} очк.", p.hand.len(), p.score);
                                if p.hand.len() == 1 && p.uno_called {
                                    info = format!("ПОСЛЕДНЯЯ! · {} очк.", p.score);
                                }
                                ui_.add(egui::Label::new(RichText::new(info).font(theme::regular(12.0)).color(
                                    if p.hand.len() == 1 { theme::GOLD } else { theme::TEXT_DIM },
                                )).extend());
                            });
                        });
                    });
            });
    }

    // ---- speech bubbles -----------------------------------------------------------
    let mut stacked: std::collections::HashMap<usize, f32> = Default::default();
    for (i, b) in ui.bubbles.iter().enumerate().rev() {
        let stack = stacked.entry(b.seat).or_insert(0.0);
        let stack_offset = *stack;
        *stack += 34.0;
        let anchor = if b.seat == me {
            Some(Vec2::new(screen.center().x, screen.bottom() - 250.0))
        } else {
            cam.and_then(|(c, gt)| {
                c.world_to_viewport(gt, head_top(rel_seat(b.seat, me, n), n) + Vec3::Y * 0.45)
                    .ok()
                    .map(|p| Vec2::new(p.x, p.y))
            })
        };
        let Some(anchor) = anchor else { continue };
        let rise = b.age * 18.0;
        let alpha = ((BUBBLE_LIFE - b.age) / 0.4).clamp(0.0, 1.0);
        egui::Area::new(egui::Id::new(("bubble", i)))
            .fixed_pos(Pos2::new(anchor.x, anchor.y - 40.0 - rise - stack_offset))
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
                        ui_.label(RichText::new(&b.text).font(bold(18.0)).color(with_alpha(text_color, (255.0 * alpha) as u8)));
                    });
            });
    }

    // ---- top-left: game info ------------------------------------------------------
    egui::Area::new(egui::Id::new("uno-info"))
        .fixed_pos(Pos2::new(16.0, 16.0))
        .show(&ctx, |ui_| {
            hud_frame().show(ui_, |ui_| {
                ui_.set_min_width(230.0);
                ui_.label(RichText::new("Уно").font(bold(20.0)));
                let target = if view.target_score == 0 {
                    "одна партия".to_string()
                } else {
                    format!("до {} очков", view.target_score)
                };
                ui_.label(RichText::new(format!("Раунд {} · {target}", view.round)).color(theme::TEXT_DIM));
                ui_.add_space(4.0);
                ui_.horizontal(|ui_| {
                    let (rect, _) = ui_.allocate_exact_size(Vec2::splat(22.0), egui::Sense::hover());
                    ui_.painter().circle_filled(rect.center(), 10.0, color32(view.active_color));
                    ui_.painter().circle_stroke(rect.center(), 10.0, theme::stroke(1.5, Color32::WHITE));
                    ui_.label(RichText::new(format!("Цвет: {}", view.active_color.name_ru())).font(semibold(15.0)));
                });
                let dir = match view.direction {
                    uno_core::Direction::Clockwise => "⟳ по часовой",
                    uno_core::Direction::CounterClockwise => "⟲ против часовой",
                };
                ui_.label(RichText::new(format!("Направление: {dir}")).color(theme::TEXT_DIM));
                ui_.label(RichText::new(format!("В колоде: {}", view.draw_pile.len())).color(theme::TEXT_DIM));
            });
        });

    // ---- top-right: buttons ---------------------------------------------------------
    egui::Area::new(egui::Id::new("uno-menu"))
        .anchor(Align2::RIGHT_TOP, Vec2::new(-16.0, 16.0))
        .show(&ctx, |ui_| {
            ui_.horizontal(|ui_| {
                if ui_.button("⚙").on_hover_text("Управление: клавиши и их назначение").clicked() {
                    ui.show_settings = true;
                }
                if ui_.button("📊 Счёт").clicked() {
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

    // ---- event feed -----------------------------------------------------------------
    egui::Area::new(egui::Id::new("uno-feed"))
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

    // ---- centre banner ----------------------------------------------------------------
    if let Some(b) = &ui.banner {
        let t = b.age / BANNER_LIFE;
        let alpha = if t < 0.15 { t / 0.15 } else { ((1.0 - t) / 0.3).clamp(0.0, 1.0) };
        let scale = 1.0 + (1.0 - (t / 0.15).min(1.0)) * 0.3;
        let painter = ctx.layer_painter(egui::LayerId::new(egui::Order::Foreground, egui::Id::new("banner")));
        let pos = Pos2::new(screen.center().x, screen.top() + screen.height() * 0.3);
        painter.text(
            pos + Vec2::new(2.0, 3.0),
            Align2::CENTER_CENTER,
            &b.text,
            bold(44.0 * scale),
            Color32::from_black_alpha((160.0 * alpha) as u8),
        );
        painter.text(pos, Align2::CENTER_CENTER, &b.text, bold(44.0 * scale), with_alpha(b.color, (255.0 * alpha) as u8));
    }

    // ---- bottom: status + actions -------------------------------------------------------
    let my = &view.players[me];
    let my_turn = acting == Some(me);
    let status = match &view.phase {
        Phase::Turn if my_turn => "Ваш ход: сыграйте карту или возьмите из колоды".to_string(),
        Phase::AfterDraw { .. } if my_turn => "Можно сыграть взятую карту или пропустить ход".to_string(),
        Phase::ChooseColor { seat, .. } if *seat == me => "Выберите цвет".to_string(),
        Phase::Challenge { victim, .. } if *victim == me => "Вам +4 — оспорить?".to_string(),
        Phase::RoundOver { .. } => "Раунд окончен".to_string(),
        Phase::GameOver { .. } => "Игра окончена".to_string(),
        _ => match acting {
            Some(a) => format!("Ходит {}…", name(&view, a)),
            None => String::new(),
        },
    };
    egui::Area::new(egui::Id::new("uno-status"))
        .anchor(Align2::CENTER_TOP, Vec2::new(0.0, 16.0))
        .interactable(false)
        .show(&ctx, |ui_| {
            hud_frame()
                .inner_margin(egui::Margin::symmetric(18, 8))
                .stroke(if my_turn {
                    theme::stroke(2.0, color32(view.active_color))
                } else {
                    theme::stroke(1.0, Color32::from_white_alpha(18))
                })
                .show(ui_, |ui_| {
                    ui_.add(egui::Label::new(RichText::new(status).font(semibold(17.0)).color(if my_turn { theme::GOLD } else { theme::TEXT })).extend());
                });
        });

    let can_draw = my_turn && view.phase == Phase::Turn;
    let can_pass = my_turn && matches!(view.phase, Phase::AfterDraw { .. });
    let can_call = keys::can_call_last(&view, me);
    let catch_target = view.uno_vulnerable.filter(|t| *t != me);

    egui::Area::new(egui::Id::new("uno-actions"))
        .anchor(Align2::RIGHT_BOTTOM, Vec2::new(-20.0, -20.0))
        .show(&ctx, |ui_| {
            hud_frame().show(ui_, |ui_| {
                ui_.set_width(220.0);
                ui_.horizontal(|ui_| {
                    theme::avatar(ui_, &my.name, rgb(SEAT_COLORS[me % 8]), 34.0);
                    ui_.vertical(|ui_| {
                        ui_.spacing_mut().item_spacing.y = 0.0;
                        ui_.label(RichText::new(&my.name).font(semibold(15.0)));
                        ui_.label(RichText::new(format!("{} карт · {} очк.", my.hand.len(), my.score)).color(theme::TEXT_DIM));
                    });
                });
                ui_.add_space(6.0);
                let hint = |action: Action| format!("  [{}]", binds.label(action));
                if action_button(ui_, &format!("Взять карту{}", hint(Action::Draw)), theme::ACCENT, can_draw, 220.0).clicked() {
                    session.submit_uno(UnoAction::Draw);
                }
                if can_pass
                    && action_button(ui_, &format!("Оставить и пропустить{}", hint(Action::Pass)), theme::PANEL_HI, true, 220.0).clicked()
                {
                    session.submit_uno(UnoAction::Pass);
                }
                let armed = ui.uno_armed && my.hand.len() == 2;
                let label = if armed {
                    "✔ «Последняя!» готово".to_string()
                } else {
                    format!("«Последняя!»{}", hint(Action::LastCard))
                };
                let resp = action_button(ui_, &label, if armed { theme::GOOD } else { theme::GOLD }, can_call, 220.0);
                if resp.clicked() {
                    keys::call_last(&mut session, &mut ui, &view, me);
                }
                if let Some(t) = catch_target {
                    let text = format!("Поймать {}!{}", name(&view, t), hint(Action::Catch));
                    if action_button(ui_, &text, theme::BAD, true, 220.0).clicked() {
                        session.submit_uno(UnoAction::CatchUno { target: t });
                    }
                }
                ui_.add_space(2.0);
                ui_.label(
                    RichText::new("Клик по карте — сыграть, по колоде — взять. ⚙ — клавиши")
                        .font(theme::regular(11.5))
                        .color(theme::TEXT_DIM),
                );
            });
        });

    // ---- modal dialogs ----------------------------------------------------------------
    ui.modal_open = false;
    if let Phase::ChooseColor { seat, .. } = view.phase {
        if seat == me {
            ui.modal_open = true;
            modal(&ctx, "choose-color", |ui_| {
                ui_.label(RichText::new("Выберите цвет").font(bold(24.0)));
                ui_.add_space(8.0);
                ui_.horizontal(|ui_| {
                    for c in CardColor::ALL {
                        let count = my.hand.iter().filter(|x| x.color == Some(c)).count();
                        let (rect, resp) = ui_.allocate_exact_size(Vec2::new(96.0, 120.0), egui::Sense::click());
                        let hovered = resp.hovered();
                        let r = if hovered { rect.expand(3.0) } else { rect };
                        ui_.painter().rect_filled(r, 14, color32(c));
                        ui_.painter().rect_stroke(r, 14, theme::stroke(if hovered { 3.0 } else { 1.0 }, Color32::WHITE), StrokeKind::Inside);
                        ui_.painter().text(r.center() - Vec2::new(0.0, 10.0), Align2::CENTER_CENTER, c.name_ru(), semibold(15.0), Color32::WHITE);
                        ui_.painter().text(
                            r.center() + Vec2::new(0.0, 18.0),
                            Align2::CENTER_CENTER,
                            format!("у вас: {count}"),
                            theme::regular(12.0),
                            Color32::from_white_alpha(220),
                        );
                        let key_action = match c {
                            CardColor::Red => Action::Red,
                            CardColor::Yellow => Action::Yellow,
                            CardColor::Green => Action::Green,
                            CardColor::Blue => Action::Blue,
                        };
                        ui_.painter().text(
                            r.center() + Vec2::new(0.0, 36.0),
                            Align2::CENTER_CENTER,
                            binds.label(key_action),
                            theme::regular(12.0),
                            Color32::from_white_alpha(170),
                        );
                        if resp.on_hover_cursor(egui::CursorIcon::PointingHand).clicked() {
                            session.submit_uno(UnoAction::ChooseColor(c));
                        }
                    }
                });
            });
        }
    }
    if let Phase::Challenge { offender, victim, .. } = view.phase {
        if victim == me {
            ui.modal_open = true;
            modal(&ctx, "challenge", |ui_| {
                ui_.label(RichText::new("Вам «+4»").font(bold(24.0)));
                ui_.label(format!(
                    "{} сыграл «Возьми четыре». Если считаете, что у него была карта текущего цвета, — оспорьте.",
                    name(&view, offender)
                ));
                ui_.label(
                    RichText::new("Блеф раскрыт — он берёт 4, а вы ходите. Ошиблись — вы берёте 6 и пропускаете ход.")
                        .color(theme::TEXT_DIM),
                );
                ui_.add_space(8.0);
                ui_.horizontal(|ui_| {
                    if action_button(ui_, &format!("Оспорить [{}]", binds.label(Action::Challenge)), theme::BAD, true, 200.0).clicked() {
                        session.submit_uno(UnoAction::Challenge);
                    }
                    if action_button(ui_, &format!("Взять 4 карты [{}]", binds.label(Action::Accept)), theme::PANEL_HI, true, 200.0).clicked() {
                        session.submit_uno(UnoAction::AcceptDrawFour);
                    }
                });
            });
        }
    }
    let over = matches!(view.phase, Phase::RoundOver { .. } | Phase::GameOver { .. });
    if over || ui.show_scores {
        ui.modal_open = over;
        let (title, subtitle) = match view.phase {
            Phase::RoundOver { winner, points } => (
                format!("{} выигрывает раунд!", name(&view, winner)),
                format!("+{points} очков · играем до {}", view.target_score),
            ),
            Phase::GameOver { winner } => (
                format!("🏆 Победитель: {}", name(&view, winner)),
                "Игра окончена".to_string(),
            ),
            _ => ("Счёт".to_string(), format!("Раунд {}", view.round)),
        };
        modal(&ctx, "scores", |ui_| {
            ui_.label(RichText::new(title).font(bold(26.0)));
            ui_.label(RichText::new(subtitle).color(theme::TEXT_DIM));
            ui_.add_space(8.0);
            let mut order: Vec<usize> = (0..n).collect();
            order.sort_by_key(|i| std::cmp::Reverse(view.players[*i].score));
            egui::Grid::new("score-grid").striped(true).min_col_width(80.0).show(ui_, |ui_| {
                ui_.label(RichText::new("Игрок").color(theme::TEXT_DIM));
                ui_.label(RichText::new("Карт").color(theme::TEXT_DIM));
                ui_.label(RichText::new("Очки").color(theme::TEXT_DIM));
                ui_.end_row();
                for i in order {
                    let p = &view.players[i];
                    ui_.horizontal(|ui_| {
                        theme::avatar(ui_, &p.name, rgb(SEAT_COLORS[i % 8]), 22.0);
                        ui_.label(&p.name);
                    });
                    ui_.label(p.hand.len().to_string());
                    ui_.label(RichText::new(p.score.to_string()).font(semibold(15.0)));
                    ui_.end_row();
                }
            });
            ui_.add_space(10.0);
            ui_.horizontal(|ui_| match view.phase {
                Phase::RoundOver { .. } => {
                    if session.is_host {
                        if action_button(ui_, &format!("Следующий раунд [{}]", binds.label(Action::Confirm)), theme::GOOD, true, 220.0).clicked() {
                            if let Some(i) = session.my_index() {
                                session.host_apply_uno(i, UnoAction::NextRound);
                            }
                        }
                    } else {
                        ui_.label(RichText::new("Ждём, пока хост начнёт следующий раунд…").color(theme::TEXT_DIM));
                    }
                }
                Phase::GameOver { .. } => {
                    if session.is_host {
                        if action_button(ui_, "Сыграть ещё (в лобби)", theme::GOOD, true, 220.0).clicked() {
                            session.back_to_lobby();
                        }
                    } else {
                        ui_.label(RichText::new("Хост может начать новую игру").color(theme::TEXT_DIM));
                    }
                    if action_button(ui_, "В меню", theme::PANEL_HI, true, 140.0).clicked() {
                        ui.confirm_leave = true;
                    }
                }
                _ => {
                    if action_button(ui_, "Закрыть", theme::PANEL_HI, true, 140.0).clicked() {
                        ui.show_scores = false;
                    }
                }
            });
        });
    }

    if ui.show_settings {
        ui.modal_open = true;
        modal(&ctx, "uno-settings", |ui_| {
            ui_.set_width(460.0);
            ui_.label(RichText::new("Управление").font(bold(24.0)));
            ui_.label(
                RichText::new("Клавиши привязаны к положению, как на латинской раскладке QWERTY.")
                    .color(theme::TEXT_DIM),
            );
            ui_.add_space(8.0);
            binds_grid(ui_, "uno-binds", &binds, &mut ui.capture);
            ui_.add_space(8.0);
            ui_.label(
                RichText::new(
                    "Нажмите кнопку с клавишей, затем нужную клавишу. Esc — отмена. \
                     Если клавиша уже занята, действия меняются местами.",
                )
                .font(theme::regular(12.0))
                .color(theme::TEXT_DIM),
            );
            ui_.add_space(10.0);
            ui_.horizontal(|ui_| {
                if action_button(ui_, "По умолчанию", theme::PANEL_HI, true, 200.0).clicked() {
                    profile.data.uno_binds.reset();
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
        ui.modal_open = true;
        modal(&ctx, "confirm-leave", |ui_| {
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

