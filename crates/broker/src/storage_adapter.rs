use anyhow::Result;
use std::path::Path;
use std::sync::Arc;
use std::time::Instant;

use grepzilla_segment::common::preview::{snippet_with_highlight, truncate_chars_with_ellipsis};
use grepzilla_segment::gram::{required_grams_from_wildcard, BooleanOp};
use grepzilla_segment::segjson::JsonSegmentReader;
use grepzilla_segment::v2::reader::BinSegmentReader;
use grepzilla_segment::SegmentReader;
use grepzilla_segment::{normalizer, verify::VerifyEngine};

use crate::search::executor::{SegmentTaskInput, SegmentTaskOutput};
use crate::search::types::Hit;

pub async fn search_one_segment(
    input: SegmentTaskInput,
    eng: Arc<dyn VerifyEngine>,
    _ctok: tokio_util::sync::CancellationToken,
) -> Result<SegmentTaskOutput> {
    // нормализуем wildcard только для извлечения обязательных 3-грамм
    let nq = normalizer::normalize(&input.wildcard);
    let grams = required_grams_from_wildcard(&nq)?;

    let mut hits: Vec<Hit> = Vec::new();
    let mut candidates: u64 = 0;

    // метрики
    let mut prefilter_ms: u64 = 0;
    let mut verify_ms: u64 = 0;
    let mut prefetch_ms: u64 = 0;
    let mut warmed_docs: u64 = 0;

    let is_v2 = Path::new(&input.seg_path).join("meta.bin").exists();
    if is_v2 {
        // -------- V2 ----------
        let reader = BinSegmentReader::open_segment(&input.seg_path)?;

        let t0 = Instant::now();
        let bm = reader.prefilter(BooleanOp::And, &grams, non_empty(&input.field))?;
        prefilter_ms += t0.elapsed().as_millis() as u64;

        // прогрев OnceCell — ориентируемся на page_size
        let warm_cap = (input.page_size.saturating_mul(4)).min(5_000);
        let warm_vec: Vec<u32> = bm
            .iter()
            .skip(skip_from_cursor(input.cursor_docid))
            .take(warm_cap)
            .collect();
        let tp0 = Instant::now();
        warmed_docs = warm_vec.len() as u64;
        reader.prefetch_docs(warm_vec.into_iter());
        prefetch_ms += tp0.elapsed().as_millis() as u64;

        for doc_id in bm.iter() {
            // пагинация: пропускаем doc_id <= cursor
            if let Some(cur) = input.cursor_docid {
                if (doc_id as u64) <= cur {
                    continue;
                }
            }
            candidates += 1;
            if candidates > input.max_candidates {
                break;
            }

            // выясняем поле-матч
            let tv0 = Instant::now();
            let matched_field = match non_empty(&input.field) {
                Some(f) => {
                    if let Some(doc) = reader.get_doc(doc_id) {
                        let ok = doc.fields.get(f).map(|t| eng.is_match(t)).unwrap_or(false);
                        ok.then(|| f.to_string())
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
                    // превью строго из matched_field с подсветкой
                    let preview = if let Some(txt) = doc.fields.get(&mf) {
                        if let Some((s, e)) = eng.find(txt) {
                            snippet_with_highlight(txt, s, e, 180)
                        } else {
                            truncate_chars_with_ellipsis(txt, 180)
                        }
                    } else {
                        truncate_chars_with_ellipsis(
                            doc.fields.values().next().map(|s| s.as_str()).unwrap_or(""),
                            180,
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

        Ok(SegmentTaskOutput {
            seg_path: input.seg_path,
            last_docid: hits.last().map(|h| h.doc_id as u64),
            candidates,
            hits,
            prefilter_ms,
            verify_ms,
            prefetch_ms,
            warmed_docs,
        })
    } else {
        // -------- V1 ----------
        let reader = JsonSegmentReader::open_segment(&input.seg_path)?;

        let t0 = Instant::now();
        let bm = reader.prefilter(BooleanOp::And, &grams, non_empty(&input.field))?;
        prefilter_ms += t0.elapsed().as_millis() as u64;

        for doc_id in bm.iter() {
            if let Some(cur) = input.cursor_docid {
                if (doc_id as u64) <= cur {
                    continue;
                }
            }
            candidates += 1;
            if candidates > input.max_candidates {
                break;
            }

            let tv0 = Instant::now();
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
                    let preview = if let Some(ref mf) = matched_field {
                        if let Some(txt) = doc.fields.get(mf) {
                            if let Some((s, e)) = eng.find(txt) {
                                snippet_with_highlight(txt, s, e, 180)
                            } else {
                                truncate_chars_with_ellipsis(txt, 180)
                            }
                        } else {
                            truncate_chars_with_ellipsis(
                                doc.fields.values().next().map(|s| s.as_str()).unwrap_or(""),
                                180,
                            )
                        }
                    } else {
                        truncate_chars_with_ellipsis(
                            doc.fields.values().next().map(|s| s.as_str()).unwrap_or(""),
                            180,
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

        Ok(SegmentTaskOutput {
            seg_path: input.seg_path,
            last_docid: hits.last().map(|h| h.doc_id as u64),
            candidates,
            hits,
            prefilter_ms,
            verify_ms,
            prefetch_ms, // 0 для V1
            warmed_docs, // 0 для V1
        })
    }
}

#[inline]
fn non_empty(s: &str) -> Option<&str> {
    if s.is_empty() { None } else { Some(s) }
}

#[inline]
fn skip_from_cursor(cur: Option<u64>) -> usize {
    match cur {
        Some(v) if v > 0 => v as usize,
        _ => 0,
    }
}
