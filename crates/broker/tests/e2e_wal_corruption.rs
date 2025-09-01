// path: crates/broker/tests/e2e_wal_corruption.rs
use axum::{http::{Request, StatusCode}, body::Body};
use tower::ServiceExt;
use serde_json::json;
mod helpers; use helpers::make_router_with_config;
use broker::config::BrokerConfig;

#[tokio::test]
async fn corrupt_wal_sidecar_yields_segment_error_but_ok() {
    let tmp = tempfile::tempdir().unwrap();
    let cfg = BrokerConfig {
        addr:"127.0.0.1:0".into(),
        wal_dir: tmp.path().join("wal").to_string_lossy().to_string(),
        segment_out_dir: tmp.path().join("segments").to_string_lossy().to_string(),
        parallelism:1, hot_cap:10_000,
        manifest_path: Some(tmp.path().join("manifest.json").to_string_lossy().to_string()),
        shard: 1,
    };
    let app = make_router_with_config(cfg);

    // нормальный ingest
    let req = Request::builder().method("POST").uri("/ingest")
        .header("content-type","application/json")
        .body(Body::from(json!([{"_id":"a","text":{"body":"x"}}]).to_string())).unwrap();
    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    // найдём последний wal и испортим сайдкар
    let wal_dir = tmp.path().join("wal");
    let mut paths: Vec<_> = std::fs::read_dir(&wal_dir).unwrap().map(|e| e.unwrap().path()).collect();
    paths.sort();
    let side = paths.into_iter().find(|p| p.extension().and_then(|s| s.to_str()) == Some("xxh3")).unwrap();
    std::fs::write(&side, b"deadbeef").unwrap();

    // второй ingest вернёт 200, но в теле будет segment_error
    let req2 = Request::builder().method("POST").uri("/ingest")
        .header("content-type","application/json")
        .body(Body::from(json!([{"_id":"b","text":{"body":"y"}}]).to_string())).unwrap();
    let resp2 = app.clone().oneshot(req2).await.unwrap();
    assert_eq!(resp2.status(), StatusCode::OK);
    let body = http_body_util::BodyExt::collect(resp2.into_body()).await.unwrap().to_bytes();
    let v: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert!(v.get("segment_error").is_some(), "expected segment_error: {v}");
}
