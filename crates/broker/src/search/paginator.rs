// crates/broker/src/search/paginator.rs

use std::collections::HashMap;

use crate::search::executor::SegmentTaskOutput;
use crate::search::types::{Hit, PerSegPos, SearchCursor};

/// Простой пагинатор: склеивает результаты по сегментам в порядке их прихода,
/// набирает первую страницу `page_size`, и строит курсор per-seg.
pub struct Paginator;

impl Paginator {
    /// Возвращает: (hits_page, cursor, candidates_total)
    pub fn merge(parts: Vec<SegmentTaskOutput>, page_size: usize) -> (Vec<Hit>, SearchCursor, u64) {
        let mut hits: Vec<Hit> = Vec::new();
        let mut per_seg: HashMap<String, PerSegPos> = HashMap::new();
        let mut candidates_total: u64 = 0;

        // суммируем кандидатов
        for p in &parts {
            candidates_total += p.candidates as u64;
        }

        // набираем хиты (первая страница) и фиксируем last_docid per-seg
        for p in parts.into_iter() {
            for h in p.hits {
                if hits.len() >= page_size {
                    break;
                }
                hits.push(h);
            }

            per_seg.insert(
                p.seg_path.clone(),
                PerSegPos {
                    // executor может вернуть None, если сегмент ещё не читал документов
                    last_docid: p.last_docid.unwrap_or(0),
                },
            );
        }

        let cursor = SearchCursor {
            per_seg,
            // pin_generation (pin_gen) заполнит координатор, если работаем через manifest
            pin_gen: None,
        };

        (hits, cursor, candidates_total)
    }
}
