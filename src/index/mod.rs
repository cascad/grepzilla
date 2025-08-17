pub mod gram;
pub mod inverted;
pub mod normalizer;

use crate::query::{ExecPlan, Hit, compile_pattern};
use inverted::{DocId, InvertedIndex};
use normalizer::normalize;
use serde_json::Value;
use std::collections::{BTreeMap, HashMap};

pub struct InMemoryIndex {
    inv: InvertedIndex,                             // gram -> bitmap(docIDs)
    docs: HashMap<DocId, BTreeMap<String, String>>, // stored text fields (normalized)
}

impl InMemoryIndex {
    pub fn new() -> Self {
        Self {
            inv: InvertedIndex::new(),
            docs: HashMap::new(),
        }
    }

    pub fn add_json_doc(&mut self, v: Value) -> anyhow::Result<()> {
        use uuid::Uuid;
        // Extract _id or generate
        let id = v
            .get("_id")
            .and_then(|x| x.as_str())
            .map(|s| s.to_string())
            .unwrap_or_else(|| Uuid::new_v4().to_string());
        let doc_id: DocId = self.inv.map_ext_id(&id);

        // Collect string fields; index all strings; store only strings for preview
        let mut stored: BTreeMap<String, String> = BTreeMap::new();
        collect_strings("", &v, &mut |path, s| {
            let ns = normalize(s);
            self.inv.add_text(doc_id, &ns);
            stored.insert(path.to_string(), ns);
        });
        self.docs.insert(doc_id, stored);
        Ok(())
    }

    pub fn search(&self, plan: &ExecPlan, limit: u32, offset: u32) -> anyhow::Result<Vec<Hit>> {
        use rayon::prelude::*;
        let mut cand = self.inv.execute(plan)?; // roaring bitmap of docIds
        // Simple pagination on docID order (stable)
        let mut out = Vec::new();
        let mut skipped = 0u32;
        for doc_id in cand.iter() {
            // ascending doc ids; croaring 2.x yields u32
            if skipped < offset {
                skipped += 1;
                continue;
            }
            if (out.len() as u32) >= limit {
                break;
            }

            // Везде, где нужен ключ в HashMap<DocId, _>, оставляем &doc_id:
            let matches = compile_pattern(&plan.matcher).map(|re| match plan.field.as_deref() {
                Some(field) => {
                    if let Some(map) = self.docs.get(&doc_id) {
                        map.get(field).map(|t| re.is_match(t)).unwrap_or(false)
                    } else {
                        false
                    }
                }
                None => {
                    if let Some(map) = self.docs.get(&doc_id) {
                        map.values().any(|t| re.is_match(t))
                    } else {
                        false
                    }
                }
            })?;
            if !matches {
                continue;
            }

            let preview = self
                .docs
                .get(&doc_id)
                .and_then(|m| {
                    if let Some(body) = m.get("text.body") {
                        Some(body.clone())
                    } else if let Some(title) = m.get("text.title") {
                        Some(title.clone())
                    } else {
                        m.values().next().cloned()
                    }
                })
                .unwrap_or_default();

            out.push(Hit {
                doc_id: self.inv.reverse_id(doc_id).to_string(),
                preview,
            });
        }
        Ok(out)
    }
}

fn collect_strings(path: &str, v: &Value, f: &mut impl FnMut(&str, &str)) {
    match v {
        Value::String(s) => {
            // Index all strings; if you prefer only "text.*" paths, gate here
            f(path, s);
        }
        Value::Object(map) => {
            for (k, vv) in map {
                let np = if path.is_empty() {
                    k.clone()
                } else {
                    format!("{path}.{k}")
                };
                collect_strings(&np, vv, f);
            }
        }
        Value::Array(arr) => {
            for (i, vv) in arr.iter().enumerate() {
                let np = if path.is_empty() {
                    format!("[{i}")
                } else {
                    format!("{path}[{i}")
                };
                collect_strings(&np, vv, f);
            }
        }
        _ => {}
    }
}
