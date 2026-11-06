//! Shared look: fonts, colours and a few custom egui widgets.

use std::sync::Arc;

use bevy::color::Color;
use bevy_egui::egui::{
    self, epaint::Shadow, Align2, Color32, CornerRadius, FontData, FontDefinitions, FontFamily,
    FontId, Margin, Pos2, Rect, Response, Sense, Stroke, StrokeKind, Ui, Vec2,
};
use bevy_egui::egui::emath::TSTransform;

pub const BACKGROUND_3D: Color = Color::srgb(0.055, 0.06, 0.075);

pub const BG: Color32 = Color32::from_rgb(14, 16, 20);
pub const PANEL: Color32 = Color32::from_rgb(24, 27, 33);
pub const PANEL_HI: Color32 = Color32::from_rgb(34, 38, 46);
pub const BORDER: Color32 = Color32::from_rgb(48, 53, 63);
pub const TEXT: Color32 = Color32::from_rgb(232, 234, 237);
pub const TEXT_DIM: Color32 = Color32::from_rgb(150, 156, 166);
pub const ACCENT: Color32 = Color32::from_rgb(64, 132, 246);
pub const GOOD: Color32 = Color32::from_rgb(52, 178, 102);
pub const WARN: Color32 = Color32::from_rgb(240, 170, 40);
pub const BAD: Color32 = Color32::from_rgb(220, 70, 70);
pub const GOLD: Color32 = Color32::from_rgb(255, 214, 64);

const FONT_REGULAR: &[u8] = include_bytes!("../../assets/fonts/Inter-Regular.otf");
const FONT_SEMIBOLD: &[u8] = include_bytes!("../../assets/fonts/Inter-SemiBold.otf");
const FONT_BOLD: &[u8] = include_bytes!("../../assets/fonts/Inter-ExtraBold.otf");

pub fn rgb(c: [u8; 3]) -> Color32 {
    Color32::from_rgb(c[0], c[1], c[2])
}

pub fn semibold(size: f32) -> FontId {
    FontId::new(size, FontFamily::Name("semibold".into()))
}

pub fn bold(size: f32) -> FontId {
    FontId::new(size, FontFamily::Name("bold".into()))
}

pub fn regular(size: f32) -> FontId {
    FontId::proportional(size)
}

pub fn install(ctx: &egui::Context) {
    let mut fonts = FontDefinitions::default();
    fonts
        .font_data
        .insert("inter".into(), Arc::new(FontData::from_static(FONT_REGULAR)));
    fonts
        .font_data
        .insert("inter-semibold".into(), Arc::new(FontData::from_static(FONT_SEMIBOLD)));
    fonts
        .font_data
        .insert("inter-bold".into(), Arc::new(FontData::from_static(FONT_BOLD)));

    // Keep egui's bundled emoji/symbol fonts as fallbacks after Inter.
    let fallbacks = fonts
        .families
        .get(&FontFamily::Proportional)
        .cloned()
        .unwrap_or_default();
    fonts
        .families
        .entry(FontFamily::Proportional)
        .or_default()
        .insert(0, "inter".into());
    let mut semi = vec!["inter-semibold".to_string()];
    semi.extend(fallbacks.iter().cloned());
    fonts.families.insert(FontFamily::Name("semibold".into()), semi);
    let mut bold_family = vec!["inter-bold".to_string()];
    bold_family.extend(fallbacks.iter().cloned());
    fonts.families.insert(FontFamily::Name("bold".into()), bold_family);
    ctx.set_fonts(fonts);

    ctx.style_mut(|style| {
        use egui::TextStyle::*;
        style.text_styles = [
            (Small, regular(12.0)),
            (Body, regular(15.0)),
            (Monospace, FontId::monospace(14.0)),
            (Button, semibold(15.0)),
            (Heading, bold(24.0)),
        ]
        .into();
        style.spacing.item_spacing = Vec2::new(8.0, 8.0);
        style.spacing.button_padding = Vec2::new(14.0, 7.0);
        style.spacing.interact_size.y = 30.0;
        let v = &mut style.visuals;
        *v = egui::Visuals::dark();
        v.override_text_color = Some(TEXT);
        v.panel_fill = PANEL;
        v.window_fill = PANEL;
        v.extreme_bg_color = Color32::from_rgb(12, 14, 17);
        v.faint_bg_color = PANEL_HI;
        v.window_stroke = Stroke::new(1.0_f32, BORDER);
        v.window_corner_radius = CornerRadius::same(14);
        v.window_shadow = Shadow {
            offset: [0, 10],
            blur: 30,
            spread: 0,
            color: Color32::from_black_alpha(140),
        };
        v.selection.bg_fill = ACCENT;
        for w in [
            &mut v.widgets.inactive,
            &mut v.widgets.hovered,
            &mut v.widgets.active,
            &mut v.widgets.open,
            &mut v.widgets.noninteractive,
        ] {
            w.corner_radius = CornerRadius::same(9);
        }
        v.widgets.inactive.weak_bg_fill = PANEL_HI;
        v.widgets.inactive.bg_fill = PANEL_HI;
        v.widgets.hovered.weak_bg_fill = Color32::from_rgb(46, 52, 63);
        v.widgets.hovered.bg_stroke = Stroke::new(1.0_f32, Color32::from_rgb(80, 90, 108));
        v.widgets.active.weak_bg_fill = Color32::from_rgb(56, 64, 78);
        v.widgets.noninteractive.bg_stroke = Stroke::new(1.0_f32, BORDER);
    });
}

/// Translucent rounded panel used for HUD overlays.
pub fn hud_frame() -> egui::Frame {
    egui::Frame::new()
        .fill(Color32::from_rgba_unmultiplied(18, 20, 25, 225))
        .stroke(Stroke::new(1.0_f32, Color32::from_rgba_unmultiplied(255, 255, 255, 18)))
        .corner_radius(CornerRadius::same(14))
        .inner_margin(Margin::same(14))
        .shadow(Shadow {
            offset: [0, 6],
            blur: 24,
            spread: 0,
            color: Color32::from_black_alpha(110),
        })
}

pub fn card_frame() -> egui::Frame {
    egui::Frame::new()
        .fill(PANEL)
        .stroke(Stroke::new(1.0_f32, BORDER))
        .corner_radius(CornerRadius::same(14))
        .inner_margin(Margin::same(16))
}

/// A large coloured action button. Disabled buttons are drawn greyed out.
pub fn action_button(ui: &mut Ui, text: &str, color: Color32, enabled: bool, width: f32) -> Response {
    let height = 44.0;
    let (rect, resp) = ui.allocate_exact_size(
        Vec2::new(width, height),
        if enabled { Sense::click() } else { Sense::hover() },
    );
    if ui.is_rect_visible(rect) {
        let hovered = enabled && resp.hovered();
        let pressed = enabled && resp.is_pointer_button_down_on();
        let base = if enabled { color } else { Color32::from_rgb(58, 62, 70) };
        let fill = if pressed {
            darken(base, 0.82)
        } else if hovered {
            lighten(base, 1.12)
        } else {
            base
        };
        let rect = if pressed { rect.translate(Vec2::new(0.0, 1.0)) } else { rect };
        let painter = ui.painter();
        painter.rect_filled(rect.translate(Vec2::new(0.0, 3.0)), 11, darken(base, 0.55));
        painter.rect_filled(rect, 11, fill);
        painter.rect_stroke(
            rect,
            11,
            Stroke::new(1.0_f32, Color32::from_white_alpha(if hovered { 60 } else { 28 })),
            StrokeKind::Inside,
        );
        let text_color = if enabled {
            if is_light(base) { Color32::from_rgb(20, 20, 24) } else { Color32::WHITE }
        } else {
            TEXT_DIM
        };
        painter.text(rect.center(), Align2::CENTER_CENTER, text, semibold(16.0), text_color);
    }
    if enabled {
        resp.on_hover_cursor(egui::CursorIcon::PointingHand)
    } else {
        resp
    }
}

pub fn darken(c: Color32, k: f32) -> Color32 {
    Color32::from_rgba_unmultiplied(
        (c.r() as f32 * k) as u8,
        (c.g() as f32 * k) as u8,
        (c.b() as f32 * k) as u8,
        c.a(),
    )
}

pub fn lighten(c: Color32, k: f32) -> Color32 {
    Color32::from_rgba_unmultiplied(
        (c.r() as f32 * k).min(255.0) as u8,
        (c.g() as f32 * k).min(255.0) as u8,
        (c.b() as f32 * k).min(255.0) as u8,
        c.a(),
    )
}

pub fn is_light(c: Color32) -> bool {
    (c.r() as f32 * 0.299 + c.g() as f32 * 0.587 + c.b() as f32 * 0.114) > 160.0
}

pub fn with_alpha(c: Color32, a: u8) -> Color32 {
    Color32::from_rgba_unmultiplied(c.r(), c.g(), c.b(), a)
}

/// Round avatar with the player's initial.
pub fn avatar(ui: &mut Ui, name: &str, color: Color32, size: f32) -> Response {
    let (rect, resp) = ui.allocate_exact_size(Vec2::splat(size), Sense::hover());
    paint_avatar(ui.painter(), rect, name, color);
    resp
}

pub fn paint_avatar(painter: &egui::Painter, rect: Rect, name: &str, color: Color32) {
    let r = rect.width() / 2.0;
    painter.circle_filled(rect.center(), r, color);
    painter.circle_stroke(rect.center(), r, Stroke::new(1.5_f32, Color32::from_white_alpha(70)));
    let initial: String = name.chars().next().map(|c| c.to_uppercase().collect()).unwrap_or_default();
    painter.text(
        rect.center(),
        Align2::CENTER_CENTER,
        initial,
        bold(r * 1.05),
        if is_light(color) { Color32::from_rgb(25, 25, 30) } else { Color32::WHITE },
    );
}

/// Seat colours for players in the card game and lobby avatars.
pub const SEAT_COLORS: [[u8; 3]; 8] = [
    [0x4c, 0x8d, 0xf6],
    [0xe8, 0x5d, 0x4f],
    [0x3c, 0xb4, 0x6e],
    [0xf2, 0xa1, 0x2c],
    [0xa0, 0x5c, 0xe6],
    [0x2c, 0xc0, 0xc8],
    [0xe6, 0x5c, 0xb4],
    [0xb8, 0xb8, 0x40],
];

/// `Stroke::new` with an explicitly typed width.
pub fn stroke(width: f32, color: Color32) -> Stroke {
    Stroke::new(width, color)
}

/// A centred dialog over a dimmed screen.
pub fn modal(ctx: &egui::Context, id: &str, add: impl FnOnce(&mut Ui)) {
    let painter = ctx.layer_painter(egui::LayerId::new(egui::Order::Middle, egui::Id::new(("dim", id))));
    painter.rect_filled(ctx.screen_rect(), 0, Color32::from_black_alpha(90));
    egui::Window::new(id)
        .title_bar(false)
        .resizable(false)
        .collapsible(false)
        .anchor(Align2::CENTER_CENTER, egui::Vec2::ZERO)
        .frame(card_frame().inner_margin(Margin::same(22)))
        .show(ctx, add);
}

/// A process-wide unique number, for egui ids of short-lived widgets.
pub fn unique_key() -> u64 {
    use std::sync::atomic::{AtomicU64, Ordering};
    static NEXT: AtomicU64 = AtomicU64::new(1);
    NEXT.fetch_add(1, Ordering::Relaxed)
}

/// Shows an [`egui::Area`] and keeps it from jumping when its size changes.
///
/// egui places an area by its pivot or anchor using the size it had on the
/// previous frame. So when the content changes size (a longer status line,
/// other buttons, a new chip count) a centred or right/bottom-aligned area is
/// drawn one frame in the wrong place and snaps back on the next: the HUD
/// visibly flickers. When the size changed, this pass is discarded and egui
/// lays the UI out again with the new size before anything is painted.
pub fn show_area<R>(
    ctx: &egui::Context,
    area: egui::Area,
    add_contents: impl FnOnce(&mut Ui) -> R,
) -> egui::InnerResponse<R> {
    let id = area.layer().id;
    let before = ctx.memory(|m| m.area_rect(id)).map(|r| r.size());
    let response = area.show(ctx, add_contents);
    let after = response.response.rect.size();
    let resized = before.is_none_or(|b| (b - after).abs().max_elem() > 0.5);
    if resized && !ctx.will_discard() {
        ctx.request_discard("HUD area changed size");
    }
    response
}

/// A big caption in the middle of the table («Раунд 2», the chosen colour…)
/// that pops in at `scale` and fades with `alpha`.
///
/// The text is always laid out at the same font size and the pop is a scale
/// of its layer. Animating the font size itself made egui rasterise the glyphs
/// at a new size on every frame of the pop, i.e. grow its font atlas and
/// upload a new atlas texture each frame.
#[allow(clippy::too_many_arguments)]
pub fn banner(ctx: &egui::Context, id: &str, pos: Pos2, text: &str, size: f32, color: Color32, alpha: f32, scale: f32) {
    let layer = egui::LayerId::new(egui::Order::Foreground, egui::Id::new(id));
    // Scales around `pos`: pos * scale + translation == pos.
    ctx.set_transform_layer(layer, TSTransform::new(pos.to_vec2() * (1.0 - scale), scale));
    let painter = ctx.layer_painter(layer);
    let font = bold(size);
    painter.text(
        pos + Vec2::new(2.0, 3.0),
        Align2::CENTER_CENTER,
        text,
        font.clone(),
        Color32::from_black_alpha((160.0 * alpha) as u8),
    );
    painter.text(pos, Align2::CENTER_CENTER, text, font, with_alpha(color, (255.0 * alpha) as u8));
}
