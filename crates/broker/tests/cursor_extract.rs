// crates/broker/tests/cursor_extract.rs
use broker::search::types::PerSegPos;

fn extract_last_docid(cursor: &Option<serde_json::Value>, seg: &str) -> Option<u64> {
    // скопировано из broker/src/search/mod.rs для теста изоляции
    cursor
        .as_ref()
        .and_then(|c| c.get("per_seg"))
        .and_then(|ps| ps.get(seg))
        .and_then(|s| s.get("last_docid"))
        .and_then(|v| v.as_u64())
}

#[test]
fn extract_last_docid_from_cursor_json() {
    let mut per_seg = serde_json::Map::new();
    per_seg.insert(
        "segments/000001".to_string(),
        serde_json::json!(PerSegPos { last_docid: 42 }),
    );
    let cur = serde_json::json!({ "per_seg": per_seg });

    let got = extract_last_docid(&Some(cur), "segments/000001");
    assert_eq!(got, Some(42));

    let none = extract_last_docid(
        &Some(serde_json::json!({ "per_seg": {} })),
        "segments/000001",
    );
    assert_eq!(none, None);
}
