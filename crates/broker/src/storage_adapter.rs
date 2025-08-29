use tokio_util::sync::CancellationToken;
use regex::Regex;
use tracing::{debug, warn};

use grepzilla_segment::SegmentReader;
use grepzilla_segment::gram::{required_grams_from_wildcard, BooleanOp};
use grepzilla_segment::normalizer::normalize;
use grepzilla_segment::segjson::JsonSegmentReader;

use crate::search::executor::{SegmentTaskInput, SegmentTaskOutput};
use crate::search::types::Hit;

/// Экранируем regex-метасимволы, оставляя * и ? (wildcard обрабатываем ниже).
fn escape_regex_meta_keep_wildcards(s: &str) -> String {
    let mut out = String::with_capacity(s.len() * 2);
    for ch in s.chars() {
        match ch {
            '.' | '+' | '(' | ')' | '|' | '{' | '}' | '[' | ']' | '^' | '$' | '\\' => {
                out.push('\\');
                out.push(ch);
            }
            '*' | '?' => out.push(ch),
            _ => out.push(ch),
        }
    }
    out
}

/// Конвертация wildcard → regex над уже нормализованной строкой.
/// "*" -> ".*", "?" -> ".", без якорей (подстрочный матч).
fn wildcard_norm_to_regex(norm_wildcard: &str) -> anyhow::Result<Regex> {
    let escaped = escape_regex_meta_keep_wildcards(norm_wildcard);
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

    // 1) Обязательные триграммы из wildcard (внутри есть normalize)
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

    // 2) Открываем сегмент
    let seg_path = input.seg_path.clone();
    let field_opt: Option<&str> = if input.field.is_empty() { None } else { Some(input.field.as_str()) };

    debug!(seg=%seg_path, "about to open segment");
    let reader = match JsonSegmentReader::open_segment(&seg_path) {
        Ok(r) => r,
        Err(e) => {
            warn!(seg=%seg_path, error=?e, "failed to open segment");
            return Ok(SegmentTaskOutput {
                seg_path: input.seg_path,
                hits: vec![],
                last_docid: input.cursor_docid,
                candidates: 0,
            });
        }
    };
    debug!(seg=%seg_path, "segment opened OK");

    if ct.is_cancelled() {
        anyhow::bail!("cancelled");
    }

    // 3) Префильтр по триграммам (+ маска поля)
    let bm_candidates = reader.prefilter(BooleanOp::And, &req_grams, field_opt)?;
    debug!(
        seg=%seg_path,
        grams=%req_grams.len(),
        candidates=%bm_candidates.cardinality(),
        field=?field_opt,
        "prefilter done"
    );

    // 4) Regex по НОРМАЛИЗОВАННОМУ wildcard (индекс хранит нормализованные строки)
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

    // 5) Перебор кандидатов c учётом курсора и лимита
    let max_candidates = input.max_candidates;
    let mut hits: Vec<Hit> = Vec::new();
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
            // matched? и какое имя поля считать matched_field
            let (is_match, matched_field_name): (bool, String) = match field_opt {
                Some(f) => {
                    let ok = doc.fields.get(f).map(|t| re.is_match(t)).unwrap_or(false);
                    (ok, f.to_string())
                }
                None => {
                    // ищем по всем строковым полям и берём первое совпавшее имя
                    let mut found: Option<String> = None;
                    for (name, text) in &doc.fields {
                        if re.is_match(text) {
                            found = Some(name.clone());
                            break;
                        }
                    }
                    (found.is_some(), found.unwrap_or_default())
                }
            };

            if !is_match {
                continue;
            }

            // (сниппет можем строить на будущее — пока в Hit его нет)
            // let snippet_src = doc.fields.get(&matched_field_name).cloned()
            //     .or_else(|| doc.fields.get("text.body").cloned())
            //     .or_else(|| doc.fields.get("text.title").cloned())
            //     .unwrap_or_default();
            // let _preview = build_snippet(&re, &snippet_src, 80);

            hits.push(Hit {
                ext_id: doc.ext_id.clone(),
                doc_id,
                matched_field: matched_field_name,
            });
        }
    }

    Ok(SegmentTaskOutput {
        seg_path: input.seg_path,
        hits,
        last_docid,
        candidates: candidates_seen,
    })
}

// приватный хелпер (на будущее: если будет поле preview)
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
