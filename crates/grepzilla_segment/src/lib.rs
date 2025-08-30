pub mod gram;
pub mod normalizer;
pub mod segjson;

use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

pub mod common;
pub mod cursor;
pub mod manifest;
pub mod manifest_store;
pub mod search;
pub mod v2;

/// Внешняя модель документа при ingest
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InputDoc(serde_json::Value);

/// Витрина документа, хранится в сегменте для превью/верификации
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StoredDoc {
    pub doc_id: u32,                      // локальный id в сегменте
    pub ext_id: String,                   // внешний _id
    pub fields: BTreeMap<String, String>, // только строковые поля, уже нормализованные
}

/// Метаданные сегмента (минимум для MVP)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SegmentMetaV1 {
    pub version: u32, // 1
    pub doc_count: u32,
    pub gram_count: u32,
}

/// Точки расширения: читатель/писатель сегмента
pub trait SegmentWriter {
    fn write_segment(&mut self, input_jsonl: &str, out_dir: &str) -> Result<()>;
}

pub trait SegmentReader {
    fn open_segment(path: &str) -> Result<Self>
    where
        Self: Sized;
    fn doc_count(&self) -> u32;
    /// Вернуть кандидатов по обязательным 3-граммам с учётом логики AND/OR/NOT.
    fn prefilter(
        &self,
        op: gram::BooleanOp,
        grams: &[String],
        field: Option<&str>,
    ) -> anyhow::Result<croaring::Bitmap>;
    /// Вытащить документ
    fn get_doc(&self, doc_id: u32) -> Option<&StoredDoc>;
}
