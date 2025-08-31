use std::sync::Arc;
use std::time::Duration;
use tokio::task::JoinSet;
use tokio::time::timeout;
use tokio_util::sync::CancellationToken;

use grepzilla_segment::verify::VerifyEngine;

#[derive(Clone)]
pub struct SegmentTaskInput {
    pub seg_path: String,
    pub wildcard: String,
    pub field: String,
    pub cursor_docid: Option<u64>,
    pub max_candidates: u64,
    pub page_size: usize,
    // NEW: уже скомпилированный движок на весь запрос
    pub verify_engine: Arc<dyn VerifyEngine>,
}

// Ручной Debug: не требуем Debug для dyn VerifyEngine
impl std::fmt::Debug for SegmentTaskInput {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SegmentTaskInput")
            .field("seg_path", &self.seg_path)
            .field("wildcard", &self.wildcard)
            .field("field", &self.field)
            .field("cursor_docid", &self.cursor_docid)
            .field("max_candidates", &self.max_candidates)
            .field("page_size", &self.page_size)
            .field("verify_engine", &"<dyn VerifyEngine>")
            .finish()
    }
}

#[derive(Clone, Debug)]
pub struct SegmentTaskOutput {
    pub seg_path: String,
    pub last_docid: Option<u64>,
    pub candidates: u64,
    pub hits: Vec<crate::search::types::Hit>,
    pub prefilter_ms: u64,
    pub verify_ms: u64,
    pub prefetch_ms: u64,
    pub warmed_docs: u64,
}

#[derive(Clone, Debug)]
pub struct ParallelExecutor {
    parallelism: usize,
}

impl ParallelExecutor {
    pub fn new(parallelism: usize) -> Self {
        Self { parallelism }
    }

    /// Запустить все задачи с ограничением параллелизма.
    /// Возвращает: (части-результаты, deadline_hit, saturated_sem)
    pub async fn run_all<F, Fut>(
        &self,
        ct: CancellationToken,
        mut tasks: Vec<SegmentTaskInput>,
        search_fn: F,
        _page_size: usize,
        deadline: Option<Duration>,
    ) -> (Vec<SegmentTaskOutput>, bool, usize)
    where
        F: Fn(SegmentTaskInput, CancellationToken) -> Fut + Copy + Send + 'static,
        Fut: std::future::Future<Output = anyhow::Result<SegmentTaskOutput>> + Send + 'static,
    {
        // простая реализация: запускаем все таски; лимит параллелизма может контролироваться семафором (опущено ради краткости)
        let mut set = JoinSet::new();
        let mut deadline_hit = false;

        // общий токен отмены: если deadline сработал — отменяем всё
        let child = ct.child_token();

        for input in tasks.drain(..) {
            let ctok = child.clone();
            set.spawn(async move { search_fn(input, ctok).await });
        }

        let mut parts = Vec::new();

        let join_all = async {
            while let Some(res) = set.join_next().await {
                match res {
                    Ok(Ok(out)) => parts.push(out),
                    Ok(Err(_e)) => {
                        // можно логировать; для простоты игнорируем ошибочную часть
                    }
                    Err(_join_err) => {}
                }
            }
        };

        if let Some(dl) = deadline {
            match timeout(dl, join_all).await {
                Ok(_) => {}
                Err(_) => {
                    deadline_hit = true;
                    child.cancel();
                    // дождёмся аккуратно оставшихся
                    while let Some(_res) = set.join_next().await {}
                }
            }
        } else {
            join_all.await;
        }

        // saturated_sem — в этой упрощённой версии не считаем, вернём 0
        (parts, deadline_hit, 0)
    }
}
