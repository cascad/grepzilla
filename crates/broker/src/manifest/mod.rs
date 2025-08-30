use anyhow::{bail, Result};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

pub mod fs;

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct ShardEntry {
    #[serde(rename = "gen")]
    pub generation: u64,
    pub segments: Vec<String>,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct ManifestV1 {
    pub version: u32,
    pub shards: HashMap<u64, ShardEntry>,
}

/// ТВОЙ формат:
/// {
///   "shards":   { "0": 1, "1": 7 },
///   "segments": { "0:1": ["..."], "1:7": ["..."] }
/// }
#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct ManifestFlat {
    pub shards: HashMap<u64, u64>,              // shard -> gen
    pub segments: HashMap<String, Vec<String>>, // "shard:gen" -> paths
}

#[derive(Debug, Clone)]
pub struct SegRef {
    pub shard: u64,
    pub gen: u64,
    pub path: String,
}

/// Унифицированный вид внутри брокера
#[derive(Debug, Clone)]
pub struct ManifestUnified {
    pub pin_gen: HashMap<u64, u64>,             // shard -> gen
    pub segs: HashMap<(u64, u64), Vec<String>>, // (shard, gen) -> paths
}

impl ManifestUnified {
    fn from_v1(m: ManifestV1) -> Self {
        let mut pin_gen = HashMap::new();
        let mut segs = HashMap::new();
        for (sh, ent) in m.shards {
            pin_gen.insert(sh, ent.generation);
            segs.insert((sh, ent.generation), ent.segments);
        }
        Self { pin_gen, segs }
    }

    fn from_flat(m: ManifestFlat) -> Self {
        let mut pin_gen = m.shards.clone();
        let mut segs = HashMap::new();
        for (k, paths) in m.segments {
            // ожидаем "shard:gen"
            if let Some((a, b)) = k.split_once(':') {
                if let (Ok(sh), Ok(gen)) = (a.parse::<u64>(), b.parse::<u64>()) {
                    segs.insert((sh, gen), paths);
                }
            }
        }
        Self { pin_gen, segs }
    }

    pub fn resolve(&self, shards: &[u64]) -> (Vec<SegRef>, HashMap<u64, u64>) {
        let mut out = Vec::new();
        let mut pin = HashMap::new();
        for &sh in shards {
            if let Some(&gen) = self.pin_gen.get(&sh) {
                pin.insert(sh, gen);
                if let Some(paths) = self.segs.get(&(sh, gen)) {
                    for p in paths {
                        out.push(SegRef {
                            shard: sh,
                            gen,
                            path: p.clone(),
                        });
                    }
                }
            }
        }
        (out, pin)
    }
}

#[async_trait]
pub trait ManifestStore: Send + Sync {
    async fn load(&self) -> Result<ManifestUnified>;
    async fn resolve(&self, shards: &[u64]) -> Result<(Vec<SegRef>, HashMap<u64, u64>)>;
}
