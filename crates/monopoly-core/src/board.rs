//! The 40 tiles of the Moscow edition board.

use serde::{Deserialize, Serialize};

pub type TileIndex = usize;

pub const BOARD_SIZE: usize = 40;
pub const JAIL_INDEX: TileIndex = 10;
pub const GO_TO_JAIL_INDEX: TileIndex = 30;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ColorGroup {
    Brown,
    LightBlue,
    Pink,
    Orange,
    Red,
    Yellow,
    Green,
    DarkBlue,
}

impl ColorGroup {
    pub const ALL: [ColorGroup; 8] = [
        ColorGroup::Brown,
        ColorGroup::LightBlue,
        ColorGroup::Pink,
        ColorGroup::Orange,
        ColorGroup::Red,
        ColorGroup::Yellow,
        ColorGroup::Green,
        ColorGroup::DarkBlue,
    ];

    pub fn house_cost(self) -> i32 {
        match self {
            ColorGroup::Brown | ColorGroup::LightBlue => 50,
            ColorGroup::Pink | ColorGroup::Orange => 100,
            ColorGroup::Red | ColorGroup::Yellow => 150,
            ColorGroup::Green | ColorGroup::DarkBlue => 200,
        }
    }

    /// sRGB stripe colour.
    pub fn rgb(self) -> [u8; 3] {
        match self {
            ColorGroup::Brown => [0x8B, 0x45, 0x13],
            ColorGroup::LightBlue => [0xAA, 0xE0, 0xFA],
            ColorGroup::Pink => [0xD9, 0x3A, 0x96],
            ColorGroup::Orange => [0xF7, 0x94, 0x1D],
            ColorGroup::Red => [0xED, 0x1B, 0x24],
            ColorGroup::Yellow => [0xFE, 0xF2, 0x00],
            ColorGroup::Green => [0x1F, 0xB2, 0x5A],
            ColorGroup::DarkBlue => [0x00, 0x72, 0xBB],
        }
    }
}

/// Rent ladder of a street: base, with a monopoly, 1–4 houses, hotel.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RentLadder {
    pub base: i32,
    pub mono: i32,
    pub houses: [i32; 4],
    pub hotel: i32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TileKind {
    Go,
    Street {
        group: ColorGroup,
        price: i32,
        rent: RentLadder,
    },
    Station {
        price: i32,
    },
    Utility {
        price: i32,
    },
    Chance,
    Chest,
    Tax {
        amount: i32,
    },
    Jail,
    FreeParking,
    GoToJail,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Tile {
    pub index: TileIndex,
    pub name: &'static str,
    pub kind: TileKind,
}

impl Tile {
    pub fn price(&self) -> Option<i32> {
        match self.kind {
            TileKind::Street { price, .. }
            | TileKind::Station { price }
            | TileKind::Utility { price } => Some(price),
            _ => None,
        }
    }

    pub fn is_purchasable(&self) -> bool {
        self.price().is_some()
    }

    pub fn group(&self) -> Option<ColorGroup> {
        match self.kind {
            TileKind::Street { group, .. } => Some(group),
            _ => None,
        }
    }
}

const fn rent(base: i32, mono: i32, h1: i32, h2: i32, h3: i32, h4: i32, hotel: i32) -> RentLadder {
    RentLadder {
        base,
        mono,
        houses: [h1, h2, h3, h4],
        hotel,
    }
}

const fn street(
    index: TileIndex,
    name: &'static str,
    group: ColorGroup,
    price: i32,
    rent: RentLadder,
) -> Tile {
    Tile {
        index,
        name,
        kind: TileKind::Street { group, price, rent },
    }
}

const fn simple(index: TileIndex, name: &'static str, kind: TileKind) -> Tile {
    Tile { index, name, kind }
}

use ColorGroup::*;

pub static BOARD: [Tile; BOARD_SIZE] = [
    simple(0, "СТАРТ", TileKind::Go),
    street(1, "Маросейка", Brown, 60, rent(2, 4, 10, 30, 90, 160, 250)),
    simple(2, "Общественная казна", TileKind::Chest),
    street(3, "Варварка", Brown, 60, rent(4, 8, 20, 60, 180, 320, 450)),
    simple(4, "Подоходный налог", TileKind::Tax { amount: 200 }),
    simple(5, "Рижский вокзал", TileKind::Station { price: 200 }),
    street(6, "Киевская", LightBlue, 100, rent(6, 12, 30, 90, 270, 400, 550)),
    simple(7, "Шанс", TileKind::Chance),
    street(8, "Огарёва", LightBlue, 100, rent(6, 12, 30, 90, 270, 400, 550)),
    street(9, "Якиманка", LightBlue, 120, rent(8, 16, 40, 100, 300, 450, 600)),
    simple(10, "ТЮРЬМА", TileKind::Jail),
    street(11, "Полянка", Pink, 140, rent(10, 20, 50, 150, 450, 625, 750)),
    simple(12, "Электростанция", TileKind::Utility { price: 150 }),
    street(13, "Сретенка", Pink, 140, rent(10, 20, 50, 150, 450, 625, 750)),
    street(14, "Неглинная", Pink, 160, rent(12, 24, 60, 180, 500, 700, 900)),
    simple(15, "Курский вокзал", TileKind::Station { price: 200 }),
    street(16, "Ордынка", Orange, 180, rent(14, 28, 70, 200, 550, 750, 950)),
    simple(17, "Общественная казна", TileKind::Chest),
    street(18, "Пятницкая", Orange, 180, rent(14, 28, 70, 200, 550, 750, 950)),
    street(19, "Лубянка", Orange, 200, rent(16, 32, 80, 220, 600, 800, 1000)),
    simple(20, "БЕСПЛАТНАЯ СТОЯНКА", TileKind::FreeParking),
    street(21, "Земляной Вал", Red, 220, rent(18, 36, 90, 250, 700, 875, 1050)),
    simple(22, "Шанс", TileKind::Chance),
    street(23, "Кутузовский пр-т", Red, 220, rent(18, 36, 90, 250, 700, 875, 1050)),
    street(24, "Проспект Мира", Red, 240, rent(20, 40, 100, 300, 750, 925, 1100)),
    simple(25, "Северный вокзал", TileKind::Station { price: 200 }),
    street(26, "Пушкинская", Yellow, 260, rent(22, 44, 110, 330, 800, 975, 1150)),
    street(27, "Маяковская", Yellow, 260, rent(22, 44, 110, 330, 800, 975, 1150)),
    simple(28, "Водопровод", TileKind::Utility { price: 150 }),
    street(29, "Новый Арбат", Yellow, 280, rent(24, 48, 120, 360, 850, 1025, 1200)),
    simple(30, "ОТПРАВЛЯЙТЕСЬ В ТЮРЬМУ", TileKind::GoToJail),
    street(31, "Сивцев Вражек", Green, 300, rent(26, 52, 130, 390, 900, 1100, 1275)),
    street(32, "Арбат", Green, 300, rent(26, 52, 130, 390, 900, 1100, 1275)),
    simple(33, "Общественная казна", TileKind::Chest),
    street(34, "Поварская", Green, 320, rent(28, 56, 150, 450, 1000, 1200, 1400)),
    simple(35, "Ленинградский вокзал", TileKind::Station { price: 200 }),
    simple(36, "Шанс", TileKind::Chance),
    street(37, "Тверская", DarkBlue, 350, rent(35, 70, 175, 500, 1100, 1300, 1500)),
    simple(38, "Сверхналог на роскошь", TileKind::Tax { amount: 100 }),
    street(39, "Малая Бронная", DarkBlue, 400, rent(50, 100, 200, 600, 1400, 1700, 2000)),
];

pub fn tile(index: TileIndex) -> &'static Tile {
    &BOARD[index % BOARD_SIZE]
}

pub fn tiles_in_group(group: ColorGroup) -> impl Iterator<Item = TileIndex> {
    BOARD
        .iter()
        .filter(move |t| t.group() == Some(group))
        .map(|t| t.index)
}

pub const STATION_INDICES: [TileIndex; 4] = [5, 15, 25, 35];
pub const UTILITY_INDICES: [TileIndex; 2] = [12, 28];

pub fn mortgage_value(index: TileIndex) -> i32 {
    tile(index).price().map(|p| p / 2).unwrap_or(0)
}

/// Mortgage value plus 10 % interest, rounded up.
pub fn unmortgage_cost(index: TileIndex) -> i32 {
    let v = mortgage_value(index);
    v + (v + 9) / 10
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn board_is_consistent() {
        for (i, t) in BOARD.iter().enumerate() {
            assert_eq!(t.index, i);
        }
        assert_eq!(tiles_in_group(Brown).count(), 2);
        assert_eq!(tiles_in_group(Red).count(), 3);
        assert_eq!(tiles_in_group(DarkBlue).count(), 2);
        for i in STATION_INDICES {
            assert!(matches!(tile(i).kind, TileKind::Station { .. }));
        }
        for i in UTILITY_INDICES {
            assert!(matches!(tile(i).kind, TileKind::Utility { .. }));
        }
        assert_eq!(tile(JAIL_INDEX).kind, TileKind::Jail);
        assert_eq!(unmortgage_cost(1), 33);
        assert_eq!(unmortgage_cost(39), 220);
    }
}
