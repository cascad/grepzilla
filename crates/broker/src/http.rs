// crates/broker/src/http.rs
use axum::{routing::post, Router, extract::State, Json};
use std::sync::Arc;

use crate::search::SearchCoordinator;
use crate::search::types::{SearchRequest, SearchResponse};

#[derive(Clone)]
pub struct AppState {
    pub coord: Arc<SearchCoordinator>,
}

pub fn router(state: AppState) -> Router {
    Router::new()
        .route("/search", post(search))
        .with_state(state)
}

async fn search(
    State(st): State<AppState>,
    Json(req): Json<SearchRequest>,
) -> Result<Json<SearchResponse>, (axum::http::StatusCode, String)> {
    eprintln!("HIT /search"); std::io::Write::flush(&mut std::io::stderr()).ok();
    st.coord.handle(req).await
        .map(Json)
        .map_err(|e| (axum::http::StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))
}
