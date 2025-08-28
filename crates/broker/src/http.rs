use std::sync::Arc;

use axum::{extract::State, routing::post, Json, Router};

use crate::manifest::fs::FsManifestStore;
use crate::manifest::ManifestStore;
use crate::search::types::{SearchMetrics, SearchRequest, SearchResponse};
use crate::search::SearchCoordinator;

#[derive(Clone)]
pub struct AppState {
    pub coord: Arc<SearchCoordinator>,
}

pub fn router(state: AppState) -> Router<AppState> {
    Router::<AppState>::new()
        .route("/search", post(search_handler))
        .with_state(state)
}

pub fn build_app() -> Router<AppState> {
    let default_parallel = 4;

    // Если указан путь к манифесту — включаем B6 (manifest/pin_gen),
    // иначе остаёмся в B5 (segments из запроса).
    let coord = if let Ok(path) = std::env::var("GZ_MANIFEST_PATH") {
        let fs_store = FsManifestStore { path: path };

        // ВАЖНО: тот же трейт, что ждёт координатор: crate::manifest::ManifestStore
        let store_arc: Arc<dyn ManifestStore> = Arc::new(fs_store);

        SearchCoordinator::new(default_parallel).with_manifest(store_arc)
    } else {
        SearchCoordinator::new(default_parallel)
    };

    let state = AppState {
        coord: Arc::new(coord),
    };
    router(state)
}

pub async fn search_handler(
    State(app): State<AppState>,
    Json(req): Json<SearchRequest>,
) -> Json<SearchResponse> {
    let resp = match app.coord.handle(req).await {
        Ok(r) => r,
        Err(_) => SearchResponse {
            hits: vec![],
            cursor: None,
            metrics: SearchMetrics {
                candidates_total: 0,
                time_to_first_hit_ms: 0,
                deadline_hit: false,
                saturated_sem: 0,
            },
        },
    };
    Json(resp)
}
