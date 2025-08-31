use std::net::SocketAddr;
use std::sync::Arc;

use anyhow::Result;
use tracing_subscriber::{fmt, EnvFilter};

use broker::http_api::{self, AppState};
use broker::search::SearchCoordinator;
use broker::config::BrokerConfig;
use broker::ingest::hot::HotMem;

#[tokio::main]
async fn main() -> Result<()> {
    init_tracing();

    let cfg = BrokerConfig::from_env();

    // HotMem с ограничением из конфига
    let hot = HotMem::new().with_cap(cfg.hot_cap);

    let coord = Arc::new(SearchCoordinator::new(cfg.parallelism).with_hot(hot.clone()));

    let state = AppState {
        coord: coord.clone(),
        cfg: cfg.clone(),
        hot,
    };

    let app = http_api::router(state);

    let addr: SocketAddr = cfg.addr.parse().unwrap_or_else(|_| "0.0.0.0:8080".parse().unwrap());
    tracing::info!(address = %addr, "broker listening");
    println!("=== BROKER START {}", env!("CARGO_PKG_VERSION"));

    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(listener, app).await?;

    Ok(())
}

fn init_tracing() {
    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"));
    let _ = fmt().with_env_filter(filter).with_target(false).try_init();
}
