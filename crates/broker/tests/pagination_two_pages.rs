// crates/broker/tests/pagination_two_pages.rs
use broker::search::executor::SegmentTaskOutput;
use broker::search::paginator::Paginator;
use broker::search::types::{Hit, PerSegPos, SearchCursor};
use std::collections::HashMap;

fn mk_seg(seg_path: &str, hits: Vec<Hit>, last_docid: u64) -> SegmentTaskOutput {
    SegmentTaskOutput {
        seg_path: seg_path.to_string(),
        last_docid: Some(last_docid),
        candidates: hits.len() as u64,
        hits,
        prefilter_ms: 0,
        verify_ms: 0,
        prefetch_ms: 0,
        warmed_docs: 0,
    }
}

#[test]
fn two_pages_do_not_overlap() {
    // Page 1: возьмем 3 уникальных ext_id
    let p1_parts = vec![
        mk_seg(
            "segments/A",
            vec![
                Hit {
                    ext_id: "id-1".into(),
                    doc_id: 1,
                    matched_field: "text".into(),
                    preview: "...".into(),
                },
                Hit {
                    ext_id: "id-2".into(),
                    doc_id: 2,
                    matched_field: "text".into(),
                    preview: "...".into(),
                },
            ],
            /*last_docid*/ 2,
        ),
        mk_seg(
            "segments/B",
            vec![Hit {
                ext_id: "id-3".into(),
                doc_id: 7,
                matched_field: "text".into(),
                preview: "...".into(),
            }],
            /*last_docid*/ 7,
        ),
    ];
    let (hits1, cursor1, _c1, _d1, _t1) = Paginator::merge(p1_parts, /*page_size*/ 3);
    assert_eq!(hits1.len(), 3);

    // Имитируем второй запрос: из cursor1 читается last_docid per seg (в реале это делает coordinator),
    // а storage вернёт нам уже "следующие" документы (моделируем другими hits).
    let p2_parts = vec![
        mk_seg(
            "segments/A",
            vec![Hit {
                ext_id: "id-4".into(),
                doc_id: 3,
                matched_field: "text".into(),
                preview: "...".into(),
            }],
            /*last_docid*/ 3,
        ),
        mk_seg(
            "segments/B",
            vec![Hit {
                ext_id: "id-5".into(),
                doc_id: 8,
                matched_field: "text".into(),
                preview: "...".into(),
            }],
            /*last_docid*/ 8,
        ),
    ];
    let (hits2, cursor2, _c2, _d2, _t2) = Paginator::merge(p2_parts, /*page_size*/ 3);
    assert_eq!(hits2.len(), 2);

    // Проверяем отсутствие пересечения по ext_id
    let s1: std::collections::BTreeSet<_> = hits1.iter().map(|h| h.ext_id.as_str()).collect();
    let s2: std::collections::BTreeSet<_> = hits2.iter().map(|h| h.ext_id.as_str()).collect();
    assert!(s1.is_disjoint(&s2), "страницы не должны повторять hits");

    // Курсор обновился по обоим сегментам
    assert_eq!(cursor1.per_seg.get("segments/A").unwrap().last_docid, 2);
    assert_eq!(cursor1.per_seg.get("segments/B").unwrap().last_docid, 7);
    assert_eq!(cursor2.per_seg.get("segments/A").unwrap().last_docid, 3);
    assert_eq!(cursor2.per_seg.get("segments/B").unwrap().last_docid, 8);
}
