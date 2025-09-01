// path: crates/broker/tests/helpers.rs
use axum::Router;
use broker::config::BrokerConfig;
use broker::http_api::{router, AppState};
use broker::ingest::hot::HotMem;
use broker::search::SearchCoordinator;
use std::sync::Arc;

pub fn make_router_with_config(cfg: BrokerConfig) -> Router {
    // создаём HotMem с нужной ёмкостью
    let hot = HotMem::new().with_cap(cfg.hot_cap);

    // ВАЖНО: прокинуть hot в координатор поиска,
    // чтобы /search видел документы, которые мы только что залили через /ingest
    let coord = Arc::new(
        SearchCoordinator::new(cfg.parallelism)
            .with_hot(hot.clone())
    );

    let state = AppState { coord, cfg, hot };
    router(state)
}

pub fn make_router_with_parallelism(parallelism: usize) -> Router {
    let mut cfg = BrokerConfig::from_env();
    cfg.parallelism = parallelism;
    make_router_with_config(cfg)
}

// Удобный хелпер для тестов cap’а
pub fn make_router_with_parallelism_and_cap(parallelism: usize, cap: usize) -> Router {
    let mut cfg = BrokerConfig::from_env();
    cfg.parallelism = parallelism;
    cfg.hot_cap = cap;
    make_router_with_config(cfg)
}
