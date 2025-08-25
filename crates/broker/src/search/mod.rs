// broker/src/search/mod.rs
pub mod executor;
pub mod paginator;
pub mod types;

use crate::search::executor::{ParallelExecutor, SegmentTaskInput, SegmentTaskOutput};
use crate::search::paginator::Paginator;
use crate::search::types::*;
use tokio_util::sync::CancellationToken;

pub struct SearchCoordinator {
    default_parallelism: usize,
}

impl SearchCoordinator {
    pub fn new(default_parallelism: usize) -> Self {
        Self {
            default_parallelism,
        }
    }

    pub async fn handle(&self, req: SearchRequest) -> anyhow::Result<SearchResponse> {
        // старт для метрики TTFH
        let start_instant = std::time::Instant::now();

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

        // Собираем задачи по сегментам
        let tasks = req
            .segments
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

        // Поиск по одному сегменту — реальный вызов адаптера
        let search_fn = |input: SegmentTaskInput, ctok: CancellationToken| async move {
            let out = crate::storage_adapter::search_one_segment(input, ctok).await?;
            Ok::<SegmentTaskOutput, anyhow::Error>(out)
        };

        let (parts, deadline_hit, saturated_sem) = executor
            .run_all(ct.clone(), tasks, search_fn, req.page.size, deadline)
            .await;

        let (hits, cursor, candidates_total) = Paginator::merge(parts, req.page.size);

        // простая оценка time-to-first-hit: если есть хоть один хит — берем общее elapsed
        let time_to_first_hit_ms: u128 = if !hits.is_empty() {
            start_instant.elapsed().as_millis()
        } else {
            0
        };

        let resp = SearchResponse {
            hits,
            cursor: Some(cursor),
            metrics: SearchMetrics {
                candidates_total,
                time_to_first_hit_ms: time_to_first_hit_ms as u64, // если тип u64/u128 — подгони
                deadline_hit,
                saturated_sem,
            },
        };
        Ok(resp)
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
