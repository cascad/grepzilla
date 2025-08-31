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
    ) -> (Vec<Hit>, SearchCursor, u64, u64, u64, u64, u64, u64) {
        let mut hits: Vec<Hit> = Vec::new();
        let mut per_seg: HashMap<String, PerSegPos> = HashMap::new();
        let mut candidates_total: u64 = 0;

        // NEW: агрегированные метрики
        let mut agg_prefilter_ms = 0u64;
        let mut agg_verify_ms = 0u64;
        let mut agg_prefetch_ms = 0u64;
        let mut agg_warmed_docs = 0u64;

        // дедуп по ext_id
        let mut seen_ext: HashSet<String> = HashSet::new();
        let mut dedup_dropped = 0u64;

        for p in parts.into_iter() {
            candidates_total += p.candidates;
            agg_prefilter_ms += p.prefilter_ms;
            agg_verify_ms += p.verify_ms;
            agg_prefetch_ms += p.prefetch_ms;
            agg_warmed_docs += p.warmed_docs;

            for h in p.hits {
                if hits.len() >= page_size {
                    break;
                }
                if seen_ext.insert(h.ext_id.clone()) {
                    hits.push(h);
                } else {
                    dedup_dropped += 1;
                }
            }

            per_seg.insert(
                p.seg_path.clone(),
                PerSegPos {
                    last_docid: p.last_docid.unwrap_or(0),
                },
            );
        }

        let cursor = SearchCursor {
            per_seg,
            pin_gen: None,
        };

        (
            hits,
            cursor,
            candidates_total,
            dedup_dropped,
            agg_prefilter_ms,
            agg_verify_ms,
            agg_prefetch_ms,
            agg_warmed_docs,
        )
    }
}
