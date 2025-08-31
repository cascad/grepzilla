// crates/broker/src/search/paginator.rs

use std::collections::{HashMap, HashSet};

use crate::search::executor::SegmentTaskOutput;
use crate::search::types::{Hit, PerSegPos, SearchCursor};

const HOT_SEG_NAME: &str = "__hot__"; // ← NEW

pub struct Paginator;

impl Paginator {
    pub fn merge(
        parts: Vec<SegmentTaskOutput>,
        page_size: usize,
    ) -> (Vec<Hit>, SearchCursor, u64, u64, (u64, u64, u64, u64)) {
        let mut hits: Vec<Hit> = Vec::new();
        let mut per_seg: HashMap<String, PerSegPos> = HashMap::new();
        let mut candidates_total: u64 = 0;

        // агрегированные метрики
        let mut prefilter_ms_total = 0u64;
        let mut verify_ms_total = 0u64;
        let mut prefetch_ms_total = 0u64;
        let mut warmed_docs_total = 0u64;

        // дедуп по ext_id
        let mut seen_ext: HashSet<String> = HashSet::new();
        let mut dedup_dropped: u64 = 0;

        for p in parts.into_iter() {
            candidates_total += p.candidates;
            prefilter_ms_total += p.prefilter_ms;
            verify_ms_total += p.verify_ms;
            prefetch_ms_total += p.prefetch_ms;
            warmed_docs_total += p.warmed_docs;

            // набираем хиты до page_size, дедуп по ext_id
            for h in p.hits {
                if hits.len() >= page_size {
                    break;
                }
                if !seen_ext.insert(h.ext_id.clone()) {
                    dedup_dropped += 1;
                    continue;
                }
                hits.push(h);
            }

            // ⬇️ Не кладём служебный hot-сегмент в курсор
            if p.seg_path != HOT_SEG_NAME {
                per_seg.insert(
                    p.seg_path.clone(),
                    PerSegPos {
                        last_docid: p.last_docid.unwrap_or(0),
                    },
                );
            }
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
            (prefilter_ms_total, verify_ms_total, prefetch_ms_total, warmed_docs_total),
        )
    }
}
