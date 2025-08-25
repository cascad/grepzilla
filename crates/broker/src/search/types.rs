// broker/src/search/types.rs
use serde::{Deserialize, Serialize};
use std::time::Duration;

#[derive(Debug, Clone, Deserialize)]
pub struct SearchLimits {
    pub parallelism: Option<usize>,
    pub deadline_ms: Option<u64>,
    pub max_candidates: Option<u64>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct PageReq {
    pub size: usize,
    pub cursor: Option<serde_json::Value>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct SearchRequest {
    pub wildcard: String,
    pub field: String,
    pub segments: Vec<String>,
    pub page: PageReq,
    pub limits: Option<SearchLimits>,
}

#[derive(Debug, Clone, Serialize)]
pub struct SearchMetrics {
    pub candidates_total: u64,
    pub time_to_first_hit_ms: u64,
    pub deadline_hit: bool,
    pub saturated_sem: u32,
}

#[derive(Debug, Clone, Serialize)]
pub struct Cursor {
    pub per_seg: serde_json::Value, // { seg_path: { last_docid: u64 } }
    pub pin_gen: Option<u64>,
}

#[derive(Debug, Clone, Serialize)]
pub struct SearchResponse {
    pub hits: Vec<serde_json::Value>,
    pub cursor: Option<Cursor>,
    pub metrics: SearchMetrics,
}

impl SearchLimits {
    pub fn deadline(&self) -> Option<Duration> {
        self.deadline_ms.map(Duration::from_millis)
    }
}
