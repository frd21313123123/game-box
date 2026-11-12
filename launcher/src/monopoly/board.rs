//! The property card shown when the mouse is over a tile of the board.

use bevy_egui::egui::{self, Align2, Color32, RichText, Sense, Vec2};
use monopoly_core::{mortgage_value, tile, unmortgage_cost, GameState, TileKind};

use crate::theme::{self, bold, rgb, semibold, stroke};

const CREAM: Color32 = Color32::from_rgb(0xfb, 0xf8, 0xef);
const INK: Color32 = Color32::from_rgb(0x1a, 0x1d, 0x22);

/// Property card shown when hovering a tile.
pub fn tile_tooltip(ctx: &egui::Context, state: &GameState, i: usize) {
    let t = tile(i);
    // Open towards the middle of the screen, so the card never runs off an
    // edge (over the bottom row it used to be cut off).
    let pointer = ctx.pointer_hover_pos().unwrap_or_default();
    let screen = ctx.screen_rect();
    let right = pointer.x > screen.center().x;
    let below = pointer.y > screen.center().y;
    let pivot = match (right, below) {
        (false, false) => Align2::LEFT_TOP,
        (true, false) => Align2::RIGHT_TOP,
        (false, true) => Align2::LEFT_BOTTOM,
        (true, true) => Align2::RIGHT_BOTTOM,
    };
    let offset = Vec2::new(if right { -18.0 } else { 18.0 }, if below { -18.0 } else { 18.0 });
    crate::theme::show_area(ctx, egui::Area::new(egui::Id::new("tile-tip"))
        .order(egui::Order::Tooltip)
        .fixed_pos(pointer + offset)
        .pivot(pivot)
        .constrain(true)
        .interactable(false), |ui| {
            egui::Frame::new()
                .fill(CREAM)
                .stroke(stroke(1.0, INK))
                .corner_radius(10)
                .inner_margin(egui::Margin::same(12))
                .show(ui, |ui| {
                    ui.set_width(230.0);
                    ui.visuals_mut().override_text_color = Some(INK);
                    if let Some(g) = t.group() {
                        let (rect, _) = ui.allocate_exact_size(Vec2::new(230.0, 34.0), Sense::hover());
                        ui.painter().rect_filled(rect, 6, rgb(g.rgb()));
                        ui.painter().text(rect.center(), Align2::CENTER_CENTER, t.name.to_uppercase(), bold(15.0), if theme::is_light(rgb(g.rgb())) { INK } else { Color32::WHITE });
                    } else {
                        ui.label(RichText::new(t.name).font(bold(17.0)));
                    }
                    let row = |ui: &mut egui::Ui, k: &str, v: String| {
                        ui.horizontal(|ui| {
                            ui.label(k);
                            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                ui.label(RichText::new(v).font(semibold(14.0)));
                            });
                        });
                    };
                    match t.kind {
                        TileKind::Street { group, price, rent } => {
                            row(ui, "Цена", format!("₽{price}"));
                            row(ui, "Рента", format!("₽{}", rent.base));
                            row(ui, "С монополией", format!("₽{}", rent.mono));
                            for (k, v) in rent.houses.iter().enumerate() {
                                row(ui, &format!("С {} дом.", k + 1), format!("₽{v}"));
                            }
                            row(ui, "С отелем", format!("₽{}", rent.hotel));
                            row(ui, "Дом / отель", format!("₽{}", group.house_cost()));
                        }
                        TileKind::Station { price } => {
                            row(ui, "Цена", format!("₽{price}"));
                            for n in 1..=4 {
                                row(ui, &format!("{n} вокзал(а)"), format!("₽{}", 25 * 2i32.pow(n - 1)));
                            }
                        }
                        TileKind::Utility { price } => {
                            row(ui, "Цена", format!("₽{price}"));
                            ui.label("1 предприятие: 4× кубики");
                            ui.label("2 предприятия: 10× кубики");
                        }
                        TileKind::Tax { amount } => {
                            ui.label(format!("Налог: ₽{amount}"));
                        }
                        TileKind::Chance => {
                            ui.label("Вытяните карту «Шанс»");
                        }
                        TileKind::Chest => {
                            ui.label("Вытяните карту «Общественная казна»");
                        }
                        TileKind::Go => {
                            ui.label("Пройдите СТАРТ — получите ₽200");
                        }
                        TileKind::Jail => {
                            ui.label("Тюрьма / просто посещение");
                        }
                        TileKind::FreeParking => {
                            ui.label("Ничего не происходит");
                        }
                        TileKind::GoToJail => {
                            ui.label("Отправляйтесь в тюрьму");
                        }
                    }
                    if t.is_purchasable() {
                        row(ui, "Залог", format!("₽{}", mortgage_value(i)));
                        row(ui, "Выкуп залога", format!("₽{}", unmortgage_cost(i)));
                        ui.separator();
                        match state.owner_of(i) {
                            Some(o) => {
                                ui.label(RichText::new(format!("Владелец: {} {}", o.symbol(), o.name)).font(semibold(14.0)));
                                if state.is_mortgaged(i) {
                                    ui.label(RichText::new("ЗАЛОЖЕНО").color(Color32::from_rgb(0xc0, 0x40, 0x20)));
                                } else if let Some((_, rent)) = state.compute_rent(i, 7) {
                                    let note = if matches!(t.kind, TileKind::Utility { .. }) { " (при 7 на кубиках)" } else { "" };
                                    ui.label(format!("Текущая рента: ₽{rent}{note}"));
                                }
                            }
                            None => {
                                ui.label("Не куплено — можно приобрести");
                            }
                        }
                    }
                });
        });
}
