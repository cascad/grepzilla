use super::*;
use tokio::fs;

#[derive(Debug, Clone)]
pub struct FsManifestStore {
    pub path: std::path::PathBuf,
}

#[async_trait::async_trait]
impl ManifestStore for FsManifestStore {
    async fn load(&self) -> anyhow::Result<ManifestUnified> {
        let data = fs::read(&self.path).await?;

        // Пытаемся как V1
        if let Ok(v1) = serde_json::from_slice::<ManifestV1>(&data) {
            return Ok(ManifestUnified::from_v1(v1));
        }
        // Пытаемся как плоский
        if let Ok(flat) = serde_json::from_slice::<ManifestFlat>(&data) {
            return Ok(ManifestUnified::from_flat(flat));
        }

        bail!("manifest: unknown format (neither V1 nor flat)");
    }

    async fn resolve(
        &self,
        shards: &[u64],
    ) -> anyhow::Result<(Vec<SegRef>, std::collections::HashMap<u64, u64>)> {
        let uni = self.load().await?;
        Ok(uni.resolve(shards))
    }
}