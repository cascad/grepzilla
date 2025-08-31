// crates/broker/src/search/executor.rs
use anyhow::Result;
use std::future::Future;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::{mpsc, Semaphore};
use tokio_util::sync::CancellationToken;

/// Вход для задачи по одному сегменту.
#[derive(Clone)]
pub struct SegmentTaskInput {
    pub seg_path: String,
    pub wildcard: String,
    /// Пустая строка == нет фильтра поля
    pub field: String,
    pub cursor_docid: Option<u64>,
    pub max_candidates: u64,
    /// Для прогрева и эвристик (prefetch)
    pub page_size: usize,
    /// Движок верификации уже собран координатором (Arc<dyn VerifyEngine>)
    pub verify_engine: Arc<dyn grepzilla_segment::verify::VerifyEngine>,
}

/// Выход одной задачи.
#[derive(Debug)]
pub struct SegmentTaskOutput {
    pub seg_path: String,
    pub last_docid: Option<u64>,
    pub candidates: u64,
    pub hits: Vec<crate::search::types::Hit>,

    // Пер-сегментные метрики
    pub prefilter_ms: u64,
    pub verify_ms: u64,
    pub prefetch_ms: u64,
    pub warmed_docs: u64,
}

impl SegmentTaskOutput {
    pub fn empty(path: String) -> Self {
        Self {
            seg_path: path,
            last_docid: None,
            candidates: 0,
            hits: Vec::new(),
            prefilter_ms: 0,
            verify_ms: 0,
            prefetch_ms: 0,
            warmed_docs: 0,
        }
    }
}

/// Параллельный исполнитель с семафором + дедлайном + ранней остановкой.
pub struct ParallelExecutor {
    sem: Arc<Semaphore>,
}

impl ParallelExecutor {
    pub fn new(parallelism: usize) -> Self {
        Self {
            sem: Arc::new(Semaphore::new(parallelism.max(1))),
        }
    }

    /// Запускает все задачи `inputs` c дедлайном `deadline` и общей отменой `root_ct`.
    ///
    /// - `search_fn`: async Fn(SegmentTaskInput, CancellationToken) -> Result<SegmentTaskOutput>
    /// - Ранняя остановка: как только суммарно собрано ≥ page_size хитов, кооперативно отменяем остальные.
    ///
    /// Возвращает `(parts, deadline_hit, saturated_sem)`
    pub async fn run_all<F, Fut>(
        &self,
        root_ct: CancellationToken,
        inputs: Vec<SegmentTaskInput>,
        search_fn: F,
        page_size: usize,
        deadline: Option<Duration>,
    ) -> (Vec<SegmentTaskOutput>, bool, usize)
    where
        F: Fn(SegmentTaskInput, CancellationToken) -> Fut + Send + Sync + 'static + Clone,
        Fut: Future<Output = Result<SegmentTaskOutput>> + Send + 'static,
    {
        // Быстрый путь: нет задач — считаем, что дедлайн «хитнулся», если он вообще задан.
        if inputs.is_empty() {
            return (Vec::new(), deadline.is_some(), 0);
        }

        let (tx, mut rx) = mpsc::unbounded_channel::<SegmentTaskOutput>();

        // Глобальный токен, который отменим либо по root_ct, либо по дедлайну.
        let merged_ct = CancellationToken::new();

        // Флаг дедлайна (без now_or_never).
        let deadline_hit = Arc::new(AtomicBool::new(false));

        // Привяжем merged_ct к root_ct.
        {
            let merged_ct = merged_ct.clone();
            tokio::spawn(async move {
                root_ct.cancelled().await;
                merged_ct.cancel();
            });
        }

        // Таймер дедлайна (если задан).
        if let Some(dl) = deadline {
            let merged_ct = merged_ct.clone();
            let dl_flag = deadline_hit.clone();
            tokio::spawn(async move {
                tokio::time::sleep(dl).await;
                dl_flag.store(true, Ordering::Relaxed);
                merged_ct.cancel();
            });
        }

        // Счётчик собранных хитов (для ранней остановки)
        let total_hits = Arc::new(AtomicUsize::new(0));

        // Учёт насыщения семафора (сколько раз try_acquire провалился)
        let mut saturated_sem = 0usize;

        // Запускаем задачи
        for inp in inputs {
            let sem = self.sem.clone();
            let txc = tx.clone();
            let search_fn_c = search_fn.clone();
            let merged_ct = merged_ct.clone();
            let total_hits = total_hits.clone();

            // Не стартуем лишние задачи, если уже набрано достаточно
            if total_hits.load(Ordering::Relaxed) >= page_size {
                let _ = txc.send(SegmentTaskOutput::empty(inp.seg_path.clone()));
                continue;
            }

            // Семафор:
            // быстрый путь — пробуем мгновенно через КЛОН семафора (он будет потреблён),
            // медленный путь — ждём на исходном `sem`.
            let fast_sem = sem.clone();
            let permit = match fast_sem.try_acquire_owned() {
                Ok(p) => p,
                Err(_) => {
                    saturated_sem += 1;
                    match sem.acquire_owned().await {
                        Ok(p) => p,
                        Err(_) => {
                            // семафор закрыт
                            let _ = txc.send(SegmentTaskOutput::empty(inp.seg_path.clone()));
                            continue;
                        }
                    }
                }
            };

            // Реальный запуск
            tokio::spawn(async move {
                let _g = permit;
                // Детский токен от merged_ct для задачи
                let task_ct = merged_ct.child_token();

                let out_res = search_fn_c(inp.clone(), task_ct.clone()).await;
                let mut out = match out_res {
                    Ok(v) => v,
                    Err(_) => SegmentTaskOutput::empty(inp.seg_path.clone()),
                };

                // Увеличим глобальный счётчик и, если страница набрана — отменим остальных
                let prev = total_hits.fetch_add(out.hits.len(), Ordering::Relaxed);
                if prev + out.hits.len() >= page_size {
                    task_ct.cancel();
                }

                let _ = txc.send(out);
            });
        }

        drop(tx); // закрываем канал — сигнал сборщику

        // Собираем результаты
        let mut parts: Vec<SegmentTaskOutput> = Vec::new();
        while let Some(p) = rx.recv().await {
            parts.push(p);
        }

        (parts, deadline_hit.load(Ordering::Relaxed), saturated_sem)
    }
}
