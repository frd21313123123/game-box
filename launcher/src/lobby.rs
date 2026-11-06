//! The lobby: seats, bots, pawn choice, opening the game to the network.

use bevy::prelude::*;
use bevy_egui::egui::{self, Align2, Color32, RichText, Stroke, Vec2};
use bevy_egui::{EguiContexts, EguiPrimaryContextPass};
use monopoly_core::{TOKENS, TOKEN_COLORS};

use crate::net::{GameKind, NetMode, SeatKind, Session, MAX_SEATS};
use crate::steam::SteamApi;
use crate::theme::{self, action_button, bold, rgb, semibold, SEAT_COLORS};
use crate::AppState;

pub struct LobbyPlugin;

impl Plugin for LobbyPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<LobbyUi>()
            .add_systems(EguiPrimaryContextPass, lobby_ui.run_if(in_state(AppState::Lobby)).run_if(resource_exists::<crate::ThemeReady>));
    }
}

#[derive(Resource, Default)]
struct LobbyUi {
    renaming: Option<(u32, String)>,
}

fn kind_badge(kind: SeatKind, me: bool) -> (&'static str, Color32) {
    match kind {
        _ if me => ("Вы", theme::ACCENT),
        SeatKind::Host => ("Хост", theme::GOLD),
        SeatKind::Remote => ("Игрок", theme::GOOD),
        SeatKind::Bot => ("Бот", theme::TEXT_DIM),
        SeatKind::Local => ("За этим ПК", theme::WARN),
    }
}

fn lobby_ui(
    mut contexts: EguiContexts,
    mut session: ResMut<Session>,
    mut lobby_ui: ResMut<LobbyUi>,
    steam: Res<SteamApi>,
    mut next: ResMut<NextState<AppState>>,
) -> Result {
    let ctx = contexts.ctx_mut()?.clone();
    if !session.active {
        next.set(AppState::Menu);
        return Ok(());
    }
    let game = session.lobby.game;
    let is_host = session.is_host;

    egui::TopBottomPanel::top("lobby-top")
        .frame(egui::Frame::new().fill(theme::BG).inner_margin(egui::Margin::symmetric(28, 18)))
        .show_separator_line(false)
        .show(&ctx, |ui| {
            ui.horizontal(|ui| {
                if ui.button("← Покинуть лобби").clicked() {
                    session.leave(&steam);
                    next.set(AppState::Menu);
                }
                ui.add_space(12.0);
                ui.label(RichText::new(format!("Лобби · {}", game.title())).font(bold(28.0)));
            });
        });

    egui::CentralPanel::default()
        .frame(egui::Frame::new().fill(theme::BG).inner_margin(egui::Margin::symmetric(28, 8)))
        .show(&ctx, |ui| {
            if session.connecting {
                ui.add_space(80.0);
                ui.vertical_centered(|ui| {
                    ui.spinner();
                    ui.label(RichText::new("Подключаемся к хосту…").font(semibold(18.0)));
                });
                return;
            }
            ui.columns(2, |cols| {
                // ---- seats ----------------------------------------------------
                let left = &mut cols[0];
                theme::card_frame().show(left, |ui| {
                    ui.set_min_height(560.0);
                    ui.horizontal(|ui| {
                        ui.label(RichText::new(format!("Игроки ({}/{MAX_SEATS})", session.lobby.seats.len())).font(bold(20.0)));
                    });
                    ui.add_space(6.0);
                    let seats = session.lobby.seats.clone();
                    let my = session.my_seat;
                    egui::ScrollArea::vertical().max_height(420.0).show(ui, |ui| {
                        for (i, seat) in seats.iter().enumerate() {
                            let me = seat.id == my;
                            egui::Frame::new()
                                .fill(if me { theme::PANEL_HI } else { theme::with_alpha(theme::PANEL_HI, 140) })
                                .corner_radius(12)
                                .inner_margin(egui::Margin::symmetric(12, 8))
                                .show(ui, |ui| {
                                    ui.horizontal(|ui| {
                                        let color = if game == GameKind::Monopoly {
                                            rgb(TOKEN_COLORS[seat.color])
                                        } else {
                                            rgb(SEAT_COLORS[i % 8])
                                        };
                                        theme::avatar(ui, &seat.name, color, 34.0);
                                        ui.vertical(|ui| {
                                            ui.spacing_mut().item_spacing.y = 2.0;
                                            ui.horizontal(|ui| {
                                                let editable = is_host && matches!(seat.kind, SeatKind::Local | SeatKind::Bot);
                                                match &mut lobby_ui.renaming {
                                                    Some((id, text)) if *id == seat.id => {
                                                        let r = ui.add(egui::TextEdit::singleline(text).desired_width(150.0).char_limit(20));
                                                        if r.lost_focus() {
                                                            let (id, text) = lobby_ui.renaming.take().unwrap();
                                                            session.rename_seat(id, &text);
                                                        }
                                                    }
                                                    _ => {
                                                        let label = ui.label(RichText::new(&seat.name).font(semibold(16.0)));
                                                        if editable && label.on_hover_text("Двойной клик — переименовать").double_clicked() {
                                                            lobby_ui.renaming = Some((seat.id, seat.name.clone()));
                                                        }
                                                    }
                                                }
                                                let (badge, c) = kind_badge(seat.kind, me);
                                                egui::Frame::new()
                                                    .fill(theme::with_alpha(c, 40))
                                                    .corner_radius(8)
                                                    .inner_margin(egui::Margin::symmetric(8, 2))
                                                    .show(ui, |ui| {
                                                        ui.label(RichText::new(badge).font(semibold(12.0)).color(c));
                                                    });
                                            });
                                            if game == GameKind::Monopoly {
                                                let t = &TOKENS[seat.token];
                                                ui.label(RichText::new(format!("{} {}", t.symbol, t.name)).color(theme::TEXT_DIM));
                                            }
                                        });
                                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                            if is_host && !me && ui.button("✖").on_hover_text("Убрать").clicked() {
                                                session.remove_seat(seat.id);
                                            }
                                        });
                                    });
                                    // Pawn and colour pickers (own seat; the host also sets bots and local players).
                                    let can_pick = me || (is_host && matches!(seat.kind, SeatKind::Bot | SeatKind::Local));
                                    if game == GameKind::Monopoly && can_pick {
                                        ui.add_space(4.0);
                                        token_picker(ui, &mut session, seat.id, seat.token, seat.color);
                                    }
                                });
                            ui.add_space(4.0);
                        }
                    });
                    if is_host {
                        ui.add_space(8.0);
                        ui.horizontal(|ui| {
                            let room = session.lobby.seats.len() < MAX_SEATS;
                            if action_button(ui, "+ Бот", theme::PANEL_HI, room, 140.0).clicked() {
                                session.add_bot();
                            }
                            if game == GameKind::Monopoly
                                && action_button(ui, "+ Игрок за этим ПК", theme::PANEL_HI, room, 200.0)
                                    .on_hover_text("Ещё один человек играет на этом же компьютере (по очереди)")
                                    .clicked()
                            {
                                session.add_local_player();
                            }
                        });
                    }
                });

                // ---- settings & network ------------------------------------------
                let right = &mut cols[1];
                theme::card_frame().show(right, |ui| {
                    ui.set_min_height(560.0);
                    ui.label(RichText::new("Настройки").font(bold(20.0)));
                    ui.add_space(6.0);
                    match game {
                        GameKind::Uno => {
                            ui.label("Длительность матча");
                            ui.horizontal(|ui| {
                                for (label, value) in [("Одна партия", 0), ("До 200 очков", 200), ("До 500 очков", 500)] {
                                    let selected = session.lobby.uno_target == value;
                                    let r = ui.add_enabled(is_host, egui::Button::new(label).selected(selected));
                                    if r.clicked() {
                                        session.set_uno_target(value);
                                    }
                                }
                            });
                            ui.add_space(4.0);
                            ui.label(
                                RichText::new(
                                    "Правила: цвет или значение, «Пропуск», «Смена направления», «+2», «Заказ цвета», \
                                     «+4» с правом оспорить, штраф +2 за забытое «Последняя!».",
                                )
                                .color(theme::TEXT_DIM),
                            );
                        }
                        GameKind::Monopoly => {
                            ui.label(RichText::new("Классические правила: ₽1500 на старте, ₽200 за круг, аукционы, торговля, залог, тюрьма.").color(theme::TEXT_DIM));
                        }
                    }

                    ui.add_space(16.0);
                    ui.separator();
                    ui.label(RichText::new("Сетевая игра").font(bold(20.0)));
                    ui.add_space(4.0);
                    match session.mode {
                        NetMode::Offline => {
                            ui.label(RichText::new("Сейчас игра только на этом компьютере. Откройте её, чтобы позвать друзей.").color(theme::TEXT_DIM));
                            ui.add_space(6.0);
                            if steam.available() {
                                if action_button(ui, "Пригласить друзей Steam", theme::ACCENT, true, 320.0).clicked() {
                                    match session.open_steam(&steam) {
                                        Ok(()) => steam.open_invite_overlay(),
                                        Err(e) => session.notify(e),
                                    }
                                }
                                ui.add_space(4.0);
                            }
                            if action_button(ui, "Открыть по локальной сети / IP", theme::PANEL_HI, true, 320.0).clicked() {
                                if let Err(e) = session.open_tcp() {
                                    session.notify(e);
                                }
                            }
                        }
                        NetMode::TcpHost | NetMode::SteamHost => {
                            ui.label(RichText::new("Игра открыта").font(semibold(16.0)).color(theme::GOOD));
                            if let Some(hint) = session.lobby.join_hint.clone() {
                                ui.horizontal(|ui| {
                                    ui.label("Адрес для друзей:");
                                    ui.label(RichText::new(&hint).font(semibold(16.0)).color(theme::GOLD));
                                    if session.mode == NetMode::TcpHost && ui.button("📋").on_hover_text("Скопировать").clicked() {
                                        ui.ctx().copy_text(hint.clone());
                                    }
                                });
                            }
                            if session.mode == NetMode::TcpHost {
                                ui.label(
                                    RichText::new("Через интернет: пробросьте TCP-порт 27015 на роутере и дайте друзьям внешний IP.")
                                        .color(theme::TEXT_DIM),
                                );
                            }
                            if session.mode == NetMode::SteamHost && ui.button("Пригласить ещё друзей").clicked() {
                                steam.open_invite_overlay();
                            }
                        }
                        NetMode::TcpClient | NetMode::SteamClient => {
                            ui.label(RichText::new("Вы подключены к игре хоста").color(theme::GOOD));
                        }
                    }

                    ui.add_space(24.0);
                    ui.separator();
                    ui.add_space(8.0);
                    if is_host {
                        let enough = session.lobby.seats.len() >= 2;
                        if action_button(ui, "▶  Начать игру", theme::GOOD, enough, 320.0).clicked() {
                            if let Err(e) = session.start_game() {
                                session.notify(e);
                            }
                        }
                        if !enough {
                            ui.colored_label(theme::WARN, "Нужно минимум 2 игрока — добавьте бота.");
                        }
                    } else {
                        ui.horizontal(|ui| {
                            ui.spinner();
                            ui.label(RichText::new("Ждём, пока хост начнёт игру…").font(semibold(16.0)));
                        });
                    }
                });
            });
        });
    let _ = Align2::CENTER_CENTER;
    let _ = Vec2::ZERO;
    let _ = Stroke::NONE;
    Ok(())
}

fn token_picker(ui: &mut egui::Ui, session: &mut Session, seat_id: u32, token: usize, color: usize) {
    let taken_tokens: Vec<usize> = session.lobby.seats.iter().filter(|s| s.id != seat_id).map(|s| s.token).collect();
    let taken_colors: Vec<usize> = session.lobby.seats.iter().filter(|s| s.id != seat_id).map(|s| s.color).collect();
    ui.horizontal_wrapped(|ui| {
        for (i, t) in TOKENS.iter().enumerate() {
            let enabled = !taken_tokens.contains(&i);
            let r = ui
                .add_enabled(enabled, egui::Button::new(RichText::new(t.symbol).size(18.0)).selected(i == token).min_size(Vec2::new(34.0, 30.0)))
                .on_hover_text(t.name);
            if r.clicked() {
                session.set_token(seat_id, i);
            }
        }
    });
    ui.horizontal(|ui| {
        for (i, c) in TOKEN_COLORS.iter().enumerate() {
            let enabled = !taken_colors.contains(&i);
            let (rect, resp) = ui.allocate_exact_size(Vec2::splat(24.0), if enabled { egui::Sense::click() } else { egui::Sense::hover() });
            let fill = if enabled { rgb(*c) } else { theme::with_alpha(rgb(*c), 50) };
            ui.painter().circle_filled(rect.center(), 10.0, fill);
            if i == color {
                ui.painter().circle_stroke(rect.center(), 11.5, theme::stroke(2.0, Color32::WHITE));
            }
            if resp.clicked() {
                session.set_color(seat_id, i);
            }
        }
    });
}
