use crate::manifest::ManifestStore;
use crate::search::types::{GenId, SearchRequest, ShardId};
use std::collections::HashMap;
use std::sync::Arc;

pub struct SegmentSelector<S: ManifestStore> {
    pub store: Arc<S>,
}

impl<S: ManifestStore> SegmentSelector<S> {
    pub async fn plan(
        &self,
        req: &SearchRequest,
        pinned: Option<HashMap<ShardId, GenId>>,
    ) -> anyhow::Result<(Vec<String>, HashMap<ShardId, GenId>)> {
        // RAW режим — отдаем как есть
        if !req.segments.is_empty() {
            return Ok((req.segments.clone(), pinned.unwrap_or_default()));
        }

        // режим по шардам через манифест
        let pin = if let Some(pg) = pinned {
            pg
        } else {
            self.store.current().await?
        };

        let mut selected = Vec::new();
        if let Some(shards) = &req.shards {
            for &shard in shards {
                if let Some(generation) = pin.get(&shard) {
                    let mut segs = self.store.segments_for(shard, *generation).await?;
                    selected.append(&mut segs);
                }
            }
        }
        Ok((selected, pin))
    }
}
