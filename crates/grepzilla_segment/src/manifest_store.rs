use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use anyhow::{Result, bail};

use crate::manifest::ManifestPtr;

/// Абстракция над хранилищем указателя на текущий манифест (в проде — etcd)
pub trait ManifestStore: Send + Sync + 'static {
    fn get_ptr(&self, shard: u64) -> Result<ManifestPtr>;
    /// CAS по ожидаемому gen: меняет pointer на new_ptr только если текущий gen == expected_gen
    fn cas_ptr(&self, shard: u64, expected_gen: u64, new_ptr: &ManifestPtr) -> Result<()>;
}

/// In-memory реализация для dev/test
#[derive(Clone, Default)]
pub struct InMemoryManifestStore {
    inner: Arc<Mutex<HashMap<u64, ManifestPtr>>>,
}

impl InMemoryManifestStore {
    pub fn new() -> Self {
        Self {
            inner: Arc::new(Mutex::new(HashMap::new())),
        }
    }
    pub fn insert(&self, shard: u64, ptr: ManifestPtr) {
        self.inner.lock().unwrap().insert(shard, ptr);
    }
}

impl ManifestStore for InMemoryManifestStore {
    fn get_ptr(&self, shard: u64) -> Result<ManifestPtr> {
        self.inner
            .lock()
            .unwrap()
            .get(&shard)
            .cloned()
            .ok_or_else(|| anyhow::anyhow!("manifest_ptr not found for shard {}", shard))
    }

    fn cas_ptr(&self, shard: u64, expected_gen: u64, new_ptr: &ManifestPtr) -> Result<()> {
        let mut g = self.inner.lock().unwrap();
        match g.get(&shard) {
            Some(cur) if cur.r#gen == expected_gen => {
                g.insert(shard, new_ptr.clone());
                Ok(())
            }
            Some(cur) => {
                bail!(
                    "CAS failed: current gen={}, expected {}",
                    cur.r#gen,
                    expected_gen
                )
            }
            None => {
                bail!("CAS failed: no current pointer for shard {}", shard)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::manifest::ManifestPtr;

    fn sample(r#gen: u64) -> ManifestPtr {
        ManifestPtr {
            epoch: 1,
            r#gen,
            url: format!("mem://m/{}", r#gen),
            checksum: "sha256:x".into(),
            updated_at: "2025-08-20T12:00:00Z".into(),
        }
    }

    #[test]
    fn get_and_cas_ok() {
        let store = InMemoryManifestStore::new();
        store.insert(7, sample(1));
        let cur = store.get_ptr(7).unwrap();
        assert_eq!(cur.r#gen, 1);
        store.cas_ptr(7, 1, &sample(2)).unwrap();
        let cur2 = store.get_ptr(7).unwrap();
        assert_eq!(cur2.r#gen, 2);
    }

    #[test]
    fn cas_fails_on_wrong_gen() {
        let store = InMemoryManifestStore::new();
        store.insert(7, sample(5));
        let err = store.cas_ptr(7, 4, &sample(6)).unwrap_err();
        let msg = format!("{}", err);
        assert!(msg.contains("CAS failed"));
    }
}
