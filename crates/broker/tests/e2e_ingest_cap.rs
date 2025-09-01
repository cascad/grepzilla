// path: crates/broker/tests/e2e_ingest_cap.rs
use axum::{
    body::Body,
    http::{Request, StatusCode},
};
use http_body_util::BodyExt as _;
use serde_json::json;
use tower::ServiceExt;

mod helpers;
use helpers::make_router_with_config;

use broker::config::BrokerConfig;

#[tokio::test]
async fn hotmem_respects_cap() {
    // собираем изолированный конфиг без env
    let tmp = tempfile::tempdir().unwrap();
    let cfg = BrokerConfig {
        addr: "127.0.0.1:0".into(),
        wal_dir: tmp.path().join("wal").to_string_lossy().to_string(),
        segment_out_dir: tmp.path().join("segments").to_string_lossy().to_string(),
        parallelism: 1,
        hot_cap: 3, // cap = 3, чтобы "1","2" выкинулись
        manifest_path: None,
        shard: 0,
    };

    let app = make_router_with_config(cfg);

    // 1) ingest 5 документов
    let docs = json!([
      {"_id":"1","text":{"body":"doc1 играет"}},
      {"_id":"2","text":{"body":"doc2 играет"}},
      {"_id":"3","text":{"body":"doc3 играет"}},
      {"_id":"4","text":{"body":"doc4 играет"}},
      {"_id":"5","text":{"body":"doc5 играет"}}
    ]);
    let req = Request::builder()
        .method("POST")
        .uri("/ingest")
        .header("content-type", "application/json")
        .body(Body::from(docs.to_string()))
        .unwrap();
    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    // 2) search по нужному полю (важно указать field)
    let body = json!({
        "wildcard":"*игра*",
        "field":"text.body",
        "page":{"size":10}
    });
    let req = Request::builder()
        .method("POST")
        .uri("/search")
        .header("content-type", "application/json")
        .body(Body::from(body.to_string()))
        .unwrap();
    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    let bytes = resp.into_body().collect().await.unwrap().to_bytes();
    let v: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    let ids: Vec<String> = v["hits"]
        .as_array()
        .unwrap()
        .iter()
        .map(|h| h["ext_id"].as_str().unwrap().to_string())
        .collect();

    // теперь должны отсутствовать "1" и "2", а "3","4","5" быть
    assert!(!ids.contains(&"1".to_string()), "unexpected 1 in {:?}", ids);
    assert!(!ids.contains(&"2".to_string()), "unexpected 2 in {:?}", ids);
    assert!(ids.iter().any(|x| x == "3"), "expected 3 in {:?}", ids);
    assert!(ids.iter().any(|x| x == "4"), "expected 4 in {:?}", ids);
    assert!(ids.iter().any(|x| x == "5"), "expected 5 in {:?}", ids);
}
