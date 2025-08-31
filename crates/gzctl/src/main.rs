// crates/gzctl/src/main.rs
use anyhow::Result;
use clap::ValueEnum;
use clap::{Parser, Subcommand};
use std::collections::HashMap;
use std::path::Path;
use std::time::Instant;

use grepzilla_segment::common::preview::{PreviewOpts, build_preview};
use grepzilla_segment::gram::{BooleanOp, required_grams_from_wildcard};
use grepzilla_segment::segjson::{JsonSegmentReader, JsonSegmentWriter};
use grepzilla_segment::v2::reader::BinSegmentReader;
use grepzilla_segment::v2::writer::BinSegmentWriter;
use grepzilla_segment::{SegmentReader, SegmentWriter};

use grepzilla_segment::normalizer::normalize;
use grepzilla_segment::verify::{EnvVerifyFactory, VerifyEngine, VerifyFactory};

#[derive(Parser)]
#[command(
    version,
    about = "Grepzilla control: build/search SegmentV1 (JSON) or V2 (bin)"
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
    /// Поиск в одном сегменте (wildcard-паттерн)
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
            search_one_segment_cli(&seg, &q, field.as_deref(), limit, offset, debug_metrics)?;
        }
    }
    Ok(())
}

fn search_one_segment_cli(
    seg: &str,
    wildcard: &str,
    field: Option<&str>,
    limit: usize,
    offset: usize,
    debug_metrics: bool,
) -> Result<()> {
    let start = Instant::now();

    // 1) нормализуем wildcard и извлекаем обязательные триграммы
    let norm_wc = normalize(wildcard);
    let grams = required_grams_from_wildcard(&norm_wc)?;

    // 2) компилируем VerifyEngine один раз
    let eng = EnvVerifyFactory::from_env().compile(&norm_wc)?;

    // 3) autodetect V2/V1
    let is_v2 = Path::new(seg).join("meta.bin").exists();

    let mut shown = 0usize;
    let mut skipped = 0usize;
    let mut candidates = 0usize;
    let mut verified = 0usize;
    let mut by_field: HashMap<String, usize> = HashMap::new();
    let mut scanned_docs = 0usize;

    if is_v2 {
        // -------- V2 ----------
        let reader = BinSegmentReader::open_segment(seg)?;
        let bm = reader.prefilter(BooleanOp::And, &grams, field)?;

        // прогрев документов для сниппетов
        let prefetch_cap = (limit.saturating_mul(4)).min(5_000);
        let warm: Vec<u32> = bm.iter().take(prefetch_cap).collect();
        reader.prefetch_docs(warm.into_iter());

        for doc_id in bm.iter() {
            candidates += 1;
            scanned_docs += 1;

            if skipped < offset {
                skipped += 1;
                continue;
            }
            if shown >= limit {
                break;
            }

            if let Some(doc) = reader.get_doc(doc_id) {
                // verify: либо конкретное поле, либо первое совпавшее
                let (matched, matched_field) = match field {
                    Some(f) => {
                        let ok = doc.fields.get(f).map(|t| eng.is_match(t)).unwrap_or(false);
                        (ok, ok.then(|| f.to_string()))
                    }
                    None => {
                        if let Some((k, _)) = doc.fields.iter().find(|(_, t)| eng.is_match(t)) {
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

                // превью — общий helper
                let preview = build_preview(
                    doc,
                    PreviewOpts {
                        preferred_fields: &["text.title", "text.body", "title", "body"],
                        max_len: 180,
                        highlight_needle: longest_literal_needle(&norm_wc).as_deref(),
                    },
                );

                let stat_field = matched_field
                    .as_deref()
                    .unwrap_or_else(|| pick_preview_field_name(doc, field).unwrap_or("-"));
                *by_field.entry(stat_field.to_string()).or_insert(0) += 1;

                println!(
                    "{}\t{}\t{}: {}",
                    doc.ext_id,
                    doc_id,
                    pick_preview_field_name(doc, field).unwrap_or("-"),
                    preview
                );
                shown += 1;
            }
        }
    } else {
        // -------- V1 ----------
        let reader = JsonSegmentReader::open_segment(seg)?;
        let bm = reader.prefilter(BooleanOp::And, &grams, field)?;

        for doc_id in bm.iter() {
            candidates += 1;
            scanned_docs += 1;

            if skipped < offset {
                skipped += 1;
                continue;
            }
            if shown >= limit {
                break;
            }

            if let Some(doc) = reader.get_doc(doc_id) {
                // verify
                let (matched, matched_field) = match field {
                    Some(f) => {
                        let ok = doc.fields.get(f).map(|t| eng.is_match(t)).unwrap_or(false);
                        (ok, ok.then(|| f.to_string()))
                    }
                    None => {
                        if let Some((k, _)) = doc.fields.iter().find(|(_, t)| eng.is_match(t)) {
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

                let preview = build_preview(
                    doc,
                    PreviewOpts {
                        preferred_fields: &["text.title", "text.body", "title", "body"],
                        max_len: 180,
                        highlight_needle: longest_literal_needle(&norm_wc).as_deref(),
                    },
                );

                let stat_field = matched_field
                    .as_deref()
                    .unwrap_or_else(|| pick_preview_field_name(doc, field).unwrap_or("-"));
                *by_field.entry(stat_field.to_string()).or_insert(0) += 1;

                println!(
                    "{}\t{}\t{}: {}",
                    doc.ext_id,
                    doc_id,
                    pick_preview_field_name(doc, field).unwrap_or("-"),
                    preview
                );
                shown += 1;
            }
        }
    }

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
            "segment_path": seg,
            "query": wildcard,
            "field_filter": field,
            "elapsed_ms": elapsed_ms,
            "doc_count": if is_v2 {
                // быстрая оценка: не тянем doc_count из V2 повторно
                null::<u32>()
            } else {
                null::<u32>()
            },
            "scanned_docs": scanned_docs,
            "candidates_total": candidates,
            "verified_total": verified,
            "hits_total": shown,
            "ratios": {
                "prefilter_to_verify": ratio_verified,
                "verify_to_hits": ratio_hits,
            },
            "by_field": by_field,
        });
        eprintln!("{}", serde_json::to_string_pretty(&metrics)?);
    }

    Ok(())
}

// утилита: выбрать имя поля для превью — логика та же, что и в storage_adapter::build_preview
fn pick_preview_field_name<'a>(
    doc: &'a grepzilla_segment::StoredDoc,
    field_filter: Option<&'a str>,
) -> Option<&'a str> {
    if let Some(f) = field_filter {
        if doc.fields.contains_key(f) {
            return Some(f);
        }
    }
    for f in ["text.body", "text.title", "body", "title"] {
        if doc.fields.contains_key(f) {
            return Some(f);
        }
    }
    doc.fields.keys().next().map(|s| s.as_str())
}

// крошечный хак, чтобы легко положить null с типом
fn null<T>() -> Option<T> {
    None
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
    if best.is_empty() { None } else { Some(best) }
}
