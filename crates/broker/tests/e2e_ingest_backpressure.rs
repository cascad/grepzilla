// path: crates/broker/tests/e2e_ingest_backpressure.rs

use axum::{
    body::Body,
    http::{Request, StatusCode},
};
use broker::config::BrokerConfig;
use serde_json::json;
use tower::ServiceExt;

mod helpers;
use helpers::make_router_with_config;

#[tokio::test]
async fn returns_503_when_hotmem_hard_cap_reached() {
    let tmp = tempfile::tempdir().unwrap();
    let cfg = BrokerConfig {
        addr: "127.0.0.1:0".into(),
        wal_dir: tmp.path().join("wal").to_string_lossy().to_string(),
        segment_out_dir: tmp.path().join("segments").to_string_lossy().to_string(),
        parallelism: 1,
        hot_cap: 1, // важное место
        manifest_path: Some(
            tmp.path()
                .join("manifest.json")
                .to_string_lossy()
                .to_string(),
        ),
        shard: 1,
    };

    let app = make_router_with_config(cfg);

    let doc = json!([{"_id":"x","text":{"body":"a"}}]).to_string();

    // первый проходит
    let req1 = Request::builder()
        .method("POST")
        .uri("/ingest")
        .header("content-type", "application/json")
        .body(Body::from(doc.clone()))
        .unwrap();
    let r1 = app.clone().oneshot(req1).await.unwrap();
    assert_eq!(r1.status(), StatusCode::OK);

    // второй — 503
    let req2 = Request::builder()
        .method("POST")
        .uri("/ingest")
        .header("content-type", "application/json")
        .body(Body::from(doc))
        .unwrap();
    let r2 = app.clone().oneshot(req2).await.unwrap();
    assert_eq!(r2.status(), StatusCode::SERVICE_UNAVAILABLE);
}
