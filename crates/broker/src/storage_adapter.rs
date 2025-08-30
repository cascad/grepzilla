use anyhow::Result;
use regex::Regex;
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
    // 1) нормализуем wildcard (StoredDoc.fields уже нормализованы)
    let nq = grepzilla_segment::normalizer::normalize(&input.wildcard);
    let grams = required_grams_from_wildcard(&nq)?;
    let rx = wildcard_to_regex_case_insensitive(&nq)?;

    let mut hits: Vec<Hit> = Vec::new();
    let mut candidates: u64 = 0;

    let is_v2 = Path::new(&input.seg_path).join("meta.bin").exists();
    if is_v2 {
        // -------- V2 ----------
        let reader = BinSegmentReader::open_segment(&input.seg_path)?;
        let bm = reader.prefilter(BooleanOp::And, &grams, non_empty(&input.field))?;

        // old -> .take(2_000), прогреть OnceCell по первым ~2k id после курсора (для быстрого TTFH)
        // new -> prefetch = page_size * 4 (cap 5000)
        let warm_cap = (input.page_size.saturating_mul(4)).min(5_000);
        let warm: Vec<u32> = bm
            .iter()
            .skip(skip_from_cursor(input.cursor_docid))
            .take(warm_cap)
            .collect();
        reader.prefetch_docs(warm.into_iter());

        for doc_id in bm.iter() {
            // пропускаем только если курсор есть и doc_id <= last_docid
            if let Some(cur) = input.cursor_docid {
                if (doc_id as u64) <= cur {
                    continue;
                }
            }
            candidates += 1;
            if candidates > input.max_candidates {
                break;
            }

            if let Some(doc) = reader.get_doc(doc_id) {
                // verify: либо заданное поле, либо первое совпавшее
                let (matched, matched_field) = match non_empty(&input.field) {
                    Some(f) => {
                        let ok = doc.fields.get(f).map(|t| rx.is_match(t)).unwrap_or(false);
                        (ok, ok.then(|| f.to_string()))
                    }
                    None => {
                        if let Some((k, _)) = doc.fields.iter().find(|(_, t)| rx.is_match(t)) {
                            (true, Some(k.clone()))
                        } else {
                            (false, None)
                        }
                    }
                };
                if !matched {
                    continue;
                }

                // NEW: превью — сперва из matched_field с подсветкой regex’ом;
                // если вдруг поля нет (теоретически), fallback на общий build_preview.
                let preview = if let Some(ref mf) = matched_field {
                    if let Some(txt) = doc.fields.get(mf) {
                        build_snippet(&rx, txt, 180)
                    } else {
                        build_preview(
                            doc,
                            PreviewOpts {
                                preferred_fields: &["text.title", "text.body", "title", "body"],
                                max_len: 180,
                                highlight_needle: grams.first().map(|s| s.as_str()),
                            },
                        )
                    }
                } else {
                    build_preview(
                        doc,
                        PreviewOpts {
                            preferred_fields: &["text.title", "text.body", "title", "body"],
                            max_len: 180,
                            highlight_needle: grams.first().map(|s| s.as_str()),
                        },
                    )
                };

                hits.push(Hit {
                    ext_id: doc.ext_id.clone(),
                    doc_id: doc.doc_id,
                    matched_field: matched_field.clone().unwrap_or_default(),
                    preview,
                });

                if hits.len() >= 1024 {
                    break;
                } // локальный батч для пагинатора
            }
        }

        Ok(SegmentTaskOutput {
            seg_path: input.seg_path,
            last_docid: hits.last().map(|h| h.doc_id as u64),
            candidates,
            hits,
        })
    } else {
        // -------- V1 ----------
        let reader = JsonSegmentReader::open_segment(&input.seg_path)?;
        let bm = reader.prefilter(BooleanOp::And, &grams, non_empty(&input.field))?;

        for doc_id in bm.iter() {
            // пропускаем только если курсор есть и doc_id <= last_docid
            if let Some(cur) = input.cursor_docid {
                if (doc_id as u64) <= cur {
                    continue;
                }
            }
            candidates += 1;
            if candidates > input.max_candidates {
                break;
            }

            if let Some(doc) = reader.get_doc(doc_id) {
                let (matched, matched_field) = match non_empty(&input.field) {
                    Some(f) => {
                        let ok = doc.fields.get(f).map(|t| rx.is_match(t)).unwrap_or(false);
                        (ok, ok.then(|| f.to_string()))
                    }
                    None => {
                        if let Some((k, _)) = doc.fields.iter().find(|(_, t)| rx.is_match(t)) {
                            (true, Some(k.clone()))
                        } else {
                            (false, None)
                        }
                    }
                };
                if !matched {
                    continue;
                }

                // NEW: тот же подход к превью
                let preview = if let Some(ref mf) = matched_field {
                    if let Some(txt) = doc.fields.get(mf) {
                        build_snippet(&rx, txt, 180)
                    } else {
                        build_preview(
                            doc,
                            PreviewOpts {
                                preferred_fields: &["text.title", "text.body", "title", "body"],
                                max_len: 180,
                                highlight_needle: grams.first().map(|s| s.as_str()),
                            },
                        )
                    }
                } else {
                    build_preview(
                        doc,
                        PreviewOpts {
                            preferred_fields: &["text.title", "text.body", "title", "body"],
                            max_len: 180,
                            highlight_needle: grams.first().map(|s| s.as_str()),
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

        Ok(SegmentTaskOutput {
            seg_path: input.seg_path,
            last_docid: hits.last().map(|h| h.doc_id as u64),
            candidates,
            hits,
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

/// Строит сниппет вокруг первого regex-совпадения в `text`,
/// с квадратными скобками для подсветки. Если совпадения нет — усечение.
fn build_snippet(rx: &Regex, text: &str, window: usize) -> String {
    if let Some(m) = rx.find(text) {
        let start = m.start();
        let end = m.end();
        let ctx = window.saturating_sub(end - start + 2) / 2; // «…» по краям
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
    } else {
        if text.len() > window {
            format!("{}…", &text[..window])
        } else {
            text.to_string()
        }
    }
}

fn wildcard_to_regex_case_insensitive(pat: &str) -> anyhow::Result<Regex> {
    // (?si): dotall + case-insensitive
    let mut rx = String::from("(?si)");
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
    Ok(Regex::new(&rx)?)
}
