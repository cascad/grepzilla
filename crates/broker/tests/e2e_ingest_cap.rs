// crates/broker/tests/e2e_ingest_cap.rs
use axum::{body::Body, http::{Request, StatusCode}};
use tower::ServiceExt;
use serde_json::json;
use http_body_util::BodyExt as _;

mod helpers;
// было: use helpers::make_router_with_parallelism;
use helpers::make_router_with_parallelism_and_cap;

#[tokio::test]
async fn hotmem_respects_cap() {
    // cap = 3, чтобы "1","2" выкинулись
    let app = make_router_with_parallelism_and_cap(1, 3);

    // 1) ingest 5 документов
    let docs = json!([
      {"_id":"1","text":{"body":"doc1 играет"}},
      {"_id":"2","text":{"body":"doc2 играет"}},
      {"_id":"3","text":{"body":"doc3 играет"}},
      {"_id":"4","text":{"body":"doc4 играет"}},
      {"_id":"5","text":{"body":"doc5 играет"}}
    ]);
    let req = Request::builder().method("POST").uri("/ingest")
      .header("content-type","application/json")
      .body(Body::from(docs.to_string())).unwrap();
    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    // 2) search
    let body = json!({"wildcard":"*игра*","page":{"size":10}});
    let req = Request::builder().method("POST").uri("/search")
      .header("content-type","application/json")
      .body(Body::from(body.to_string())).unwrap();
    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    let bytes = resp.into_body().collect().await.unwrap().to_bytes();
    let v: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    let ids: Vec<String> = v["hits"].as_array().unwrap()
      .iter().map(|h| h["ext_id"].as_str().unwrap().to_string()).collect();

    // теперь должны отсутствовать "1" и "2"
    assert!(!ids.contains(&"1".to_string()), "unexpected 1 in {:?}", ids);
    assert!(!ids.contains(&"2".to_string()), "unexpected 2 in {:?}", ids);
    assert!(ids.iter().any(|x| x=="3"));
    assert!(ids.iter().any(|x| x=="4"));
    assert!(ids.iter().any(|x| x=="5"));
}
