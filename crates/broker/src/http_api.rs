use std::sync::Arc;

use axum::{extract::State, routing::post, Json, Router};

use crate::search::types::{SearchRequest, SearchResponse};
use crate::search::SearchCoordinator;

use serde_json::json;

// ДОБАВЬ:
use crate::manifest::fs::FsManifestStore;
use crate::manifest::ManifestStore;

#[derive(Clone)]
pub struct AppState {
    pub coord: Arc<SearchCoordinator>,
}

pub fn router(state: AppState) -> Router {
    Router::<AppState>::new()
        .route("/search", post(search))
        .with_state(state)
}

pub async fn search(
    State(st): State<AppState>,
    Json(mut req): Json<SearchRequest>,
) -> Result<Json<SearchResponse>, (axum::http::StatusCode, String)> {
    // будем помнить pin_gen, если резолвили shards
    let mut resolved_pin_gen: Option<std::collections::HashMap<u64, u64>> = None;

    // B6: если пришли shards, резолвим их через manifest.json
    if let Some(shards) = req.shards.clone() {
        let manifest_path =
            std::env::var("GZ_MANIFEST").unwrap_or_else(|_| "manifest.json".to_string());
        let store = FsManifestStore { path: manifest_path.into() };

        let (seg_refs, pin_map) = store
            .resolve(&shards)
            .await
            .map_err(internal)?;

        // подменяем segments
        req.segments = seg_refs.into_iter().map(|r| r.path).collect();

        // запоминаем pin_gen для ответа
        resolved_pin_gen = Some(pin_map.clone());

        // для совместимости можно положить pin_gen и во входной курсор
        match &mut req.page.cursor {
            Some(cur) => {
                let obj = cur.as_object_mut().unwrap();
                obj.insert("pin_gen".to_string(), serde_json::to_value(pin_map).unwrap());
            }
            None => {
                req.page.cursor = Some(json!({ "per_seg": {}, "pin_gen": pin_map }));
            }
        }

        // shards дальше не нужны
        req.shards = None;
    }

    // выполняем поиск
    let mut resp = st
        .coord
        .handle(req)
        .await
        .map_err(internal)?;

    // ВАЖНО: проставляем pin_gen в ОТВЕТ, если резолвили shards
    if let (Some(pin), Some(cursor)) = (resolved_pin_gen, resp.cursor.as_mut()) {
        cursor.pin_gen = Some(pin);
    }

    Ok(Json(resp))
}

fn internal<E: ToString>(e: E) -> (axum::http::StatusCode, String) {
    (axum::http::StatusCode::INTERNAL_SERVER_ERROR, e.to_string())
}
