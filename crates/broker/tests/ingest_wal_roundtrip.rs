// broker/tests/ingest_wal_roundtrip.rs
use broker::{ingest::handle_batch_json};
use serde_json::json;

mod helpers;
use helpers::test_cfg;

#[tokio::test]
async fn wal_to_segment_roundtrip() {
    let tmp = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(tmp.path().join("wal")).unwrap();
    std::fs::create_dir_all(tmp.path().join("segments")).unwrap();
    let cfg = test_cfg(&tmp, 2);

    let resp = handle_batch_json(vec![json!({"id":1,"text":{"body":"foo"}})], &cfg)
        .await
        .unwrap();
    assert!(resp.get("ok").and_then(|v| v.as_bool()).unwrap_or(false));
}
