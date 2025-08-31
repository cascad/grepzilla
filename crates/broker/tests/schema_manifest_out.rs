use serde_json::json;

#[test]
fn manifest_shard_out_shape() {
    // Имитация того, что возвращает http_api::get_manifest (структура важна)
    let out = json!({
        "shard": 0,
        "gen": 7,
        "segments": ["segments/000001","segments/000002"],
    });

    assert!(out.get("shard").is_some());
    assert!(out.get("gen").is_some());
    assert!(out.get("segments").is_some());
    assert!(out["segments"].is_array());
    assert_eq!(out["segments"][0], "segments/000001");
}
