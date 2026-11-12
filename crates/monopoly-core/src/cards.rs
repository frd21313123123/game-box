//! Chance and Community Chest cards.

use serde::{Deserialize, Serialize};

use crate::board::TileIndex;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Deck {
    Chance,
    Chest,
}

impl Deck {
    pub fn name(self) -> &'static str {
        match self {
            Deck::Chance => "Шанс",
            Deck::Chest => "Общественная казна",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CardEffect {
    MoveTo { tile: TileIndex, collect_go: bool },
    MoveRelative { delta: i32 },
    MoveToNearestStation { pay_double: bool },
    MoveToNearestUtility,
    CollectBank(i32),
    PayBank(i32),
    CollectEach(i32),
    PayEach(i32),
    GoToJail,
    GetOutOfJailFree,
    PayRepairs { per_house: i32, per_hotel: i32 },
}

#[derive(Clone, Copy, Debug)]
pub struct Card {
    pub text: &'static str,
    pub effect: CardEffect,
}

use CardEffect::*;

pub static CHANCE_CARDS: [Card; 16] = [
    Card { text: "Перейдите на «СТАРТ». Получите ₽200", effect: MoveTo { tile: 0, collect_go: true } },
    Card { text: "Отправляйтесь на Проспект Мира. Если пройдёте СТАРТ — получите ₽200", effect: MoveTo { tile: 24, collect_go: true } },
    Card { text: "Отправляйтесь на Малую Бронную", effect: MoveTo { tile: 39, collect_go: false } },
    Card { text: "Отправляйтесь на Полянку. Если пройдёте СТАРТ — получите ₽200", effect: MoveTo { tile: 11, collect_go: true } },
    Card { text: "Перейдите на ближайший вокзал. Если он чужой — заплатите двойную ренту", effect: MoveToNearestStation { pay_double: true } },
    Card { text: "Ваше здание и кредит погашены. Получите ₽150", effect: CollectBank(150) },
    Card { text: "Перейдите на ближайшее коммунальное предприятие. Если оно чужое — заплатите 10× сумму на кубиках", effect: MoveToNearestUtility },
    Card { text: "Банк выплачивает вам дивиденды ₽50", effect: CollectBank(50) },
    Card { text: "Освобождение из тюрьмы. Сохраните карту до использования", effect: GetOutOfJailFree },
    Card { text: "Вернитесь на 3 клетки назад", effect: MoveRelative { delta: -3 } },
    Card { text: "Отправляйтесь в тюрьму. Не проходите поле «СТАРТ»", effect: GoToJail },
    Card { text: "Общий ремонт недвижимости: заплатите ₽25 за каждый дом и ₽100 за каждый отель", effect: PayRepairs { per_house: 25, per_hotel: 100 } },
    Card { text: "Штраф за превышение скорости. Заплатите ₽15", effect: PayBank(15) },
    Card { text: "Совершите поездку до Рижского вокзала. Если пройдёте СТАРТ — получите ₽200", effect: MoveTo { tile: 5, collect_go: true } },
    Card { text: "Председатель правления избрал вас. Заплатите каждому игроку по ₽50", effect: PayEach(50) },
    Card { text: "Ваш заём на строительство созрел. Получите ₽150", effect: CollectBank(150) },
];

pub static CHEST_CARDS: [Card; 16] = [
    Card { text: "Банковская ошибка в вашу пользу. Получите ₽200", effect: CollectBank(200) },
    Card { text: "Выгодная продажа акций. Получите ₽50", effect: CollectBank(50) },
    Card { text: "Освобождение из тюрьмы. Сохраните карту до использования", effect: GetOutOfJailFree },
    Card { text: "Отправляйтесь в тюрьму. Не проходите поле «СТАРТ»", effect: GoToJail },
    Card { text: "Возврат подоходного налога. Получите ₽20", effect: CollectBank(20) },
    Card { text: "Наступил ваш день рождения. Получите по ₽10 от каждого игрока", effect: CollectEach(10) },
    Card { text: "Услуги врача. Заплатите ₽50", effect: PayBank(50) },
    Card { text: "Оплата больницы. Заплатите ₽100", effect: PayBank(100) },
    Card { text: "Оплата обучения. Заплатите ₽150", effect: PayBank(150) },
    Card { text: "Получено наследство. Получите ₽100", effect: CollectBank(100) },
    Card { text: "Получите доход по процентам. ₽25", effect: CollectBank(25) },
    Card { text: "Вы заняли второе место на конкурсе красоты. Получите ₽10", effect: CollectBank(10) },
    Card { text: "Наступила зрелость вашего страхового полиса. Получите ₽100", effect: CollectBank(100) },
    Card { text: "Получите ₽45 за консультационные услуги", effect: CollectBank(45) },
    Card { text: "Взнос на ремонт улиц: заплатите ₽40 за каждый дом и ₽115 за каждый отель", effect: PayRepairs { per_house: 40, per_hotel: 115 } },
    Card { text: "Получите ₽100 из праздничного фонда", effect: CollectBank(100) },
];

pub fn card(deck: Deck, index: usize) -> &'static Card {
    match deck {
        Deck::Chance => &CHANCE_CARDS[index],
        Deck::Chest => &CHEST_CARDS[index],
    }
}
