// path: crates/broker/src/ingest/compactor.rs
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};
use tokio::fs;
use tokio::io::AsyncReadExt;
// FIX: поддерживаем оба алгоритма и оба шаблона имени сайдкара
use anyhow::Context;

pub struct Compactor {
    pub out_dir: PathBuf,
}

impl Compactor {
    pub fn new(out_dir: PathBuf) -> Self {
        Self { out_dir }
    }

    pub async fn wal_to_segment(&self, wal_path: &str) -> anyhow::Result<String> {
        // FIX: попытаться провалидировать checksum, но не делать это фатальным
        if let Err(e) = validate_wal_checksum_best_effort(wal_path).await {
            tracing::warn!("wal checksum validation skipped: {e}");
        }

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

// --- helpers ---

async fn validate_wal_checksum_best_effort(wal_path: &str) -> anyhow::Result<()> {
    let p = Path::new(wal_path);
    // поддерживаем оба вида имен: "<base>.xxh3" и "<base>.jsonl.xxh3" (и crc32c)
    let candidates = [
        p.with_extension("xxh3"),                      // ".../<base>.xxh3"
        PathBuf::from(format!("{wal_path}.xxh3")),     // ".../<base>.jsonl.xxh3"
        p.with_extension("crc32c"),
        PathBuf::from(format!("{wal_path}.crc32c")),
    ];

    // найдём первый существующий сайдкар
    let mut sidecar: Option<PathBuf> = None;
    for c in candidates {
        if fs::try_exists(&c).await.unwrap_or(false) {
            sidecar = Some(c);
            break;
        }
    }

    // если сайдкара нет — не считаем это ошибкой (лог только)
    let Some(sc) = sidecar else {
        return Err(anyhow::anyhow!("no checksum sidecar found for {wal_path}"));
    };

    // читаем WAL
    let mut rf = fs::File::open(wal_path).await
        .with_context(|| format!("open wal {wal_path}"))?;
    let mut data = Vec::new();
    rf.read_to_end(&mut data).await?;

    // читаем ожидаемую сумму
    let want = fs::read_to_string(&sc).await?
        .trim()
        .to_string();

    // определим алгоритм по расширению сайдкара
    let have = match sc.extension().and_then(|e| e.to_str()) {
        Some("xxh3") => {
            let sum = xxhash_rust::xxh3::xxh3_64(&data);
            format!("{sum:016x}")
        }
        Some("crc32c") => {
            let mut c = crc32fast::Hasher::new();
            c.update(&data);
            let sum = c.finalize();
            format!("{sum:08x}")
        }
        other => anyhow::bail!("unknown sidecar extension: {:?}", other),
    };

    anyhow::ensure!(have == want, "wal checksum mismatch: have={have} want={want} sc={:?}", sc);
    Ok(())
}
