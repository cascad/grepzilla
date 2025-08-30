use grepzilla_segment::gram::{BooleanOp, required_grams_from_wildcard};
use grepzilla_segment::v2::reader::BinSegmentReader;
use grepzilla_segment::v2::writer::BinSegmentWriter;
use grepzilla_segment::{SegmentReader, SegmentWriter};
use std::fs::{self, File};
use std::io::Write;
use tempfile::TempDir;

#[test]
fn v2_prefilter_and_field_mask_roundtrip() {
    // 1) tmp + jsonl с двумя документами, оба матчятся по "*игра*"
    let tmp = TempDir::new().unwrap();
    let segdir = tmp.path().join("segV2");
    fs::create_dir_all(&segdir).unwrap();

    let jsonl = tmp.path().join("in.jsonl");
    let mut f = File::create(&jsonl).unwrap();
    writeln!(
        f,
        r#"{{"_id":"1","text":{{"body":"котенок играет с клубком"}}}}"#
    )
    .unwrap();
    writeln!(
        f,
        r#"{{"_id":"2","text":{{"body":"щенок играет с мячиком"}}}}"#
    )
    .unwrap();

    // 2) writer V2 → сегмент
    let mut w = BinSegmentWriter::default();
    w.write_segment(jsonl.to_str().unwrap(), segdir.to_str().unwrap())
        .unwrap();

    // 3) reader V2
    let r = BinSegmentReader::open_segment(segdir.to_str().unwrap()).unwrap();
    assert_eq!(
        r.doc_count(),
        2,
        "doc_count must equal number of jsonl lines"
    );

    // 4) префильтр по обязательным 3-граммам + маска поля "text.body"
    let grams = required_grams_from_wildcard("*игра*").unwrap();
    let bm = r
        .prefilter(BooleanOp::And, &grams, Some("text.body"))
        .unwrap();

    // Ожидаем doc_id {0,1}
    let ids: Vec<u32> = bm.iter().collect();
    assert_eq!(ids, vec![0, 1], "prefilter should yield both docs");

    // 5) если указать несуществующее поле — должно стать пусто
    let bm_empty = r
        .prefilter(BooleanOp::And, &grams, Some("no.such.field"))
        .unwrap();
    assert!(
        bm_empty.is_empty(),
        "unknown field mask must zero the bitmap"
    );
}
