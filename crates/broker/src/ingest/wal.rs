// path: crates/broker/src/ingest/wal.rs
use serde_json::Value;
use std::path::{Path, PathBuf};
use tokio::{fs, io::AsyncWriteExt};
// NEW:
use std::time::{SystemTime, UNIX_EPOCH};
use tokio::io::AsyncReadExt;
use xxhash_rust::xxh3::xxh3_64; // NEW // NEW

// NEW: режимы durability из env GZ_WAL_FSYNC
#[derive(Clone, Copy, Debug)]
enum WalFsyncMode {
    Always,
    Batch,
    Disabled,
}

fn wal_fsync_mode_from_env() -> WalFsyncMode {
    match std::env::var("GZ_WAL_FSYNC").as_deref() {
        Ok("always") => WalFsyncMode::Always,
        Ok("disabled") => WalFsyncMode::Disabled,
        _ => WalFsyncMode::Batch,
    }
}

pub struct Wal {
    dir: PathBuf,
    // NEW:
    fsync_mode: WalFsyncMode,
}

impl Wal {
    pub fn new<P: AsRef<Path>>(dir: P) -> Self {
        Self {
            dir: dir.as_ref().into(),
            fsync_mode: wal_fsync_mode_from_env(), // NEW
        }
    }

    /// Пишет батч в *.jsonl атомарно: .tmp -> sync -> rename
    /// Также создаёт *.xxh3 с checksum содержимого (для валидации в компакторе).
    pub async fn append_batch(&self, batch: &[Value]) -> anyhow::Result<(String, usize)> {
        fs::create_dir_all(&self.dir).await.ok();

        // NEW: предсказуемые имена: <ts>-<rand>.jsonl
        let ts = SystemTime::now().duration_since(UNIX_EPOCH)?.as_millis();
        let base = format!("{ts:016}-{}", nanoid::nanoid!());
        let path_tmp = self.dir.join(format!("{base}.jsonl.tmp"));
        let path_fin = self.dir.join(format!("{base}.jsonl"));
        let path_sum = self.dir.join(format!("{base}.xxh3")); // NEW: сайдкар

        // пишем во временный файл
        let mut f = fs::File::create(&path_tmp).await?;
        let mut n = 0usize;
        for v in batch {
            let line = serde_json::to_string(v)?;
            f.write_all(line.as_bytes()).await?;
            f.write_all(b"\n").await?;
            n += 1;
        }
        f.flush().await?;

        // NEW: fsync в соответствии с режимом
        match self.fsync_mode {
            WalFsyncMode::Always => {
                f.sync_data().await?;
            }
            WalFsyncMode::Batch => {
                f.sync_data().await?;
            } // батч = файл, тоже синкаем
            WalFsyncMode::Disabled => {}
        }

        // ВАЖНО для Windows: закрыть дескриптор перед rename
        drop(f);

        // атомарно переименовываем
        fs::rename(&path_tmp, &path_fin).await?;

        // NEW: записываем сайдкар чексуммы
        let mut rf = fs::File::open(&path_fin).await?;
        let mut data = Vec::new();
        rf.read_to_end(&mut data).await?;
        let sum = xxh3_64(&data);
        fs::write(&path_sum, format!("{sum:016x}")).await?;

        Ok((path_fin.to_string_lossy().to_string(), n))
    }

    // NEW: служебная валидация (необязательная, но удобная для e2e/отладки)
    pub async fn validate_checksum<P: AsRef<Path>>(path: P) -> anyhow::Result<bool> {
        let p = path.as_ref();
        let sum_path = p.with_extension("xxh3");
        let mut rf = fs::File::open(p).await?;
        let mut data = Vec::new();
        rf.read_to_end(&mut data).await?;
        let sum = xxh3_64(&data);
        let want = fs::read_to_string(&sum_path).await?.trim().to_string();
        Ok(format!("{sum:016x}") == want)
    }
}
