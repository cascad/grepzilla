use axum::http::Request;
use http_body_util::BodyExt;
use tower::ServiceExt;

mod helpers;
use helpers::make_router_with_parallelism;

#[tokio::test]
async fn deadline_is_reported() {
    // пустые сегменты + крошечный дедлайн → deadline_hit = true

    let req = serde_json::json!({
        "wildcard":"*a*",
        "segments": [],
        "page":{"size":10,"cursor":null},
        "limits":{"parallelism":2,"deadline_ms":1,"max_candidates":200000}
    });
    let app = make_router_with_parallelism(2);
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
