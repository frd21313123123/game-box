//! Main menu: profile, the two game cards, join dialog; plus global toasts.

use bevy::prelude::*;
use bevy_egui::egui::{
    self, emath::Rot2, epaint::TextShape, Align2, Color32, Pos2, Rect, RichText, Shape, Stroke, StrokeKind,
    Vec2,
};
use bevy_egui::{EguiContexts, EguiPrimaryContextPass};
use monopoly_core::{ColorGroup, BOARD};

use crate::net::{GameKind, Session};
use crate::profile::Profile;
use crate::sfx::Sfx;
use crate::steam::SteamApi;
use crate::theme::{self, action_button, bold, rgb, semibold};
use crate::AppState;

pub struct MenuPlugin;

impl Plugin for MenuPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<MenuUi>()
            .init_resource::<Toasts>()
            .add_systems(EguiPrimaryContextPass, (menu_ui.run_if(in_state(AppState::Menu)), toasts_ui).chain().run_if(resource_exists::<crate::ThemeReady>));
    }
}

#[derive(Resource, Default)]
struct MenuUi {
    join: Option<GameKind>,
    address: String,
    editing_name: Option<String>,
    error: Option<String>,
}

#[derive(Resource, Default)]
struct Toasts(Vec<(String, f32)>);

fn toasts_ui(
    mut contexts: EguiContexts,
    mut session: ResMut<Session>,
    mut toasts: ResMut<Toasts>,
    time: Res<Time>,
) -> Result {
    let ctx = contexts.ctx_mut()?;
    for n in session.notices.drain(..) {
        toasts.0.push((n, 0.0));
    }
    for t in toasts.0.iter_mut() {
        t.1 += time.delta_secs();
    }
    toasts.0.retain(|t| t.1 < 4.0);
    if toasts.0.len() > 4 {
        let excess = toasts.0.len() - 4;
        toasts.0.drain(..excess);
    }
    if toasts.0.is_empty() {
        return Ok(());
    }
    egui::Area::new(egui::Id::new("toasts"))
        .anchor(Align2::CENTER_BOTTOM, Vec2::new(0.0, -150.0))
        .order(egui::Order::Tooltip)
        .interactable(false)
        .show(ctx, |ui| {
            for (text, age) in &toasts.0 {
                let alpha = ((4.0 - age) / 0.5).clamp(0.0, 1.0);
                egui::Frame::new()
                    .fill(Color32::from_rgba_unmultiplied(28, 31, 38, (240.0 * alpha) as u8))
                    .stroke(theme::stroke(1.0, theme::with_alpha(theme::ACCENT, (160.0 * alpha) as u8)))
                    .corner_radius(10)
                    .inner_margin(egui::Margin::symmetric(16, 8))
                    .show(ui, |ui| {
                        ui.label(RichText::new(text).font(semibold(15.0)).color(theme::with_alpha(theme::TEXT, (255.0 * alpha) as u8)));
                    });
            }
        });
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn menu_ui(
    mut contexts: EguiContexts,
    mut menu: ResMut<MenuUi>,
    mut profile: ResMut<Profile>,
    mut session: ResMut<Session>,
    steam: Res<SteamApi>,
    mut next: ResMut<NextState<AppState>>,
    mut sfx: EventWriter<Sfx>,
    mut exit: EventWriter<AppExit>,
) -> Result {
    let ctx = contexts.ctx_mut()?.clone();
    egui::CentralPanel::default()
        .frame(egui::Frame::new().fill(theme::BG))
        .show(&ctx, |ui| {
            let full = ui.max_rect();
            paint_backdrop(ui.painter(), full);

            // Header.
            ui.add_space(18.0);
            ui.horizontal(|ui| {
                ui.add_space(28.0);
                ui.vertical(|ui| {
                    ui.label(RichText::new("GAME BOX").font(bold(40.0)).color(Color32::WHITE));
                    ui.label(RichText::new("Настольные игры с друзьями и ботами").font(theme::regular(16.0)).color(theme::TEXT_DIM));
                });
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.add_space(28.0);
                    if ui.button("Выход").clicked() {
                        exit.write(AppExit::Success);
                    }
                    let sound = if profile.data.sound { "🔊 Звук" } else { "🔇 Звук" };
                    if ui.button(sound).clicked() {
                        profile.data.sound = !profile.data.sound;
                        profile.save();
                    }
                    profile_chip(ui, &mut profile, &mut menu, steam.available());
                });
            });

            // Game cards.
            let card_w = 430.0;
            let card_h = 520.0;
            let gap = 40.0;
            let total = card_w * 2.0 + gap;
            let top = full.top() + 150.0;
            let left = full.center().x - total / 2.0;
            for (i, game) in [GameKind::Monopoly, GameKind::Uno].into_iter().enumerate() {
                let rect = Rect::from_min_size(Pos2::new(left + i as f32 * (card_w + gap), top), Vec2::new(card_w, card_h));
                let mut child = ui.new_child(egui::UiBuilder::new().max_rect(rect));
                game_card(&mut child, game, rect, &mut session, &profile, &mut menu, &mut next, &mut sfx);
            }

            // Footer.
            let footer = if steam.available() {
                "Steam подключён: ники берутся из Steam, друзей можно звать через оверлей (Shift+Tab)."
            } else {
                "Играйте с ботами, по локальной сети или через интернет по IP. Соберите с функцией «steam», чтобы играть через Steam."
            };
            ui.painter().text(
                Pos2::new(full.center().x, full.bottom() - 26.0),
                Align2::CENTER_CENTER,
                footer,
                theme::regular(13.0),
                theme::TEXT_DIM,
            );
        });

    // Join dialog.
    if let Some(game) = menu.join {
        let mut open = true;
        egui::Window::new(format!("Присоединиться — {}", game.title()))
            .collapsible(false)
            .resizable(false)
            .anchor(Align2::CENTER_CENTER, Vec2::ZERO)
            .open(&mut open)
            .show(&ctx, |ui| {
                ui.set_width(420.0);
                ui.label("Адрес хоста (IP или IP:порт). Хост видит его в своём лобби.");
                let resp = ui.add(
                    egui::TextEdit::singleline(&mut menu.address)
                        .hint_text("например 192.168.1.10:27015")
                        .desired_width(f32::INFINITY),
                );
                if let Some(err) = &menu.error {
                    ui.colored_label(theme::BAD, err);
                }
                let enter = resp.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
                ui.add_space(6.0);
                if action_button(ui, "Подключиться", theme::ACCENT, !menu.address.trim().is_empty(), 420.0).clicked() || enter {
                    let address = menu.address.trim().to_string();
                    match session.join_tcp(&address, game, &profile) {
                        Ok(()) => {
                            profile.data.last_address = address;
                            profile.save();
                            menu.join = None;
                            menu.error = None;
                            next.set(AppState::Lobby);
                        }
                        Err(e) => menu.error = Some(e),
                    }
                }
                if steam.available() {
                    ui.add_space(4.0);
                    ui.label(
                        RichText::new("Через Steam: примите приглашение друга или нажмите «Присоединиться к игре» в списке друзей.")
                            .color(theme::TEXT_DIM),
                    );
                }
            });
        if !open {
            menu.join = None;
            menu.error = None;
        }
    }

    // Nickname editor.
    if let Some(mut name) = menu.editing_name.take() {
        let mut keep = true;
        egui::Window::new("Ваш ник")
            .collapsible(false)
            .resizable(false)
            .anchor(Align2::CENTER_CENTER, Vec2::ZERO)
            .show(&ctx, |ui| {
                ui.set_width(320.0);
                let resp = ui.add(egui::TextEdit::singleline(&mut name).char_limit(20).desired_width(f32::INFINITY));
                resp.request_focus();
                let enter = ui.input(|i| i.key_pressed(egui::Key::Enter));
                ui.horizontal(|ui| {
                    if ui.button("Сохранить").clicked() || enter {
                        let trimmed = name.trim();
                        if !trimmed.is_empty() {
                            profile.data.name = trimmed.to_string();
                            profile.save();
                        }
                        keep = false;
                    }
                    if ui.button("Отмена").clicked() {
                        keep = false;
                    }
                });
            });
        if keep {
            menu.editing_name = Some(name);
        }
    }
    Ok(())
}

fn profile_chip(ui: &mut egui::Ui, profile: &mut Profile, menu: &mut MenuUi, steam: bool) {
    let name = profile.display_name();
    egui::Frame::new()
        .fill(theme::PANEL)
        .stroke(theme::stroke(1.0, theme::BORDER))
        .corner_radius(24)
        .inner_margin(egui::Margin { left: 6, right: 14, top: 5, bottom: 5 })
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                theme::avatar(ui, &name, theme::ACCENT, 32.0);
                ui.vertical(|ui| {
                    ui.spacing_mut().item_spacing.y = 0.0;
                    ui.label(RichText::new(&name).font(semibold(15.0)));
                    if steam && profile.steam_name.is_some() {
                        ui.label(RichText::new("ник из Steam").font(theme::regular(11.0)).color(theme::GOOD));
                    } else if ui
                        .add(egui::Label::new(RichText::new("изменить ник").font(theme::regular(11.0)).color(theme::ACCENT)).sense(egui::Sense::click()))
                        .on_hover_cursor(egui::CursorIcon::PointingHand)
                        .clicked()
                    {
                        menu.editing_name = Some(profile.data.name.clone());
                    }
                });
            });
        });
}

#[allow(clippy::too_many_arguments)]
fn game_card(
    ui: &mut egui::Ui,
    game: GameKind,
    rect: Rect,
    session: &mut Session,
    profile: &Profile,
    menu: &mut MenuUi,
    next: &mut NextState<AppState>,
    sfx: &mut EventWriter<Sfx>,
) {
    let painter = ui.painter().clone();
    let hovered = ui.rect_contains_pointer(rect);
    painter.rect_filled(rect.translate(Vec2::new(0.0, 8.0)), 22, Color32::from_black_alpha(90));
    painter.rect_filled(rect, 22, theme::PANEL);
    painter.rect_stroke(
        rect,
        22,
        theme::stroke(if hovered { 2.0 } else { 1.0 }, if hovered { theme::ACCENT } else { theme::BORDER }),
        StrokeKind::Inside,
    );
    let art = Rect::from_min_size(rect.min + Vec2::splat(16.0), Vec2::new(rect.width() - 32.0, 250.0));
    match game {
        GameKind::Monopoly => paint_monopoly_art(&painter, art),
        GameKind::Uno => paint_cards_art(&painter, art, ui.input(|i| i.time) as f32),
    }

    let mut y = art.bottom() + 18.0;
    painter.text(Pos2::new(rect.left() + 24.0, y), Align2::LEFT_TOP, game.title(), bold(28.0), Color32::WHITE);
    y += 40.0;
    let (desc, players) = match game {
        GameKind::Monopoly => (
            "Покупайте улицы Москвы, стройте дома и отели,\nторгуйтесь и разоряйте соперников.",
            "2–8 игроков · 2D-поле",
        ),
        GameKind::Uno => (
            "Сбрасывайте карты по цвету или значению,\nне забудьте крикнуть «Последняя!».",
            "2–8 игроков · 3D-стол",
        ),
    };
    painter.text(Pos2::new(rect.left() + 24.0, y), Align2::LEFT_TOP, desc, theme::regular(15.0), theme::TEXT_DIM);
    y += 50.0;
    painter.text(Pos2::new(rect.left() + 24.0, y), Align2::LEFT_TOP, players, semibold(13.0), theme::ACCENT);

    let buttons = Rect::from_min_size(Pos2::new(rect.left() + 24.0, rect.bottom() - 70.0), Vec2::new(rect.width() - 48.0, 46.0));
    let mut b = ui.new_child(egui::UiBuilder::new().max_rect(buttons).layout(egui::Layout::left_to_right(egui::Align::Center)));
    let w = (buttons.width() - 10.0) / 2.0;
    if action_button(&mut b, "Создать игру", theme::GOOD, true, w).clicked() {
        sfx.write(Sfx::Click);
        session.host(game, profile);
        next.set(AppState::Lobby);
    }
    if action_button(&mut b, "Присоединиться", theme::PANEL_HI, true, w).clicked() {
        sfx.write(Sfx::Click);
        menu.join = Some(game);
        if menu.address.is_empty() {
            menu.address = profile.data.last_address.clone();
        }
    }
}

fn paint_backdrop(painter: &egui::Painter, rect: Rect) {
    // Soft coloured glows.
    for (c, pos, r) in [
        (Color32::from_rgba_unmultiplied(64, 132, 246, 14), Pos2::new(rect.left() + 200.0, rect.top() + 120.0), 420.0),
        (Color32::from_rgba_unmultiplied(232, 93, 79, 10), Pos2::new(rect.right() - 160.0, rect.bottom() - 120.0), 460.0),
    ] {
        for k in 0..6 {
            painter.circle_filled(pos, r * (1.0 - k as f32 * 0.14), c);
        }
    }
}

fn group_color(g: ColorGroup) -> Color32 {
    rgb(g.rgb())
}

/// A tiny Monopoly board.
fn paint_monopoly_art(painter: &egui::Painter, rect: Rect) {
    painter.rect_filled(rect, 16, Color32::from_rgb(20, 40, 30));
    let size = rect.height() - 24.0;
    let board = Rect::from_center_size(rect.center(), Vec2::splat(size));
    painter.rect_filled(board, 6, Color32::from_rgb(207, 230, 210));
    let cell = size / 11.0;
    for tile in BOARD.iter() {
        let (cx, cy) = match tile.index {
            0..=10 => (10 - tile.index, 10),
            11..=20 => (0, 10 - (tile.index - 10)),
            21..=30 => (tile.index - 20, 0),
            _ => (10, tile.index - 30),
        };
        let r = Rect::from_min_size(board.min + Vec2::new(cx as f32 * cell, cy as f32 * cell), Vec2::splat(cell)).shrink(0.6);
        painter.rect_filled(r, 1, Color32::from_rgb(251, 248, 239));
        if let Some(g) = tile.group() {
            let stripe = match tile.index {
                1..=9 => Rect::from_min_size(r.min, Vec2::new(r.width(), r.height() * 0.3)),
                11..=19 => Rect::from_min_size(Pos2::new(r.right() - r.width() * 0.3, r.top()), Vec2::new(r.width() * 0.3, r.height())),
                21..=29 => Rect::from_min_size(Pos2::new(r.left(), r.bottom() - r.height() * 0.3), Vec2::new(r.width(), r.height() * 0.3)),
                _ => Rect::from_min_size(r.min, Vec2::new(r.width() * 0.3, r.height())),
            };
            painter.rect_filled(stripe, 0, group_color(g));
        }
    }
    let galley = painter.layout_no_wrap("МОНОПОЛИЯ".into(), bold(size * 0.1), Color32::from_rgb(180, 30, 30));
    let angle = -std::f32::consts::FRAC_PI_4;
    let half = galley.size() / 2.0;
    let offset = Rot2::from_angle(angle) * half;
    painter.add(Shape::Text(TextShape::new(board.center() - offset, galley, Color32::RED).with_angle(angle)));
    // Pawns.
    for (i, c) in [[0xe6, 0x19, 0x4b], [0x43, 0x63, 0xd8], [0x3c, 0xb4, 0x4b]].iter().enumerate() {
        let p = board.min + Vec2::new(cell * (3.5 + i as f32 * 2.2), size - cell * 0.5);
        painter.circle_filled(p + Vec2::new(1.0, 2.0), cell * 0.32, Color32::from_black_alpha(80));
        painter.circle_filled(p, cell * 0.32, rgb(*c));
        painter.circle_stroke(p, cell * 0.32, theme::stroke(1.5, Color32::WHITE));
    }
}

/// A fan of cards that gently sways.
fn paint_cards_art(painter: &egui::Painter, rect: Rect, time: f32) {
    painter.rect_filled(rect, 16, Color32::from_rgb(18, 70, 48));
    let colors = [
        [0xE5, 0x2D, 0x27],
        [0xF7, 0xC3, 0x1A],
        [0x2F, 0xA8, 0x4F],
        [0x1F, 0x6F, 0xD6],
        [0x2A, 0x2D, 0x36],
    ];
    let labels = ["7", "+2", "3", "⟲", "+4"];
    let pivot = Pos2::new(rect.center().x, rect.bottom() + 60.0);
    for i in 0..5 {
        let a = (i as f32 - 2.0) * 0.26 + (time * 0.8).sin() * 0.02;
        let size = Vec2::new(96.0, 144.0);
        let center = pivot + Rot2::from_angle(a) * Vec2::new(0.0, -200.0);
        paint_card(painter, center, size, a, rgb(colors[i]), labels[i]);
    }
}

fn paint_card(painter: &egui::Painter, center: Pos2, size: Vec2, angle: f32, color: Color32, label: &str) {
    let rot = Rot2::from_angle(angle);
    let corners = |s: Vec2| -> Vec<Pos2> {
        [(-1.0, -1.0), (1.0, -1.0), (1.0, 1.0), (-1.0, 1.0)]
            .iter()
            .map(|(x, y)| center + rot * Vec2::new(x * s.x / 2.0, y * s.y / 2.0))
            .collect()
    };
    painter.add(Shape::convex_polygon(
        corners(size + Vec2::splat(4.0)).iter().map(|p| *p + Vec2::new(3.0, 5.0)).collect(),
        Color32::from_black_alpha(70),
        Stroke::NONE,
    ));
    painter.add(Shape::convex_polygon(corners(size), Color32::from_rgb(251, 247, 238), Stroke::NONE));
    painter.add(Shape::convex_polygon(corners(size - Vec2::splat(10.0)), color, Stroke::NONE));
    painter.circle_filled(center, size.x * 0.32, Color32::from_rgb(251, 247, 238));
    let galley = painter.layout_no_wrap(label.into(), bold(30.0), color);
    let half = galley.size() / 2.0;
    painter.add(Shape::Text(
        TextShape::new(center - rot * half, galley, color).with_angle(angle),
    ));
}
