// crates/broker/tests/metrics_null.rs
use broker::search::executor::SegmentTaskOutput;
use broker::search::paginator::Paginator;

#[test]
fn metrics_are_null_when_empty() {
    // Пустые parts имитируют: ничего не нашли/не запускали.
    let parts: Vec<SegmentTaskOutput> = Vec::new();
    let (hits, cursor, candidates_total, dedup_dropped, totals) =
        Paginator::merge(parts, /*page_size*/ 10);

    // Собираем ответ так же, как координатор делает (упрощённо, без времени/флагов)
    let (prefilter_ms_total, verify_ms_total, prefetch_ms_total, warmed_docs_total) = totals;

    let has_any_metrics = candidates_total > 0
        || prefilter_ms_total > 0
        || verify_ms_total > 0
        || prefetch_ms_total > 0
        || warmed_docs_total > 0;

    let resp = broker::search::types::SearchResponse {
        hits,
        cursor: Some(cursor),
        metrics: broker::search::types::SearchMetrics {
            candidates_total,
            time_to_first_hit_ms: 0,
            deadline_hit: false,
            saturated_sem: 0,
            dedup_dropped,
            prefilter_ms: if has_any_metrics { Some(prefilter_ms_total) } else { None },
            verify_ms:   if has_any_metrics { Some(verify_ms_total)   } else { None },
            prefetch_ms: if has_any_metrics { Some(prefetch_ms_total) } else { None },
            warmed_docs: if has_any_metrics { Some(warmed_docs_total) } else { None },
        },
    };

    // Проверяем именно сериализацию в JSON → null
    let j = serde_json::to_value(&resp).expect("json");
    let m = &j["metrics"];
    assert_eq!(m["prefilter_ms"], serde_json::Value::Null);
    assert_eq!(m["verify_ms"],   serde_json::Value::Null);
    assert_eq!(m["prefetch_ms"], serde_json::Value::Null);
    assert_eq!(m["warmed_docs"], serde_json::Value::Null);
}
