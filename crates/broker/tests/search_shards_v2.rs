use std::{fs::File, io::Write, sync::Arc};

use axum::{http::Request, Router};
use broker::http_api::{router as make_router, AppState};
use broker::search::SearchCoordinator;
use http_body_util::BodyExt; // BodyExt::collect()
use tower::ServiceExt;

use grepzilla_segment::v2::writer::BinSegmentWriter;
use grepzilla_segment::SegmentWriter;

fn write_jsonl(path: &std::path::Path, lines: &[&str]) {
    let mut f = File::create(path).unwrap();
    for l in lines {
        writeln!(f, "{}", l).unwrap();
    }
}

fn build_v2_segment(dir: &std::path::Path, jsonl: &std::path::Path) {
    std::fs::create_dir_all(dir).unwrap();
    BinSegmentWriter::default()
        .write_segment(jsonl.to_str().unwrap(), dir.to_str().unwrap())
        .unwrap();
}

fn make_router_with_default_parallelism(p: usize) -> Router {
    make_router(AppState {
        coord: Arc::new(SearchCoordinator::new(p)),
    })
}

fn write_manifest_exact(tempdir: &std::path::Path, manifest_json: &serde_json::Value) {
    let path = tempdir.join("manifest.json");
    std::fs::write(&path, serde_json::to_vec_pretty(manifest_json).unwrap()).unwrap();
    std::env::set_var("GZ_MANIFEST", path.to_str().unwrap());
}

async fn post_json(
    app: &Router,
    uri: &str,
    body: &serde_json::Value,
) -> (axum::http::StatusCode, Vec<u8>) {
    let resp = app
        .clone()
        .oneshot(
            Request::post(uri)
                .header("content-type", "application/json")
                .body(axum::body::Body::from(serde_json::to_vec(body).unwrap()))
                .unwrap(),
        )
        .await
        .unwrap();

    let status = resp.status();
    let collected = resp.into_body().collect().await.unwrap();
    let bytes = collected.to_bytes().to_vec();
    (status, bytes)
}

async fn get_json(app: &Router, uri: &str) -> serde_json::Value {
    let resp = app
        .clone()
        .oneshot(Request::get(uri).body(axum::body::Body::empty()).unwrap())
        .await
        .unwrap();
    assert!(
        resp.status().is_success(),
        "GET {} status = {}",
        uri,
        resp.status()
    );
    let collected = resp.into_body().collect().await.unwrap();
    let bytes = collected.to_bytes();
    serde_json::from_slice(&bytes).unwrap()
}

#[tokio::test]
async fn search_v2_shards_dedup_preview() {
    let td = tempfile::tempdir().unwrap();
    let root = td.path();

    // seg for shard 0:1
    let seg01 = root.join("seg01");
    let in01 = root.join("s01.jsonl");
    write_jsonl(
        &in01,
        &[
            r#"{"_id":"2","text":{"title":"A","body":"щенок играет с мячиком"}}"#,
            r#"{"_id":"3","text":{"title":"B","body":"кот тоже играет"}}"#,
        ],
    );
    build_v2_segment(&seg01, &in01);

    // seg for shard 1:7  (с дублем ext_id "2", чтобы проверить дедуп)
    let seg17 = root.join("seg17");
    let in17 = root.join("s17.jsonl");
    write_jsonl(
        &in17,
        &[
            r#"{"_id":"2","text":{"title":"B","body":"щенок играет дома"}}"#,
            r#"{"_id":"4","text":{"title":"B4","body":"дети играют на улице"}}"#,
        ],
    );
    build_v2_segment(&seg17, &in17);

    // manifest — ВАЖНО: точный формат, как у тебя в проде
    let manifest = serde_json::json!({
        "shards":   { "0": 1, "1": 7 },
        "segments": {
          "0:1": [ seg01.to_str().unwrap() ],
          "1:7": [ seg17.to_str().unwrap() ]
        }
    });
    write_manifest_exact(root, &manifest);

    let app = make_router_with_default_parallelism(4);

    // sanity: GET /manifest/{0,1}
    let m0 = get_json(&app, "/manifest/0").await;
    assert_eq!(m0["gen"], 1, "manifest[0] = {}", m0);
    let m1 = get_json(&app, "/manifest/1").await;
    assert_eq!(m1["gen"], 7, "manifest[1] = {}", m1);

    // поиск через SHARDS
    let req = serde_json::json!({
        "wildcard":"*игра*",
        "field":"text.body",
        "shards":[0,1],
        "page":{"size":10,"cursor":null},
        "limits":{"parallelism":2,"deadline_ms":1000,"max_candidates":200000}
    });

    let (status, body) = post_json(&app, "/search", &req).await;
    if !status.is_success() {
        let s = String::from_utf8_lossy(&body);
        panic!("POST /search status={} body={}", status, s);
    }
    let v: serde_json::Value = serde_json::from_slice(&body).unwrap();

    // должно быть ≥3 хита: (ext_id=2 дедупнется, остальные два уникальны)
    let hits = v["hits"].as_array().unwrap();
    assert!(
        hits.len() >= 3,
        "expected >=3 hits (after dedup), got {}. full: {}",
        hits.len(),
        v
    );

    // подсветка
    for h in hits {
        let p = h["preview"].as_str().unwrap();
        assert!(p.contains('[') && p.contains(']'), "no highlight: {}", p);
    }

    // per_seg: оба сегмента
    assert_eq!(
        v["cursor"]["per_seg"].as_object().unwrap().len(),
        2,
        "per_seg: {}",
        v["cursor"]["per_seg"]
    );

    // pin_gen оба присутствуют и совпадают
    assert_eq!(v["cursor"]["pin_gen"]["0"], 1);
    assert_eq!(v["cursor"]["pin_gen"]["1"], 7);

    // дедуп должен выкинуть ровно один (по ext_id "2")
    assert_eq!(
        v["metrics"]["dedup_dropped"].as_u64().unwrap_or(0),
        1,
        "dedup_dropped mismatch: {}",
        v["metrics"]
    );
}
