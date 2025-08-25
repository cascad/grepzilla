// crates/broker/src/storage_adapter.rs
use grepzilla_segment::SegmentReader;
use tokio_util::sync::CancellationToken;
use tracing::warn;

use crate::search::executor::{SegmentTaskInput, SegmentTaskOutput};

use regex::Regex;

use grepzilla_segment::gram::{required_grams_from_wildcard, BooleanOp};
use grepzilla_segment::normalizer::normalize;
use grepzilla_segment::segjson::JsonSegmentReader;

/// Простой экранизатор regex-метасимволов, кроме * и ? (мы обработаем их ниже).
fn escape_regex_meta_keep_wildcards(s: &str) -> String {
    let mut out = String::with_capacity(s.len() * 2);
    for ch in s.chars() {
        match ch {
            // regex-метасимволы
            '.' | '+' | '(' | ')' | '|' | '{' | '}' | '[' | ']' | '^' | '$' | '\\' => {
                out.push('\\');
                out.push(ch);
            }
            // wildcard — не экранируем, обработаем ниже
            '*' | '?' => out.push(ch),
            _ => out.push(ch),
        }
    }
    out
}

/// Конвертация wildcard → regex над УЖЕ нормализованной строкой.
/// "*" -> ".*", "?" -> ".", без якорей (подстрочный матч).
fn wildcard_norm_to_regex(norm_wildcard: &str) -> anyhow::Result<Regex> {
    let escaped = escape_regex_meta_keep_wildcards(norm_wildcard);
    // заменим wildcard на regex-эквиваленты
    let mut pat = String::with_capacity(escaped.len() + 8);
    for ch in escaped.chars() {
        match ch {
            '*' => pat.push_str(".*"),
            '?' => pat.push('.'),
            _ => pat.push(ch),
        }
    }
    Ok(Regex::new(&pat)?)
}

pub async fn search_one_segment(
    input: SegmentTaskInput,
    ct: CancellationToken,
) -> anyhow::Result<SegmentTaskOutput> {
    if ct.is_cancelled() {
        anyhow::bail!("cancelled");
    }

    println!("{:?}", input.seg_path.clone());
    // 1) обязательные триграммы из wildcard (внутри есть normalize)
    let req_grams = match required_grams_from_wildcard(&input.wildcard) {
        Ok(g) => g,
        Err(e) => {
            warn!(error=?e, wildcard=%input.wildcard, "weak or invalid wildcard; skipping segment");
            return Ok(SegmentTaskOutput {
                seg_path: input.seg_path,
                hits: Vec::new(),
                last_docid: input.cursor_docid,
                candidates: 0,
            });
        }
    };

    // 2) открываем сегмент синхронно (без spawn_blocking — чтобы не возиться с 'static/Send)
    let seg_path = input.seg_path.clone();
    let field_opt = if input.field.is_empty() {
        None
    } else {
        Some(input.field.as_str())
    };

    tracing::debug!(seg=%seg_path, "about to open segment");
    let reader = match JsonSegmentReader::open_segment(&seg_path) {
        Ok(r) => r,
        Err(e) => {
            tracing::warn!(seg=%seg_path, error=?e, "failed to open segment");
            return Ok(SegmentTaskOutput {
                seg_path: input.seg_path,
                hits: vec![],
                last_docid: input.cursor_docid,
                candidates: 0,
            });
        }
    };
    tracing::debug!(seg=%seg_path, "segment opened OK");

    if ct.is_cancelled() {
        anyhow::bail!("cancelled");
    }

    // 3) префильтр по триграммам (+ маска поля)
    let bm_candidates = reader.prefilter(BooleanOp::And, &req_grams, field_opt)?;
    tracing::debug!(seg=%seg_path, grams=%req_grams.len(), candidates=%bm_candidates.cardinality(), field=?field_opt, "prefilter done");

    // 4) regex по НОРМАЛИЗОВАННОМУ wildcard (индекс хранит нормализованные строки)
    let norm_wc = normalize(&input.wildcard);
    let re = match wildcard_norm_to_regex(&norm_wc) {
        Ok(r) => r,
        Err(e) => {
            warn!(error=?e, wildcard=%input.wildcard, norm=%norm_wc, "failed to build regex; skipping segment");
            return Ok(SegmentTaskOutput {
                seg_path: input.seg_path,
                hits: Vec::new(),
                last_docid: input.cursor_docid,
                candidates: 0,
            });
        }
    };

    // 5) перебор кандидатов с уважением курсора и лимита кандидатов
    let max_candidates = input.max_candidates;
    let mut hits: Vec<serde_json::Value> = Vec::new();
    let mut candidates_seen: u64 = 0;
    let mut last_docid: Option<u64> = input.cursor_docid;

    for doc_id in bm_candidates.iter() {
        if ct.is_cancelled() {
            anyhow::bail!("cancelled");
        }

        if let Some(cur) = input.cursor_docid {
            if (doc_id as u64) <= cur {
                continue;
            }
        }

        candidates_seen += 1;
        last_docid = Some(doc_id as u64);
        if candidates_seen > max_candidates {
            break;
        }

        if let Some(doc) = reader.get_doc(doc_id) {
            let matched = if let Some(f) = field_opt {
                if let Some(val) = doc.fields.get(f) {
                    re.is_match(val)
                } else {
                    false
                }
            } else {
                // любой строковый field
                doc.fields.values().any(|v| re.is_match(v))
            };

            if matched {
                hits.push(serde_json::json!({
                    "ext_id": doc.ext_id,
                    "doc_id": doc.doc_id,
                    "matched_field": field_opt,
                }));
            }
        }
    }

    Ok(SegmentTaskOutput {
        seg_path: input.seg_path,
        hits,
        last_docid,
        candidates: candidates_seen,
    })
}
