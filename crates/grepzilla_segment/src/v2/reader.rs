// crates/grepzilla_segment/src/v2/reader.rs
use anyhow::{Result, anyhow, bail};
use croaring::{Bitmap, Portable};
use memmap2::Mmap;
use once_cell::sync::OnceCell;
use std::{
    collections::{BTreeMap, HashMap},
    fs::File,
    io::{Cursor, Read},
    path::{Path, PathBuf},
};

use crate::gram::BooleanOp;
use crate::v2::crc::crc64_ecma;
use crate::v2::types::{META_HEADER_LEN, META_MAGIC, META_VERSION};
use crate::{SegmentReader, StoredDoc}; // StoredDoc теперь используем

pub struct BinSegmentReader {
    _meta_mmap: Mmap,
    grams_idx: Mmap,
    grams_dat: Mmap,
    fields_idx: Mmap,
    fields_dat: Mmap,

    // --- V2 docs.dat ---
    docs_dat: Mmap,
    docs_offsets_start: u64,
    docs_payload_start: u64,
    docs_cells: Vec<OnceCell<StoredDoc>>, // ленивый кеш по doc_id

    doc_count: u32,
    field_offsets: HashMap<String, (u64, u64)>, // field_name -> (off,len) в fields.dat
    field_names_by_id: Vec<String>,             // индекс = field_id
}

impl SegmentReader for BinSegmentReader {
    fn open_segment(path: &str) -> Result<Self> {
        let base = Path::new(path);

        // meta.bin
        let meta_p = base.join("meta.bin");
        let meta_f = File::open(&meta_p)?;
        let meta_m = unsafe { Mmap::map(&meta_f)? };
        if meta_m.len() < (META_HEADER_LEN as usize + 8) {
            bail!("meta.bin too small");
        }
        let magic = u32::from_le_bytes(meta_m[0..4].try_into().unwrap());
        let version = u16::from_le_bytes(meta_m[4..6].try_into().unwrap());
        if magic != META_MAGIC || version != META_VERSION {
            bail!("not a V2 segment (magic/version mismatch)");
        }
        // CRC64 по телу без последнего u64
        let file_len = meta_m.len();
        if file_len < (META_HEADER_LEN as usize + 8) {
            bail!("meta.bin too small");
        }
        let body = &meta_m[..file_len - 8];
        let crc_expect = u64::from_le_bytes(meta_m[file_len - 8..].try_into().unwrap());
        let crc_calc = crc64_ecma(body);
        if crc_calc != crc_expect {
            bail!("meta.bin CRC mismatch");
        }

        let mut u64buf = [0u8; 8];
        u64buf.copy_from_slice(&meta_m[8..16]); // hdr.doc_count (u64)
        let doc_count = u64::from_le_bytes(u64buf) as u32;

        // grams.idx/dat
        let grams_idx_m = mmap_with_crc(base.join("grams.idx"))?;
        let grams_dat_m = mmap_with_crc(base.join("grams.dat"))?;

        // fields.idx/dat
        let fields_idx_m = mmap_with_crc(base.join("fields.idx"))?;
        let fields_dat_m = mmap_with_crc(base.join("fields.dat"))?;

        // распарсим fields.idx → (словарь оффсетов по имени, список имён по field_id)
        let (field_offsets, field_names_by_id) = parse_fields_index(&fields_idx_m)?;

        // --- docs.dat ---
        let docs_dat_m = mmap_with_crc(base.join("docs.dat"))?;
        if docs_dat_m.len() < 8 + 8 + 8 + 8 {
            bail!("docs.dat too small");
        }
        let dd_body = &docs_dat_m[..docs_dat_m.len() - 8]; // без CRC
        if &dd_body[0..8] != b"GZDOCS2\0" {
            bail!("docs.dat bad magic");
        }
        // DOC_COUNT
        let mut b8 = [0u8; 8];
        b8.copy_from_slice(&dd_body[8..16]);
        let docs_doc_count = u64::from_le_bytes(b8) as u32;
        if docs_doc_count != doc_count {
            bail!("docs.dat doc_count mismatch (meta vs docs.dat)");
        }
        // OFFSETS_COUNT = doc_count + 1
        b8.copy_from_slice(&dd_body[16..24]);
        let offsets_count = u64::from_le_bytes(b8);
        if offsets_count != (docs_doc_count as u64) + 1 {
            bail!("docs.dat offsets_count mismatch");
        }
        let docs_offsets_start = 24u64;
        let docs_payload_start = docs_offsets_start + offsets_count * 8;

        // инициализируем OnceCell на каждый документ (стабильная длина)
        let docs_cells = (0..doc_count as usize).map(|_| OnceCell::new()).collect();

        Ok(Self {
            _meta_mmap: meta_m,
            grams_idx: grams_idx_m,
            grams_dat: grams_dat_m,
            fields_idx: fields_idx_m,
            fields_dat: fields_dat_m,

            docs_dat: docs_dat_m,
            docs_offsets_start,
            docs_payload_start,
            docs_cells,

            doc_count,
            field_offsets,
            field_names_by_id,
        })
    }

    fn doc_count(&self) -> u32 {
        self.doc_count
    }

    fn prefilter(&self, op: BooleanOp, grams: &[String], field: Option<&str>) -> Result<Bitmap> {
        use BooleanOp::*;

        // Если не нашлось ни одной валидной 3-граммы — считаем «все документы»,
        // а затем пересекаем с маской поля (если задана).
        if grams.iter().all(|g| g.as_bytes().len() < 3) {
            let mut all = Bitmap::new();
            if self.doc_count > 0 {
                all.add_range(0..self.doc_count);
            }
            if let Some(fname) = field {
                if let Some((off, len)) = self.field_offsets.get(fname).copied() {
                    let mask = read_field_bitmap(&self.fields_dat, off, len)?;
                    all.and_inplace(&mask);
                } else {
                    all.clear();
                }
            }
            return Ok(all);
        }

        let mut vec_bm: Vec<Bitmap> = Vec::new();

        for g in grams {
            if g.as_bytes().len() < 3 {
                continue;
            }
            let key = [g.as_bytes()[0], g.as_bytes()[1], g.as_bytes()[2]];
            if let Some((off, len)) = lookup_gram(&self.grams_idx, key)? {
                let bm = read_postings(&self.grams_dat, off, len)?;
                vec_bm.push(bm);
            } else {
                // нет такой 3-граммы — для AND это обнуляет всё
                match op {
                    And => return Ok(Bitmap::new()),
                    Or | Not => {} // ничего
                }
            }
        }

        let mut acc = match op {
            And => {
                let mut it = vec_bm.into_iter();
                if let Some(mut first) = it.next() {
                    for bm in it {
                        first.and_inplace(&bm);
                    }
                    first
                } else {
                    Bitmap::new()
                }
            }
            Or => {
                let mut out = Bitmap::new();
                for bm in vec_bm {
                    out.or_inplace(&bm);
                }
                out
            }
            Not => {
                // Вселенная: [0..doc_count)
                let mut all = Bitmap::new();
                if self.doc_count > 0 {
                    all.add_range(0..self.doc_count);
                }
                for bm in vec_bm {
                    all.andnot_inplace(&bm);
                }
                all
            }
        };

        // Маска поля (если задана)
        if let Some(fname) = field {
            if let Some((off, len)) = self.field_offsets.get(fname).copied() {
                let mask = read_field_bitmap(&self.fields_dat, off, len)?;
                acc.and_inplace(&mask);
            } else {
                acc.clear();
            }
        }
        Ok(acc)
    }

    fn get_doc(&self, doc_id: u32) -> Option<&StoredDoc> {
        if doc_id >= self.doc_count {
            return None;
        }
        if let Some(doc) = self.docs_cells[doc_id as usize].get() {
            return Some(doc);
        }
        let (from, to) = self.doc_bounds(doc_id)?;
        let parsed = match self.parse_doc(doc_id, from, to) {
            Ok(d) => d,
            Err(_) => return None,
        };
        let _ = self.docs_cells[doc_id as usize].set(parsed);
        self.docs_cells[doc_id as usize].get()
    }
}

// --- docs.dat helpers ---

impl BinSegmentReader {
    /// Синхронный прогрев OnceCell по списку doc_id.
    pub fn prefetch_docs<I: IntoIterator<Item = u32>>(&self, ids: I) {
        for id in ids {
            let _ = self.get_doc(id);
        }
    }

    #[inline]
    fn docs_offsets_slice(&self) -> &[u8] {
        &self.docs_dat[self.docs_offsets_start as usize..self.docs_payload_start as usize]
    }

    #[inline]
    fn docs_payload_slice(&self) -> &[u8] {
        &self.docs_dat[self.docs_payload_start as usize..self.docs_dat.len() - 8] // без CRC
    }

    fn doc_bounds(&self, doc_id: u32) -> Option<(usize, usize)> {
        if doc_id >= self.doc_count {
            return None;
        }
        let offs = self.docs_offsets_slice();
        let i = doc_id as usize;
        let from = u64::from_le_bytes(offs[i * 8..i * 8 + 8].try_into().ok()?) as usize;
        let to = u64::from_le_bytes(offs[(i + 1) * 8..(i + 1) * 8 + 8].try_into().ok()?) as usize;
        Some((from, to))
    }

    fn parse_doc(&self, doc_id: u32, from: usize, to: usize) -> Result<StoredDoc> {
        let payload = &self.docs_payload_slice()[from..to];
        let mut p = 0usize;

        // ext_id
        let (ext_len, adv1) = get_uvar_u64(&payload[p..])?;
        p += adv1;
        let ext_end = p + (ext_len as usize);
        if ext_end > payload.len() {
            bail!("docs.dat ext_id OOB");
        }
        let ext_id = std::str::from_utf8(&payload[p..ext_end])?.to_string();
        p = ext_end;

        // fields_len
        let (fl, adv2) = get_uvar_u64(&payload[p..])?;
        p += adv2;

        let mut fields: BTreeMap<String, String> = BTreeMap::new();
        for _ in 0..fl {
            let (fid, a1) = get_uvar_u64(&payload[p..])?;
            p += a1;
            let (slen, a2) = get_uvar_u64(&payload[p..])?;
            p += a2;
            let s_end = p + (slen as usize);
            if s_end > payload.len() {
                bail!("docs.dat field string OOB");
            }
            let s = std::str::from_utf8(&payload[p..s_end])?.to_string();
            p = s_end;
            if let Some(name) = self.field_names_by_id.get(fid as usize) {
                fields.insert(name.clone(), s);
            }
        }

        Ok(StoredDoc {
            doc_id,
            ext_id,
            fields,
        })
    }
}

// --- helpers (без изменений снизу, кроме использования выше) ---

fn mmap_with_crc(p: PathBuf) -> Result<Mmap> {
    let f = File::open(&p)?;
    let m = unsafe { Mmap::map(&f)? };
    if m.len() < 8 {
        bail!("file too small: {}", p.display());
    }
    let crc_expect = u64::from_le_bytes(m[m.len() - 8..].try_into().unwrap());
    let crc_calc = crc64_ecma(&m[..m.len() - 8]);
    if crc_expect != crc_calc {
        bail!("CRC64 mismatch: {}", p.display());
    }
    Ok(m)
}

fn parse_fields_index(idx: &Mmap) -> Result<(HashMap<String, (u64, u64)>, Vec<String>)> {
    if idx.len() < 4 + 2 + 2 + 4 + 4 + 8 {
        bail!("fields.idx too small");
    }
    let magic = u32::from_le_bytes(idx[0..4].try_into().unwrap());
    if magic != 0x475A4649 {
        bail!("fields.idx bad magic");
    }
    let field_count = u32::from_le_bytes(idx[8..12].try_into().unwrap()) as usize;
    let name_dict_len = u32::from_le_bytes(idx[12..16].try_into().unwrap()) as usize;

    // name_dict
    let mut p = 16usize;
    let end = p + name_dict_len;
    let mut names: Vec<String> = Vec::with_capacity(field_count);
    for _ in 0..field_count {
        let (len, adv) = get_uvar(&idx[p..])?;
        p += adv;
        let s = std::str::from_utf8(&idx[p..p + len])?.to_string();
        p += len;
        names.push(s);
    }
    // records
    let mut map = HashMap::new();
    for fid in 0..field_count {
        let base = end + fid * (4 + 8 + 8);
        let id = u32::from_le_bytes(idx[base..base + 4].try_into().unwrap()) as usize;
        let off = u64::from_le_bytes(idx[base + 4..base + 12].try_into().unwrap());
        let len = u64::from_le_bytes(idx[base + 12..base + 20].try_into().unwrap());
        let name = names
            .get(id)
            .ok_or_else(|| anyhow!("bad field id"))?
            .clone();
        map.insert(name, (off, len));
    }
    Ok((map, names))
}

fn get_uvar(bytes: &[u8]) -> Result<(usize, usize)> {
    let mut shift = 0u32;
    let mut val = 0usize;
    for (i, b) in bytes.iter().enumerate() {
        let v = (*b & 0x7F) as usize;
        val |= v << shift;
        if (*b & 0x80) == 0 {
            return Ok((val, i + 1));
        }
        shift += 7;
        if shift > 63 {
            bail!("varint too long");
        }
    }
    bail!("truncated varint")
}

fn lookup_gram(idx: &Mmap, key: [u8; 3]) -> Result<Option<(u64, u64)>> {
    // header: magic(4) ver(2) flags(2) count(u32) record_len(u32=24)
    if idx.len() < 4 + 2 + 2 + 4 + 4 + 8 {
        bail!("grams.idx too small");
    }
    let count = u32::from_le_bytes(idx[8..12].try_into().unwrap()) as usize;
    let rec_len = u32::from_le_bytes(idx[12..16].try_into().unwrap()) as usize;
    let base = 16usize;
    // бинарный поиск
    let mut lo = 0isize;
    let mut hi = count as isize - 1;
    while lo <= hi {
        let mid = (lo + hi) >> 1;
        let off = base + (mid as usize) * rec_len;
        let k = &idx[off..off + 3];
        match k.cmp(&key) {
            std::cmp::Ordering::Less => lo = mid + 1,
            std::cmp::Ordering::Greater => hi = mid - 1,
            std::cmp::Ordering::Equal => {
                let off64 = u64::from_le_bytes(idx[off + 3..off + 11].try_into().unwrap());
                let len64 = u64::from_le_bytes(idx[off + 11..off + 19].try_into().unwrap());
                return Ok(Some((off64, len64)));
            }
        }
    }
    Ok(None)
}

fn read_postings(dat: &Mmap, off: u64, len: u64) -> Result<Bitmap> {
    let start = off as usize;
    let end = (off + len) as usize;
    let body = &dat[start..end]; // без CRC64 — смещения/длины от meta уже учтены без CRC
    if body.len() < 1 + 4 {
        bail!("postings too small");
    }
    let kind = body[0];
    let doc_cnt = u32::from_le_bytes(body[1..5].try_into().unwrap()) as usize;
    match kind {
        1 => {
            // inline varints
            let mut ids: Vec<u32> = Vec::with_capacity(doc_cnt);
            let mut p = 5usize;
            if doc_cnt > 0 {
                let (first, adv) = get_uvar_u64(&body[p..])?;
                p += adv;
                ids.push(first as u32);
                let mut prev = first as u32;
                for _ in 1..doc_cnt {
                    let (d, adv2) = get_uvar_u64(&body[p..])?;
                    p += adv2;
                    prev = prev + (d as u32);
                    ids.push(prev);
                }
            }
            let mut bm = Bitmap::new();
            for id in ids {
                bm.add(id);
            }
            Ok(bm)
        }
        2 => bail!("block codec not implemented yet"),
        _ => bail!("unknown postings kind {}", kind),
    }
}

fn get_uvar_u64(bytes: &[u8]) -> Result<(u64, usize)> {
    let mut shift = 0u32;
    let mut val = 0u64;
    for (i, b) in bytes.iter().enumerate() {
        val |= ((b & 0x7F) as u64) << shift;
        if (b & 0x80) == 0 {
            return Ok((val, i + 1));
        }
        shift += 7;
        if shift > 63 {
            bail!("varint too long");
        }
    }
    bail!("truncated varint")
}

fn read_field_bitmap(dat: &Mmap, off: u64, len: u64) -> Result<Bitmap> {
    let start = off as usize;
    let end = (off + len) as usize;
    let body = &dat[start..end];
    if body.is_empty() {
        return Ok(Bitmap::new());
    }
    match body[0] {
        1 => {
            // roaring_stream
            if body.len() < 1 + 4 {
                bail!("fields roaring too small");
            }
            let payload_len = u32::from_le_bytes(body[1..5].try_into().unwrap()) as usize;
            let payload = &body[5..5 + payload_len];
            Ok(Bitmap::deserialize::<Portable>(payload))
        }
        2 => {
            // tiny_set (не пишем пока; оставлено на будущее)
            bail!("tiny_set not implemented in reader")
        }
        k => bail!("unknown field bitmap kind {}", k),
    }
}
