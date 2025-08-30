// crates/grepzilla_segment/tests/v2_prefilter_and_get_doc.rs

use std::fs::File;
use std::io::Write;

use grepzilla_segment::SegmentReader as _;
use grepzilla_segment::SegmentWriter as _;

use grepzilla_segment::gram::BooleanOp;
use grepzilla_segment::v2::reader::BinSegmentReader;
use grepzilla_segment::v2::writer::BinSegmentWriter;

#[test]
fn v2_prefilter_then_get_doc() {
    let tmp = tempfile::tempdir().unwrap();
    let seg_dir = tmp.path().join("seg_v2");
    let jsonl_path = tmp.path().join("in.jsonl");

    // два документа, ищем "мир" по text.body
    let mut jf = File::create(&jsonl_path).unwrap();
    writeln!(
        jf,
        r#"{{"_id":"X1","title":"Ignore","text":{{"body":"No match here"}}}}"#
    )
    .unwrap();
    writeln!(
        jf,
        r#"{{"_id":"X2","title":"Русский","text":{{"body":"привет, мир!"}}}}"#
    )
    .unwrap();

    // build
    let mut w = BinSegmentWriter::default();
    w.write_segment(jsonl_path.to_str().unwrap(), seg_dir.to_str().unwrap())
        .expect("writer failed");

    // read
    let r = BinSegmentReader::open_segment(seg_dir.to_str().unwrap()).expect("open_segment");
    assert_eq!(r.doc_count(), 2);

    // префильтр: AND одной 3-граммы "мир", поле text.body
    let grams = vec!["мир".to_string()];
    let bm = r
        .prefilter(BooleanOp::And, &grams, Some("text.body"))
        .expect("prefilter");
    let ids: Vec<u32> = bm.iter().collect();
    assert_eq!(ids, vec![1], "должен найти второй документ");

    // get_doc + проверка содержимого
    let d = r.get_doc(1).expect("doc 1");
    assert_eq!(d.ext_id, "X2");
    assert!(d.fields["text.body"].to_lowercase().contains("мир"));
}
