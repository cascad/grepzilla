use anyhow::Result;
use std::path::Path;
use std::sync::Arc;

use grepzilla_segment::common::preview::{build_preview, PreviewOpts};
use grepzilla_segment::gram::{required_grams_from_wildcard, BooleanOp};
use grepzilla_segment::segjson::JsonSegmentReader;
use grepzilla_segment::v2::reader::BinSegmentReader;
use grepzilla_segment::verify::VerifyEngine;
use grepzilla_segment::SegmentReader;

use crate::search::executor::{SegmentTaskInput, SegmentTaskOutput};
use crate::search::types::Hit;

pub async fn search_one_segment(
    mut input: SegmentTaskInput,
    eng: Arc<dyn VerifyEngine>,
    _ctok: tokio_util::sync::CancellationToken,
) -> Result<SegmentTaskOutput> {
    let nq = grepzilla_segment::normalizer::normalize(&input.wildcard);
    let grams = required_grams_from_wildcard(&nq)?;

    let mut hits: Vec<Hit> = Vec::new();
    let mut candidates: u64 = 0;

    let mut prefilter_ms = 0u64;
    let mut verify_ms = 0u64;
    let mut prefetch_ms = 0u64;
    let mut warmed_docs = 0u64;

    let is_v2 = Path::new(&input.seg_path).join("meta.bin").exists();
    if is_v2 {
        // V2
        let reader = BinSegmentReader::open_segment(&input.seg_path)?;

        let t0 = std::time::Instant::now();
        let bm = reader.prefilter(BooleanOp::And, &grams, non_empty(&input.field))?;
        prefilter_ms += t0.elapsed().as_millis() as u64;

        // прогрев
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
                // превью
                if let Some(doc) = reader.get_doc(doc_id) {
                    let preview = build_preview(
                        doc,
                        PreviewOpts {
                            preferred_fields: &["text.title", "text.body", "title", "body"],
                            max_len: 180,
                            highlight_needle: longest_literal_needle(&nq).as_deref(),
                        },
                    );
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
        // V1
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
                    let preview = build_preview(
                        doc,
                        PreviewOpts {
                            preferred_fields: &["text.title", "text.body", "title", "body"],
                            max_len: 180,
                            highlight_needle: longest_literal_needle(&nq).as_deref(),
                        },
                    );
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
            prefetch_ms,
            warmed_docs,
        })
    }
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
    // wc уже нормализован
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
