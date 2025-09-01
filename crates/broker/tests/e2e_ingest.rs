// path: crates/broker/tests/e2e_ingest.rs

use axum::{http::{Request, StatusCode}, body::Body};
use http_body_util::BodyExt as _;
use serde_json::json;
use tower::ServiceExt;

// NEW: сериализация доступа к process-wide env
use std::sync::{Mutex, OnceLock};
static ENV_LOCK: OnceLock<Mutex<()>> = OnceLock::new();

mod helpers;
use helpers::make_router_with_parallelism;

#[tokio::test]
async fn ingest_then_immediate_search_from_hotmem() {
    // сериализуем и ставим env ДО Router
    let _guard = ENV_LOCK.get_or_init(|| Mutex::new(())).lock().unwrap();

    let tmp = tempfile::tempdir().unwrap();
    std::env::set_var("GZ_WAL_DIR", tmp.path().join("wal"));
    std::env::set_var("GZ_SHARD", "7");
    std::env::set_var("GZ_MANIFEST", tmp.path().join("manifest.json"));
    // при необходимости можно поджать лимиты HotMem:
    std::env::set_var("GZ_HOT_SOFT", "1000");
    std::env::set_var("GZ_HOT_HARD", "2000");

    let app = make_router_with_parallelism(1);

    // 1) ingest
    let docs = json!([{"_id":"x","text":{"body":"first"}},{"_id":"y","text":{"body":"second"}}]).to_string();
    let req = Request::builder()
        .method("POST")
        .uri("/ingest")
        .header("content-type", "application/json")
        .body(Body::from(docs))
        .unwrap();
    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK, "POST /ingest must be 200");

    // 2) immediate search (тут оставь твой реальный запрос)
    // Примерно так, если ищешь по segments/unified манифесту — замени на свой:
    let search_req = json!({
        "wildcard": "*first*",
        "field": "text.body",
        "segments": [],        // если у тебя тест ходит только в hot, можешь оставить пусто
        "page": { "size": 10, "cursor": null },
        "limits": { "parallelism": 2, "deadline_ms": 100, "max_candidates": 200000 }
    }).to_string();

    let req2 = Request::builder()
        .method("POST")
        .uri("/search")
        .header("content-type", "application/json")
        .body(Body::from(search_req))
        .unwrap();
    let resp2 = app.clone().oneshot(req2).await.unwrap();
    assert_eq!(resp2.status(), StatusCode::OK, "POST /search must be 200");

    // cleanup
    std::env::remove_var("GZ_WAL_DIR");
    std::env::remove_var("GZ_SHARD");
    std::env::remove_var("GZ_MANIFEST");
    std::env::remove_var("GZ_HOT_SOFT");
    std::env::remove_var("GZ_HOT_HARD");
}
