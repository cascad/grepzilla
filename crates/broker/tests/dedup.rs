// crates/broker/tests/dedup.rs
use broker::search::executor::SegmentTaskOutput;
use broker::search::paginator::Paginator;
use broker::search::types::Hit;

fn mk_seg(seg_path: &str, hits: Vec<Hit>) -> SegmentTaskOutput {
    SegmentTaskOutput {
        seg_path: seg_path.to_string(),
        last_docid: Some(123),
        candidates: hits.len() as u64,
        hits,
        prefilter_ms: 0,
        verify_ms: 0,
        prefetch_ms: 0,
        warmed_docs: 0,
    }
}

#[test]
fn merge_dedups_by_ext_id_across_segments() {
    let h1 = Hit {
        ext_id: "same-ext-id".into(),
        doc_id: 10,
        matched_field: "text.body".into(),
        preview: "first".into(),
    };
    let h2 = Hit {
        ext_id: "same-ext-id".into(),
        doc_id: 11,
        matched_field: "text.body".into(),
        preview: "second".into(),
    };

    let parts = vec![
        mk_seg("segments/000001", vec![h1.clone()]),
        mk_seg("segments/000002", vec![h2.clone()]),
    ];

    let (hits, _cursor, _cand, dedup_dropped, _totals) =
        Paginator::merge(parts, /*page_size*/ 10);

    assert_eq!(
        hits.len(),
        1,
        "остаться должен один hit (уникальный ext_id)"
    );
    assert_eq!(hits[0].ext_id, "same-ext-id");
    assert_eq!(dedup_dropped, 1, "ровно один дубликат должен быть отброшен");
}
