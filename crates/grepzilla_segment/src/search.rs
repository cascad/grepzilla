use anyhow::Result;
use regex::Regex;

use crate::SegmentReader;
use crate::gram::{BooleanOp, required_grams_from_wildcard};
use crate::normalizer::normalize;
use crate::segjson::JsonSegmentReader;

#[derive(Debug)]
pub struct Hit {
    pub doc_id: u32,
    pub ext_id: String,
    pub matched_field: Option<String>,
}

#[derive(Debug)]
pub struct SegSearchOut {
    pub hits: Vec<Hit>,
    pub last_docid: Option<u64>,
    pub candidates: u64,
}

fn wildcard_norm_to_regex(norm_wildcard: &str) -> Result<Regex> {
    let mut pat = String::new();
    for ch in norm_wildcard.chars() {
        match ch {
            '*' => pat.push_str(".*"),
            '?' => pat.push('.'),
            _ => regex::escape(&ch.to_string()).push_str(&mut pat),
        }
    }
    Ok(Regex::new(&pat)?)
}

pub fn search_one_segment(
    seg_path: &str,
    field: Option<&str>,
    wildcard: &str,
    from_doc: Option<u64>,
    max_candidates: u64,
) -> Result<SegSearchOut> {
    // normalize + grams
    let grams = required_grams_from_wildcard(wildcard)?;
    let reader = JsonSegmentReader::open_segment(seg_path)?;
    let bm = reader.prefilter(BooleanOp::And, &grams, field)?;
    let norm_wc = normalize(wildcard);
    let re = wildcard_norm_to_regex(&norm_wc)?;

    let mut hits = Vec::new();
    let mut last_docid = from_doc;
    let mut candidates = 0u64;

    for doc_id in bm.iter() {
        if let Some(cur) = from_doc {
            if doc_id as u64 <= cur {
                continue;
            }
        }
        candidates += 1;
        last_docid = Some(doc_id as u64);
        if candidates > max_candidates {
            break;
        }

        if let Some(doc) = reader.get_doc(doc_id) {
            let matched = if let Some(f) = field {
                doc.fields.get(f).map(|v| re.is_match(v)).unwrap_or(false)
            } else {
                doc.fields.values().any(|v| re.is_match(v))
            };

            if matched {
                hits.push(Hit {
                    doc_id,
                    ext_id: doc.ext_id.clone(),
                    matched_field: field.map(str::to_owned),
                });
            }
        }
    }

    Ok(SegSearchOut {
        hits,
        last_docid,
        candidates,
    })
}
