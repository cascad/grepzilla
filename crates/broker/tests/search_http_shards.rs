use std::fs::{self, File};
use std::io::Write;
use std::sync::Arc;

use axum::{body::Body, http::{Request, StatusCode}};
use tower::ServiceExt;
use tempfile::TempDir;

use broker::http::router;
use broker::search::SearchCoordinator;
use broker::manifest::fs::FsManifestStore;
use grepzilla_segment::segjson::JsonSegmentWriter;
use grepzilla_segment::SegmentWriter;
use serde_json::json;

#[tokio::test]
async fn http_search_via_manifest_shards() {
    // tmp
    let tmp = TempDir::new().unwrap();

    // seg1
    let seg1 = tmp.path().join("seg1");
    fs::create_dir_all(&seg1).unwrap();
    let in1 = tmp.path().join("in1.jsonl");
    let mut f1 = File::create(&in1).unwrap();
    writeln!(f1, r#"{{"_id":"1","text":{{"body":"первая игра"}}}}"#).unwrap();
    let mut w = JsonSegmentWriter::default();
    w.write_segment(in1.to_str().unwrap(), seg1.to_str().unwrap()).unwrap();

    // seg2
    let seg2 = tmp.path().join("seg2");
    fs::create_dir_all(&seg2).unwrap();
    let in2 = tmp.path().join("in2.jsonl");
    let mut f2 = File::create(&in2).unwrap();
    writeln!(f2, r#"{{"_id":"2","text":{{"body":"вторая игра"}}}}"#).unwrap();
    w.write_segment(in2.to_str().unwrap(), seg2.to_str().unwrap()).unwrap();

    // manifest.json
    let manifest_path = tmp.path().join("manifest.json");
    let manifest = json!({
        "version": 1,
        "shards": {
            "0": { "gen": 7, "segments": [ seg1.to_string_lossy(), seg2.to_string_lossy() ] }
        }
    });
    fs::write(&manifest_path, serde_json::to_vec_pretty(&manifest).unwrap()).unwrap();

    // app с манифестом
    let coord = SearchCoordinator::new(4)
        .with_manifest(Arc::new(FsManifestStore { path: manifest_path }));
    let app = router(broker::http::AppState { coord: Arc::new(coord) });

    // запрос только с shards
    let body_val = json!({
        "wildcard": "*игра*",
        "field": "text.body",
        "shards": [0],
        "page": { "size": 10, "cursor": null },
        "limits": { "parallelism": 2, "deadline_ms": 1000, "max_candidates": 1000 }
    });

    let req = Request::builder()
        .method("POST").uri("/search")
        .header("content-type","application/json")
        .body(Body::from(serde_json::to_vec(&body_val).unwrap()))
        .unwrap();

    let resp = app.clone().oneshot(req).await.unwrap();
    let status = resp.status();
    let bytes = axum::body::to_bytes(resp.into_body(), 64 * 1024).await.unwrap();
    assert_eq!(status, StatusCode::OK, "HTTP {}", String::from_utf8_lossy(&bytes));

    let v: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    let hits = v["hits"].as_array().unwrap();
    assert!(hits.len() >= 2, "{}", v);

    let per_seg = v["cursor"]["per_seg"].as_object().unwrap();
    assert_eq!(per_seg.len(), 2, "{}", v);

    let pin = v["cursor"]["pin_gen"].as_object().expect("pin_gen");
    assert_eq!(pin.get("0").unwrap().as_u64().unwrap(), 7, "{}", v);
}
