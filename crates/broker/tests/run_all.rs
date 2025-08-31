use std::{fs::File, io::Write};

use axum::{http::Request, Router};
use http_body_util::BodyExt; // BodyExt::collect()
use tower::ServiceExt;

use grepzilla_segment::v2::writer::BinSegmentWriter;
use grepzilla_segment::SegmentWriter;

mod helpers;
use helpers::make_router_with_parallelism;

// ---------- helpers ----------

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

fn write_manifest_exact(tempdir: &std::path::Path, manifest_json: &serde_json::Value) {
    let path = tempdir.join("manifest.json");
    std::fs::write(&path, serde_json::to_vec_pretty(manifest_json).unwrap()).unwrap();
    std::env::set_var("GZ_MANIFEST", path.to_str().unwrap());
}

async fn post_json(app: &Router, uri: &str, body: &serde_json::Value) -> serde_json::Value {
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
    assert!(resp.status().is_success(), "status = {}", resp.status());
    let collected = resp.into_body().collect().await.unwrap();
    let bytes = collected.to_bytes();
    serde_json::from_slice(&bytes).unwrap()
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

// ---------- tests ----------

/// e2e: один шард через shards/manifest, 2 разных ext_id → 2 хита, подсветка, pin_gen
#[tokio::test]
async fn end_to_end_search_single_shard() {
    let td = tempfile::tempdir().unwrap();
    let root = td.path();

    // сегмент для shard=0, gen=2
    let seg0 = root.join("seg0");
    let in0 = root.join("data0.jsonl");
    write_jsonl(
        &in0,
        &[
            r#"{"_id":"2","text":{"title":"A","body":"щенок играет с мячиком"}}"#,
            r#"{"_id":"3","text":{"title":"B","body":"кот играет дома"}}"#,
        ],
    );
    build_v2_segment(&seg0, &in0);

    // manifest в ТВОЁМ ФОРМАТЕ (как прислал)
    let manifest = serde_json::json!({
      "shards":   { "0": 2 },                // shard 0 → gen 2
      "segments": { "0:2": [ seg0.to_str().unwrap() ] }
    });
    write_manifest_exact(root, &manifest);

    let app = make_router_with_parallelism(2);

    // sanity: GET /manifest/0
    let m = get_json(&app, "/manifest/0").await;
    assert_eq!(m["shard"], 0);
    assert_eq!(m["gen"], 2);

    // поиск через shards
    let req = serde_json::json!({
        "wildcard":"*игра*",
        "field":"text.body",
        "shards":[0],
        "page":{"size":10,"cursor":null},
        "limits":{"parallelism":1,"deadline_ms":1000,"max_candidates":200000}
    });

    let v = post_json(&app, "/search", &req).await;

    // ожидаем 2 хита (ext_id уникальны)
    let hits = v["hits"].as_array().unwrap();
    assert_eq!(hits.len(), 2, "unexpected hits: {}", v);

    // подсветка
    for h in hits {
        let p = h["preview"].as_str().unwrap();
        assert!(p.contains('[') && p.contains(']'), "no highlight: {}", p);
    }

    // per_seg один
    assert_eq!(
        v["cursor"]["per_seg"].as_object().unwrap().len(),
        1,
        "per_seg: {}",
        v["cursor"]["per_seg"]
    );

    // pin_gen есть и равен 2 для shard 0
    // (координатор переносит pin_gen в курсор после resolve())
    assert_eq!(v["cursor"]["pin_gen"]["0"], 2);
}

/// e2e: два шарда через shards/manifest, по 2 разных ext_id в каждом → >=4 хитов, pin_gen оба
#[tokio::test]
async fn end_to_end_search_two_shards() {
    let td = tempfile::tempdir().unwrap();
    let root = td.path();

    // seg for shard 0, gen 1
    let seg_01 = root.join("seg01");
    let in_01 = root.join("s01.jsonl");
    write_jsonl(
        &in_01,
        &[
            r#"{"_id":"10","text":{"title":"A","body":"щенок играет с мячиком"}}"#,
            r#"{"_id":"11","text":{"title":"A2","body":"он играет во дворе"}}"#,
        ],
    );
    build_v2_segment(&seg_01, &in_01);

    // seg for shard 1, gen 7
    let seg_17 = root.join("seg17");
    let in_17 = root.join("s17.jsonl");
    write_jsonl(
        &in_17,
        &[
            r#"{"_id":"20","text":{"title":"B","body":"играет кошка"}}"#,
            r#"{"_id":"21","text":{"title":"B2","body":"дети любят играть на улице"}}"#,
        ],
    );
    build_v2_segment(&seg_17, &in_17);

    // manifest В ТВОЁМ ФОРМАТЕ
    let manifest = serde_json::json!({
      "shards":   { "0": 1, "1": 7 },
      "segments": {
        "0:1": [ seg_01.to_str().unwrap() ],
        "1:7": [ seg_17.to_str().unwrap() ]
      }
    });
    write_manifest_exact(root, &manifest);

    let app = make_router_with_parallelism(4);

    // sanity: GET /manifest/{0,1}
    let m0 = get_json(&app, "/manifest/0").await;
    assert_eq!(m0["gen"], 1);
    let m1 = get_json(&app, "/manifest/1").await;
    assert_eq!(m1["gen"], 7);

    // поиск через shards (оба шарда)
    let req = serde_json::json!({
        "wildcard":"*игра*",
        "field":"text.body",
        "shards":[0,1],
        "page":{"size":10,"cursor":null},
        "limits":{"parallelism":2,"deadline_ms":1000,"max_candidates":200000}
    });

    let v = post_json(&app, "/search", &req).await;

    // ≥4 хитов, т.к. ext_id уникальны и в каждом сегменте по 2 кандидата
    let hits = v["hits"].as_array().unwrap();
    assert!(
        hits.len() >= 4,
        "Expected >=4 hits, got {}. Full: {}",
        hits.len(),
        v
    );

    // подсветка
    for h in hits {
        let p = h["preview"].as_str().unwrap();
        assert!(p.contains('[') && p.contains(']'), "no highlight: {}", p);
    }

    // per_seg: оба сегмента присутствуют
    assert_eq!(
        v["cursor"]["per_seg"].as_object().unwrap().len(),
        2,
        "per_seg: {}",
        v["cursor"]["per_seg"]
    );

    // pin_gen оба присутствуют и совпадают с манифестом
    assert_eq!(v["cursor"]["pin_gen"]["0"], 1);
    assert_eq!(v["cursor"]["pin_gen"]["1"], 7);

    // дедуп не выкинул ничего (ext_id уникальны)
    assert_eq!(
        v["metrics"]["dedup_dropped"].as_u64().unwrap_or(0),
        0,
        "dedup_dropped != 0: {}",
        v["metrics"]
    );
}
