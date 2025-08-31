use axum::{body::Body, http::{Request, StatusCode}};
use std::{sync::Arc};
use tempfile::tempdir;
use tower::ServiceExt;

#[tokio::test]
async fn manifest_404_for_unknown_shard() {
    let dir = tempdir().unwrap();
    let manifest_path = dir.path().join("manifest.json");
    // Только shard=1 опубликован
    std::fs::write(
        &manifest_path,
        r#"{
            "shards": { "1": 1 },
            "segments": { "1:1": ["segments/xyz"] }
        }"#,
    ).unwrap();
    std::env::set_var("GZ_MANIFEST", manifest_path.to_string_lossy().to_string());

    let coord = broker::search::SearchCoordinator::new(2);
    let app = broker::http_api::router(broker::http_api::AppState { coord: Arc::new(coord) });

    let resp = app
        .oneshot(Request::builder().uri("/manifest/0").method("GET").body(Body::empty()).unwrap())
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
}
