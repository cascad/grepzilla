// broker/src/ingest/compactor.rs
use std::path::PathBuf;
use tokio::{fs, io::AsyncReadExt};

pub struct Compactor {
    pub out_dir: PathBuf,
}

impl Compactor {
    pub fn new(out_dir: PathBuf) -> Self {
        Self { out_dir }
    }

    pub async fn wal_to_segment(&self, wal_path: &str) -> anyhow::Result<String> {
        // Читаем WAL jsonl и вызываем существующий сборщик сегментов (или внутренний API)
        // Пока просто копируем как плейсхолдер
        let seg_dir = self.out_dir.join(nanoid::nanoid!());
        fs::create_dir_all(&seg_dir).await?;
        let mut src = fs::File::open(wal_path).await?;
        let mut data = Vec::new();
        src.read_to_end(&mut data).await?;
        fs::write(seg_dir.join("data.jsonl"), data).await?;
        Ok(seg_dir.to_string_lossy().to_string())
    }
}
