// path: crates/broker/tests/ingest_wal_roundtrip.rs
use broker::ingest::handle_batch_json;
use broker::config::BrokerConfig;
use serde_json::json;

#[tokio::test]
async fn wal_to_segment_roundtrip() {
    let tmp = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(tmp.path().join("wal")).unwrap();
    std::fs::create_dir_all(tmp.path().join("segments")).unwrap();

    let cfg = test_cfg(&tmp, 2);

    let resp = handle_batch_json(vec![json!({"_id":"1","text":{"body":"foo"}})], &cfg)
        .await
        .unwrap();

    assert!(resp.get("ok").and_then(|v| v.as_bool()).unwrap_or(false));
    assert!(resp.get("segment").and_then(|v| v.as_str()).is_some(), "segment path missing");
}

// локальный конфиг для изоляции теста (без env)
fn test_cfg(tmp: &tempfile::TempDir, parallelism: usize) -> BrokerConfig {
    BrokerConfig {
        addr: "127.0.0.1:0".into(),
        wal_dir: tmp.path().join("wal").to_string_lossy().to_string(),
        segment_out_dir: tmp.path().join("segments").to_string_lossy().to_string(),
        parallelism,
        hot_cap: 10_000,
        manifest_path: Some(tmp.path().join("manifest.json").to_string_lossy().to_string()),
        shard: 0,
    }
}
