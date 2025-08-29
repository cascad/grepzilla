use super::*;
use tokio::fs;

#[derive(Debug, Clone)]
pub struct FsManifestStore {
    pub path: std::path::PathBuf, // PathBuf !
}

#[async_trait::async_trait]
impl ManifestStore for FsManifestStore {
    async fn load(&self) -> anyhow::Result<ManifestV1> {
        let data = fs::read(&self.path).await?;
        let m: ManifestV1 = serde_json::from_slice(&data)?;
        Ok(m)
    }

    async fn resolve(
        &self,
        shards: &[u64],
    ) -> anyhow::Result<(Vec<SegRef>, std::collections::HashMap<u64, u64>)> {
        let m = self.load().await?;
        let mut out = Vec::new();
        let mut pin = std::collections::HashMap::new();
        for &sh in shards {
            if let Some(ent) = m.shards.get(&sh) {
                pin.insert(sh, ent.generation);
                for p in &ent.segments {
                    out.push(SegRef {
                        shard: sh,
                        gen: ent.generation,
                        path: p.clone(),
                    });
                }
            }
        }
        Ok((out, pin))
    }
}
