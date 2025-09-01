// path: crates/broker/src/http_api.rs

use std::sync::Arc;

use axum::extract::Path;
use axum::routing::get;
use axum::{extract::State, routing::post, Json, Router};
use axum::http::HeaderMap;

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
use crate::ingest::{ApplyResult, Backpressure};

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
        // FIX: сигнатура get_manifest теперь принимает State(AppState),
        // axum сам инжектит State, маршрут остаётся тем же
        .route("/manifest/:shard", get(get_manifest))
        .route("/ingest", post(ingest_batch))
        .with_state(state)
}

pub async fn search(
    State(st): State<AppState>,
    Json(mut req): Json<SearchRequest>,
) -> Result<Json<SearchResponse>, (axum::http::StatusCode, String)> {
    let mut resolved_pin_gen: Option<std::collections::HashMap<u64, u64>> = None;

    // shards → resolve через манифест
    if let Some(shards) = req.shards.clone() {
        // FIX: берём путь из конфига
        let manifest_path = st
            .cfg
            .manifest_path
            .clone()
            .unwrap_or_else(|| "manifest.json".to_string());
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

// FIX: берём State(AppState), читаем манифест из st.cfg.manifest_path
async fn get_manifest(
    State(st): State<AppState>,
    Path(shard): Path<u64>,
) -> Result<Json<ManifestShardOut>, (axum::http::StatusCode, String)> {
    let manifest_path = st
        .cfg
        .manifest_path
        .clone()
        .unwrap_or_else(|| "manifest.json".to_string());
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
    let data = match tokio::fs::read(&manifest_path).await {
        Ok(d) => d,
        Err(_) => {
            return Err((axum::http::StatusCode::NOT_FOUND, format!("shard {shard} not found")));
        }
    };
    // если файл пустой/плохой — отдаём 404
    let flat: crate::manifest::ManifestFlat = match serde_json::from_slice(&data) {
        Ok(v) => v,
        Err(_) => {
            return Err((axum::http::StatusCode::NOT_FOUND, format!("shard {shard} not found")));
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

/// POST /ingest
pub async fn ingest_batch(
    State(st): State<AppState>,
    headers: HeaderMap,
    Json(body): Json<Value>,
) -> impl axum::response::IntoResponse {
    let records_vec: Vec<Value> = match body {
        Value::Array(arr) => arr,
        other => vec![other],
    };

    let idempotency_key = headers
        .get("Idempotency-Key")
        .and_then(|h| h.to_str().ok())
        .map(|s| s.to_string());

    // 0) Горячая память — мгновенная видимость (с учётом идемпотентности/лимитов)
    let applied: ApplyResult = match st.hot.apply(records_vec.clone(), idempotency_key) {
        Ok(a) => a,
        Err(Backpressure { retry_after_ms }) => {
            return (
                axum::http::StatusCode::SERVICE_UNAVAILABLE,
                Json(json!({
                    "ok": false,
                    "hot_added": 0,
                    "idempotent": false,
                    "backlog_ms": retry_after_ms
                })),
            );
        }
    };

    // Если чистый повтор — не пишем на диск и не публикуем сегмент
    if applied.idempotent {
        return (
            axum::http::StatusCode::OK,
            Json(json!({
                "ok": true,
                "hot_added": 0,
                "idempotent": true
            })),
        );
    }

    // 1) WAL → segment (нефатальные ошибки после HotMem)
    let mut seg_path: Option<String> = None;
    let mut segment_error: Option<String> = None;
    let out = match handle_batch_json(records_vec, &st.cfg).await {
        Ok(v) => {
            seg_path = v
                .get("segment")
                .and_then(|x| x.as_str())
                .map(|s| s.to_string());
            v
        }
        Err(e) => {
            tracing::error!("ingest: handle_batch_json failed: {e}");
            segment_error = Some(e.to_string());
            json!({ "ok": true })
        }
    };

    // 2) Публикация в манифест (через cfg, без env)
    let mut manifest_error: Option<String> = None;
    if let (Some(manifest_path), Some(seg)) = (st.cfg.manifest_path.clone(), seg_path.clone()) {
        let shard: u64 = st.cfg.shard;
        let store = FsManifestStore { path: manifest_path.clone().into() };
        if let Some(parent) = std::path::Path::new(&manifest_path).parent() {
            let _ = tokio::fs::create_dir_all(parent).await;
        }
        match store.append_segment(shard, seg.clone()).await {
            Ok(_) => {}
            Err(e) => {
                let exists = tokio::fs::try_exists(&manifest_path).await.unwrap_or(false);
                if !exists {
                    let _ = tokio::fs::write(&manifest_path, b"{}").await;
                    if let Err(e2) = store.append_segment(shard, seg).await {
                        tracing::warn!(%e2, "append_segment failed after init");
                        manifest_error = Some(e2.to_string());
                    }
                } else {
                    tracing::warn!(%e, "append_segment failed");
                    manifest_error = Some(e.to_string());
                }
            }
        }
    }

    // 3) Ответ
    let mut out_obj = out.as_object().cloned().unwrap_or_default();
    out_obj.insert("hot_added".into(), json!(applied.added));
    out_obj.insert("idempotent".into(), json!(false));
    if let Some(ms) = applied.backlog_ms {
        out_obj.insert("backlog_ms".into(), json!(ms));
    }
    if let Some(err) = segment_error {
        out_obj.insert("segment_error".into(), json!(err));
    }
    if let Some(err) = manifest_error {
        out_obj.insert("manifest_error".into(), json!(err));
    }

    (axum::http::StatusCode::OK, Json(serde_json::Value::Object(out_obj)))
}

fn internal<E: ToString>(e: E) -> (axum::http::StatusCode, String) {
    (axum::http::StatusCode::INTERNAL_SERVER_ERROR, e.to_string())
}

pub async fn healthz() -> Json<serde_json::Value> {
    Json(serde_json::json!({ "status": "ok" }))
}
