use crate::index::normalizer::normalize;
use anyhow::{Context, bail};
use regex::Regex;

#[derive(Debug, Clone, Copy)]
pub enum BooleanOp {
    And,
    Or,
    Not,
}

#[derive(Debug, Clone)]
pub struct ExecPlan {
    pub op: BooleanOp,
    pub grams: Vec<String>, // required grams
    pub matcher: String,    // original wildcard pattern, normalized
    pub field: Option<String>,
}

#[derive(Debug, Clone, Default)]
pub struct SearchOpts {
    pub limit: u32,
    pub offset: u32,
}

#[derive(Debug, Clone)]
pub struct Hit {
    pub doc_id: String,
    pub preview: String,
}

pub fn parse_query(s: &str) -> anyhow::Result<(ExecPlan, SearchOpts)> {
    // Very small parser: supports --limit N --offset N at end; AND/OR/NOT only at top-level; optional field:
    let mut parts: Vec<&str> = s.split_whitespace().collect();
    let mut opts = SearchOpts {
        limit: 10,
        offset: 0,
    };
    // parse trailing options
    let mut i = 0;
    while i < parts.len() {
        if parts[i] == "--limit" && i + 1 < parts.len() {
            opts.limit = parts[i + 1].parse().unwrap_or(10);
            parts.drain(i..=i + 1);
        } else if parts[i] == "--offset" && i + 1 < parts.len() {
            opts.offset = parts[i + 1].parse().unwrap_or(0);
            parts.drain(i..=i + 1);
        } else {
            i += 1;
        }
    }

    let joined = parts.join(" ");
    // Simple detection of boolean op
    let (op, lhs, rhs) = if let Some(pos) = joined.find(" AND ") {
        (BooleanOp::And, &joined[..pos], &joined[pos + 5..])
    } else if let Some(pos) = joined.find(" OR ") {
        (BooleanOp::Or, &joined[..pos], &joined[pos + 4..])
    } else if let Some(pos) = joined.find(" NOT ") {
        (BooleanOp::Not, &joined[..pos], &joined[pos + 5..])
    } else {
        (BooleanOp::And, &joined[..], "")
    };

    let mut grams = Vec::new();
    let mut matcher = String::new();
    let mut field = None;

    let mut collect = |expr: &str| -> anyhow::Result<()> {
        let expr = expr.trim();
        if expr.is_empty() {
            return Ok(());
        }
        let (fld, pat) = if let Some(colon) = expr.find(':') {
            (Some(expr[..colon].to_string()), &expr[colon + 1..])
        } else {
            (None, expr)
        };
        let pat_norm = normalize(pat);
        let required = required_grams(&pat_norm)?;
        if grams.is_empty() {
            grams.extend(required);
        } else {
            grams.extend(required);
        }
        if matcher.is_empty() {
            matcher = pat_norm;
        }
        if field.is_none() {
            field = fld;
        }
        Ok(())
    };

    collect(lhs)?;
    collect(rhs)?;

    if grams.is_empty() {
        bail!("pattern too weak; include at least 3 consecutive literal chars")
    }

    Ok((
        ExecPlan {
            op,
            grams,
            matcher,
            field,
        },
        opts,
    ))
}

/// Extract required 3-grams from a wildcard pattern.
/// We take the longest literal runs (≥3) and emit their trigrams.
pub fn required_grams(pat_norm: &str) -> anyhow::Result<Vec<String>> {
    let mut out = Vec::new();
    let mut buf = String::new();
    for ch in pat_norm.chars() {
        match ch {
            '*' | '?' => {
                if buf.chars().count() >= 3 {
                    push_tris(&buf, &mut out);
                }
                buf.clear();
            }
            _ => buf.push(ch),
        }
    }
    if buf.chars().count() >= 3 {
        push_tris(&buf, &mut out);
    }
    Ok(out)
}

fn push_tris(s: &str, out: &mut Vec<String>) {
    let chars: Vec<char> = s.chars().collect();
    for w in chars.windows(3) {
        out.push(w.iter().collect());
    }
}

/// Compile wildcard to a fast DFA regex.
pub fn compile_pattern(pat_norm: &str) -> anyhow::Result<Regex> {
    // Escape regex meta in literals, then replace wildcards
    let mut rx = String::from("(?s)"); // dot matches newline
    for ch in pat_norm.chars() {
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
    Regex::new(&rx).context("compile regex")
}
