// broker/src/config.rs
use serde::Deserialize;

#[derive(Debug, Deserialize)]
pub struct BrokerConfig {
    pub parallelism: usize,
    pub wal_dir: String,
    pub segment_out_dir: String,
}

impl Default for BrokerConfig {
    fn default() -> Self {
        Self {
            parallelism: 4,
            wal_dir: "wal".into(),
            segment_out_dir: "segments".into(),
        }
    }
}
