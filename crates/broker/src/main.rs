// Файл: crates/broker/src/main.rs
use std::collections::HashMap;
use std::net::SocketAddr;

use anyhow::Result;
use axum::{routing::post, Json, Router};
use regex::Regex;
use serde::{Deserialize, Serialize};
use tracing_subscriber::{fmt, EnvFilter};

use grepzilla_segment::cursor::{Budgets, SearchCursor, ShardPos};
use grepzilla_segment::gram::{required_grams_from_wildcard, BooleanOp};
use grepzilla_segment::segjson::JsonSegmentReader;
use grepzilla_segment::SegmentReader;

#[tokio::main]
async fn main() -> Result<()> {
    init_tracing();

    let app = Router::new().route("/search", post(search_handler));

    let addr: SocketAddr = "0.0.0.0:8080".parse().unwrap();
    // structured log: label = %Display
    tracing::info!(address = %addr, "broker listening");

    // axum 0.7: TcpListener + axum::serve
    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(listener, app).await?;

    Ok(())
}

fn init_tracing() {
    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"));
    let _ = fmt().with_env_filter(filter).with_target(false).try_init();
}

// --- API модели ---
#[derive(Debug, Deserialize)]
struct PageIn {
    size: usize,
    cursor: Option<SearchCursor>,
}

#[derive(Debug, Deserialize)]
struct SearchIn {
    wildcard: String,
    field: Option<String>,
    // Вариант 1: прямые пути до сегментов (проще всего для демо)
    segments: Vec<String>,
    // Вариант 2 (на будущее): shards/consistency/pin_gen — читаем из manifest store
    // shards: Option<Vec<u64>>,
    page: PageIn,
}

#[derive(Debug, Serialize)]
struct Hit {
    ext_id: String,
    doc_id: u32,
    preview: String,
}

#[derive(Debug, Serialize)]
struct SearchOut {
    hits: Vec<Hit>,
    cursor: Option<SearchCursor>,
}

// --- Логика поиска ---
async fn search_handler(Json(req): Json<SearchIn>) -> Json<SearchOut> {
    let grams = match required_grams_from_wildcard(&req.wildcard) {
        Ok(g) => g,
        Err(_) => return Json(SearchOut { hits: vec![], cursor: None }),
    };
    let rx = wildcard_to_regex(&req.wildcard).expect("bad regex");

    // Загружаем все сегменты (для демо в лоб)
    let mut readers = Vec::new();
    for p in &req.segments {
        match JsonSegmentReader::open_segment(p) {
            Ok(r) => readers.push((p.clone(), r)),
            Err(err) => {
                // FIX: нельзя писать error=%?err — используем Debug или Display
                tracing::warn!(path = %p, error = ?err, "failed to open segment");
            }
        }
    }

    let mut hits = Vec::new();
    let mut next_state: Vec<ShardPos> = Vec::new();
    let mut pin_gen: HashMap<u64, u64> = HashMap::new(); // демо: пустой, gen не пинится

    let limit = req.page.size.max(1).min(1000);

    // Курсор: last_docid per segment path (используем segment=путь)
    let mut last_by_seg: HashMap<String, u32> = HashMap::new();
    if let Some(cur) = &req.page.cursor {
        for sp in &cur.state {
            last_by_seg.insert(sp.segment.clone(), sp.last_docid);
        }
        pin_gen = cur.pin_gen.clone(); // на будущее
    }

    'outer: for (seg_path, reader) in &readers {
        let bm = match reader.prefilter(BooleanOp::And, &grams, req.field.as_deref()) {
            Ok(b) => b,
            Err(_) => continue,
        };
        let start_after = last_by_seg.get(seg_path).copied();
        let mut last_seen = start_after.unwrap_or(0);

        for doc_id in bm.iter() {
            if let Some(prev) = start_after {
                if doc_id <= prev {
                    continue;
                }
            }
            if let Some(doc) = reader.get_doc(doc_id) {
                // Проверяем совпадение
                let matched = match req.field.as_deref() {
                    Some(f) => doc.fields.get(f).map(|t| rx.is_match(t)).unwrap_or(false),
                    None => doc.fields.values().any(|t| rx.is_match(t)),
                };
                if !matched {
                    continue;
                }

                // Сниппет
                let text = doc
                    .fields
                    .get("text.body")
                    .or_else(|| doc.fields.get("text.title"))
                    .cloned()
                    .unwrap_or_default();
                let preview = build_snippet(&rx, &text, 80);

                hits.push(Hit {
                    ext_id: doc.ext_id.clone(),
                    doc_id,
                    preview,
                });
                last_seen = doc_id;
                if hits.len() >= limit {
                    break 'outer;
                }
            }
        }

        // Сохраняем позицию для этого сегмента
        next_state.push(ShardPos {
            shard: 0,
            segment: seg_path.clone(),
            block: 0,
            last_docid: last_seen,
        });
    }

    let cursor = if hits.is_empty() {
        None
    } else {
        Some(SearchCursor {
            matcher_hash: simple_hash(&req.wildcard, req.field.as_deref()),
            pin_gen,
            state: next_state,
            budgets: Budgets {
                candidates: 0,
                verify_ms: 0,
            },
        })
    };

    Json(SearchOut { hits, cursor })
}

fn wildcard_to_regex(pat: &str) -> Result<Regex, regex::Error> {
    let mut rx = String::from("(?s)");
    for ch in pat.chars() {
        match ch {
            '*' => rx.push_str(".*"),
            '?' => rx.push('.'),
            c => {
                if "\\.^$|()[]{}+*?".contains(c) {
                    rx.push('\\');
                }
                rx.push(c);
            }
        }
    }
    Regex::new(&rx)
}

fn build_snippet(rx: &Regex, text: &str, window: usize) -> String {
    if let Some(m) = rx.find(text) {
        let start = m.start();
        let end = m.end();
        let ctx = window.saturating_sub((end - start).min(window) + 2) / 2;
        let from = start.saturating_sub(ctx);
        let to = (end + ctx).min(text.len());
        let mut out = String::new();
        if from > 0 {
            out.push('…');
        }
        out.push_str(&text[from..start]);
        out.push('[');
        out.push_str(&text[start..end]);
        out.push(']');
        out.push_str(&text[end..to]);
        if to < text.len() {
            out.push('…');
        }
        out
    } else if text.len() > window {
        format!("{}…", &text[..window])
    } else {
        text.to_string()
    }
}

fn simple_hash(q: &str, field: Option<&str>) -> String {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    q.hash(&mut h);
    field.unwrap_or("").hash(&mut h);
    format!("{:x}", h.finish())
}
