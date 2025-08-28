use crate::search::executor::{ParallelExecutor, SegmentTaskInput, SegmentTaskOutput};
use crate::search::manifest::FsManifestStore;
use crate::search::paginator::Paginator;
use crate::search::selector::SegmentSelector;
use crate::search::types::*;
use std::collections::HashMap;
use std::sync::Arc;
use tokio_util::sync::CancellationToken;

pub mod executor;
pub mod manifest;
pub mod paginator;
pub mod selector;
pub mod snippet;
pub mod types;

pub struct SearchCoordinator {
    default_parallelism: usize,
    manifest: Arc<FsManifestStore>,
}

impl SearchCoordinator {
    pub fn new(default_parallelism: usize) -> Self {
        Self {
            default_parallelism,
            manifest: Arc::new(FsManifestStore {
                path: "manifest.json".into(),
            }),
        }
    }

    pub async fn handle(&self, req: SearchRequest) -> anyhow::Result<SearchResponse> {
        let start = std::time::Instant::now();

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

        // --- B6: выбор сегментов ---
        let selector = SegmentSelector {
            store: self.manifest.clone(),
        };
        let pinned_in = extract_pin_gen(&req.page.cursor);
        let (selected_segments, pin_gen) = selector.plan(&req, pinned_in).await?;

        // Сборка задач
        let tasks = selected_segments
            .iter()
            .map(|seg| SegmentTaskInput {
                seg_path: seg.clone(),
                wildcard: req.wildcard.clone(),
                field: req.field.clone(),
                cursor_docid: extract_last_docid(&req.page.cursor, seg),
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

        // Вставляем pinned gen’ы в курсор
        cursor.pin_gen = Some(pin_gen);

        let ttfh = if hits.is_empty() {
            0
        } else {
            start.elapsed().as_millis() as u64
        };

        Ok(SearchResponse {
            hits,
            cursor: Some(cursor),
            metrics: SearchMetrics {
                candidates_total,
                time_to_first_hit_ms: ttfh,
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

// fn extract_pin_gen(cursor: &Option<serde_json::Value>) -> Option<std::collections::HashMap<u64,u64>> {
//     let m = cursor.as_ref()?.get("pin_gen")?.as_object()?;
//     Some(
//         m.iter()
//             .filter_map(|(k, v)| {
//                 let shard = k.parse::<u64>().ok()?;
//                 let gen = v.as_u64()?;
//                 Some((shard, gen))
//             })
//             .collect()
//     )
// }

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
