// Файл: crates/grepzilla_segment/src/segjson.rs
use crate::gram::{self, BooleanOp};
use crate::normalizer::normalize;
use crate::{SegmentMetaV1, SegmentReader, SegmentWriter, StoredDoc};

use anyhow::Result;
use croaring::Bitmap;
use std::collections::{BTreeMap, HashMap};
use std::fs::{self, File};
use std::io::{BufRead, BufReader, Write};

/// JSON-реализация сегмента V1:
/// - grams.json      : { trigram -> [doc_id, ...] }
/// - field_masks.json: { field   -> [doc_id, ...] }   // добавлено в A2
/// - docs.jsonl      : StoredDoc по строке (с doc_id)
/// - meta.json       : SegmentMetaV1
#[derive(Default)]
pub struct JsonSegmentWriter;

impl SegmentWriter for JsonSegmentWriter {
    fn write_segment(&mut self, input_jsonl: &str, out_dir: &str) -> Result<()> {
        fs::create_dir_all(out_dir)?;

        let f = File::open(input_jsonl)?;
        let br = BufReader::new(f);

        let mut next_id: u32 = 0;
        let mut grams: HashMap<String, Bitmap> = HashMap::new();
        let mut field_masks: HashMap<String, Bitmap> = HashMap::new();
        let mut docs: Vec<StoredDoc> = Vec::new();

        for line in br.lines() {
            let line = line?;
            if line.trim().is_empty() {
                continue;
            }
            let v: serde_json::Value = serde_json::from_str(&line)?;

            let ext_id = v
                .get("_id")
                .and_then(|x| x.as_str())
                .unwrap_or("")
                .to_string();

            // Собираем строковые поля, нормализуем и индексируем
            let mut stored: BTreeMap<String, String> = BTreeMap::new();
            collect_strings("", &v, &mut |path, s| {
                let ns = normalize(s);
                stored.insert(path.to_string(), ns.clone());

                // n-gram индекс
                for g in gram::trigrams(&ns) {
                    grams.entry(g).or_insert_with(Bitmap::new).add(next_id);
                }
                // маска поля
                field_masks
                    .entry(path.to_string())
                    .or_insert_with(Bitmap::new)
                    .add(next_id);
            });

            docs.push(StoredDoc {
                doc_id: next_id,
                ext_id,
                fields: stored,
            });
            next_id += 1;
        }

        // grams.json
        let grams_path = format!("{}/grams.json", out_dir);
        let mut grams_dump: HashMap<&str, Vec<u32>> = HashMap::new();
        for (k, bm) in &grams {
            grams_dump.insert(k.as_str(), bm.iter().collect());
        }
        let mut gf = File::create(&grams_path)?;
        serde_json::to_writer_pretty(&mut gf, &grams_dump)?;

        // field_masks.json (A2)
        let masks_path = format!("{}/field_masks.json", out_dir);
        let mut masks_dump: HashMap<&str, Vec<u32>> = HashMap::new();
        for (k, bm) in &field_masks {
            masks_dump.insert(k.as_str(), bm.iter().collect());
        }
        let mut mf = File::create(&masks_path)?;
        serde_json::to_writer_pretty(&mut mf, &masks_dump)?;

        // docs.jsonl
        let docs_path = format!("{}/docs.jsonl", out_dir);
        let mut df = File::create(&docs_path)?;
        for d in &docs {
            serde_json::to_writer(&mut df, d)?;
            df.write_all(b"\n")?;
        }

        // meta.json
        let meta = SegmentMetaV1 {
            version: 1,
            doc_count: next_id,
            gram_count: grams.len() as u32,
        };
        let meta_path = format!("{}/meta.json", out_dir);
        let mut meta_f = File::create(&meta_path)?;
        serde_json::to_writer_pretty(&mut meta_f, &meta)?;
        Ok(())
    }
}

pub struct JsonSegmentReader {
    meta: SegmentMetaV1,
    grams: HashMap<String, Bitmap>,
    field_masks: HashMap<String, Bitmap>,
    docs: Vec<StoredDoc>,
}

impl SegmentReader for JsonSegmentReader {
    fn open_segment(path: &str) -> Result<Self> {
        // meta
        let meta: SegmentMetaV1 = read_json(&format!("{}/meta.json", path))?;

        // grams
        let grams_map: HashMap<String, Vec<u32>> = read_json(&format!("{}/grams.json", path))?;
        let mut grams: HashMap<String, Bitmap> = HashMap::new();
        for (g, ids) in grams_map.into_iter() {
            let mut bm = Bitmap::new();
            for id in ids {
                bm.add(id);
            }
            grams.insert(g, bm);
        }

        // field_masks (optional для бэккомпат, но в A2 ожидается)
        let masks_map: HashMap<String, Vec<u32>> =
            read_json_opt(&format!("{}/field_masks.json", path)).unwrap_or_default();
        let mut field_masks: HashMap<String, Bitmap> = HashMap::new();
        for (field, ids) in masks_map.into_iter() {
            let mut bm = Bitmap::new();
            for id in ids {
                bm.add(id);
            }
            field_masks.insert(field, bm);
        }

        // docs
        let docs = read_jsonl::<StoredDoc>(&format!("{}/docs.jsonl", path))?;

        Ok(Self {
            meta,
            grams,
            field_masks,
            docs,
        })
    }

    fn doc_count(&self) -> u32 {
        self.meta.doc_count
    }

    fn prefilter(&self, op: BooleanOp, grams: &[String], field: Option<&str>) -> Result<Bitmap> {
        use BooleanOp::*;
        let mut acc = match op {
            And => {
                let mut it = grams.iter();
                let first = it.next().ok_or_else(|| anyhow::anyhow!("no grams"))?;
                let mut tmp = self.grams.get(first).cloned().unwrap_or_else(Bitmap::new);
                for g in it {
                    if let Some(bm) = self.grams.get(g) {
                        tmp.and_inplace(bm);
                    } else {
                        tmp.clear();
                        break;
                    }
                }
                tmp
            }
            Or => {
                let mut tmp = Bitmap::new();
                for g in grams {
                    if let Some(bm) = self.grams.get(g) {
                        tmp.or_inplace(bm);
                    }
                }
                tmp
            }
            Not => {
                let mut tmp = Bitmap::new();
                if self.meta.doc_count > 0 {
                    tmp.add_range(0..self.meta.doc_count);
                }
                for g in grams {
                    if let Some(bm) = self.grams.get(g) {
                        tmp.andnot_inplace(bm);
                    }
                }
                tmp
            }
        };

        // Пересечение с маской поля (A2)
        if let Some(field_name) = field {
            if let Some(mask) = self.field_masks.get(field_name) {
                acc.and_inplace(mask);
            } else {
                // Поля нет — пусто
                acc.clear();
            }
        }
        Ok(acc)
    }

    fn get_doc(&self, doc_id: u32) -> Option<&StoredDoc> {
        self.docs.get(doc_id as usize)
    }
}

// -------- helpers --------
fn read_json<T: for<'de> serde::Deserialize<'de>>(path: &str) -> Result<T> {
    let f = File::open(path)?;
    Ok(serde_json::from_reader(f)?)
}

fn read_json_opt<T: for<'de> serde::Deserialize<'de>>(path: &str) -> Option<T> {
    File::open(path)
        .ok()
        .and_then(|f| serde_json::from_reader(f).ok())
}

fn read_jsonl<T: for<'de> serde::Deserialize<'de>>(path: &str) -> Result<Vec<T>> {
    let f = File::open(path)?;
    let br = BufReader::new(f);
    let mut out = Vec::new();
    for line in br.lines() {
        let line = line?;
        if line.trim().is_empty() {
            continue;
        }
        out.push(serde_json::from_str(&line)?);
    }
    Ok(out)
}

/// Обходит JSON и вызывает `f(path, s)` для всех строковых полей.
/// Пример путей: `text.title`, `text.body`, `tags[0]`
fn collect_strings(path: &str, v: &serde_json::Value, f: &mut impl FnMut(&str, &str)) {
    match v {
        serde_json::Value::String(s) => f(path, s),
        serde_json::Value::Object(map) => {
            for (k, vv) in map {
                let np = if path.is_empty() {
                    k.clone()
                } else {
                    format!("{path}.{k}")
                };
                collect_strings(&np, vv, f);
            }
        }
        serde_json::Value::Array(arr) => {
            for (i, vv) in arr.iter().enumerate() {
                let np = if path.is_empty() {
                    format!("{}[{i}]", path)
                } else {
                    format!("{path}[{i}]")
                };
                collect_strings(&np, vv, f);
            }
        }
        _ => {}
    }
}
