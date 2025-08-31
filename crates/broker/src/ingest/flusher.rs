// crates/broker/src/ingest/flusher.rs
use anyhow::Result;
use serde_json::Value;
use std::path::{Path, PathBuf};
use tokio::io::AsyncWriteExt;

pub struct Flusher {
    out_dir: PathBuf,
}

impl Flusher {
    pub fn new(out_dir: impl AsRef<Path>) -> Self {
        Self { out_dir: out_dir.as_ref().to_path_buf() }
    }

    pub fn choose_segment_path(&self) -> Result<std::path::PathBuf> {
        let ts = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH)?.as_millis();
        Ok(self.out_dir.join(format!("{ts:013}")))
    }

    pub async fn flush_to_segment(&self, docs: Vec<Value>) -> Result<PathBuf> {
        tokio::fs::create_dir_all(&self.out_dir).await.ok();
        let seg_path = self.choose_segment_path()?;
        tokio::fs::create_dir_all(&seg_path).await?;

        // пишем временный docs.jsonl
        let tmp = seg_path.join("docs.jsonl");
        let mut f = tokio::fs::File::create(&tmp).await?;
        for v in docs {
            let line = serde_json::to_string(&v)?;
            use tokio::io::AsyncWriteExt;
            f.write_all(line.as_bytes()).await?;
            f.write_all(b"\n").await?;
        }
        f.flush().await?;

        // собираем сегмент из docs.jsonl
        use grepzilla_segment::segjson::JsonSegmentWriter;
        use grepzilla_segment::SegmentWriter;

        let mut writer = JsonSegmentWriter::default();
        writer.write_segment(
            &tmp.to_string_lossy(),
            &seg_path.to_string_lossy(),
        )?;

        // можно удалить промежуточный файл
        let _ = tokio::fs::remove_file(tmp).await;

        Ok(seg_path)
    }
}
