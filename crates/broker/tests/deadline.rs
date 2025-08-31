use std::sync::Arc;

use axum::{http::Request, Router};
use broker::http_api::{router as make_router, AppState};
use broker::search::SearchCoordinator;
use http_body_util::BodyExt;
use tower::ServiceExt;

fn make_router_with_default_parallelism(p: usize) -> Router {
    make_router(AppState {
        coord: Arc::new(SearchCoordinator::new(p)),
    })
}

#[tokio::test]
async fn deadline_is_reported() {
    // пустые сегменты + крошечный дедлайн → deadline_hit = true
    let app = make_router_with_default_parallelism(2);
    let req = serde_json::json!({
        "wildcard":"*a*",
        "segments": [],
        "page":{"size":10,"cursor":null},
        "limits":{"parallelism":2,"deadline_ms":1,"max_candidates":200000}
    });
    let resp = app
        .clone()
        .oneshot(
            Request::post("/search")
                .header("content-type", "application/json")
                .body(axum::body::Body::from(serde_json::to_vec(&req).unwrap()))
                .unwrap(),
        )
        .await
        .unwrap();

    assert!(resp.status().is_success());
    let body = resp.into_body().collect().await.unwrap().to_bytes();
    let v: serde_json::Value = serde_json::from_slice(&body).unwrap();

    assert!(v["metrics"]["deadline_hit"].as_bool().unwrap());
}
