use serde::Deserialize;

#[derive(Clone, Deserialize)]
pub struct BrokerConfig {
    pub addr: String,
    pub wal_dir: String,
    pub segment_out_dir: String,
    #[serde(default = "default_parallelism")]
    pub parallelism: usize,
    #[serde(default = "default_hot_cap")]
    pub hot_cap: usize, // NEW
}

fn default_parallelism() -> usize { 4 }
fn default_hot_cap() -> usize { 10_000 }

impl BrokerConfig {
    pub fn from_env() -> Self {
        let addr = std::env::var("GZ_ADDR").unwrap_or_else(|_| "0.0.0.0:8080".into());
        let wal_dir = std::env::var("GZ_WAL_DIR").unwrap_or_else(|_| "wal".into());
        let segment_out_dir = std::env::var("GZ_SEGMENTS_DIR").unwrap_or_else(|_| "segments".into());
        let parallelism = std::env::var("GZ_PARALLELISM").ok().and_then(|s| s.parse().ok()).unwrap_or(default_parallelism());
        let hot_cap = std::env::var("GZ_HOT_CAP").ok().and_then(|s| s.parse().ok()).unwrap_or(default_hot_cap());

        Self { addr, wal_dir, segment_out_dir, parallelism, hot_cap }
    }
}
