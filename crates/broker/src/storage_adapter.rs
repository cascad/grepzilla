// crates/broker/src/storage_adapter.rs

use anyhow::Result;
use std::path::Path;

use grepzilla_segment::common::preview::{build_preview, PreviewOpts};
use grepzilla_segment::gram::{required_grams_from_wildcard, BooleanOp};
use grepzilla_segment::segjson::JsonSegmentReader;
use grepzilla_segment::v2::reader::BinSegmentReader;
use grepzilla_segment::SegmentReader;

use crate::search::executor::{SegmentTaskInput, SegmentTaskOutput};
use crate::search::types::Hit;

pub async fn search_one_segment(
    input: SegmentTaskInput,
    _ctok: tokio_util::sync::CancellationToken,
) -> Result<SegmentTaskOutput> {
    // normalize wildcard (StoredDoc.fields уже нормализованы)
    let nq = grepzilla_segment::normalizer::normalize(&input.wildcard);
    let grams = required_grams_from_wildcard(&nq)?;
    // движок верификации берём из input (Arc<dyn VerifyEngine>)
    let eng = input.verify_engine.clone();

    let mut hits: Vec<Hit> = Vec::new();
    let mut candidates: u64 = 0;

    // метрики сегмента
    let mut prefilter_ms = 0u64;
    let mut verify_ms = 0u64;
    let mut prefetch_ms = 0u64;
    let mut warmed_docs = 0u64;

    // Будем отслеживать последний реально просмотренный doc_id
    let mut last_scanned: Option<u32> = None;

    let is_v2 = Path::new(&input.seg_path).join("meta.bin").exists();
    if is_v2 {
        // -------- V2 ----------
        let reader = BinSegmentReader::open_segment(&input.seg_path)?;

        let t0 = std::time::Instant::now();
        let bm = reader.prefilter(BooleanOp::And, &grams, non_empty(&input.field))?;
        prefilter_ms += t0.elapsed().as_millis() as u64;

        // прогрев OnceCell: page_size * 4 (cap 5000) после курсора
        let warm_cap = (input.page_size.saturating_mul(4)).min(5_000);
        let warm_vec: Vec<u32> = bm
            .iter()
            .skip(skip_from_cursor(input.cursor_docid))
            .take(warm_cap)
            .collect();
        let tpf0 = std::time::Instant::now();
        warmed_docs = warm_vec.len() as u64;
        reader.prefetch_docs(warm_vec.into_iter());
        prefetch_ms += tpf0.elapsed().as_millis() as u64;

        for doc_id in bm.iter() {
            if let Some(cur) = input.cursor_docid {
                if (doc_id as u64) <= cur {
                    continue;
                }
            }

            // отметим, что этот doc действительно просмотрен
            last_scanned = Some(doc_id);

            candidates += 1;
            if candidates > input.max_candidates {
                break;
            }

            let tv0 = std::time::Instant::now();
            let matched_field = match non_empty(&input.field) {
                Some(f) => {
                    if let Some(doc) = reader.get_doc(doc_id) {
                        let ok = doc.fields.get(f).map(|t| eng.is_match(t)).unwrap_or(false);
                        if ok {
                            Some(f.to_string())
                        } else {
                            None
                        }
                    } else {
                        None
                    }
                }
                None => {
                    if let Some(doc) = reader.get_doc(doc_id) {
                        doc.fields
                            .iter()
                            .find(|(_, t)| eng.is_match(t))
                            .map(|(k, _)| k.clone())
                    } else {
                        None
                    }
                }
            };
            verify_ms += tv0.elapsed().as_millis() as u64;

            if let Some(mf) = matched_field {
                if let Some(doc) = reader.get_doc(doc_id) {
                    // Пытаемся взять точный матч-спан для превью из verify-движка
                    let preview = if let Some(txt) = doc.fields.get(&mf) {
                        if let Some((s, e)) = eng.find(txt) {
                            snippet_with_highlight_utf8(txt, s, e, 180)
                        } else {
                            // fallback — старый универсальный билд
                            build_preview(
                                doc,
                                PreviewOpts {
                                    preferred_fields: &["text.title", "text.body", "title", "body"],
                                    max_len: 180,
                                    highlight_needle: longest_literal_needle(&nq).as_deref(),
                                },
                            )
                        }
                    } else {
                        build_preview(
                            doc,
                            PreviewOpts {
                                preferred_fields: &["text.title", "text.body", "title", "body"],
                                max_len: 180,
                                highlight_needle: longest_literal_needle(&nq).as_deref(),
                            },
                        )
                    };

                    hits.push(Hit {
                        ext_id: doc.ext_id.clone(),
                        doc_id: doc.doc_id,
                        matched_field: mf,
                        preview,
                    });
                    if hits.len() >= 1024 {
                        break;
                    }
                }
            }
        }
    } else {
        // -------- V1 ----------
        let reader = JsonSegmentReader::open_segment(&input.seg_path)?;

        let t0 = std::time::Instant::now();
        let bm = reader.prefilter(BooleanOp::And, &grams, non_empty(&input.field))?;
        prefilter_ms += t0.elapsed().as_millis() as u64;

        for doc_id in bm.iter() {
            if let Some(cur) = input.cursor_docid {
                if (doc_id as u64) <= cur {
                    continue;
                }
            }

            // отметим просмотренный doc
            last_scanned = Some(doc_id);

            candidates += 1;
            if candidates > input.max_candidates {
                break;
            }

            let tv0 = std::time::Instant::now();
            let (matched, matched_field) = match non_empty(&input.field) {
                Some(f) => {
                    if let Some(doc) = reader.get_doc(doc_id) {
                        let ok = doc.fields.get(f).map(|t| eng.is_match(t)).unwrap_or(false);
                        (ok, ok.then(|| f.to_string()))
                    } else {
                        (false, None)
                    }
                }
                None => {
                    if let Some(doc) = reader.get_doc(doc_id) {
                        if let Some((k, _)) = doc.fields.iter().find(|(_, t)| eng.is_match(t)) {
                            (true, Some(k.clone()))
                        } else {
                            (false, None)
                        }
                    } else {
                        (false, None)
                    }
                }
            };
            verify_ms += tv0.elapsed().as_millis() as u64;

            if matched {
                if let Some(doc) = reader.get_doc(doc_id) {
                    // Аналогичный путь: пытаемся из matched_field взять точный матч
                    let preview = if let Some(mf) = matched_field.as_ref() {
                        if let Some(txt) = doc.fields.get(mf) {
                            if let Some((s, e)) = eng.find(txt) {
                                snippet_with_highlight_utf8(txt, s, e, 180)
                            } else {
                                build_preview(
                                    doc,
                                    PreviewOpts {
                                        preferred_fields: &[
                                            "text.title",
                                            "text.body",
                                            "title",
                                            "body",
                                        ],
                                        max_len: 180,
                                        highlight_needle: longest_literal_needle(&nq).as_deref(),
                                    },
                                )
                            }
                        } else {
                            build_preview(
                                doc,
                                PreviewOpts {
                                    preferred_fields: &["text.title", "text.body", "title", "body"],
                                    max_len: 180,
                                    highlight_needle: longest_literal_needle(&nq).as_deref(),
                                },
                            )
                        }
                    } else {
                        build_preview(
                            doc,
                            PreviewOpts {
                                preferred_fields: &["text.title", "text.body", "title", "body"],
                                max_len: 180,
                                highlight_needle: longest_literal_needle(&nq).as_deref(),
                            },
                        )
                    };

                    hits.push(Hit {
                        ext_id: doc.ext_id.clone(),
                        doc_id: doc.doc_id,
                        matched_field: matched_field.unwrap_or_default(),
                        preview,
                    });
                    if hits.len() >= 1024 {
                        break;
                    }
                }
            }
        }
    }

    // Если что-то просмотрели — last_scanned; иначе вернём исходный курсор (или 0)
    let final_last = last_scanned
        .map(|d| d as u64)
        .or(input.cursor_docid)
        .unwrap_or(0);

    Ok(SegmentTaskOutput {
        seg_path: input.seg_path,
        last_docid: Some(final_last),
        candidates,
        hits,
        prefilter_ms,
        verify_ms,
        prefetch_ms,
        warmed_docs,
    })
}

#[inline]
fn non_empty(s: &str) -> Option<&str> {
    if s.is_empty() {
        None
    } else {
        Some(s)
    }
}

#[inline]
fn skip_from_cursor(cur: Option<u64>) -> usize {
    match cur {
        Some(v) if v > 0 => v as usize,
        _ => 0,
    }
}

#[inline]
fn longest_literal_needle(wc: &str) -> Option<String> {
    let mut best = String::new();
    let mut cur = String::new();
    for ch in wc.chars() {
        match ch {
            '*' | '?' => {
                if cur.len() > best.len() {
                    best = std::mem::take(&mut cur);
                } else {
                    cur.clear();
                }
            }
            _ => cur.push(ch),
        }
    }
    if cur.len() > best.len() {
        best = cur;
    }
    if best.is_empty() {
        None
    } else {
        Some(best)
    }
}

/// Построить сниппет по байтовым смещениям матча с корректными границами UTF-8.
/// Всегда вставляет `[` и `]` вокруг матча; добавляет `…` если фрагмент усечён.
fn snippet_with_highlight_utf8(
    s: &str,
    m_start_b: usize,
    m_end_b: usize,
    max_chars: usize,
) -> String {
    if max_chars == 0 || s.is_empty() {
        return String::new();
    }

    let (byte_of_char, total_chars) = index_chars(s);

    // Переводим байтовые индексы в индексы по символам
    let m_start_c = byte_to_char_idx(&byte_of_char, m_start_b);
    let m_end_c = byte_to_char_idx(&byte_of_char, m_end_b);
    let match_len_c = m_end_c.saturating_sub(m_start_c);

    let budget = max_chars.saturating_sub(match_len_c + 2); // +2 на скобки
    let ctx = budget / 2;

    let from_c = m_start_c.saturating_sub(ctx);
    let to_c = (m_end_c + ctx).min(total_chars);

    let from_b = byte_of_char[from_c];
    let to_b = byte_of_char[to_c];

    let mut out = String::new();
    if from_c > 0 {
        out.push('…');
    }
    out.push_str(&s[from_b..m_start_b]);
    out.push('[');
    out.push_str(&s[m_start_b..m_end_b]);
    out.push(']');
    out.push_str(&s[m_end_b..to_b]);
    if to_c < total_chars {
        out.push('…');
    }

    ensure_max_chars(&out, max_chars + 4)
}

/// Таблица «индекс символа → байтовое смещение», с завершающим s.len().
fn index_chars(s: &str) -> (Vec<usize>, usize) {
    let mut byte_of_char = Vec::with_capacity(s.len() + 1);
    for (b, _) in s.char_indices() {
        byte_of_char.push(b);
    }
    byte_of_char.push(s.len());
    let total_chars = byte_of_char.len() - 1;
    (byte_of_char, total_chars)
}

/// Перевод байтового индекса в индекс символа (ближайшая левая граница).
fn byte_to_char_idx(byte_of_char: &[usize], byte_idx: usize) -> usize {
    match byte_of_char.binary_search(&byte_idx) {
        Ok(i) => i,
        Err(pos) => pos.saturating_sub(1),
    }
}

/// Жёсткая страховка по длине в символах (без паник).
fn ensure_max_chars(s: &str, max_chars: usize) -> String {
    let mut it = s.chars();
    let mut out = String::new();
    for _ in 0..max_chars {
        if let Some(ch) = it.next() {
            out.push(ch);
        } else {
            return out;
        }
    }
    if it.next().is_some() {
        out.push('…');
    }
    out
}
