use anyhow::{Result, anyhow};
use croaring::Bitmap;
use serde_json::Value;
use std::{
    collections::{BTreeMap, HashMap},
    fs::{File, OpenOptions},
    io::{BufRead, BufReader, Read, Seek, SeekFrom, Write},
    path::Path,
};

use crate::v2::crc::crc64_ecma;
use crate::v2::types::{META_HEADER_LEN, MetaHeader};
use crate::{normalizer::normalize, v2::codec::put_varint_to_writer};
use croaring::Portable;

pub struct BinSegmentWriter;
impl Default for BinSegmentWriter {
    fn default() -> Self {
        Self
    }
}

impl crate::SegmentWriter for BinSegmentWriter {
    fn write_segment(&mut self, input_jsonl: &str, out_dir: &str) -> Result<()> {
        std::fs::create_dir_all(out_dir)?;

        // --- 1) Пройдём jsonl: соберём docs, grams, field_masks ---
        let f = File::open(input_jsonl)?;
        let br = BufReader::new(f);

        let mut doc_count: u32 = 0;
        let mut grams: HashMap<[u8; 3], Vec<u32>> = HashMap::new();
        let mut field_masks: HashMap<String, Bitmap> = HashMap::new();

        for line in br.lines() {
            let line = line?;
            if line.trim().is_empty() {
                continue;
            }
            let v: Value = serde_json::from_str(&line)?;
            // обойдём все строковые поля (как в V1)
            collect_strings("", &v, &mut |path, s| {
                let ns = normalize(s);
                // grams
                for g in crate::gram::trigrams(&ns) {
                    let mut key = [0u8; 3];
                    for (i, b) in g.as_bytes().iter().take(3).enumerate() {
                        key[i] = *b;
                    }
                    grams.entry(key).or_default().push(doc_count);
                }
                // field mask
                field_masks
                    .entry(path.to_string())
                    .or_default()
                    .add(doc_count);
            });

            doc_count += 1;
        }

        // Отсортируем doc_id в листах и удалим дубли (на всякий)
        for v in grams.values_mut() {
            v.sort_unstable();
            v.dedup();
        }

        // --- 2) Подготовим файлы ---
        let meta_path = Path::new(out_dir).join("meta.bin");
        let grams_idx_path = Path::new(out_dir).join("grams.idx");
        let grams_dat_path = Path::new(out_dir).join("grams.dat");
        let fields_idx_path = Path::new(out_dir).join("fields.idx");
        let fields_dat_path = Path::new(out_dir).join("fields.dat");
        let docs_dat_path = Path::new(out_dir).join("docs.dat");

        // grams.dat
        let mut grams_dat = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(true)
            .open(&grams_dat_path)?;
        // Формат: [u8 kind=1][u32 doc_count][varint first][varint delta...], КАЖДАЯ запись подряд,
        // CRC64 будет в конце файла.
        let mut grams_index: Vec<([u8; 3], u64, u64)> = Vec::new();
        // Сортировка ключей по лексикографическому порядку
        let mut keys: Vec<[u8; 3]> = grams.keys().cloned().collect();
        keys.sort_unstable();

        for key in keys {
            let list = grams.get(&key).unwrap();
            let offset = grams_dat.stream_position()?;
            // kind=1
            grams_dat.write_all(&[1u8])?;
            grams_dat.write_all(&(list.len() as u32).to_le_bytes())?;
            if list.is_empty() {
                // пустой список — ок
            } else {
                // первый docid
                put_varint_to_writer(list[0] as u64, &mut grams_dat)?;
                // дельты
                let mut prev = list[0];
                for &d in &list[1..] {
                    let delta = (d - prev) as u64;
                    put_varint_to_writer(delta, &mut grams_dat);
                    prev = d;
                }
            }
            let end = grams_dat.stream_position()?;
            let length = end - offset;
            grams_index.push((key, offset, length));
        }
        // footer CRC64 для grams.dat
        let (grams_dat_body_len, _crc) = finalize_with_crc64(&mut grams_dat)?;

        // grams.idx
        let mut grams_idx = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(true)
            .open(&grams_idx_path)?;
        // header
        grams_idx.write_all(&0x475A4944u32.to_le_bytes())?; // "GZID"
        grams_idx.write_all(&1u16.to_le_bytes())?; // version
        grams_idx.write_all(&0u16.to_le_bytes())?; // flags
        grams_idx.write_all(&(grams_index.len() as u32).to_le_bytes())?;
        grams_idx.write_all(&(24u32).to_le_bytes())?; // record_len (выравненный)
        // records
        for (key, off, len) in &grams_index {
            grams_idx.write_all(key)?;
            grams_idx.write_all(&off.to_le_bytes())?;
            grams_idx.write_all(&len.to_le_bytes())?;
            grams_idx.write_all(&[0u8; 5])?; // pad до 24
        }
        // footer CRC64
        let (grams_idx_body_len, _crc) = finalize_with_crc64(&mut grams_idx)?;

        // fields.dat
        let mut fields_dat = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(true)
            .open(&fields_dat_path)?;
        // name_dict: соберём список имён (детерминированный порядок)
        let mut field_names: Vec<String> = field_masks.keys().cloned().collect();
        field_names.sort(); // FieldId = индекс
        // records для idx: (field_id, off, len)
        let mut field_records: Vec<(u32, u64, u64)> = Vec::new();
        for (fid, name) in field_names.iter().enumerate() {
            let bm = field_masks.get(name).unwrap();
            let off = fields_dat.stream_position()?;
            // kind=1 roaring_stream
            fields_dat.write_all(&[1u8])?;
            let bytes = bm.serialize::<Portable>(); // croaring portable serialize
            fields_dat.write_all(&(bytes.len() as u32).to_le_bytes())?;
            fields_dat.write_all(&bytes)?;
            let end = fields_dat.stream_position()?;
            field_records.push((fid as u32, off, end - off));
        }
        // footer CRC64
        let (fields_dat_body_len, _crc) = finalize_with_crc64(&mut fields_dat)?;

        // fields.idx
        let mut fields_idx = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(true)
            .open(&fields_idx_path)?;
        // header
        fields_idx.write_all(&0x475A4649u32.to_le_bytes())?; // "GZFI"
        fields_idx.write_all(&1u16.to_le_bytes())?; // version
        fields_idx.write_all(&0u16.to_le_bytes())?; // flags
        fields_idx.write_all(&(field_names.len() as u32).to_le_bytes())?;
        // name_dict_len: посчитаем заранее
        let mut name_dict_buf = Vec::new();
        for name in &field_names {
            put_uvar(name.len() as u64, &mut name_dict_buf);
            name_dict_buf.extend_from_slice(name.as_bytes());
        }
        fields_idx.write_all(&(name_dict_buf.len() as u32).to_le_bytes())?;
        // name_dict
        fields_idx.write_all(&name_dict_buf)?;
        // records
        for (fid, off, len) in &field_records {
            fields_idx.write_all(&fid.to_le_bytes())?;
            fields_idx.write_all(&off.to_le_bytes())?;
            fields_idx.write_all(&len.to_le_bytes())?;
        }
        // footer CRC64
        let (fields_idx_body_len, _crc) = finalize_with_crc64(&mut fields_idx)?;

        // docs.dat — пока пустой (только CRC64 пустого тела)

        let mut docs_dat = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(true)
            .open(&docs_dat_path)?;
        let (docs_dat_body_len, _crc) = finalize_with_crc64(&mut docs_dat)?;

        // meta.bin
        let mut hdr = MetaHeader::default();
        hdr.doc_count = doc_count as u64;
        hdr.gram_count = grams_index.len() as u64;
        hdr.grams_idx_len = grams_idx_body_len;
        hdr.grams_dat_len = grams_dat_body_len;
        hdr.fields_idx_len = fields_idx_body_len;
        hdr.fields_dat_len = fields_dat_body_len;
        hdr.docs_dat_len = docs_dat_body_len;
        let mut meta = File::create(&meta_path)?;
        write_meta_with_crc(&mut meta, &hdr)?;
        Ok(())
    }
}

// --- helpers ---

fn write_meta_with_crc(f: &mut File, hdr: &MetaHeader) -> anyhow::Result<()> {
    let mut buf = Vec::with_capacity(META_HEADER_LEN as usize + 8);

    // header (ровно META_HEADER_LEN байт)
    buf.extend_from_slice(&hdr.magic.to_le_bytes());
    buf.extend_from_slice(&hdr.version.to_le_bytes());
    buf.extend_from_slice(&hdr.header_len.to_le_bytes());
    buf.extend_from_slice(&hdr.doc_count.to_le_bytes());
    buf.extend_from_slice(&hdr.gram_count.to_le_bytes());
    buf.extend_from_slice(&hdr.grams_idx_len.to_le_bytes());
    buf.extend_from_slice(&hdr.grams_dat_len.to_le_bytes());
    buf.extend_from_slice(&hdr.fields_idx_len.to_le_bytes());
    buf.extend_from_slice(&hdr.fields_dat_len.to_le_bytes());
    buf.extend_from_slice(&hdr.docs_dat_len.to_le_bytes());
    while buf.len() < META_HEADER_LEN as usize {
        buf.push(0);
    }

    // footer CRC64 по всему header’у
    let crc = crate::v2::crc::crc64_ecma(&buf);
    buf.extend_from_slice(&crc.to_le_bytes());

    // записать ВСЁ
    use std::io::Write;
    f.write_all(&buf)?;
    Ok(())
}

fn file_len_without_crc(p: &std::path::Path) -> Result<u64> {
    let md = std::fs::metadata(p)?;
    let sz = md.len();
    Ok(if sz >= 8 { sz - 8 } else { 0 })
}

fn put_uvar(x: u64, out: &mut Vec<u8>) {
    crate::v2::codec::put_varint(x, out)
}

fn collect_strings(path: &str, v: &serde_json::Value, f: &mut impl FnMut(&str, &str)) {
    match v {
        Value::String(s) => f(path, s),
        Value::Object(map) => {
            for (k, vv) in map {
                let np = if path.is_empty() {
                    k.clone()
                } else {
                    format!("{path}.{k}")
                };
                collect_strings(&np, vv, f);
            }
        }
        Value::Array(arr) => {
            for (i, vv) in arr.iter().enumerate() {
                let np = if path.is_empty() {
                    format!("{}[{i}]", path)
                } else {
                    format!("{path}[{i}]")
                };
                collect_strings(&np, vv, f);
            }
        }
        _ => {}
    }
}

fn write_crc64_footer(f: &mut File) -> anyhow::Result<()> {
    use std::io::{Read, Seek, SeekFrom, Write};
    let end = f.stream_position()?;
    f.seek(SeekFrom::Start(0))?;
    let mut buf = Vec::with_capacity(end as usize);
    f.read_to_end(&mut buf)?;
    let crc = crate::v2::crc::crc64_ecma(&buf);
    f.seek(SeekFrom::End(0))?;
    f.write_all(&crc.to_le_bytes())?;
    Ok(())
}

fn finalize_with_crc64(f: &mut std::fs::File) -> anyhow::Result<(u64 /*body_len*/, u64 /*crc*/)> {
    use std::io::{Read, Seek, SeekFrom, Write};
    // длина тела ДО футера
    let body_len = f.stream_position()?; // где сейчас находится курсор
    f.seek(SeekFrom::Start(0))?;
    let mut buf = Vec::with_capacity(body_len as usize);
    f.read_to_end(&mut buf)?;
    let crc = crate::v2::crc::crc64_ecma(&buf);
    f.seek(SeekFrom::End(0))?;
    f.write_all(&crc.to_le_bytes())?;
    Ok((body_len, crc))
}
