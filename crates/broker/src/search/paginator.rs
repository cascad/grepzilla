use std::collections::{HashMap, HashSet};

use crate::search::executor::SegmentTaskOutput;
use crate::search::types::{Hit, PerSegPos, SearchCursor};

/// Простой пагинатор: склеивает результаты по сегментам в порядке их прихода,
/// набирает первую страницу `page_size`, строит курсор per-seg и считает метрики.
/// Дедуп по ext_id: если документ встречается в нескольких сегментах, берём первый.
pub struct Paginator;

impl Paginator {
    /// Возвращает: (hits_page, cursor, candidates_total, dedup_dropped)
    pub fn merge(
        parts: Vec<SegmentTaskOutput>,
        page_size: usize,
    ) -> (Vec<Hit>, SearchCursor, u64, u64) {
        let mut hits: Vec<Hit> = Vec::with_capacity(page_size);
        let mut per_seg: HashMap<String, PerSegPos> = HashMap::new();
        let mut candidates_total: u64 = 0;
        let mut seen_ext: HashSet<String> = HashSet::new();

        // для подсчёта dedup
        let mut incoming_hits_total: u64 = 0;

        // 1) суммируем кандидатов и потенциальные входящие хиты
        for p in &parts {
            candidates_total += p.candidates as u64;
            incoming_hits_total += p.hits.len() as u64;
        }

        // 2) набираем страницу с дедупом по ext_id
        for p in parts.into_iter() {
            if hits.len() < page_size {
                for h in p.hits {
                    if !seen_ext.insert(h.ext_id.clone()) {
                        // дубликат — пропускаем
                        continue;
                    }
                    hits.push(h);
                    if hits.len() >= page_size {
                        break;
                    }
                }
            }

            // per-seg курсор фиксируем всегда
            per_seg.insert(
                p.seg_path.clone(),
                PerSegPos {
                    last_docid: p.last_docid.unwrap_or(0),
                },
            );
        }

        // 3) dedup_dropped = сколько хитов мы отбросили из-за дубликатов
        let dedup_dropped = incoming_hits_total.saturating_sub(hits.len() as u64);

        let cursor = SearchCursor {
            per_seg,
            pin_gen: None, // заполнит координатор, если работаем через manifest/shards
        };

        (hits, cursor, candidates_total, dedup_dropped)
    }
}
