// crates/broker/src/search/mod.rs

pub mod executor;
pub mod paginator;
pub mod types;

use std::sync::Arc;
use tokio_util::sync::CancellationToken;

use crate::manifest::{ManifestStore, SegRef};
use crate::search::executor::{ParallelExecutor, SegmentTaskInput};
use crate::search::paginator::Paginator;
use crate::search::types::*;
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

    pub async fn handle(&self, req: SearchRequest) -> anyhow::Result<SearchResponse> {
        let start = std::time::Instant::now();

        // 0) Компилируем движок верификации один раз на весь запрос
        let eng = self.verify_factory.compile(&req.wildcard)?;

        // 1) Выбираем сегменты (shards → manifest; иначе — segments из запроса)
        let mut pin_gen = std::collections::HashMap::new();
        let selected: Vec<SegRef> =
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

        // 2) Приоритизируем свежие гены внутри шарда
        let mut selected = selected;
        selected.sort_by(|a, b| match a.shard.cmp(&b.shard) {
            std::cmp::Ordering::Equal => b.gen.cmp(&a.gen), // DESC по gen
            other => other,
        });

        // 3) Лимиты/параллелизм
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

        // 4) Формируем таски (каждому даём verify_engine: Arc<dyn VerifyEngine>)
        let tasks = selected
            .iter()
            .map(|s| SegmentTaskInput {
                seg_path: s.path.clone(),
                wildcard: req.wildcard.clone(),
                field: req.field.clone().unwrap_or_default(),
                cursor_docid: extract_last_docid(&req.page.cursor, &s.path),
                max_candidates: limits.max_candidates.unwrap_or(200_000),
                page_size: req.page.size,
                verify_engine: eng.clone(),
            })
            .collect::<Vec<_>>();

        let ct = CancellationToken::new();
        let deadline = limits.deadline_duration();

        // 5) Исполнение: search_one_segment читает движок из input.verify_engine
        let search_fn = |input: SegmentTaskInput, ctok: CancellationToken| async move {
            let out = crate::storage_adapter::search_one_segment(input, ctok).await?;
            Ok::<_, anyhow::Error>(out)
        };

        let (parts, deadline_hit, saturated_sem) = executor
            .run_all(ct.clone(), tasks, search_fn, req.page.size, deadline)
            .await;

        // 6) Сшиваем и агрегируем метрики
        let (hits, mut cursor, candidates_total, dedup_dropped, totals) =
            Paginator::merge(parts, req.page.size);

        let ttfh = if hits.is_empty() {
            0
        } else {
            start.elapsed().as_millis() as u64
        };

        if !pin_gen.is_empty() {
            cursor.pin_gen = Some(pin_gen);
        }

        let (prefilter_ms_total, verify_ms_total, prefetch_ms_total, warmed_docs_total) = totals;

        // Если совсем пусто — отдадим null (D6.3). Иначе — суммы.
        let has_any_metrics = candidates_total > 0
            || prefilter_ms_total > 0
            || verify_ms_total > 0
            || prefetch_ms_total > 0
            || warmed_docs_total > 0;

        Ok(SearchResponse {
            hits,
            cursor: Some(cursor),
            metrics: SearchMetrics {
                candidates_total,
                time_to_first_hit_ms: ttfh,
                deadline_hit,
                saturated_sem: saturated_sem as u64,
                dedup_dropped,
                prefilter_ms: if has_any_metrics {
                    Some(prefilter_ms_total)
                } else {
                    None
                },
                verify_ms: if has_any_metrics {
                    Some(verify_ms_total)
                } else {
                    None
                },
                prefetch_ms: if has_any_metrics {
                    Some(prefetch_ms_total)
                } else {
                    None
                },
                warmed_docs: if has_any_metrics {
                    Some(warmed_docs_total)
                } else {
                    None
                },
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
