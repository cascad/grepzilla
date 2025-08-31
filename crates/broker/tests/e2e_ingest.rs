use axum::{body::Body, http::{Request, StatusCode}};
use tower::ServiceExt;
use serde_json::json;
use http_body_util::BodyExt as _; // добавляем для collect()

mod helpers;
use helpers::make_router_with_parallelism;

#[tokio::test]
async fn ingest_then_immediate_search_from_hotmem() {
    // поднимаем роутер на 2 воркера
    let app = make_router_with_parallelism(2);

    // 1) /ingest — добавляем два документа
    let docs = json!([
        {"_id":"hot1","text":{"body":"свежее сообщение играет"}},
        {"_id":"hot2","text":{"body":"ещё горячее тоже играет"}}
    ]);
    let req = Request::builder()
        .method("POST")
        .uri("/ingest")
        .header("content-type", "application/json")
        .body(Body::from(docs.to_string()))
        .unwrap();

    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    // 2) /search — сразу ищем без shards/segments
    let search_body = json!({
        "wildcard": "*игра*",
        "page": { "size": 10 }
    });

    let req = Request::builder()
        .method("POST")
        .uri("/search")
        .header("content-type", "application/json")
        .body(Body::from(search_body.to_string()))
        .unwrap();

    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    // читаем тело без hyper
    let bytes = resp.into_body().collect().await.unwrap().to_bytes();
    let v: serde_json::Value = serde_json::from_slice(&bytes).unwrap();

    // Проверки: есть хиты, ext_id hot1/hot2 присутствуют
    let hits = v.get("hits").and_then(|h| h.as_array()).unwrap();
    let ids: Vec<_> = hits
        .iter()
        .map(|h| h.get("ext_id").unwrap().as_str().unwrap().to_string())
        .collect();

    assert!(ids.contains(&"hot1".to_string()), "missing hot1 in {:?}", ids);
    assert!(ids.contains(&"hot2".to_string()), "missing hot2 in {:?}", ids);

    // Бонус: метрики присутствуют
    assert!(v.get("metrics").is_some());
}
