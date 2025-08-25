// broker/src/search/paginator.rs
use crate::search::types::*;
use serde_json::json;

pub struct Paginator;

impl Paginator {
    pub fn merge(
        mut parts: Vec<crate::search::executor::SegmentTaskOutput>,
        page_size: usize,
    ) -> (Vec<serde_json::Value>, Cursor, u64) {
        // Наивный мердж: просто конкат, затем truncate
        parts.sort_by(|a,b| a.seg_path.cmp(&b.seg_path)); // стабильно
        let mut hits = Vec::new();
        let mut candidates_total = 0u64;
        let mut per_seg = serde_json::Map::new();

        for part in parts {
            candidates_total += part.candidates;
            let remain = page_size.saturating_sub(hits.len());
            if remain > 0 {
                let take = part.hits.into_iter().take(remain);
                hits.extend(take);
            }
            per_seg.insert(part.seg_path, json!({ "last_docid": part.last_docid }));
        }

        let cursor = Cursor {
            per_seg: serde_json::Value::Object(per_seg),
            pin_gen: None,
        };
        (hits, cursor, candidates_total)
    }
}
