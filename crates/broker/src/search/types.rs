use serde::{Deserialize, Serialize};
use std::{collections::HashMap, time::Duration};

pub type ShardId = u64;
pub type GenId = u64;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PageIn {
    pub size: usize,
    /// Произвольный JSON: { per_seg: { "<seg_path>": { last_docid: n } }, pin_gen: { "<shard>": gen } }
    pub cursor: Option<serde_json::Value>,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct SearchLimits {
    #[serde(default)]
    pub parallelism: Option<usize>,
    #[serde(default)]
    pub deadline_ms: Option<u64>,
    #[serde(default)]
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

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct SearchRequest {
    pub wildcard: String,
    #[serde(default)]
    pub field: Option<String>, // ← опционально
    #[serde(default)]
    pub segments: Vec<String>, // ← для B5: можно слать напрямую
    #[serde(default)]
    pub shards: Option<Vec<u64>>, // ← для B6: выбираем сегменты по шардам
    pub page: PageIn,
    #[serde(default)]
    pub limits: Option<SearchLimits>,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct PerSegPos {
    pub last_docid: u64,
}

#[derive(Debug, Clone, Serialize)]
pub struct Cursor {
    pub per_seg: HashMap<String, PerSegPos>,
    #[serde(default)]
    pub pin_gen: Option<HashMap<u64, u64>>, // ← B6: gen по шардам
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
