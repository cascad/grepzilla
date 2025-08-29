use std::fs::{self, File};
use std::io::Write;
use std::sync::Arc;

use axum::{
    body::Body,
    http::{Request, StatusCode},
};
use serde_json::json;
use tempfile::TempDir;
use tower::ServiceExt; // для oneshot()

use broker::http_api::{router, AppState};
use broker::search::SearchCoordinator;
use grepzilla_segment::segjson::JsonSegmentWriter;
use grepzilla_segment::SegmentWriter;

#[tokio::test]
async fn end_to_end_search_returns_hits() {
    // 1. Временный каталог + сегмент
    let tmp = TempDir::new().unwrap();
    let seg_dir = tmp.path().join("segA");
    fs::create_dir_all(&seg_dir).unwrap();

    let in_path = tmp.path().join("input.jsonl");
    let mut f = File::create(&in_path).unwrap();
    writeln!(
        f,
        r#"{{"_id":"1","text":{{"body":"котенок играет с клубком"}}}}"#
    )
    .unwrap();
    writeln!(
        f,
        r#"{{"_id":"2","text":{{"body":"щенок играет с мячиком"}}}}"#
    )
    .unwrap();

    let mut writer = JsonSegmentWriter::default();
    writer
        .write_segment(in_path.to_str().unwrap(), seg_dir.to_str().unwrap())
        .unwrap();

    // 2. Поднимаем in-memory Router
    let coord = Arc::new(SearchCoordinator::new(2));
    let app = router(AppState { coord });

    // Тело запроса через serde_json::json!
    let body_val = json!({
        "wildcard": "*игра*",
        "field": "text.body",                // Можно и опустить из-за #[serde(default)]
        "segments": [ seg_dir.to_string_lossy() ],
        // "shards": null,                    // можно не указывать
        "page": { "size": 10, "cursor": null },
        "limits": { "parallelism": 2, "deadline_ms": 1000, "max_candidates": 1000 }
    });
    let body = serde_json::to_vec(&body_val).unwrap();

    let req = Request::builder()
        .method("POST")
        .uri("/search")
        .header("content-type", "application/json")
        .body(Body::from(body))
        .unwrap();

    let resp = app.clone().into_service().oneshot(req).await.unwrap();
    let status = resp.status();
    let bytes = axum::body::to_bytes(resp.into_body(), 64 * 1024)
        .await
        .unwrap();

    // Если не 200 — напечатаем тело с ошибкой для диагностики
    if status != StatusCode::OK {
        panic!("HTTP {}: {}", status, String::from_utf8_lossy(&bytes));
    }

    let v: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    let hits = v["hits"].as_array().unwrap();
    assert_eq!(hits.len(), 2, "unexpected hits: {}", v);
    assert!(
        v["metrics"]["candidates_total"].as_u64().unwrap() >= 2,
        "bad metrics: {}",
        v
    );
}

#[tokio::test]
async fn end_to_end_search_two_segments() {
    use axum::{body::Body, http::Request};
    use serde_json::json;
    use std::fs::File;
    use std::io::Write;
    use std::sync::Arc;
    use tempfile::TempDir;
    use tower::ServiceExt;

    use broker::http_api::{router, AppState};
    use broker::search::SearchCoordinator;
    use grepzilla_segment::segjson::JsonSegmentWriter;
    use grepzilla_segment::SegmentWriter;

    // tmp каталог
    let tmp = TempDir::new().unwrap();

    // сегмент A
    let seg_a = tmp.path().join("segA");
    std::fs::create_dir_all(&seg_a).unwrap();
    let in_a = tmp.path().join("in_a.jsonl");
    let mut f = File::create(&in_a).unwrap();
    writeln!(f, r#"{{"_id":"A1","text":{{"body":"первая игра"}}}}"#).unwrap();
    writeln!(f, r#"{{"_id":"A2","text":{{"body":"ещё одна игра"}}}}"#).unwrap();
    let mut w = JsonSegmentWriter::default();
    w.write_segment(in_a.to_str().unwrap(), seg_a.to_str().unwrap())
        .unwrap();

    // сегмент B
    let seg_b = tmp.path().join("segB");
    std::fs::create_dir_all(&seg_b).unwrap();
    let in_b = tmp.path().join("in_b.jsonl");
    let mut f = File::create(&in_b).unwrap();
    writeln!(f, r#"{{"_id":"B1","text":{{"body":"новая игра"}}}}"#).unwrap();
    writeln!(f, r#"{{"_id":"B2","text":{{"body":"игра финальная"}}}}"#).unwrap();
    let mut w = JsonSegmentWriter::default();
    w.write_segment(in_b.to_str().unwrap(), seg_b.to_str().unwrap())
        .unwrap();

    // router
    let coord = Arc::new(SearchCoordinator::new(2));
    let app = router(AppState { coord });

    // запрос сразу по двум сегментам
    let body_val = json!({
        "wildcard": "*игра*",
        "field": "text.body",
        "segments": [ seg_a.to_string_lossy(), seg_b.to_string_lossy() ],
        "page": { "size": 10, "cursor": null },
        "limits": { "parallelism": 2, "deadline_ms": 1000, "max_candidates": 1000 }
    });
    let req = Request::builder()
        .method("POST")
        .uri("/search")
        .header("content-type", "application/json")
        .body(Body::from(serde_json::to_vec(&body_val).unwrap()))
        .unwrap();

    let resp = app.clone().oneshot(req).await.unwrap();
    let status = resp.status();
    let bytes = axum::body::to_bytes(resp.into_body(), 64 * 1024)
        .await
        .unwrap();
    assert_eq!(
        status,
        200,
        "HTTP error: {}",
        String::from_utf8_lossy(&bytes)
    );

    let v: serde_json::Value = serde_json::from_slice(&bytes).unwrap();

    // оба сегмента должны дать хиты
    let hits = v["hits"].as_array().unwrap();
    assert!(hits.len() >= 4, "Expected >=4 hits, got {}", hits.len());

    // курсор хранит позиции отдельно
    let per_seg = v["cursor"]["per_seg"].as_object().unwrap();
    assert!(per_seg.contains_key(&seg_a.to_string_lossy().to_string()));
    assert!(per_seg.contains_key(&seg_b.to_string_lossy().to_string()));
}
