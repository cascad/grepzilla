// crates/broker/tests/metrics_positive.rs
use broker::search::executor::SegmentTaskOutput;
use broker::search::paginator::Paginator;
use broker::search::types::{Hit, SearchResponse, SearchMetrics};

fn mk_seg(seg_path: &str, hits: Vec<Hit>, prefilter: u64, verify: u64, prefetch: u64, warmed: u64) -> SegmentTaskOutput {
    SegmentTaskOutput {
        seg_path: seg_path.to_string(),
        last_docid: Some(1),
        candidates: hits.len() as u64,
        hits,
        prefilter_ms: prefilter,
        verify_ms: verify,
        prefetch_ms: prefetch,
        warmed_docs: warmed,
    }
}

#[test]
fn metrics_are_some_and_aggregated_when_non_empty() {
    let h = Hit { ext_id: "e".into(), doc_id: 1, matched_field: "f".into(), preview: "p".into() };

    let parts = vec![
        mk_seg("A", vec![h.clone()], 2, 3, 5, 7),
        mk_seg("B", vec![h],          11, 13, 17, 19), // будет дедуп, но метрики суммируются
    ];

    let (hits, cursor, candidates_total, dedup_dropped, totals) = Paginator::merge(parts, 10);
    let (prefilter_ms_total, verify_ms_total, prefetch_ms_total, warmed_docs_total) = totals;

    let resp = SearchResponse {
        hits,
        cursor: Some(cursor),
        metrics: SearchMetrics {
            candidates_total,
            time_to_first_hit_ms: 0,
            deadline_hit: false,
            saturated_sem: 0,
            dedup_dropped,
            prefilter_ms: Some(prefilter_ms_total),
            verify_ms:   Some(verify_ms_total),
            prefetch_ms: Some(prefetch_ms_total),
            warmed_docs: Some(warmed_docs_total),
        },
    };

    // должны быть Some(...) с корректными суммами
    assert_eq!(resp.metrics.prefilter_ms, Some(2 + 11));
    assert_eq!(resp.metrics.verify_ms,   Some(3 + 13));
    assert_eq!(resp.metrics.prefetch_ms, Some(5 + 17));
    assert_eq!(resp.metrics.warmed_docs, Some(7 + 19));
}
