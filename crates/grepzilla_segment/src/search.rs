// crates/grepzilla_segment/src/search.rs
use anyhow::Result;

use crate::gram::{BooleanOp, required_grams_from_wildcard};
use crate::normalizer::normalize;
use crate::segjson::JsonSegmentReader;
use crate::verify::compile_wildcard_engine; // NEW

use crate::SegmentReader;

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

pub fn search_one_segment(
    seg_path: &str,
    field: Option<&str>,
    wildcard: &str,
    from_doc: Option<u64>,
    max_candidates: u64,
) -> Result<SegSearchOut> {
    // 1) нормализуем запрос и извлекаем обязательные 3-граммы
    let norm_wc = normalize(wildcard);
    let grams = required_grams_from_wildcard(&norm_wc)?;

    // 2) открываем сегмент V1 (JSON) и считаем префильтр
    let reader = JsonSegmentReader::open_segment(seg_path)?;
    let bm = reader.prefilter(BooleanOp::And, &grams, field)?;

    // 3) компилируем движок верификации из WILDCARD (фабрика сама нормализует)
    let eng = compile_wildcard_engine(wildcard)?; // <-- вместо compile_wildcard_regex_engine(&norm_wc)

    let mut hits = Vec::new();
    let mut last_docid = from_doc;
    let mut candidates = 0u64;

    for doc_id in bm.iter() {
        // пагинация: пропускаем всё <= last_docid
        if let Some(cur) = from_doc {
            if (doc_id as u64) <= cur {
                continue;
            }
        }
        candidates += 1;
        last_docid = Some(doc_id as u64);
        if candidates > max_candidates {
            break;
        }

        if let Some(doc) = reader.get_doc(doc_id) {
            // verify: либо заданное поле, либо первое совпавшее
            let (matched, matched_field) = match field {
                Some(f) => {
                    let ok = doc.fields.get(f).map(|v| eng.is_match(v)).unwrap_or(false);
                    (ok, ok.then(|| f.to_string()))
                }
                None => {
                    if let Some((k, _)) = doc.fields.iter().find(|(_, v)| eng.is_match(v)) {
                        (true, Some(k.clone()))
                    } else {
                        (false, None)
                    }
                }
            };

            if matched {
                hits.push(Hit {
                    doc_id,
                    ext_id: doc.ext_id.clone(),
                    matched_field,
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
