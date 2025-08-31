// crates/broker/tests/helpers.rs
use std::sync::Arc;
use axum::Router;

pub fn test_cfg(tmp: &tempfile::TempDir, parallelism: usize) -> broker::config::BrokerConfig {
    broker::config::BrokerConfig {
        addr: "127.0.0.1:0".to_string(),
        wal_dir: tmp.path().join("wal").to_string_lossy().into(),
        segment_out_dir: tmp.path().join("segments").to_string_lossy().into(),
        parallelism,
        hot_cap: 1000, // дефолт для старых тестов
    }
}

// NEW: конфиг с явным cap
pub fn test_cfg_with_cap(
    tmp: &tempfile::TempDir,
    parallelism: usize,
    hot_cap: usize,
) -> broker::config::BrokerConfig {
    broker::config::BrokerConfig {
        addr: "127.0.0.1:0".to_string(),
        wal_dir: tmp.path().join("wal").to_string_lossy().into(),
        segment_out_dir: tmp.path().join("segments").to_string_lossy().into(),
        parallelism,
        hot_cap,
    }
}

pub fn make_router_with_parallelism(parallelism: usize) -> Router {
    let tmp = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(tmp.path().join("wal")).unwrap();
    std::fs::create_dir_all(tmp.path().join("segments")).unwrap();
    let cfg = test_cfg(&tmp, parallelism);

    let hot = broker::ingest::hot::HotMem::new().with_cap(cfg.hot_cap);
    let coord = Arc::new(broker::search::SearchCoordinator::new(parallelism).with_hot(hot.clone()));

    let app = broker::http_api::router(broker::http_api::AppState { coord, cfg, hot });
    std::mem::forget(tmp);
    app
}

// NEW: роутер с явным cap
pub fn make_router_with_parallelism_and_cap(parallelism: usize, hot_cap: usize) -> Router {
    let tmp = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(tmp.path().join("wal")).unwrap();
    std::fs::create_dir_all(tmp.path().join("segments")).unwrap();
    let cfg = test_cfg_with_cap(&tmp, parallelism, hot_cap);

    let hot = broker::ingest::hot::HotMem::new().with_cap(cfg.hot_cap);
    let coord = Arc::new(broker::search::SearchCoordinator::new(parallelism).with_hot(hot.clone()));

    let app = broker::http_api::router(broker::http_api::AppState { coord, cfg, hot });
    std::mem::forget(tmp);
    app
}
