use serde::{Deserialize, Serialize};
use std::time::Duration;

pub type ShardId = u64;
pub type GenId = u64;

#[derive(Debug, Clone, Deserialize)]
pub struct PageIn {
    pub size: usize,
    /// Произвольный JSON: { per_seg: { "<seg_path>": { last_docid: n } }, pin_gen: { "<shard>": gen } }
    pub cursor: Option<serde_json::Value>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct SearchLimits {
    pub parallelism: Option<usize>,
    pub deadline_ms: Option<u64>,
    pub max_candidates: Option<u64>,
}

impl SearchLimits {
    pub fn deadline(&self) -> Option<Duration> {
        self.deadline_ms.map(Duration::from_millis)
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct PageReq {
    pub size: usize,
    pub cursor: Option<serde_json::Value>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct SearchRequest {
    pub wildcard: String,
    /// Если у тебя Optional — поменяй на Option<String>
    pub field: String,
    /// RAW-режим (напрямую пути сегментов)
    pub segments: Vec<String>,
    /// B6-режим через манифест (по шардам). Можно не передавать.
    pub shards: Option<Vec<ShardId>>,
    pub page: PageIn,
    pub limits: Option<SearchLimits>,
}

#[derive(Debug, Clone, Serialize)]
pub struct Cursor {
    pub per_seg: serde_json::Map<String, serde_json::Value>,
    pub pin_gen: Option<std::collections::HashMap<ShardId, GenId>>,
}

// --- ВЫХОД ---

#[derive(Debug, Serialize, Clone)]
pub struct Hit {
    pub ext_id: String,
    pub doc_id: u32,
    /// Можно добавить превью, если делал
    #[serde(skip_serializing_if = "Option::is_none")]
    pub preview: Option<String>,
    /// Поле, в котором совпало (если считаешь)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub matched_field: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct SearchMetrics {
    pub candidates_total: u64,
    pub time_to_first_hit_ms: u64,
    pub deadline_hit: bool,
    pub saturated_sem: usize,
}

#[derive(Debug, Serialize)]
pub struct SearchResponse {
    pub hits: Vec<Hit>,
    pub cursor: Option<Cursor>,
    pub metrics: SearchMetrics,
}
