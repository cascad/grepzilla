use anyhow::{bail, Result};
use crate::normalizer::normalize;

#[derive(Debug, Clone, Copy)]
pub enum BooleanOp { And, Or, Not }

/// Собрать все триграммы строки
pub fn trigrams(s: &str) -> Vec<String> {
    let chars: Vec<char> = s.chars().collect();
    chars.windows(3).map(|w| w.iter().collect::<String>()).collect()
}

/// Извлечь обязательные 3-граммы из wildcard-паттерна (\* и ?)
pub fn required_grams_from_wildcard(pattern: &str) -> Result<Vec<String>> {
    let pat = normalize(pattern);
    let mut out = Vec::new();
    let mut buf = String::new();
    for ch in pat.chars() {
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
    if buf.chars().count() >= 3 { push_tris(&buf, &mut out); }
    if out.is_empty() { bail!("pattern too weak; need ≥3 consecutive literal chars"); }
    Ok(out)
}

fn push_tris(s: &str, out: &mut Vec<String>) {
    let cs: Vec<char> = s.chars().collect();
    for w in cs.windows(3) { out.push(w.iter().collect()); }
}