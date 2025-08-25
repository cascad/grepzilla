// Файл: crates/broker/src/main.rs
use std::net::SocketAddr;
use std::sync::Arc;

use anyhow::Result;
use tracing_subscriber::{fmt, EnvFilter};

mod search;           // координатор и типы поиска (B5)
mod storage_adapter;  // вызов поиска по одному сегменту
mod config;           // конфиг (пока не используем, но оставляем)
mod http;             // axum Router, хендлеры

use crate::http::AppState;
use crate::search::SearchCoordinator;

#[tokio::main]
async fn main() -> Result<()> {
    init_tracing();

    // Пока жёстко: 4 воркера на сегменты. Можно взять из config позже.
    let coord = Arc::new(SearchCoordinator::new(4));
    let state = AppState { coord };

    let app = http::router(state);

    let addr: SocketAddr = "0.0.0.0:8080".parse().unwrap();
    tracing::info!(address = %addr, "broker listening");
    println!("=== BROKER START {}", env!("CARGO_PKG_VERSION"));

    // axum 0.7 стиль: TcpListener + axum::serve
    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(listener, app).await?;

    Ok(())
}

fn init_tracing() {
    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"));
    let _ = fmt().with_env_filter(filter).with_target(false).try_init();
}
