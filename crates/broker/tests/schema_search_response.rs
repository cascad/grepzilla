use broker::search::types::*;
use serde_json::json;

#[test]
fn search_response_schema_minimal() {
    // Минимальный ответ без хитов, курсор пустой, опциональные метрики -> null
    let resp = SearchResponse {
        hits: vec![],
        cursor: Some(SearchCursor::default()),
        metrics: SearchMetrics {
            candidates_total: 0,
            time_to_first_hit_ms: 0,
            deadline_hit: false,
            saturated_sem: 0,
            dedup_dropped: 0,
            prefilter_ms: None,
            verify_ms: None,
            prefetch_ms: None,
            warmed_docs: None,
        },
    };

    let v = serde_json::to_value(&resp).unwrap();

    // Верхний уровень
    assert!(v.get("hits").is_some());
    assert!(v.get("cursor").is_some());
    assert!(v.get("metrics").is_some());

    // Поля метрик и их snake_case ключи
    let m = v.get("metrics").unwrap();
    for key in [
        "candidates_total",
        "time_to_first_hit_ms",
        "deadline_hit",
        "saturated_sem",
        "dedup_dropped",
        "prefilter_ms",
        "verify_ms",
        "prefetch_ms",
        "warmed_docs",
    ] {
        assert!(m.get(key).is_some(), "missing key: {key}");
    }

    // Опциональные — именно null
    assert!(m.get("prefilter_ms").unwrap().is_null());
    assert!(m.get("verify_ms").unwrap().is_null());
    assert!(m.get("prefetch_ms").unwrap().is_null());
    assert!(m.get("warmed_docs").unwrap().is_null());

    // cursor.per_seg — объект
    let c = v.get("cursor").unwrap();
    let per_seg = c.get("per_seg").unwrap();
    assert!(per_seg.is_object());
    // pin_gen по умолчанию может отсутствовать (Option), проверим корректный дефолт
    assert!(c.get("pin_gen").is_none() || c.get("pin_gen").unwrap().is_object());
}

#[test]
fn search_response_schema_full_hit() {
    let resp = SearchResponse {
        hits: vec![Hit {
            ext_id: "abc".into(),
            doc_id: 123,
            matched_field: "text.body".into(),
            preview: "...".into(),
        }],
        cursor: Some(SearchCursor {
            per_seg: std::iter::once((
                "segments/000001".to_string(),
                PerSegPos { last_docid: 42 },
            ))
            .collect(),
            pin_gen: Some(std::iter::once((0u64, 7u64)).collect()),
        }),
        metrics: SearchMetrics {
            candidates_total: 10,
            time_to_first_hit_ms: 3,
            deadline_hit: false,
            saturated_sem: 0,
            dedup_dropped: 1,
            prefilter_ms: Some(5),
            verify_ms: Some(4),
            prefetch_ms: Some(1),
            warmed_docs: Some(2),
        },
    };

    let v = serde_json::to_value(&resp).unwrap();

    // hit-объект: snake_case ключи
    let h = &v["hits"][0];
    for key in ["ext_id", "doc_id", "matched_field", "preview"] {
        assert!(h.get(key).is_some(), "missing hit key: {key}");
    }

    // cursor: per_seg -> { "segments/000001": { "last_docid": 42 } }, pin_gen -> { "0": 7 }
    assert_eq!(v["cursor"]["per_seg"]["segments/000001"]["last_docid"], 42);
    assert_eq!(v["cursor"]["pin_gen"]["0"], 7);

    // Метрики заполнены Some(...)
    assert_eq!(v["metrics"]["prefilter_ms"], json!(5));
    assert_eq!(v["metrics"]["verify_ms"], json!(4));
    assert_eq!(v["metrics"]["prefetch_ms"], json!(1));
    assert_eq!(v["metrics"]["warmed_docs"], json!(2));
}
