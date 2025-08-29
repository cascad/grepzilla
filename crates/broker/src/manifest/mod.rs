use anyhow::Result;
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

pub mod fs;

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct ShardEntry {
    #[serde(rename = "gen")]
    pub generation: u64, // ← поле называется нормально
    pub segments: Vec<String>,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct ManifestV1 {
    pub version: u32,
    pub shards: HashMap<u64, ShardEntry>,
}

#[derive(Debug, Clone)]
pub struct SegRef {
    pub shard: u64,
    pub gen: u64,
    pub path: String,
}

#[async_trait]
pub trait ManifestStore: Send + Sync {
    async fn load(&self) -> Result<ManifestV1>;
    async fn resolve(&self, shards: &[u64]) -> Result<(Vec<SegRef>, HashMap<u64, u64>)>;
}
