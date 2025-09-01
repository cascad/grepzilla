// path: crates/broker/tests/wal_checksum.rs
use broker::ingest::wal::Wal;
use serde_json::json;

#[tokio::test]
async fn wal_writes_and_sidecar() {
    let tmp = tempfile::tempdir().unwrap();
    let wal = Wal::new(tmp.path().join("wal"));
    let (_path, n) = wal.append_batch(&[json!({"_id":"a"}), json!({"_id":"b"})]).await.unwrap();
    assert_eq!(n, 2);
    // пробегись по dir: есть .jsonl и .xxh3
    let mut found = (false,false);
    for e in std::fs::read_dir(tmp.path().join("wal")).unwrap() {
        let p = e.unwrap().path();
        if p.extension().and_then(|s| s.to_str()) == Some("jsonl") { found.0 = true; }
        if p.extension().and_then(|s| s.to_str()) == Some("xxh3") { found.1 = true; }
    }
    assert!(found.0 && found.1, "missing wal or sidecar");
}
