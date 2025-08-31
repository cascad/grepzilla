// crates/broker/tests/metrics_dedup.rs
use broker::search::executor::SegmentTaskOutput;
use broker::search::paginator::Paginator;
use broker::search::types::{Hit, SearchCursor, SearchMetrics, SearchResponse};

fn mk_seg(seg_path: &str, hits: Vec<Hit>, prefilter: u64, verify: u64, prefetch: u64, warmed: u64) -> SegmentTaskOutput {
    SegmentTaskOutput {
        seg_path: seg_path.to_string(),
        last_docid: Some(5),
        candidates: hits.len() as u64,
        hits,
        prefilter_ms: prefilter,
        verify_ms: verify,
        prefetch_ms: prefetch,
        warmed_docs: warmed,
    }
}

#[test]
fn paginator_aggregates_metrics_and_reports_dedup() {
    let base = Hit {
        ext_id: "dup".into(),
        doc_id: 1,
        matched_field: "text.body".into(),
        preview: "...".into(),
    };

    let parts = vec![
        mk_seg("segments/A", vec![base.clone()], 2, 3, 5, 7),
        mk_seg("segments/B", vec![base.clone()], 11, 13, 17, 19),
    ];

    let (hits, cursor, candidates_total, dedup_dropped, totals) = Paginator::merge(parts, 10);
    let (prefilter_ms_total, verify_ms_total, prefetch_ms_total, warmed_docs_total) = totals;

    // Соберём SearchResponse так же, как делает координатор
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

    assert_eq!(resp.metrics.dedup_dropped, 1);
    assert_eq!(resp.hits.len(), 1);
    assert_eq!(resp.metrics.candidates_total, 2);

    assert_eq!(resp.metrics.prefilter_ms, Some(2 + 11));
    assert_eq!(resp.metrics.verify_ms,   Some(3 + 13));
    assert_eq!(resp.metrics.prefetch_ms, Some(5 + 17));
    assert_eq!(resp.metrics.warmed_docs, Some(7 + 19));

    let per_seg = &resp.cursor.as_ref().unwrap().per_seg;
    assert!(per_seg.get("segments/A").is_some());
    assert!(per_seg.get("segments/B").is_some());
}
