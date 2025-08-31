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
        if let Ok(v1) = serde_json::from_slice::<ManifestV1>(&data) {
            return Ok(ManifestUnified::from_v1(v1));
        }
        if let Ok(flat) = serde_json::from_slice::<ManifestFlat>(&data) {
            return Ok(ManifestUnified::from_flat(flat));
        }
        anyhow::bail!("manifest: unknown format (neither V1 nor flat)");
    }

    async fn resolve(
        &self,
        shards: &[u64],
    ) -> anyhow::Result<(Vec<SegRef>, std::collections::HashMap<u64, u64>)> {
        let uni = self.load().await?;
        Ok(uni.resolve(shards))
    }

    // NEW / FIXED: детерминированный инкремент поколения
    async fn append_segment(&self, shard: u64, seg_path: String) -> anyhow::Result<()> {
        // 0) читаем как FLAT или стартуем с пустого
        let mut flat: ManifestFlat = match fs::read(&self.path).await {
            Ok(bytes) if !bytes.is_empty() => {
                match serde_json::from_slice::<ManifestFlat>(&bytes) {
                    Ok(f) => f,
                    Err(_) => ManifestFlat {
                        shards: std::collections::HashMap::new(),
                        segments: std::collections::HashMap::new(),
                    },
                }
            }
            _ => ManifestFlat {
                shards: std::collections::HashMap::new(),
                segments: std::collections::HashMap::new(),
            },
        };

        // 1) вычисляем текущий gen как максимум из shards[shard] и "shard:gen" ключей
        let mut current = flat.shards.get(&shard).copied().unwrap_or(0);
        for k in flat.segments.keys() {
            if let Some((lh, rh)) = k.split_once(':') {
                if let (Ok(s), Ok(g)) = (lh.parse::<u64>(), rh.parse::<u64>()) {
                    if s == shard && g > current {
                        current = g;
                    }
                }
            }
        }
        let next_gen = current.saturating_add(1);

        // 2) обновляем структуры
        flat.shards.insert(shard, next_gen);
        let key = format!("{shard}:{next_gen}");
        flat.segments.entry(key).or_default().push(seg_path);

        // 3) атомарная запись (и создаём директории)
        if let Some(parent) = self.path.parent() {
            fs::create_dir_all(parent).await.ok();
        }
        let tmp = self.path.with_extension("tmp");
        let data = serde_json::to_vec_pretty(&flat)?;
        fs::write(&tmp, data).await?;
        fs::rename(&tmp, &self.path).await?;
        Ok(())
    }
}
