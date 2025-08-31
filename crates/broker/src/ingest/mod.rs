// crates/broker/src/ingest/mod.rs
pub mod compactor;
pub mod wal;
pub mod memtable;
pub mod flusher;
pub mod hot;

use compactor::Compactor;
use wal::Wal;

use crate::config::BrokerConfig;
use serde_json::Value;

pub async fn handle_batch_json(
    records: Vec<Value>,
    cfg: &BrokerConfig,
) -> anyhow::Result<serde_json::Value> {
    let wal = Wal::new(&cfg.wal_dir);
    let (wal_path, appended) = wal.append_batch(&records).await?;
    let comp = Compactor::new(cfg.segment_out_dir.clone().into());
    let seg_path = comp.wal_to_segment(&wal_path).await?;
    Ok(serde_json::json!({
        "ok": true, "appended": appended, "wal": wal_path, "segment": seg_path
    }))
}
