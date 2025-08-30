// use std::fs::OpenOptions;
// use std::io::{Seek, SeekFrom, Write, Read};
// use std::path::Path;

// use crate::common::strings::collect_strings;
// use crate::v2::varint::{write_uvarint};
// use serde_json::Value;

// const MAGIC: &[u8;8] = b"GZDOCS2\0";

// pub struct V2DocsWriter {
//     /// Буфер доков для последующей записи
//     docs: Vec<DocBuf>,
//     /// Словарь: имя поля -> field_id (должен совпасть с fields.dat)
//     pub field_dict: Box<dyn Fn(&str) -> Option<u32> + Send>,
// }

// struct DocBuf {
//     ext_id: String,
//     fields: Vec<(u32, String)>, // отсортированы по field_id
// }

// impl V2DocsWriter {
//     pub fn new<F>(field_dict_lookup: F) -> Self
//     where
//         F: Fn(&str)->Option<u32> + Send + 'static
//     {
//         Self {
//             docs: Vec::new(),
//             field_dict: Box::new(field_dict_lookup),
//         }
//     }

//     pub fn add_doc_from_json(&mut self, json: &Value) -> anyhow::Result<()> {
//         let ext_id = json.get("_id")
//             .and_then(|v| v.as_str())
//             .ok_or_else(|| anyhow::anyhow!("_id missing or not string"))?
//             .to_string();

//         let mut collected = collect_strings(json);
//         // маппим имена в field_id
//         let mut fields: Vec<(u32, String)> = Vec::with_capacity(collected.len());
//         for (name, val) in collected.drain(..) {
//             if let Some(fid) = (self.field_dict)(&name) {
//                 fields.push((fid, val));
//             }
//         }
//         fields.sort_by_key(|(fid, _)| *fid);

//         self.docs.push(DocBuf { ext_id, fields });
//         Ok(())
//     }

//     pub fn flush_docs_dat(&mut self, out_dir: &Path, crc64_fn: fn(&[u8])->u64) -> anyhow::Result<()> {
//         let path = out_dir.join("docs.dat");
//         let mut f = OpenOptions::new().read(true).write(true).create(true).truncate(true).open(&path)?;

//         // header
//         f.write_all(MAGIC)?;
//         let doc_count = self.docs.len() as u64;
//         f.write_all(&doc_count.to_le_bytes())?;
//         let offsets_count = doc_count + 1;
//         f.write_all(&offsets_count.to_le_bytes())?;

//         // offsets placeholder
//         let offsets_pos = f.stream_position()?;
//         for _ in 0..offsets_count {
//             f.write_all(&0u64.to_le_bytes())?;
//         }

//         // payload start
//         let payload_start = f.stream_position()?;
//         let mut rel_offsets: Vec<u64> = Vec::with_capacity(self.docs.len()+1);

//         for doc in &self.docs {
//             let start = f.stream_position()?; // абсолютный
//             rel_offsets.push(start - payload_start);

//             // ext_id
//             write_uvarint(&mut f, doc.ext_id.len() as u64)?;
//             f.write_all(doc.ext_id.as_bytes())?;

//             // fields
//             write_uvarint(&mut f, doc.fields.len() as u64)?;
//             for (fid, s) in &doc.fields {
//                 write_uvarint(&mut f, *fid as u64)?;
//                 write_uvarint(&mut f, s.len() as u64)?;
//                 f.write_all(s.as_bytes())?;
//             }
//         }

//         // last guard offset
//         let end = f.stream_position()?;
//         rel_offsets.push(end - payload_start);

//         // write real offsets
//         f.seek(SeekFrom::Start(offsets_pos))?;
//         for o in &rel_offsets {
//             f.write_all(&o.to_le_bytes())?;
//         }

//         // CRC64 footer (по всему телу до футера)
//         f.seek(SeekFrom::Start(0))?;
//         let mut buf = Vec::with_capacity(end as usize);
//         f.take(end).read_to_end(&mut buf)?;
//         let crc = crc64_fn(&buf);
//         f.seek(SeekFrom::Start(end))?;
//         f.write_all(&crc.to_le_bytes())?;
//         f.flush()?;

//         Ok(())
//     }
// }
