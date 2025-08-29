// broker/tests/http_search_manifest.rs
use axum::{
    body::Body,
    http::{Request, StatusCode},
};
use serde_json::json;
use std::{fs, io::Write, sync::Arc};
use tempfile::TempDir;
use tower::ServiceExt;

use broker::{
    http_api::{self, AppState},
    search::SearchCoordinator,
};
use grepzilla_segment::segjson::JsonSegmentWriter;
use grepzilla_segment::SegmentWriter;

#[tokio::test]
async fn http_search_via_manifest_shards() {
    // tmp
    let tmp = TempDir::new().unwrap();

    // seg1
    let seg1 = tmp.path().join("seg1");
    fs::create_dir_all(&seg1).unwrap();
    let in1 = tmp.path().join("in1.jsonl");
    {
        let mut f1 = std::fs::File::create(&in1).unwrap();
        writeln!(f1, r#"{{"_id":"1","text":{{"body":"первая игра"}}}}"#).unwrap();
    }
    let mut w = JsonSegmentWriter::default();
    w.write_segment(in1.to_str().unwrap(), seg1.to_str().unwrap())
        .unwrap();

    // seg2
    let seg2 = tmp.path().join("seg2");
    fs::create_dir_all(&seg2).unwrap();
    let in2 = tmp.path().join("in2.jsonl");
    {
        let mut f2 = std::fs::File::create(&in2).unwrap();
        writeln!(f2, r#"{{"_id":"2","text":{{"body":"вторая игра"}}}}"#).unwrap();
    }
    w.write_segment(in2.to_str().unwrap(), seg2.to_str().unwrap())
        .unwrap();

    // manifest.json (формат V1)
    let manifest_path = tmp.path().join("manifest.json");
    let manifest = json!({
        "version": 1,
        "shards": {
            "0": { "gen": 7, "segments": [ seg1.to_string_lossy(), seg2.to_string_lossy() ] }
        }
    });
    fs::write(
        &manifest_path,
        serde_json::to_vec_pretty(&manifest).unwrap(),
    )
    .unwrap();

    // прокидываем путь до манифеста для http-хендлера
    std::env::set_var("GZ_MANIFEST", &manifest_path);

    // собираем приложение: router() + state
    let state = AppState {
        coord: Arc::new(SearchCoordinator::new(4)),
    };
    let app = http_api::router(state);

    // запрос только с shards (без segments)
    let body_val = json!({
        "wildcard": "*игра*",
        "field": "text.body",
        "shards": [0],
        "page": { "size": 10, "cursor": null },
        "limits": { "parallelism": 2, "deadline_ms": 1000, "max_candidates": 1000 }
    });

    let req = Request::builder()
        .method("POST")
        .uri("/search")
        .header("content-type", "application/json")
        .body(Body::from(serde_json::to_vec(&body_val).unwrap()))
        .unwrap();

    // axum 0.7: для oneshot нужен into_service()
    let resp = app.clone().into_service().oneshot(req).await.unwrap();
    let status = resp.status();
    let bytes = axum::body::to_bytes(resp.into_body(), 64 * 1024)
        .await
        .unwrap();
    assert_eq!(
        status,
        StatusCode::OK,
        "HTTP {}",
        String::from_utf8_lossy(&bytes)
    );

    let v: serde_json::Value = serde_json::from_slice(&bytes).unwrap();

    // хиты должны быть (оба сегмента одной шарды)
    let hits = v["hits"].as_array().unwrap();
    assert!(hits.len() >= 2, "{}", v);

    // курсор содержит позиции по обоим сегментам
    let per_seg = v["cursor"]["per_seg"].as_object().unwrap();
    assert_eq!(per_seg.len(), 2, "{}", v);

    // pin_gen проставлен из манифеста
    let pin = v["cursor"]["pin_gen"].as_object().expect("pin_gen");
    assert_eq!(pin.get("0").unwrap().as_u64().unwrap(), 7, "{}", v);
}
