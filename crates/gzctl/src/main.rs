use anyhow::Result;
use clap::ValueEnum;
use clap::{Parser, Subcommand};
use grepzilla_segment::gram::{BooleanOp, required_grams_from_wildcard};
use grepzilla_segment::segjson::{JsonSegmentReader, JsonSegmentWriter};
use grepzilla_segment::{SegmentReader, SegmentWriter};
use regex::Regex;
use std::collections::HashMap;
use std::path::Path;
use std::time::Instant;

use grepzilla_segment::v2::reader::BinSegmentReader;
use grepzilla_segment::v2::writer::BinSegmentWriter;

#[derive(Parser)]
#[command(
    version,
    about = "Grepzilla control: build/search SegmentV1 (JSON) + SegmentV2 (binary)"
)]
struct Cli {
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Copy, Clone, Eq, PartialEq, ValueEnum)]
enum SegFormat {
    V1,
    V2,
}

#[derive(Subcommand)]
enum Cmd {
    /// Построить сегмент из JSONL
    BuildSeg {
        #[arg(long)]
        input: String,
        #[arg(long)]
        out: String,
        #[arg(long, value_enum, default_value_t=SegFormat::V1)]
        format: SegFormat,
    },
    /// Поиск в одном или нескольких сегментах (список через запятую)
    ///
    /// Пример: --seg "segments/seg1,segments/seg2"
    SearchSeg {
        #[arg(long)]
        seg: String,
        #[arg(long)]
        q: String,
        #[arg(long)]
        field: Option<String>,
        #[arg(long, default_value_t = 10)]
        limit: usize,
        #[arg(long, default_value_t = 0)]
        offset: usize,
        /// Включить расширенные метрики (печатаются в stderr JSON-ом)
        #[arg(long, default_value_t = false)]
        debug_metrics: bool,
    },
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    match cli.cmd {
        Cmd::BuildSeg { input, out, format } => match format {
            SegFormat::V1 => {
                let mut w = JsonSegmentWriter::default();
                w.write_segment(&input, &out)?;
            }
            SegFormat::V2 => {
                let mut w = BinSegmentWriter::default();
                w.write_segment(&input, &out)?;
            }
        },
        Cmd::SearchSeg {
            seg,
            q,
            field,
            limit,
            offset,
            debug_metrics,
        } => {
            let start = Instant::now();
            // разберём список сегментов (через запятую)
            let segs: Vec<String> = seg
                .split(',')
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
                .collect();

            // заранее подготовим общий regex и grams (одни и те же для всех сегментов)
            let grams = required_grams_from_wildcard(&q)?;
            let rx = wildcard_to_regex(&q)?;

            // агрегированные метрики
            let mut total_doc_count = 0u64;
            let mut scanned_docs = 0usize;
            let mut candidates = 0usize;
            let mut verified = 0usize;
            let mut shown = 0usize;
            let mut skipped = 0usize;
            let mut by_field: HashMap<String, usize> = HashMap::new();

            // общий вывод: печатаем по мере нахождения (в порядке сегментов)
            'outer: for seg_dir in segs {
                let is_v2 = Path::new(&seg_dir).join("meta.bin").exists();
                if is_v2 {
                    // --- V2 ---
                    let reader = BinSegmentReader::open_segment(&seg_dir)?;
                    total_doc_count += reader.doc_count() as u64;

                    // префильтр
                    let bm = reader.prefilter(BooleanOp::And, &grams, field.as_deref())?;
                    // prefetch_docs: прогреем первые limit*2 doc_id (если их меньше — сколько есть)
                    let warm: Vec<u32> = bm.iter().take(limit.saturating_mul(2)).collect();
                    reader.prefetch_docs(warm.into_iter());

                    for doc_id in bm.iter() {
                        candidates += 1;
                        scanned_docs += 1;

                        // глобальный offset/limit
                        if skipped < offset {
                            skipped += 1;
                            continue;
                        }
                        if shown >= limit {
                            break 'outer;
                        }

                        if let Some(doc) = reader.get_doc(doc_id) {
                            // проверка совпадения
                            let (matched, matched_field) = match field.as_deref() {
                                Some(f) => {
                                    let ok =
                                        doc.fields.get(f).map(|t| rx.is_match(t)).unwrap_or(false);
                                    (ok, ok.then(|| f.to_string()))
                                }
                                None => {
                                    if let Some((k, _)) =
                                        doc.fields.iter().find(|(_, t)| rx.is_match(t))
                                    {
                                        (true, Some(k.clone()))
                                    } else {
                                        (false, None)
                                    }
                                }
                            };
                            if !matched {
                                continue;
                            }
                            verified += 1;

                            // превью
                            let (preview_field, text) = pick_preview_field(doc, field.as_deref());
                            let preview = build_snippet(&rx, &text, 80);

                            let stat_field = matched_field
                                .as_deref()
                                .unwrap_or_else(|| preview_field.unwrap_or("-"));
                            *by_field.entry(stat_field.to_string()).or_insert(0) += 1;

                            println!(
                                "{}\t{}\t{}: {}",
                                doc.ext_id,
                                doc_id,
                                preview_field.unwrap_or("-"),
                                preview
                            );
                            shown += 1;
                        }
                    }
                } else {
                    // --- V1 ---
                    let reader = JsonSegmentReader::open_segment(&seg_dir)?;
                    total_doc_count += reader.doc_count() as u64;

                    let bm = reader.prefilter(BooleanOp::And, &grams, field.as_deref())?;
                    for doc_id in bm.iter() {
                        candidates += 1;
                        scanned_docs += 1;

                        if skipped < offset {
                            skipped += 1;
                            continue;
                        }
                        if shown >= limit {
                            break 'outer;
                        }

                        if let Some(doc) = reader.get_doc(doc_id) {
                            // проверка совпадения
                            let (matched, matched_field) = match field.as_deref() {
                                Some(f) => {
                                    let ok =
                                        doc.fields.get(f).map(|t| rx.is_match(t)).unwrap_or(false);
                                    (ok, ok.then(|| f.to_string()))
                                }
                                None => {
                                    if let Some((k, _)) =
                                        doc.fields.iter().find(|(_, t)| rx.is_match(t))
                                    {
                                        (true, Some(k.clone()))
                                    } else {
                                        (false, None)
                                    }
                                }
                            };
                            if !matched {
                                continue;
                            }
                            verified += 1;

                            let (preview_field, text) = pick_preview_field(doc, field.as_deref());
                            let preview = build_snippet(&rx, &text, 80);

                            let stat_field = matched_field
                                .as_deref()
                                .unwrap_or_else(|| preview_field.unwrap_or("-"));
                            *by_field.entry(stat_field.to_string()).or_insert(0) += 1;

                            println!(
                                "{}\t{}\t{}: {}",
                                doc.ext_id,
                                doc_id,
                                preview_field.unwrap_or("-"),
                                preview
                            );
                            shown += 1;
                        }
                    }
                }
            }

            // метрики
            if debug_metrics {
                let elapsed_ms = start.elapsed().as_millis() as u64;
                let ratio_verified = if candidates > 0 {
                    (verified as f64) / (candidates as f64)
                } else {
                    0.0
                };
                let ratio_hits = if verified > 0 {
                    (shown as f64) / (verified as f64)
                } else {
                    0.0
                };
                let metrics = serde_json::json!({
                    "segments": seg,
                    "query": q,
                    "field_filter": field,
                    "elapsed_ms": elapsed_ms,
                    "total_doc_count": total_doc_count,
                    "scanned_docs": scanned_docs,
                    "candidates_total": candidates,
                    "verified_total": verified,
                    "hits_total": shown,
                    "offset": offset,
                    "limit": limit,
                    "ratios": {
                        "prefilter_to_verify": ratio_verified,
                        "verify_to_hits": ratio_hits,
                    },
                    "by_field": by_field,
                });
                eprintln!("{}", serde_json::to_string_pretty(&metrics)?);
            }

            if shown == 0 {
                println!("0 hits");
            }
        }
    }
    Ok(())
}

fn wildcard_to_regex(pat: &str) -> Result<Regex> {
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
    Ok(Regex::new(&rx)?)
}

/// Выбор поля для превью: если задан --field, пытаемся его; иначе text.body → text.title → любое
fn pick_preview_field<'a>(
    doc: &'a grepzilla_segment::StoredDoc,
    field_filter: Option<&'a str>,
) -> (Option<&'a str>, String) {
    if let Some(f) = field_filter {
        if let Some(t) = doc.fields.get(f) {
            return (Some(f), t.clone());
        }
    }
    if let Some(t) = doc.fields.get("text.body") {
        return (Some("text.body"), t.clone());
    }
    if let Some(t) = doc.fields.get("text.title") {
        return (Some("text.title"), t.clone());
    }
    // любое первое поле
    if let Some((k, v)) = doc.fields.iter().next() {
        return (Some(k.as_str()), v.clone());
    }
    (None, String::new())
}

/// Строит сниппет до ~window символов с подсветкой первой матч-зоны через [квадратные скобки].
/// Если матчей нет (маловероятно, т.к. уже проверяли) — вернёт усечённый текст без подсветки.
fn build_snippet(rx: &Regex, text: &str, window: usize) -> String {
    if let Some(m) = rx.find(text) {
        let start = m.start();
        let end = m.end();

        // Контекст по бокам
        let ctx = window.saturating_sub((end - start).min(window) + 2) / 2; // на «…» по краям
        let from = start.saturating_sub(ctx);
        let to = (end + ctx).min(text.len());

        let prefix_ellipsis = if from > 0 { "…" } else { "" };
        let suffix_ellipsis = if to < text.len() { "…" } else { "" };

        let mut out = String::new();
        out.push_str(prefix_ellipsis);
        out.push_str(&text[from..start]);
        out.push('[');
        out.push_str(&text[start..end]);
        out.push(']');
        out.push_str(&text[end..to]);
        out.push_str(suffix_ellipsis);
        out
    } else {
        // запасной вариант: первые window символов
        if text.len() > window {
            format!("{}…", &text[..window])
        } else {
            text.to_string()
        }
    }
}
