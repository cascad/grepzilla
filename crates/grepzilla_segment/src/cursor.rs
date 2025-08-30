use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Координаты продолжения внутри сегмента
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ShardPos {
    pub shard: u64,
    pub segment: String,
    pub block: u32,
    pub last_docid: u32,
}

/// Лимиты поиска (на будущее: бюджеты кандидатов/времени верификации)
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Budgets {
    pub candidates: u64,
    pub verify_ms: u64,
}

/// Курсор поиска — сериализуемая структура для продолжения пагинации.
/// Важно: `pin_gen` фиксирует конкретную генерацию манифеста per shard.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SearchCursor {
    pub matcher_hash: String,       // sha256 от (query, field, flags)
    pub pin_gen: HashMap<u64, u64>, // shard_id -> gen
    pub state: Vec<ShardPos>,       // координаты по сегментам
    pub budgets: Budgets,           // лимиты
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip_cursor() {
        let mut pin = HashMap::new();
        pin.insert(17u64, 123u64);
        let c = SearchCursor {
            matcher_hash: "abc".to_string(),
            pin_gen: pin,
            state: vec![ShardPos {
                shard: 17,
                segment: "s-abc".into(),
                block: 7,
                last_docid: 42,
            }],
            budgets: Budgets {
                candidates: 1000,
                verify_ms: 500,
            },
        };
        let j = serde_json::to_string(&c).unwrap();
        let back: SearchCursor = serde_json::from_str(&j).unwrap();
        assert_eq!(c, back);
    }
}
