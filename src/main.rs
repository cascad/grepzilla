use clap::{Parser, Subcommand};
use grepzilla::index::InMemoryIndex;
use grepzilla::query::parse_query;
use serde_json::Value;
use std::fs::File;
use std::io::{BufRead, BufReader};

#[derive(Parser)]
#[command(version, about)]
struct Cli {
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// Ingest a JSONL file into an in-memory index; optional REPL to query
    Ingest {
        path: String,
        /// Start interactive REPL after ingest
        #[arg(long)]
        repl: bool,
    },
}

fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();
    match cli.cmd {
        Cmd::Ingest { path, repl } => run_ingest(path, repl)?,
    }
    Ok(())
}

fn run_ingest(path: String, repl: bool) -> anyhow::Result<()> {
    let f = File::open(&path)?;
    let br = BufReader::new(f);
    let mut idx = InMemoryIndex::new();

    let mut n: u64 = 0;
    for line in br.lines() {
        let line = line?;
        if line.trim().is_empty() {
            continue;
        }
        let v: Value = serde_json::from_str(&line)?;
        idx.add_json_doc(v)?;
        n += 1;
    }
    eprintln!("ingested {n} docs");

    if repl {
        use std::io::{Write, stdin, stdout};
        let mut input = String::new();
        loop {
            input.clear();
            print!("query> ");
            stdout().flush().ok();
            if stdin().read_line(&mut input).is_err() {
                break;
            }
            let s = input.trim();
            if s.is_empty() || s == ":q" || s == ":quit" {
                break;
            }

            let (q, opts) = match parse_query(s) {
                Ok(x) => x,
                Err(e) => {
                    println!("parse error: {e}");
                    continue;
                }
            };
            let hits = idx.search(&q, opts.limit, opts.offset)?;
            for (rank, h) in hits.iter().enumerate() {
                println!(
                    "{}\t{}\t{}",
                    rank + 1 + opts.offset as usize,
                    h.doc_id,
                    h.preview
                );
            }
        }
    }

    Ok(())
}
