use serde::{Deserialize, Serialize};
use std::{collections::HashMap, time::{Duration, Instant}};

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct PageIn {
    pub size: usize,
    #[serde(default)]
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
    /// Относительный бюджет времени на запрос.
    pub fn deadline_duration(&self) -> Option<Duration> {
        self.deadline_ms.map(Duration::from_millis)
    }
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct SearchRequest {
    pub wildcard: String,
    #[serde(default)]
    pub field: Option<String>, // опционально
    #[serde(default)]
    pub segments: Vec<String>, // B5
    #[serde(default)]
    pub shards: Option<Vec<u64>>, // B6
    pub page: PageIn,
    #[serde(default)]
    pub limits: Option<SearchLimits>,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct PerSegPos {
    pub last_docid: u64,
}

#[derive(Debug, Deserialize, Serialize, Clone, Default)]
pub struct SearchCursor {
    pub per_seg: HashMap<String, PerSegPos>,
    #[serde(default)]
    pub pin_gen: Option<HashMap<u64, u64>>, // shard -> gen
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct Hit {
    pub ext_id: String,
    pub doc_id: u32,
    pub matched_field: String,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct SearchMetrics {
    pub candidates_total: u64,
    pub time_to_first_hit_ms: u64,
    pub deadline_hit: bool,
    pub saturated_sem: u64,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct SearchResponse {
    pub hits: Vec<Hit>,
    pub cursor: Option<SearchCursor>,
    pub metrics: SearchMetrics,
}
