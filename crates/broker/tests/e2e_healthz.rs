use axum::{
    body::Body,
    http::{Request, StatusCode},
};
use tower::ServiceExt;

mod helpers;
use helpers::make_router_with_parallelism;

#[tokio::test]
async fn healthz_ok() {
    // минимальный app
    let app = make_router_with_parallelism(2);

    let resp = app
        .oneshot(
            Request::builder()
                .uri("/healthz")
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
    assert_eq!(v["status"], "ok");
}
