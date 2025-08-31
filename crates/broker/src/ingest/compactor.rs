// crates/broker/src/ingest/compactor.rs
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};
use tokio::fs;
use tokio::io::AsyncReadExt;

pub struct Compactor {
    pub out_dir: PathBuf,
}

impl Compactor {
    pub fn new(out_dir: PathBuf) -> Self {
        Self { out_dir }
    }

    pub async fn wal_to_segment(&self, wal_path: &str) -> anyhow::Result<String> {
        // 1) создаём сегментную директорию по timestamp
        let ts_ms = SystemTime::now().duration_since(UNIX_EPOCH)?.as_millis();
        let seg_dir = self.out_dir.join(format!("{:015}", ts_ms));
        fs::create_dir_all(&seg_dir).await?;

        // 2) копируем wal → docs.jsonl (внутри сегмента)
        let mut src = fs::File::open(wal_path).await?;
        let mut data = Vec::new();
        src.read_to_end(&mut data).await?;
        let docs_path = seg_dir.join("docs.jsonl");
        fs::write(&docs_path, data).await?;

        // 3) собираем сегмент из docs.jsonl (grams.json, field_masks.json, meta.json)
        use grepzilla_segment::segjson::JsonSegmentWriter;
        use grepzilla_segment::SegmentWriter;

        let mut writer = JsonSegmentWriter::default();
        writer.write_segment(
            &docs_path.to_string_lossy(),
            &seg_dir.to_string_lossy(),
        )?;

        // 4) можно удалить промежуточный docs.jsonl (не обязательно)
        let _ = fs::remove_file(&docs_path).await;

        Ok(seg_dir.to_string_lossy().to_string())
    }
}
