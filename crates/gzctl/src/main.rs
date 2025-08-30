use anyhow::Result;
use clap::ValueEnum;
use clap::{Parser, Subcommand};
use grepzilla_segment::gram::{BooleanOp, required_grams_from_wildcard};
use grepzilla_segment::segjson::{JsonSegmentReader, JsonSegmentWriter};
use grepzilla_segment::{SegmentReader, SegmentWriter};
use regex::Regex;
use std::collections::HashMap;
use std::time::Instant;
use grepzilla_segment::v2::writer::BinSegmentWriter;

#[derive(Parser)]
#[command(version, about = "Grepzilla control: build/search SegmentV1 (JSON)")]
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
                let mut w = grepzilla_segment::segjson::JsonSegmentWriter::default();
                w.write_segment(&input, &out)?;
            }
            SegFormat::V2 => {
                let mut w = grepzilla_segment::v2::writer::BinSegmentWriter::default();
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

            let reader = JsonSegmentReader::open_segment(&seg)?;
            let grams = required_grams_from_wildcard(&q)?;
            let bm = reader.prefilter(BooleanOp::And, &grams, field.as_deref())?;
            let rx = wildcard_to_regex(&q)?;

            let mut shown = 0usize;
            let mut skipped = 0usize;
            let mut candidates = 0usize;
            let mut verified = 0usize;
            let mut by_field: HashMap<String, usize> = HashMap::new();
            let mut scanned_docs = 0usize;

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
                    // Проверяем совпадение: либо конкретное поле, либо первое подходящее
                    let (matched, matched_field) = match field.as_deref() {
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
                    verified += 1;

                    // Выбираем текст для превью (приоритет: field -> text.body -> text.title -> любое)
                    let (preview_field, text) = pick_preview_field(doc, field.as_deref());

                    // Сниппет с подсветкой первой матч-зоны
                    let preview = build_snippet(&rx, &text, 80);

                    // Имя поля для статистики: именно где матч нашёлся, иначе превью-поле
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

            if debug_metrics {
                let elapsed_ms = start.elapsed().as_millis() as u64;
                // Дополнительные вычисляемые метрики
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
                    "query": q,
                    "field_filter": field,
                    "elapsed_ms": elapsed_ms,
                    "doc_count": reader.doc_count(),
                    "scanned_docs": scanned_docs,     // сколько doc_id прошли через цикл (по bitmap)
                    "candidates_total": candidates,   // кандидаты префильтра (равно scanned_docs в данном цикле)
                    "verified_total": verified,       // прошли regex
                    "hits_total": shown,              // выданы пользователю (limit/offset учтены)
                    "ratios": {
                        "prefilter_to_verify": ratio_verified,
                        "verify_to_hits": ratio_hits,
                    },
                    "by_field": by_field,             // распределение по полям, где зафиксирован матч
                });
                eprintln!("{}", serde_json::to_string_pretty(&metrics)?);
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
