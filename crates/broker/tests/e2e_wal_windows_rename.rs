// path: crates/broker/tests/e2e_wal_windows_rename.rs
use axum::{http::{Request, StatusCode}, body::Body};
use tower::ServiceExt;

mod helpers;
use helpers::make_router_with_parallelism;

#[tokio::test]
async fn windows_rename_does_not_fail_after_drop() {
    let app = make_router_with_parallelism(1);

    let tmp = tempfile::tempdir().unwrap();
    std::env::set_var("GZ_WAL_DIR", tmp.path().join("wal"));
    std::env::set_var("GZ_SHARD", "1");
    std::env::set_var("GZ_MANIFEST", tmp.path().join("manifest.json"));

    let doc = serde_json::json!([{"_id":"x","text":{"body":"hello"}}]).to_string();
    let req = Request::builder().method("POST").uri("/ingest")
        .header("content-type","application/json")
        .body(Body::from(doc)).unwrap();
    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    std::env::remove_var("GZ_WAL_DIR");
    std::env::remove_var("GZ_SHARD");
    std::env::remove_var("GZ_MANIFEST");
}
