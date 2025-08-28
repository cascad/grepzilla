use crate::manifest::{ManifestStore, SegRef};
use crate::search::executor::{ParallelExecutor, SegmentTaskInput, SegmentTaskOutput};
use crate::search::paginator::Paginator;
use crate::search::types::*;
use std::collections::HashMap;
use std::sync::Arc;
use tokio_util::sync::CancellationToken;
use tracing::debug;

pub mod executor;
pub mod manifest;
pub mod paginator;
pub mod selector;
pub mod snippet;
pub mod types;

pub struct SearchCoordinator {
    default_parallelism: usize,
    manifest: Option<Arc<dyn ManifestStore>>,
}

impl SearchCoordinator {
    pub fn new(default_parallelism: usize) -> Self {
        Self {
            default_parallelism,
            manifest: None,
        }
    }

    pub fn with_manifest(mut self, store: std::sync::Arc<dyn ManifestStore>) -> Self {
        self.manifest = Some(store);
        self
    }

    pub async fn handle(&self, req: SearchRequest) -> anyhow::Result<SearchResponse> {
        let start_instant = std::time::Instant::now();

        // --- выбрать сегменты ---
        let mut pin_gen: std::collections::HashMap<u64, u64> = std::collections::HashMap::new();
        let selected: Vec<SegRef> =
            if let (Some(store), Some(shards)) = (&self.manifest, req.shards.as_ref()) {
                debug!(?shards, "B6: resolve shards via manifest");
                let (segs, pin) = store.resolve(shards).await?;
                pin_gen = pin;
                segs
            } else {
                // режим B5: берём, что прислал клиент
                req.segments
                    .iter()
                    .map(|p| SegRef {
                        shard: 0,
                        gen: 0,
                        path: p.clone(),
                    })
                    .collect()
            };
        debug!(count = selected.len(), ?pin_gen, "B6: selected segments");

        // --- запустить задачи ---
        let limits = req.limits.clone().unwrap_or(SearchLimits {
            parallelism: None,
            deadline_ms: None,
            max_candidates: None,
        });
        let parallelism = limits
            .parallelism
            .unwrap_or(self.default_parallelism)
            .max(1);
        let executor = ParallelExecutor::new(parallelism);

        let tasks = selected
            .iter()
            .map(|s| SegmentTaskInput {
                seg_path: s.path.clone(),
                wildcard: req.wildcard.clone(),
                field: req.field.clone().unwrap_or_default(),
                cursor_docid: extract_last_docid(&req.page.cursor, &s.path),
                max_candidates: limits.max_candidates.unwrap_or(200_000),
            })
            .collect::<Vec<_>>();

        let ct = CancellationToken::new();
        let deadline = limits.deadline();

        let search_fn = |input: SegmentTaskInput, ctok: CancellationToken| async move {
            let out = crate::storage_adapter::search_one_segment(input, ctok).await?;
            Ok::<SegmentTaskOutput, anyhow::Error>(out)
        };

        let (parts, deadline_hit, saturated_sem) = executor
            .run_all(ct.clone(), tasks, search_fn, req.page.size, deadline)
            .await;

        let (hits, mut cursor, candidates_total) = Paginator::merge(parts, req.page.size);

        // метрики
        let time_to_first_hit_ms = if !hits.is_empty() {
            start_instant.elapsed().as_millis() as u64
        } else {
            0
        };

        // проставим pin_gen, если есть
        if !pin_gen.is_empty() {
            cursor.pin_gen = Some(pin_gen.clone());
        }

        Ok(SearchResponse {
            hits,
            cursor: Some(cursor),
            metrics: SearchMetrics {
                candidates_total,
                time_to_first_hit_ms,
                deadline_hit,
                saturated_sem,
            },
        })
    }
}

fn extract_last_docid(cursor: &Option<serde_json::Value>, seg: &str) -> Option<u64> {
    cursor
        .as_ref()
        .and_then(|c| c.get("per_seg"))
        .and_then(|ps| ps.get(seg))
        .and_then(|s| s.get("last_docid"))
        .and_then(|v| v.as_u64())
}

fn extract_pin_gen(cursor: &Option<serde_json::Value>) -> Option<HashMap<u64, u64>> {
    let obj = cursor.as_ref()?.get("pin_gen")?.as_object()?;

    let mut out = HashMap::new();
    for (k, v) in obj {
        if let (Ok(shard), Some(gen)) = (k.parse::<u64>(), v.as_u64()) {
            out.insert(shard, gen);
        }
    }
    Some(out)
}
