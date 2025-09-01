// path: crates/broker/tests/e2e_ingest_idempotent.rs

use axum::{
    body::Body,
    http::{Request, StatusCode},
};
use broker::config::BrokerConfig;
use http_body_util::BodyExt as _;
use serde_json::json;
use tower::ServiceExt;

mod helpers;
use helpers::make_router_with_config;

#[tokio::test]
async fn idempotent_post_is_not_duplicated() {
    let tmp = tempfile::tempdir().unwrap();
    let cfg = BrokerConfig {
        addr: "127.0.0.1:0".into(),
        wal_dir: tmp.path().join("wal").to_string_lossy().to_string(),
        segment_out_dir: tmp.path().join("segments").to_string_lossy().to_string(),
        parallelism: 1,
        hot_cap: 10,
        manifest_path: Some(
            tmp.path()
                .join("manifest.json")
                .to_string_lossy()
                .to_string(),
        ),
        shard: 1,
    };

    let app = make_router_with_config(cfg);

    let body = json!([{"_id":"k1","text":{"body":"hello"}}]).to_string();

    let req1 = Request::builder()
        .method("POST")
        .uri("/ingest")
        .header("content-type", "application/json")
        .header("Idempotency-Key", "abc-123")
        .body(Body::from(body.clone()))
        .unwrap();
    let r1 = app.clone().oneshot(req1).await.unwrap();
    assert_eq!(r1.status(), StatusCode::OK);

    let req2 = Request::builder()
        .method("POST")
        .uri("/ingest")
        .header("content-type", "application/json")
        .header("Idempotency-Key", "abc-123")
        .body(Body::from(body))
        .unwrap();
    let r2 = app.clone().oneshot(req2).await.unwrap();
    assert_eq!(r2.status(), StatusCode::OK);
    let bytes = r2.into_body().collect().await.unwrap().to_bytes();
    let v: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(v["idempotent"], true);
}
