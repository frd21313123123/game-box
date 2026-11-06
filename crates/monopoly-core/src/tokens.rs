//! Pawns and the pawn colour palette.

pub struct Token {
    pub id: &'static str,
    pub name: &'static str,
    /// Emoji shown in lists and on the board.
    pub symbol: &'static str,
    pub color: [u8; 3],
}

pub static TOKENS: [Token; 8] = [
    Token { id: "hat", name: "Цилиндр", symbol: "🎩", color: [0x2c, 0x2c, 0x2c] },
    Token { id: "car", name: "Машинка", symbol: "🚗", color: [0xc9, 0x20, 0x20] },
    Token { id: "dog", name: "Пёсик", symbol: "🐕", color: [0x8a, 0x5a, 0x2b] },
    Token { id: "boot", name: "Ботинок", symbol: "👢", color: [0x5a, 0x33, 0x20] },
    Token { id: "ship", name: "Кораблик", symbol: "🚢", color: [0x1e, 0x4a, 0x86] },
    Token { id: "cat", name: "Котик", symbol: "🐈", color: [0xe6, 0xa1, 0x00] },
    Token { id: "rocket", name: "Ракета", symbol: "🚀", color: [0x7d, 0x2d, 0x8a] },
    Token { id: "star", name: "Звезда", symbol: "⭐", color: [0xd6, 0x3a, 0x96] },
];

pub const MAX_PLAYERS: usize = 8;
pub const MIN_PLAYERS: usize = 2;

/// Palette ordered most-distinct-first, so a small game gets well separated colours.
pub static TOKEN_COLORS: [[u8; 3]; 8] = [
    [0xe6, 0x19, 0x4b], // red
    [0x43, 0x63, 0xd8], // blue
    [0x3c, 0xb4, 0x4b], // green
    [0xf5, 0x82, 0x31], // orange
    [0x91, 0x1e, 0xb4], // purple
    [0xf0, 0x32, 0xe6], // magenta
    [0x00, 0xb4, 0xc8], // cyan
    [0xff, 0xd0, 0x00], // gold
];

/// First palette entry not in `used`, preferring `preferred` when it is free.
pub fn next_distinct_color(used: &[usize], preferred: Option<usize>) -> usize {
    if let Some(p) = preferred {
        if p < TOKEN_COLORS.len() && !used.contains(&p) {
            return p;
        }
    }
    (0..TOKEN_COLORS.len())
        .find(|c| !used.contains(c))
        .unwrap_or(0)
}

/// First token not in `used`.
pub fn next_free_token(used: &[usize]) -> Option<usize> {
    (0..TOKENS.len()).find(|t| !used.contains(t))
}
