use axum::{
    body::Body,
    http::{Request, StatusCode},
};
use tempfile::tempdir;
use tower::ServiceExt;

mod helpers;
use helpers::make_router_with_parallelism;

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
    )
    .unwrap();
    std::env::set_var("GZ_MANIFEST", manifest_path.to_string_lossy().to_string());

    let app = make_router_with_parallelism(2);

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

    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
}
