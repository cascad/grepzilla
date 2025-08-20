use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ManifestPtr {
    pub epoch: u64,
    pub r#gen: u64, // raw идентификатор (имя поля остаётся "gen")
    pub url: String,
    pub checksum: String,
    pub updated_at: String, // ISO8601
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SegmentMeta {
    pub id: String,
    pub url: String,
    pub min_doc: u32,
    pub max_doc: u32,
    pub time_min: i64,
    pub time_max: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TombMeta {
    pub cardinality: u64,
    pub url: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ManifestV1 {
    pub version: u32, // 1
    pub shard_id: u64,
    pub r#gen: u64,
    pub created_at: String, // ISO8601
    pub hwm_seqno: String,  // HLC string, e.g., "hlc:17:9123456"
    pub segments: Vec<SegmentMeta>,
    pub tombstones: TombMeta,
    pub prev_gen: Option<u64>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip_ptr() {
        let ptr = ManifestPtr {
            epoch: 3,
            r#gen: 123,
            url: "s3://x/shard-17/gen-000123/manifest.json".to_string(),
            checksum: "sha256:deadbeef".to_string(),
            updated_at: "2025-08-20T12:00:00Z".to_string(),
        };
        let j = serde_json::to_string(&ptr).unwrap();
        let back: ManifestPtr = serde_json::from_str(&j).unwrap();
        assert_eq!(ptr, back);
    }

    #[test]
    fn roundtrip_manifest() {
        let m = ManifestV1 {
            version: 1,
            shard_id: 17,
            r#gen: 123,
            created_at: "2025-08-20T12:00:00Z".to_string(),
            hwm_seqno: "hlc:17:9123456".to_string(),
            segments: vec![SegmentMeta {
                id: "s-abc".into(),
                url: "s3://x/s-abc".into(),
                min_doc: 0,
                max_doc: 9999,
                time_min: 1690000000,
                time_max: 1690099999,
            }],
            tombstones: TombMeta {
                cardinality: 5321,
                url: "s3://x/t_123.roaring".into(),
            },
            prev_gen: Some(122),
        };
        let j = serde_json::to_string_pretty(&m).unwrap();
        let back: ManifestV1 = serde_json::from_str(&j).unwrap();
        assert_eq!(m, back);
    }
}
