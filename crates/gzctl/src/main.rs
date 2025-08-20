// Файл: crates/gzctl/src/main.rs
use anyhow::Result;
use clap::{Parser, Subcommand};
use grepzilla_segment::gram::{BooleanOp, required_grams_from_wildcard};
use grepzilla_segment::segjson::{JsonSegmentReader, JsonSegmentWriter};
use grepzilla_segment::{SegmentReader, SegmentWriter};
use regex::Regex;

#[derive(Parser)]
#[command(version, about = "Grepzilla control: build/search SegmentV1 (JSON)")]
struct Cli {
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// Построить сегмент из JSONL
    BuildSeg {
        #[arg(long)]
        input: String,
        #[arg(long)]
        out: String,
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
        #[arg(long, default_value_t = false)]
        debug_metrics: bool,
    },
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    match cli.cmd {
        Cmd::BuildSeg { input, out } => {
            let mut w = JsonSegmentWriter::default();
            w.write_segment(&input, &out)?;
        }
        Cmd::SearchSeg {
            seg,
            q,
            field,
            limit,
            offset,
            debug_metrics,
        } => {
            let reader = JsonSegmentReader::open_segment(&seg)?;
            let grams = required_grams_from_wildcard(&q)?;
            let bm = reader.prefilter(BooleanOp::And, &grams, field.as_deref())?;
            let rx = wildcard_to_regex(&q)?;

            let mut shown = 0usize;
            let mut skipped = 0usize;
            let mut candidates = 0usize;
            let mut verified = 0usize;

            for doc_id in bm.iter() {
                candidates += 1;
                if skipped < offset {
                    skipped += 1;
                    continue;
                }
                if shown >= limit {
                    break;
                }

                if let Some(doc) = reader.get_doc(doc_id) {
                    // Выбираем текст для превью: сначала text.body, затем text.title, иначе любое первое строковое поле
                    let (field_name, text) = pick_preview_field(doc, field.as_deref());

                    // Проверяем совпадение в нужном поле (если field задан) иначе в любом
                    let matched = match field.as_deref() {
                        Some(f) => doc.fields.get(f).map(|t| rx.is_match(t)).unwrap_or(false),
                        None => doc.fields.values().any(|t| rx.is_match(t)),
                    };
                    if !matched {
                        continue;
                    }
                    verified += 1;

                    // Строим сниппет с подсветкой первой найденной области
                    let preview = build_snippet(&rx, &text, 80);

                    println!(
                        "{}\t{}\t{}: {}",
                        doc.ext_id,
                        doc_id,
                        field_name.unwrap_or("-"),
                        preview
                    );
                    shown += 1;
                }
            }
            if debug_metrics {
                eprintln!(
                    "candidates_total={} verified_total={} hits_total={}",
                    candidates, verified, shown
                );
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
        let ctx = window.saturating_sub((end - start).min(window) + 2) / 2; // «… » и « …»
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
