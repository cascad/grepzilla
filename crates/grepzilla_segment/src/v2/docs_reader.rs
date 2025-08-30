// use std::fs::File;
// use std::io::{Cursor, Read};
// use std::path::Path;

// use lru::LruCache;
// use memmap2::Mmap;
// use std::num::NonZeroUsize;

// use crate::v2::varint::read_uvarint;
// use crate::v2::types::StoredDoc;

// const MAGIC: &[u8;8] = b"GZDOCS2\0";

// pub struct V2DocsReader {
//     mmap: Mmap,
//     doc_count: u64,
//     offsets_count: u64,
//     offsets_start: u64,
//     payload_start: u64,
//     lru: LruCache<u32, StoredDoc>,
// }

// #[derive(thiserror::Error, Debug)]
// pub enum DocsError {
//     #[error("bad magic")]
//     BadMagic,
//     #[error("crc mismatch")]
//     BadCrc,
//     #[error("doc_id out of range")]
//     OutOfRange,
//     #[error("io: {0}")]
//     Io(#[from] std::io::Error),
// }

// impl V2DocsReader {
//     pub fn open(dir: &Path, crc64_fn: fn(&[u8])->u64) -> Result<Self, DocsError> {
//         let path = dir.join("docs.dat");
//         let f = File::open(&path)?;
//         let mmap = unsafe { Mmap::map(&f)? };

//         // проверим CRC64 футер
//         if mmap.len() < 8 { return Err(DocsError::BadCrc); }
//         let body = &mmap[..mmap.len()-8];
//         let crc_bytes = &mmap[mmap.len()-8..];
//         let want = crc64_fn(body);
//         let got = u64::from_le_bytes(crc_bytes.try_into().unwrap());
//         if want != got { return Err(DocsError::BadCrc); }

//         // парсим заголовок
//         if body.len() < 8+8+8 { return Err(DocsError::BadMagic); }
//         if &body[0..8] != MAGIC { return Err(DocsError::BadMagic); }
//         let doc_count = u64::from_le_bytes(body[8..16].try_into().unwrap());
//         let offsets_count = u64::from_le_bytes(body[16..24].try_into().unwrap());
//         let offsets_start = 24u64;
//         let offsets_bytes = offsets_count.checked_mul(8).ok_or(DocsError::BadMagic)?;
//         let payload_start = offsets_start + offsets_bytes;

//         Ok(Self {
//             mmap,
//             doc_count,
//             offsets_count,
//             offsets_start,
//             payload_start,
//             lru: LruCache::new(NonZeroUsize::new(128).unwrap()),
//         })
//     }

//     #[inline]
//     pub fn doc_count(&self) -> u32 {
//         self.doc_count as u32
//     }

//     #[inline]
//     fn offsets_slice(&self) -> &[u8] {
//         &self.mmap[self.offsets_start as usize .. (self.payload_start as usize)]
//     }

//     #[inline]
//     fn payload_slice(&self) -> &[u8] {
//         &self.mmap[self.payload_start as usize .. self.mmap.len()-8] // без CRC
//     }

//     fn get_bounds(&self, doc_id: u32) -> Result<(usize, usize), DocsError> {
//         if (doc_id as u64) >= self.doc_count { return Err(DocsError::OutOfRange); }
//         let offs = self.offsets_slice();
//         let i = doc_id as usize;
//         let from = u64::from_le_bytes(offs[i*8..i*8+8].try_into().unwrap()) as usize;
//         let to   = u64::from_le_bytes(offs[(i+1)*8..(i+1)*8+8].try_into().unwrap()) as usize;
//         Ok((from, to))
//     }

//     pub fn get_doc<'a>(&'a mut self, doc_id: u32) -> Result<&'a StoredDoc, DocsError> {
//         if let Some(doc) = self.lru.get(&doc_id) {
//             // SAFETY: lru returns &StoredDoc with cache lifetime; need to reborrow
//             // Trick: LruCache::get returns &V; we can do a second lookup to satisfy borrow checker
//         }
//         if self.lru.contains(&doc_id) {
//             // второй вызов гарантирует живую ссылку
//             return Ok(self.lru.get(&doc_id).unwrap());
//         }

//         let (from, to) = self.get_bounds(doc_id)?;
//         let payload = &self.payload_slice()[from..to];
//         let mut cur = Cursor::new(payload);

//         // ext_id
//         let ext_len = read_uvarint(&mut cur)? as usize;
//         let mut ext = vec![0u8; ext_len];
//         cur.read_exact(&mut ext)?;
//         let ext_id = String::from_utf8(ext).map_err(|_| std::io::ErrorKind::InvalidData)?;

//         // fields
//         let fields_len = read_uvarint(&mut cur)? as usize;
//         let mut fields = Vec::with_capacity(fields_len);
//         for _ in 0..fields_len {
//             let fid = read_uvarint(&mut cur)? as u32;
//             let slen = read_uvarint(&mut cur)? as usize;
//             let mut sb = vec![0u8; slen];
//             cur.read_exact(&mut sb)?;
//             let s = String::from_utf8(sb).map_err(|_| std::io::ErrorKind::InvalidData)?;
//             fields.push((fid, s));
//         }

//         let doc = StoredDoc { ext_id, fields };
//         self.lru.push(doc_id, doc);
//         Ok(self.lru.get(&doc_id).unwrap())
//     }
// }
