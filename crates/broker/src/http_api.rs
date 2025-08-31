// crates/broker/src/http_api.rs
use std::sync::Arc;

use axum::extract::Path;
use axum::routing::get;
use axum::{extract::State, routing::post, Json, Router};
use serde::Serialize;

use crate::search::types::{SearchRequest, SearchResponse};
use crate::search::SearchCoordinator;
use serde_json::{json, Value};
use tokio::fs;

// manifest
use crate::manifest::fs::FsManifestStore;
use crate::manifest::{ManifestFlat, ManifestStore};

// ingest
use crate::config::BrokerConfig;
use crate::ingest::handle_batch_json;
use crate::ingest::hot::HotMem;

#[derive(Clone)]
pub struct AppState {
    pub coord: Arc<SearchCoordinator>,
    pub cfg: BrokerConfig,
    pub hot: HotMem,
}

#[derive(Serialize)]
struct ManifestShardOut {
    shard: u64,
    gen: u64,
    segments: Vec<String>,
}

pub fn router(state: AppState) -> Router {
    Router::<AppState>::new()
        .route("/healthz", get(healthz))
        .route("/search", post(search))
        .route("/manifest/:shard", get(get_manifest))
        .route("/ingest", post(ingest_batch)) // новый эндпоинт
        .with_state(state)
}

pub async fn search(
    State(st): State<AppState>,
    Json(mut req): Json<SearchRequest>,
) -> Result<Json<SearchResponse>, (axum::http::StatusCode, String)> {
    let mut resolved_pin_gen: Option<std::collections::HashMap<u64, u64>> = None;

    // shards → resolve через манифест
    if let Some(shards) = req.shards.clone() {
        let manifest_path =
            std::env::var("GZ_MANIFEST").unwrap_or_else(|_| "manifest.json".to_string());
        let store = FsManifestStore { path: manifest_path.into() };

        let (seg_refs, pin_map) = store.resolve(&shards).await.map_err(internal)?;
        req.segments = seg_refs.into_iter().map(|r| r.path).collect();
        resolved_pin_gen = Some(pin_map.clone());

        // совместимость: кладём pin_gen и во входной курсор
        match &mut req.page.cursor {
            Some(cur) => {
                let obj = cur.as_object_mut().unwrap();
                obj.insert("pin_gen".to_string(), serde_json::to_value(pin_map).unwrap());
            }
            None => {
                req.page.cursor = Some(json!({ "per_seg": {}, "pin_gen": pin_map }));
            }
        }
        req.shards = None;
    }

    // выполняем поиск
    let mut resp = st.coord.handle(req).await.map_err(internal)?;

    // проставляем pin_gen в ОТВЕТ, если резолвили shards
    if let (Some(pin), Some(cursor)) = (resolved_pin_gen, resp.cursor.as_mut()) {
        cursor.pin_gen = Some(pin);
    }

    Ok(Json(resp))
}

async fn get_manifest(
    Path(shard): Path<u64>,
) -> Result<Json<ManifestShardOut>, (axum::http::StatusCode, String)> {
    // путь к манифесту: env или ./manifest.json
    let manifest_path =
        std::env::var("GZ_MANIFEST").unwrap_or_else(|_| "manifest.json".to_string());
    let store = FsManifestStore { path: manifest_path.clone().into() };

    // 1) пробуем unified-загрузчик (если файл в v1/unified)
    if let Ok(uni) = store.load().await {
        if let Some(&gen) = uni.pin_gen.get(&shard) {
            let segments = uni.segs.get(&(shard, gen)).cloned().unwrap_or_default();
            return Ok(Json(ManifestShardOut { shard, gen, segments }));
        }
        // падать не будем — попробуем flat-фолбэк ниже
    }

    // 2) flat-фолбэк: читаем как плоский формат
    let data = tokio::fs::read(&manifest_path).await.map_err(internal)?;
    // если файл пустой/плохой — отдаём 404
    let flat: crate::manifest::ManifestFlat =
        match serde_json::from_slice(&data) {
            Ok(v) => v,
            Err(_) => {
                return Err((axum::http::StatusCode::NOT_FOUND, format!("shard {shard} not found")))
            }
        };

    // сначала пытаемся взять generation из flat.shards
    if let Some(&gen) = flat.shards.get(&shard) {
        let key = format!("{shard}:{gen}");
        let segments = flat.segments.get(&key).cloned().unwrap_or_default();
        return Ok(Json(ManifestShardOut { shard, gen, segments }));
    }

    // если в shards нет — выведем max(gen) из ключей segments "shard:gen"
    let mut max_gen: Option<u64> = None;
    for k in flat.segments.keys() {
        // ожидаем формат "<shard>:<gen>"
        if let Some((lh, rh)) = k.split_once(':') {
            if let (Ok(s), Ok(g)) = (lh.parse::<u64>(), rh.parse::<u64>()) {
                if s == shard {
                    max_gen = Some(max_gen.map_or(g, |cur| cur.max(g)));
                }
            }
        }
    }

    if let Some(gen) = max_gen {
        let key = format!("{shard}:{gen}");
        let segments = flat.segments.get(&key).cloned().unwrap_or_default();
        return Ok(Json(ManifestShardOut { shard, gen, segments }));
    }

    // ничего не нашли
    Err((axum::http::StatusCode::NOT_FOUND, format!("shard {shard} not found")))
}


/// Принимает JSON-док(и), пишет WAL→сегмент и (если задан GZ_MANIFEST) публикует в манифест.
pub async fn ingest_batch(
    State(st): State<AppState>,
    Json(body): Json<Value>,
) -> Result<Json<serde_json::Value>, (axum::http::StatusCode, String)> {
    // Разворачиваем тело: либо массив, либо единичный объект
    let records_vec: Vec<Value> = match body {
        Value::Array(arr) => arr,
        other => vec![other],
    };

    // 0) горячая память — мгновенная видимость
    let added = st.hot.push_raw_json(records_vec.clone());

    // 1) WAL → segment
    let out = handle_batch_json(records_vec, &st.cfg).await.map_err(internal)?;
    let seg_path = out
        .get("segment")
        .and_then(|v| v.as_str())
        .unwrap_or_default()
        .to_string();

    // 2) Публикация в манифест: НЕ роняем 500 при ошибке — просто добавим поле manifest_error
    let mut manifest_error: Option<String> = None;
    if let Ok(manifest_path) = std::env::var("GZ_MANIFEST") {
        let shard: u64 = std::env::var("GZ_SHARD")
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(0);
        let store = FsManifestStore { path: manifest_path.into() };
        if let Err(e) = store.append_segment(shard, seg_path).await {
            // логируем и продолжаем
            tracing::warn!(%e, "append_segment failed");
            manifest_error = Some(e.to_string());
        }
    }

    // 3) ответ (+ hot_added, + manifest_error при наличии)
    let mut out_obj = out.as_object().cloned().unwrap_or_default();
    out_obj.insert("hot_added".into(), serde_json::json!(added));
    if let Some(err) = manifest_error {
        out_obj.insert("manifest_error".into(), serde_json::json!(err));
    }
    Ok(Json(serde_json::Value::Object(out_obj)))
}

fn internal<E: ToString>(e: E) -> (axum::http::StatusCode, String) {
    (axum::http::StatusCode::INTERNAL_SERVER_ERROR, e.to_string())
}

pub async fn healthz() -> Json<serde_json::Value> {
    Json(serde_json::json!({ "status": "ok" }))
}
