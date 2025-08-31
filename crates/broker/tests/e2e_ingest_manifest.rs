use axum::{body::Body, http::{Request, StatusCode}};
use http_body_util::BodyExt as _;
use serde_json::json;
use tower::ServiceExt;

mod helpers;
use helpers::make_router_with_parallelism;

/// Утилита: GET /manifest/:shard и вернуть JSON
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
    // 1) поднимаем приложение
    let app = make_router_with_parallelism(1);

    // 2) укажем переменные окружения для публикации
    //    манифест будет лежать в temp каталоге, путь подхватит брокер
    let tmp = tempfile::tempdir().unwrap();
    let manifest_path = tmp.path().join("manifest.json");
    std::env::set_var("GZ_MANIFEST", &manifest_path);
    std::env::set_var("GZ_SHARD", "42");

    // 3) /ingest
    let docs = json!([
      {"_id":"a","text":{"body":"alpha"}},
      {"_id":"b","text":{"body":"beta"}}
    ]);
    let req = Request::builder().method("POST").uri("/ingest")
        .header("content-type","application/json")
        .body(Body::from(docs.to_string())).unwrap();
    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    // 4) проверим через HTTP /manifest/42
    let m = http_get_manifest(&app, 42).await;
    // ожидаем вид: { "shard": 42, "gen": 1, "segments": [ ... ] }
    assert_eq!(m["shard"], 42);
    assert_eq!(m["gen"], 1);
    assert!(m["segments"].as_array().is_some());
    assert!(!m["segments"].as_array().unwrap().is_empty());

    // cleanup env
    std::env::remove_var("GZ_MANIFEST");
    std::env::remove_var("GZ_SHARD");
}

#[tokio::test]
async fn second_ingest_increments_generation() {
    let app = make_router_with_parallelism(1);

    let tmp = tempfile::tempdir().unwrap();
    let manifest_path = tmp.path().join("manifest.json");
    std::env::set_var("GZ_MANIFEST", &manifest_path);
    std::env::set_var("GZ_SHARD", "7");

    // первый батч
    let docs1 = json!([{"_id":"x","text":{"body":"first"}}]);
    let req1 = Request::builder().method("POST").uri("/ingest")
        .header("content-type","application/json")
        .body(Body::from(docs1.to_string())).unwrap();
    let r1 = app.clone().oneshot(req1).await.unwrap();
    assert_eq!(r1.status(), StatusCode::OK);

    // проверка gen=1
    let m1 = http_get_manifest(&app, 7).await;
    assert_eq!(m1["gen"], 1, "after first ingest gen must be 1");

    // второй батч
    let docs2 = json!([{"_id":"y","text":{"body":"second"}}]);
    let req2 = Request::builder().method("POST").uri("/ingest")
        .header("content-type","application/json")
        .body(Body::from(docs2.to_string())).unwrap();
    let r2 = app.clone().oneshot(req2).await.unwrap();
    assert_eq!(r2.status(), StatusCode::OK);

    // проверка gen=2
    let m2 = http_get_manifest(&app, 7).await;
    assert_eq!(m2["gen"], 2, "after second ingest gen must be 2");

    std::env::remove_var("GZ_MANIFEST");
    std::env::remove_var("GZ_SHARD");
}
