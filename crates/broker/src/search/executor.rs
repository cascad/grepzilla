// broker/src/search/executor.rs
use tokio::sync::{Semaphore, SemaphorePermit};
use tokio_util::sync::CancellationToken;
use std::sync::Arc;
use std::time::{Instant, Duration};
use futures::StreamExt;

#[derive(Debug)]
pub struct SegmentTaskInput {
    pub seg_path: String,
    pub wildcard: String,
    pub field: String,
    pub cursor_docid: Option<u64>,
    pub max_candidates: u64,
}

pub struct SegmentTaskOutput {
    pub seg_path: String,
    pub hits: Vec<serde_json::Value>,
    pub last_docid: Option<u64>,
    pub candidates: u64,
}

pub struct ParallelExecutor {
    sem: Arc<Semaphore>,
}

impl ParallelExecutor {
    pub fn new(parallelism: usize) -> Self {
        Self { sem: Arc::new(Semaphore::new(parallelism)) }
    }

    async fn run_one<F, Fut>(
        &self,
        ct: CancellationToken,
        input: SegmentTaskInput,
        search_fn: F,
    ) -> anyhow::Result<SegmentTaskOutput>
    where
        F: Fn(SegmentTaskInput, CancellationToken) -> Fut + Send + Sync + 'static + Copy,
        Fut: std::future::Future<Output = anyhow::Result<SegmentTaskOutput>>,
    {
        let _permit: SemaphorePermit<'_> = self.sem.acquire().await?;
        // Если отменили — сразу выходим
        if ct.is_cancelled() { anyhow::bail!("cancelled"); }
        // Вызов конкретной реализации поиска по сегменту
        search_fn(input, ct).await
    }

    pub async fn run_all<F, Fut>(
        &self,
        ct: CancellationToken,
        tasks: Vec<SegmentTaskInput>,
        search_fn: F,
        page_size: usize,
        deadline: Option<Duration>,
    ) -> (Vec<SegmentTaskOutput>, bool, u32)
    where
        F: Fn(SegmentTaskInput, CancellationToken) -> Fut + Send + Sync + 'static + Copy,
        Fut: std::future::Future<Output = anyhow::Result<SegmentTaskOutput>>,
    {
        let started = Instant::now();
        let mut outputs = Vec::new();
        let mut deadline_hit = false;

        let mut futs = futures::stream::FuturesUnordered::new();
        for t in tasks {
            let ct_child = ct.child_token();
            futs.push(self.run_one(ct_child, t, search_fn));
        }

        let mut collected_hits = 0usize;
        while let Some(res) = if let Some(d) = deadline {
            tokio::time::timeout(d.saturating_sub(started.elapsed()), futs.next()).await
                .unwrap_or(None)
        } else { futs.next().await } {
            match res {
                Ok(out) => {
                    collected_hits += out.hits.len();
                    outputs.push(out);
                    if collected_hits >= page_size {
                        // Достигли нужного числа — отменяем остальные
                        ct.cancel();
                        break;
                    }
                }
                Err(_) => { /* логгируем ошибку по сегменту, продолжаем */ }
            }
        }

        if futs.len() > 0 {
            // Если ещё были невыбранные фьючи и мы вышли по таймауту — пометим дедлайн
            if let Some(d) = deadline {
                if started.elapsed() >= d { deadline_hit = true; }
            }
            ct.cancel();
        }

        let saturated = (self.sem.available_permits() == 0) as u32;
        (outputs, deadline_hit, saturated)
    }
}
