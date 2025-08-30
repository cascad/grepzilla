use std::fs::{self, File};
use std::io::Write;
use std::sync::Arc;

use axum::{
    body::Body,
    http::{Request, StatusCode},
};
use serde_json::json;
use tempfile::TempDir;
use tower::util::ServiceExt;

use broker::http_api::{router, AppState};
use broker::search::SearchCoordinator;

// для сборки сегмента
use grepzilla_segment::segjson::JsonSegmentWriter;
use grepzilla_segment::SegmentWriter;

#[tokio::test]
async fn http_search_returns_hits_and_cursor() {
    // 1) Временная папка с мини-сегментом
    let tmp = TempDir::new().expect("tmpdir");
    let seg_dir = tmp.path().join("segA");
    fs::create_dir_all(&seg_dir).expect("mkdir segA");

    // Подготовим input.jsonl с двумя документами
    let in_path = tmp.path().join("input.jsonl");
    {
        let mut f = File::create(&in_path).unwrap();
        // _id обязателен? В твоём writer ext_id читается из "_id". Пусть будет.
        // Два текста, один содержит "игра".
        writeln!(
            f,
            r#"{{"_id":"1","text":{{"title":"Пример","body":"эта игра очень хороша"}}}}"#
        )
        .unwrap();
        writeln!(
            f,
            r#"{{"_id":"2","text":{{"title":"Демо","body":"ничего не найдено"}}}}"#
        )
        .unwrap();
    }

    // Собираем сегмент: JsonSegmentWriter::write_segment(...)
    let mut w = JsonSegmentWriter::default();
    w.write_segment(in_path.to_str().unwrap(), seg_dir.to_str().unwrap())
        .expect("write segment");

    // 2) Собираем приложение с координатором (по варианту 2)
    let coord = Arc::new(SearchCoordinator::new(4));
    let app = router(AppState { coord });

    // 3) Готовим HTTP-запрос POST /search
    // ВАЖНО: тело должно соответствовать broker::search::types::SearchRequest
    // Если у тебя `field` = String, оставь "text.body";
    // если Option<String> — хендлер всё равно принимает наш тип из search::types.
    let req_body = json!({
        "wildcard": "*игра*",
        "field": "text.body",
        "segments": [ seg_dir.to_string_lossy() ],
        "page": { "size": 2, "cursor": null },
        "limits": { "parallelism": 2, "deadline_ms": 1000, "max_candidates": 1000 }
    });

    let request = Request::builder()
        .method("POST")
        .uri("/search")
        .header("content-type", "application/json")
        .body(Body::from(serde_json::to_vec(&req_body).unwrap()))
        .unwrap();

    // 4) Выполняем запрос "в памяти"
    let response = app.clone().oneshot(request).await.expect("response");

    assert_eq!(response.status(), StatusCode::OK);
    let bytes = axum::body::to_bytes(response.into_body(), 64 * 1024)
        .await
        .unwrap();
    let v: serde_json::Value = serde_json::from_slice(&bytes).unwrap();

    // 5) Проверяем базовые инварианты ответа
    // hits есть и не пустой
    let hits = v
        .get("hits")
        .and_then(|h| h.as_array())
        .cloned()
        .unwrap_or_default();
    assert!(!hits.is_empty(), "expected non-empty hits, got: {v}");

    // cursor присутствует
    assert!(
        v.get("cursor").is_some(),
        "expected cursor in response, got: {v}"
    );

    // Дополнительно убедимся, что ext_id/doc_id приходят
    let first = hits.first().unwrap();
    assert!(first.get("ext_id").is_some(), "hit.ext_id missing: {first}");
    assert!(first.get("doc_id").is_some(), "hit.doc_id missing: {first}");
}
