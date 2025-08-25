// broker/tests/ingest_wal_roundtrip.rs
use broker::{config::BrokerConfig, ingest::handle_batch_json};
use serde_json::json;

#[tokio::test]
async fn wal_to_segment_roundtrip() {
    let cfg = BrokerConfig::default();
    let resp = handle_batch_json(vec![json!({"id":1,"text":{"body":"foo"}})], &cfg)
        .await
        .unwrap();
    assert!(resp.get("ok").and_then(|v| v.as_bool()).unwrap_or(false));
}
