// crates/broker/tests/e2e_manifest.rs
use axum::{
    body::Body,
    http::{Request, StatusCode},
};
use std::fs;
use tempfile::tempdir;
use tower::ServiceExt;

mod helpers;
use helpers::make_router_with_parallelism;

#[tokio::test]
async fn manifest_get_ok() {
    // 1) Подготовим временный manifest.json совместимый с FsManifestStore
    // Простой «плоский» формат: shards { shard: gen }, segments { "shard:gen": [paths...] }
    let dir = tempdir().unwrap();
    let manifest_path = dir.path().join("manifest.json");
    fs::write(
        &manifest_path,
        r#"{
            "shards": { "0": 7 },
            "segments": { "0:7": ["segments/000001","segments/000002"] }
        }"#,
    )
    .unwrap();

    // Пропишем путь через переменную окружения, как делает http_api::get_manifest
    std::env::set_var("GZ_MANIFEST", manifest_path.to_string_lossy().to_string());

    let app = make_router_with_parallelism(2);

    // 3) Запрос
    let resp = app
        .oneshot(
            Request::builder()
                .uri("/manifest/0")
                .method("GET")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::OK);

    let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX)
        .await
        .unwrap();
    let v: serde_json::Value = serde_json::from_slice(&bytes).unwrap();

    // Проверки структуры
    assert_eq!(v["shard"], 0);
    assert_eq!(v["gen"], 7);
    let segs = v["segments"].as_array().unwrap();
    assert_eq!(segs.len(), 2);
    assert_eq!(segs[0], "segments/000001");
    assert_eq!(segs[1], "segments/000002");
}
