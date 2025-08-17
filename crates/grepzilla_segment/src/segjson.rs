use crate::{InputDoc, StoredDoc, SegmentMetaV1, SegmentReader, SegmentWriter};
use crate::normalizer::normalize;
use crate::gram::{self, BooleanOp};
use anyhow::Result;
use croaring::Bitmap;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap};
use std::fs::{self, File};
use std::io::{BufRead, BufReader, Write};
use std::path::Path;

/// Простая JSON-реализация сегмента: grams.json + docs.jsonl + meta.json
#[derive(Default)]
pub struct JsonSegmentWriter;

impl crate::SegmentWriter for JsonSegmentWriter {
    fn write_segment(&mut self, input_jsonl: &str, out_dir: &str) -> Result<()> {
        fs::create_dir_all(out_dir)?;

        let f = File::open(input_jsonl)?;
        let br = BufReader::new(f);

        let mut next_id: u32 = 0;
        let mut grams: HashMap<String, Bitmap> = HashMap::new();
        let mut docs: Vec<StoredDoc> = Vec::new();

        for line in br.lines() {
            let line = line?;
            if line.trim().is_empty() { continue; }
            let v: serde_json::Value = serde_json::from_str(&line)?;
            let ext_id = v.get("_id").and_then(|x| x.as_str()).unwrap_or("").to_string();

            // Собираем строковые поля и индексируем
            let mut stored: BTreeMap<String,String> = BTreeMap::new();
            collect_strings("", &v, &mut |path, s| {
                let ns = normalize(s);
                stored.insert(path.to_string(), ns.clone());
                for g in gram::trigrams(&ns) {
                    grams.entry(g).or_insert_with(Bitmap::new).add(next_id);
                }
            });

            docs.push(StoredDoc { doc_id: next_id, ext_id, fields: stored });
            next_id += 1;
        }

        // Пишем grams.json как gram -> Vec<u32> (для простоты). В реале будет mmap + FST.
        let grams_path = format!("{}/grams.json", out_dir);
        let mut grams_dump: HashMap<&str, Vec<u32>> = HashMap::new();
        for (k, bm) in &grams {
            grams_dump.insert(k.as_str(), bm.iter().collect());
        }
        let mut gf = File::create(&grams_path)?;
        serde_json::to_writer_pretty(&mut gf, &grams_dump)?;

        // Пишем docs.jsonl
        let docs_path = format!("{}/docs.jsonl", out_dir);
        let mut df = File::create(&docs_path)?;
        for d in &docs {
            serde_json::to_writer(&mut df, d)?;
            df.write_all(b"\n")?;
        }

        // meta.json
        let meta = SegmentMetaV1 { version: 1, doc_count: next_id, gram_count: grams.len() as u32 };
        let meta_path = format!("{}/meta.json", out_dir);
        let mut mf = File::create(&meta_path)?;
        serde_json::to_writer_pretty(&mut mf, &meta)?;

        Ok(())
    }
}

/// Reader JSON-сегмента: грузит grams.json в Roaring, docs.jsonl в память
pub struct JsonSegmentReader {
    meta: SegmentMetaV1,
    grams: HashMap<String, Bitmap>,
    docs: Vec<StoredDoc>,
}

impl crate::SegmentReader for JsonSegmentReader {
    fn open_segment(path: &str) -> Result<Self> {
        let meta: SegmentMetaV1 = read_json(&format!("{}/meta.json", path))?;
        let grams_map: HashMap<String, Vec<u32>> = read_json(&format!("{}/grams.json", path))?;
        let mut grams: HashMap<String, Bitmap> = HashMap::new();
        for (g, ids) in grams_map.into_iter() {
            let mut bm = Bitmap::new();
            for id in ids { bm.add(id); }
            grams.insert(g, bm);
        }
        let docs = read_jsonl::<StoredDoc>(&format!("{}/docs.jsonl", path))?;
        Ok(Self { meta, grams, docs })
    }

    fn doc_count(&self) -> u32 { self.meta.doc_count }

    fn prefilter(&self, op: BooleanOp, grams: &[String]) -> Result<Bitmap> {
        use BooleanOp::*;
        match op {
            And => {
                let mut it = grams.iter();
                let first = it.next().ok_or_else(|| anyhow::anyhow!("no grams"))?;
                let mut acc = self.grams.get(first).cloned().unwrap_or_else(Bitmap::new);
                for g in it { if let Some(bm) = self.grams.get(g) { acc.and_inplace(bm); } else { acc.clear(); break; } }
                Ok(acc)
            }
            Or => {
                let mut acc = Bitmap::new();
                for g in grams { if let Some(bm) = self.grams.get(g) { acc.or_inplace(bm); } }
                Ok(acc)
            }
            Not => {
                let mut acc = Bitmap::new();
                if self.meta.doc_count > 0 { acc.add_range(0..self.meta.doc_count); }
                for g in grams { if let Some(bm) = self.grams.get(g) { acc.andnot_inplace(bm); } }
                Ok(acc)
            }
        }
    }

    fn get_doc(&self, doc_id: u32) -> Option<&StoredDoc> {
        self.docs.get(doc_id as usize)
    }
}

fn read_json<T: for<'de> serde::Deserialize<'de>>(path: &str) -> Result<T> {
    let f = File::open(path)?; Ok(serde_json::from_reader(f)?)
}

fn read_jsonl<T: for<'de> serde::Deserialize<'de>>(path: &str) -> Result<Vec<T>> {
    let f = File::open(path)?; let br = BufReader::new(f);
    let mut out = Vec::new();
    for line in br.lines() {
        let line = line?; if line.trim().is_empty() { continue; }
        out.push(serde_json::from_str(&line)?);
    }
    Ok(out)
}

fn collect_strings(path: &str, v: &serde_json::Value, f: &mut impl FnMut(&str, &str)) {
    match v {
        serde_json::Value::String(s) => f(path, s),
        serde_json::Value::Object(map) => {
            for (k, vv) in map {
                let np = if path.is_empty() { k.clone() } else { format!("{path}.{k}") };
                collect_strings(&np, vv, f);
            }
        }
        serde_json::Value::Array(arr) => {
            for (i, vv) in arr.iter().enumerate() {
                let np = if path.is_empty() { format!("[{i}") } else { format!("{path}[{i}") };
                collect_strings(&np, vv, f);
            }
        }
        _ => {}
    }
}