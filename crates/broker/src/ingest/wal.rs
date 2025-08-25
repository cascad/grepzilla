// broker/src/ingest/wal.rs
use tokio::{fs, io::AsyncWriteExt};
use std::path::{Path, PathBuf};
use serde_json::Value;

pub struct Wal {
    dir: PathBuf,
}

impl Wal {
    pub fn new<P: AsRef<Path>>(dir: P) -> Self { Self { dir: dir.as_ref().into() } }

    pub async fn append_batch(&self, batch: &[Value]) -> anyhow::Result<(String, usize)> {
        fs::create_dir_all(&self.dir).await.ok();
        let file = self.dir.join(format!("{}.jsonl", nanoid::nanoid!()));
        let mut f = fs::File::create(&file).await?;
        let mut n = 0usize;
        for v in batch {
            let line = serde_json::to_string(v)?;
            f.write_all(line.as_bytes()).await?;
            f.write_all(b"\n").await?;
            n += 1;
        }
        f.flush().await?;
        // На этом этапе можно сделать f.sync_data().await?;
        Ok((file.to_string_lossy().to_string(), n))
    }
}
