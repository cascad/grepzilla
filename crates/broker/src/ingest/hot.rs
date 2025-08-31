use grepzilla_segment::normalizer::normalize;
use grepzilla_segment::StoredDoc;
use serde_json::Value;
use std::collections::{BTreeMap, VecDeque};
use std::sync::{Arc, RwLock};

#[derive(Clone)]
pub struct HotMem {
    inner: Arc<RwLock<VecDeque<StoredDoc>>>,
    cap: usize,
}

impl Default for HotMem {
    fn default() -> Self {
        Self {
            inner: Arc::new(RwLock::new(VecDeque::new())),
            cap: 10_000, // дефолт
        }
    }
}

impl HotMem {
    pub fn new() -> Self { Self::default() }

    pub fn with_cap(mut self, cap: usize) -> Self {
        if cap > 0 { self.cap = cap; }
        self
    }

    pub fn len(&self) -> usize {
        self.inner.read().unwrap().len()
    }

    pub fn clear(&self) { self.inner.write().unwrap().clear(); }

    pub fn snapshot(&self) -> Vec<StoredDoc> {
        self.inner.read().unwrap().iter().cloned().collect()
    }

    /// Добавляет документы (нормализует строки). Сбрасывает самые старые при превышении cap.
    pub fn push_raw_json(&self, docs: Vec<Value>) -> usize {
        let mut g = self.inner.write().unwrap();
        let mut added = 0usize;

        for v in docs {
            let ext_id = v.get("_id").and_then(|x| x.as_str()).unwrap_or("").to_string();

            let mut fields: BTreeMap<String, String> = BTreeMap::new();
            collect_strings_local("", &v, &mut |path, s| {
                fields.insert(path.to_string(), normalize(s));
            });

            let doc_id = g.len() as u32; // локальный порядковый (только для превью UI)
            g.push_back(StoredDoc { doc_id, ext_id, fields });
            added += 1;

            // удерживаем cap
            while g.len() > self.cap {
                g.pop_front();
            }
        }
        added
    }
}

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
