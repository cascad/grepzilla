// crates/grepzilla_segment/tests/v2_docs_roundtrip.rs

use std::fs::{self, File};
use std::io::Write;
use std::path::PathBuf;

use grepzilla_segment::SegmentReader as _;
use grepzilla_segment::SegmentWriter as _;

use grepzilla_segment::v2::reader::BinSegmentReader;
use grepzilla_segment::v2::writer::BinSegmentWriter;

#[test]
fn v2_docs_roundtrip_and_crc() {
    // tmp dirs/files
    let tmp = tempfile::tempdir().unwrap();
    let seg_dir = tmp.path().join("seg_v2");
    let jsonl_path = tmp.path().join("in.jsonl");

    // фикстура JSONL
    let mut jf = File::create(&jsonl_path).unwrap();
    writeln!(
        jf,
        r#"{{"_id":"A1","title":"Hello","text":{{"body":"World"}}}}"#
    )
    .unwrap();
    writeln!(
        jf,
        r#"{{"_id":"B2","title":"Привет","text":{{"body":"мир 🌍"}}}}"#
    )
    .unwrap();
    writeln!(
        jf,
        r#"{{"_id":"C3","notes":"{}"}}"#,
        "X".repeat(8192)
    )
    .unwrap();

    // build сегмент V2
    let mut w = BinSegmentWriter::default();
    w.write_segment(jsonl_path.to_str().unwrap(), seg_dir.to_str().unwrap())
        .expect("writer failed");

    // читаем сегмент
    let r = BinSegmentReader::open_segment(seg_dir.to_str().unwrap()).expect("open_segment");
    assert_eq!(r.doc_count(), 3);

    // d0
    let d0 = r.get_doc(0).expect("doc 0");
    assert_eq!(d0.doc_id, 0);
    assert_eq!(d0.ext_id, "A1");
    assert!(d0.fields.contains_key("title"));
    assert!(d0.fields.contains_key("text.body"));
    // нормализация (как в V1): допускаем lowercased содержимое
    assert!(d0.fields["title"].contains("hello"));
    assert!(d0.fields["text.body"].to_lowercase().contains("world"));

    // d1
    let d1 = r.get_doc(1).expect("doc 1");
    assert_eq!(d1.doc_id, 1);
    assert_eq!(d1.ext_id, "B2");
    assert!(d1.fields["title"].contains("привет"));
    // Не навязываем точной нормализации эмодзи; строка должна содержать "мир"
    assert!(d1.fields["text.body"].to_lowercase().contains("мир"));

    // d2 (длинная строка)
    let d2 = r.get_doc(2).expect("doc 2");
    assert_eq!(d2.doc_id, 2);
    assert_eq!(d2.ext_id, "C3");
    assert!(d2.fields.contains_key("notes"));
    assert!(d2.fields["notes"].len() >= 8192);

    // --- CRC negative test: портим 1 байт футера docs.dat и проверяем отказ ---
    let docs_path = seg_dir.join("docs.dat");
    flip_last_byte(&docs_path);
    let bad = BinSegmentReader::open_segment(seg_dir.to_str().unwrap());
    assert!(bad.is_err(), "open must fail on corrupted docs.dat CRC");
}

fn flip_last_byte(path: &PathBuf) {
    use std::io::{Read, Seek, SeekFrom};
    let mut f = std::fs::OpenOptions::new().read(true).write(true).open(path).unwrap();
    let len = f.metadata().unwrap().len();
    assert!(len >= 1);
    f.seek(SeekFrom::Start(len - 1)).unwrap();
    let mut b = [0u8; 1];
    f.read_exact(&mut b).unwrap();
    b[0] ^= 0xFF;
    f.seek(SeekFrom::Start(len - 1)).unwrap();
    f.write_all(&b).unwrap();
}
