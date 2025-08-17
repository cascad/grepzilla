use anyhow::Result;
use clap::{Parser, Subcommand};
use grepzilla_segment::gram::{required_grams_from_wildcard, BooleanOp};
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
        /// Путь к JSONL с документами
        #[arg(long)]
        input: String,
        /// Папка для сегмента (будет создана)
        #[arg(long)]
        out: String,
    },
    /// Поиск в одном сегменте (wildcard-паттерн)
    SearchSeg {
        /// Папка сегмента
        #[arg(long)]
        seg: String,
        /// Шаблон (wildcard: * и ?) — например, "*играет*"
        #[arg(long)]
        q: String,
        /// Field scope (например, text.body)
        #[arg(long)]
        field: Option<String>,
        /// Лимит на выдачу
        #[arg(long, default_value_t = 10)]
        limit: usize,
        /// Смещение
        #[arg(long, default_value_t = 0)]
        offset: usize,
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
        } => {
            let reader = JsonSegmentReader::open_segment(&seg)?;
            let grams = required_grams_from_wildcard(&q)?;
            let bm = reader.prefilter(BooleanOp::And, &grams)?;

            // Компилируем wildcard в regex
            let rx = wildcard_to_regex(&q)?;

            // Пагинация по doc_id возрастанию
            let mut shown = 0usize;
            let mut skipped = 0usize;
            for doc_id in bm.iter() {
                if skipped < offset {
                    skipped += 1;
                    continue;
                }
                if shown >= limit {
                    break;
                }
                if let Some(doc) = reader.get_doc(doc_id) {
                    // Проверка по полю/всем полям
                    let matched = match field.as_deref() {
                        Some(f) => doc.fields.get(f).map(|t| rx.is_match(t)).unwrap_or(false),
                        None => doc.fields.values().any(|t| rx.is_match(t)),
                    };
                    if !matched {
                        continue;
                    }
                    let preview = doc
                        .fields
                        .get("text.body")
                        .or_else(|| doc.fields.get("text.title"))
                        .cloned()
                        .unwrap_or_else(|| doc.fields.values().next().cloned().unwrap_or_default());
                    println!("{}\t{}\t{}", doc.ext_id, doc_id, preview);
                    shown += 1;
                }
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
