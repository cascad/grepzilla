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
    async fn segments_for(&self, shard: ShardId, generation: GenId) -> anyhow::Result<Vec<String>>;
}

#[derive(Clone)]
pub struct FsManifestStore {
    pub path: std::path::PathBuf,
}

#[derive(Debug, Clone)]
pub struct SegRef {
    pub shard: u64,
    pub generation: u64,
    pub path: String,
}

#[async_trait]
impl ManifestStore for FsManifestStore {
    async fn current(&self) -> anyhow::Result<HashMap<ShardId, GenId>> {
        let f = std::fs::File::open(&self.path)?;
        let m: Manifest = serde_json::from_reader(f)?;
        Ok(m.shards)
    }

    async fn segments_for(&self, shard: ShardId, generation: GenId) -> anyhow::Result<Vec<String>> {
        let f = std::fs::File::open(&self.path)?;
        let m: Manifest = serde_json::from_reader(f)?;
        let key = format!("{shard}:{generation}");
        Ok(m.segments.get(&key).cloned().unwrap_or_default())
    }
}

impl FsManifestStore {
    /// Добавить сегмент к шардy: бампнуть поколение и атомарно записать файл.
    /// Возвращает новое поколение.
    pub async fn append_segment(
        &self,
        shard: ShardId,
        seg_path: String,
    ) -> anyhow::Result<GenId> {
        use tokio::io::AsyncWriteExt;

        // 1) загрузить/инициализировать манифест
        let mut m: Manifest = if self.path.exists() {
            let f = std::fs::File::open(&self.path)?;
            serde_json::from_reader(f)?
        } else {
            Manifest {
                shards: HashMap::new(),
                segments: HashMap::new(),
            }
        };

        // 2) новое поколение = старое+1 (старт с 1)
        let next_gen = m.shards.get(&shard).copied().unwrap_or(0) + 1;
        m.shards.insert(shard, next_gen);

        // 3) в новом поколении разместим свежесозданный сегмент
        let key = format!("{shard}:{next_gen}");
        m.segments.insert(key, vec![seg_path]);

        // 4) атомарная запись manifest.json
        if let Some(parent) = self.path.parent() {
            tokio::fs::create_dir_all(parent).await.ok();
        }
        let tmp = self.path.with_extension("tmp");
        {
            let mut f = tokio::fs::File::create(&tmp).await?;
            let s = serde_json::to_string_pretty(&m)?;
            f.write_all(s.as_bytes()).await?;
            f.flush().await?;
        }
        tokio::fs::rename(&tmp, &self.path).await?;
        Ok(next_gen)
    }
}
