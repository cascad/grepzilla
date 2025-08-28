use axum::async_trait;

use crate::search::types::{GenId, ShardId};
use std::collections::HashMap;

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct Manifest {
    pub shards: HashMap<ShardId, GenId>,
    pub segments: HashMap<String, Vec<String>>, // ключ "shard:gen"
}

#[async_trait]
pub trait ManifestStore: Send + Sync {
    async fn current(&self) -> anyhow::Result<HashMap<ShardId, GenId>>;
    async fn segments_for(&self, shard: ShardId, gen: GenId) -> anyhow::Result<Vec<String>>;
}

pub struct FsManifestStore {
    pub path: std::path::PathBuf,
}

#[async_trait]
impl ManifestStore for FsManifestStore {
    async fn current(&self) -> anyhow::Result<HashMap<ShardId, GenId>> {
        let f = std::fs::File::open(&self.path)?;
        let m: Manifest = serde_json::from_reader(f)?;
        Ok(m.shards)
    }

    async fn segments_for(&self, shard: ShardId, gen: GenId) -> anyhow::Result<Vec<String>> {
        let f = std::fs::File::open(&self.path)?;
        let m: Manifest = serde_json::from_reader(f)?;
        let key = format!("{shard}:{gen}");
        Ok(m.segments.get(&key).cloned().unwrap_or_default())
    }
}
