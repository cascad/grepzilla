use std::collections::HashMap;

use crate::search::{executor::SegmentTaskOutput, types::*};

pub struct Paginator;

impl Paginator {
    pub fn merge(mut parts: Vec<SegmentTaskOutput>, page_size: usize) -> (Vec<Hit>, Cursor, u64) {
        // Если у тебя уже есть сортировка/слияние — оставь её; ниже — простая версия.
        let mut hits: Vec<Hit> = Vec::new();
        let mut per_seg: HashMap<String, PerSegPos> = HashMap::new();
        let mut candidates_total: u64 = 0;

        // суммируем метрику
        for p in &parts {
            candidates_total += p.candidates as u64;
        }

        // набираем первую страницу и сохраняем last_docid per-seg
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
                    last_docid: p.last_docid.unwrap_or(0),
                },
            );
        }

        let cursor = Cursor {
            per_seg,
            pin_gen: None, // B6: координатор заполнит, если есть manifest
        };

        (hits, cursor, candidates_total)
    }
}
