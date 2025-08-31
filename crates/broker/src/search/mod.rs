pub mod executor;
pub mod paginator;
pub mod types;

use std::sync::Arc;
use tokio_util::sync::CancellationToken;

use crate::manifest::{ManifestStore, SegRef};
use crate::search::executor::{ParallelExecutor, SegmentTaskInput, SegmentTaskOutput};
use crate::search::paginator::Paginator;
use crate::search::types::*;

use grepzilla_segment::normalizer::normalize;
use grepzilla_segment::verify::{EnvVerifyFactory, VerifyFactory};

pub struct SearchCoordinator {
    default_parallelism: usize,
    manifest: Option<Arc<dyn ManifestStore>>,
    verify_factory: Arc<dyn VerifyFactory>,
}

impl SearchCoordinator {
    pub fn new(default_parallelism: usize) -> Self {
        Self {
            default_parallelism,
            manifest: None,
            verify_factory: Arc::new(EnvVerifyFactory::from_env()),
        }
    }

    pub fn with_manifest(mut self, store: Arc<dyn ManifestStore>) -> Self {
        self.manifest = Some(store);
        self
    }

    #[allow(dead_code)]
    pub fn with_verify_factory(mut self, vf: Arc<dyn VerifyFactory>) -> Self {
        self.verify_factory = vf;
        self
    }

    pub async fn handle(&self, req: SearchRequest) -> anyhow::Result<SearchResponse> {
        let start = std::time::Instant::now();

        // выбрать сегменты (shards → manifest; иначе — segments из запроса)
        let mut pin_gen = std::collections::HashMap::new();
        let mut selected: Vec<SegRef> =
            if let (Some(store), Some(shards)) = (&self.manifest, req.shards.as_ref()) {
                let (segs, pin) = store.resolve(shards).await?;
                pin_gen = pin;
                segs
            } else {
                req.segments
                    .iter()
                    .map(|p| SegRef {
                        shard: 0,
                        gen: 0,
                        path: p.clone(),
                    })
                    .collect()
            };

        // приоритизируем свежие гены внутри шарда
        selected.sort_by(|a, b| {
            use std::cmp::Ordering::*;
            match a.shard.cmp(&b.shard) {
                Equal => b.gen.cmp(&a.gen), // DESC по gen
                other => other,
            }
        });

        // Скомпилируем VerifyEngine ОДИН РАЗ на весь запрос
        let normalized_wc = normalize(&req.wildcard);
        let verify_engine = self.verify_factory.compile(&normalized_wc)?;

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
                page_size: req.page.size,
                verify_engine: verify_engine.clone(), // прокидываем движок в таск
            })
            .collect::<Vec<_>>();

        let ct = CancellationToken::new();
        let deadline = limits.deadline_duration();

        // замыкание ничего не захватывает; движок берём из input.verify_engine
        let search_fn = |input: SegmentTaskInput, ctok: CancellationToken| async move {
            let eng = input.verify_engine.clone();
            let out = crate::storage_adapter::search_one_segment(input, eng, ctok).await?;
            Ok::<SegmentTaskOutput, anyhow::Error>(out)
        };

        let (parts, deadline_hit, saturated_sem) = executor
            .run_all(ct.clone(), tasks, search_fn, req.page.size, deadline)
            .await;

        // merge + дедуп

        let (
            hits,
            mut cursor,
            candidates_total,
            dedup_dropped,
            prefilter_ms,
            verify_ms,
            prefetch_ms,
            warmed_docs,
        ) = Paginator::merge(parts, req.page.size);

        let ttfh = if hits.is_empty() {
            0
        } else {
            start.elapsed().as_millis() as u64
        };

        if !pin_gen.is_empty() {
            cursor.pin_gen = Some(pin_gen);
        }

        Ok(SearchResponse {
            hits,
            cursor: Some(cursor),
            metrics: SearchMetrics {
                candidates_total,
                time_to_first_hit_ms: ttfh,
                deadline_hit,
                saturated_sem: saturated_sem as u64,
                dedup_dropped,
                prefilter_ms: Some(prefilter_ms),
                verify_ms: Some(verify_ms),
                prefetch_ms: Some(prefetch_ms),
                warmed_docs: Some(warmed_docs),
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
