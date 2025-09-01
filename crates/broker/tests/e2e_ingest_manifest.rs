// path: crates/broker/tests/e2e_ingest_manifest.rs

use axum::{body::Body, http::{Request, StatusCode}};
use broker::config::BrokerConfig;
use http_body_util::BodyExt as _;
use serde_json::json;
use tower::ServiceExt;

mod helpers;
use helpers::make_router_with_config;


async fn http_get_manifest(app: &axum::Router, shard: u64) -> serde_json::Value {
    let req = Request::builder()
        .method("GET")
        .uri(format!("/manifest/{shard}"))
        .body(Body::empty())
        .unwrap();
    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK, "GET /manifest/{shard}");
    let bytes = resp.into_body().collect().await.unwrap().to_bytes();
    serde_json::from_slice(&bytes).unwrap()
}

#[tokio::test]
async fn ingest_appends_segment_to_manifest_for_single_shard() {
    let tmp = tempfile::tempdir().unwrap();
    let cfg = BrokerConfig {
        addr: "127.0.0.1:0".into(),
        wal_dir: tmp.path().join("wal").to_string_lossy().to_string(),
        segment_out_dir: tmp.path().join("segments").to_string_lossy().to_string(),
        parallelism: 1,
        hot_cap: 10_000,
        manifest_path: Some(tmp.path().join("manifest.json").to_string_lossy().to_string()),
        shard: 42,
    };

    let app = make_router_with_config(cfg.clone());

    let docs = json!([
      {"_id":"a","text":{"body":"alpha"}},
      {"_id":"b","text":{"body":"beta"}}
    ]);
    let req = Request::builder().method("POST").uri("/ingest")
        .header("content-type","application/json")
        .body(Body::from(docs.to_string())).unwrap();
    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK, "POST /ingest");

    let m = http_get_manifest(&app, 42).await;
    assert_eq!(m["shard"], 42);
    assert_eq!(m["gen"], 1);
    assert!(m["segments"].as_array().is_some());
    assert!(!m["segments"].as_array().unwrap().is_empty());
}

#[tokio::test]
async fn second_ingest_increments_generation() {
    let tmp = tempfile::tempdir().unwrap();
    let cfg = BrokerConfig {
        addr: "127.0.0.1:0".into(),
        wal_dir: tmp.path().join("wal").to_string_lossy().to_string(),
        segment_out_dir: tmp.path().join("segments").to_string_lossy().to_string(),
        parallelism: 1,
        hot_cap: 10_000,
        manifest_path: Some(tmp.path().join("manifest.json").to_string_lossy().to_string()),
        shard: 7,
    };

    let app = make_router_with_config(cfg);

    // 1-й батч → gen=1
    let docs1 = json!([{"_id":"x","text":{"body":"first"}}]);
    let req1 = Request::builder().method("POST").uri("/ingest")
        .header("content-type","application/json")
        .body(Body::from(docs1.to_string())).unwrap();
    let r1 = app.clone().oneshot(req1).await.unwrap();
    assert_eq!(r1.status(), StatusCode::OK);

    let m1 = http_get_manifest(&app, 7).await;
    assert_eq!(m1["gen"], 1, "after first ingest gen must be 1");

    // 2-й батч → gen=2
    let docs2 = json!([{"_id":"y","text":{"body":"second"}}]);
    let req2 = Request::builder().method("POST").uri("/ingest")
        .header("content-type","application/json")
        .body(Body::from(docs2.to_string())).unwrap();
    let r2 = app.clone().oneshot(req2).await.unwrap();
    assert_eq!(r2.status(), StatusCode::OK);

    let m2 = http_get_manifest(&app, 7).await;
    assert_eq!(m2["gen"], 2, "after second ingest gen must be 2");
}
