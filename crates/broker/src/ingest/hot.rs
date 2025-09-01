// path: crates/broker/src/ingest/hot.rs
use grepzilla_segment::normalizer::normalize;
use grepzilla_segment::StoredDoc;
use serde_json::Value;
use std::collections::{BTreeMap, VecDeque};
use std::collections::HashSet; // NEW
use std::sync::{Arc, RwLock};

pub struct ApplyResult {
    pub added: usize,
    pub idempotent: bool,
    pub backlog_ms: Option<u64>,
}

#[derive(Clone)]
pub struct HotMem {
    inner: Arc<RwLock<VecDeque<StoredDoc>>>,
    cap: usize,               // удерживаем столько doc в window
    hard_cap: usize,          // NEW: порог отказа = cap
    idempotency_seen: Arc<RwLock<HashSet<String>>>, // NEW
}

impl Default for HotMem {
    fn default() -> Self {
        let cap = 10_000;
        Self {
            inner: Arc::new(RwLock::new(VecDeque::new())),
            cap,
            hard_cap: cap,
            idempotency_seen: Arc::new(RwLock::new(HashSet::new())),
        }
    }
}

impl HotMem {
    pub fn new() -> Self { Self::default() }

    pub fn with_cap(mut self, cap: usize) -> Self {
        if cap > 0 {
            self.cap = cap;
            self.hard_cap = cap;
        }
        self
    }

    pub fn len(&self) -> usize {
        self.inner.read().unwrap().len()
    }

    pub fn clear(&self) {
        self.inner.write().unwrap().clear();
        self.idempotency_seen.write().unwrap().clear();
    }

    pub fn snapshot(&self) -> Vec<StoredDoc> {
        self.inner.read().unwrap().iter().cloned().collect()
    }

    /// Основной путь — идемпотентность + backpressure по hard_cap
    pub fn apply(&self, docs: Vec<Value>, idempotency_key: Option<String>) -> Result<ApplyResult, Backpressure> {
        if let Some(k) = idempotency_key {
            let mut seen = self.idempotency_seen.write().unwrap();
            if !seen.insert(k) {
                return Ok(ApplyResult { added: 0, idempotent: true, backlog_ms: None });
            }
        }

        // жёсткий порог по текущему размеру
        if self.len() >= self.hard_cap {
            return Err(Backpressure { retry_after_ms: 1500 });
        }

        let mut g = self.inner.write().unwrap();
        let mut added = 0usize;

        for v in docs {
            let ext_id = v.get("_id").and_then(|x| x.as_str()).unwrap_or("").to_string();

            let mut fields: BTreeMap<String, String> = BTreeMap::new();
            collect_strings_local("", &v, &mut |path, s| {
                fields.insert(path.to_string(), normalize(s));
            });

            let doc_id = g.len() as u32;
            g.push_back(StoredDoc { doc_id, ext_id, fields });
            added += 1;

            // удерживаем окно cap (самые старые выдавливаем)
            while g.len() > self.cap {
                g.pop_front();
            }
        }

        Ok(ApplyResult { added, idempotent: false, backlog_ms: None })
    }

    pub fn metrics(&self) -> (usize, usize) {
        (self.len(), self.hard_cap)
    }
}

pub struct Backpressure { pub retry_after_ms: u64 }

fn collect_strings_local(path: &str, v: &Value, f: &mut impl FnMut(&str, &str)) {
    match v {
        Value::String(s) => f(path, s),
        Value::Object(map) => {
            for (k, vv) in map {
                let np = if path.is_empty() { k.clone() } else { format!("{path}.{k}") };
                collect_strings_local(&np, vv, f);
            }
        }
        Value::Array(arr) => {
            for (i, vv) in arr.iter().enumerate() {
                let np = if path.is_empty() { format!("[{i}]") } else { format!("{path}[{i}]") };
                collect_strings_local(&np, vv, f);
            }
        }
        _ => {}
    }
}
