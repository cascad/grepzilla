use grepzilla_segment::gram::{BooleanOp, required_grams_from_wildcard};
use grepzilla_segment::segjson::{JsonSegmentReader, JsonSegmentWriter};
use grepzilla_segment::v2::reader::BinSegmentReader;
use grepzilla_segment::v2::writer::BinSegmentWriter;
use grepzilla_segment::{SegmentReader, SegmentWriter};
use std::fs::File;
use std::io::Write;

fn prepare_jsonl(p: &std::path::Path) -> anyhow::Result<()> {
    let mut f = File::create(p)?;
    writeln!(
        f,
        r#"{{"_id":"a","text":{{"title":"aaa","body":"alpha beta gamma"}},"tags":["Игры"]}}"#
    )?;
    writeln!(
        f,
        r#"{{"_id":"b","text":{{"title":"bbb","body":"delta epsilon zeta"}},"tags":["Музыка"]}}"#
    )?;
    Ok(())
}

#[test]
fn field_mask_consistency_v1_v2() -> anyhow::Result<()> {
    let td = tempfile::tempdir()?;
    let injsonl = td.path().join("data.jsonl");
    prepare_jsonl(&injsonl)?;

    // V1
    let seg1 = td.path().join("seg_v1");
    JsonSegmentWriter::default()
        .write_segment(injsonl.to_str().unwrap(), seg1.to_str().unwrap())?;
    let r1 = JsonSegmentReader::open_segment(seg1.to_str().unwrap())?;

    // V2
    let seg2 = td.path().join("seg_v2");
    BinSegmentWriter::default().write_segment(injsonl.to_str().unwrap(), seg2.to_str().unwrap())?;
    let r2 = BinSegmentReader::open_segment(seg2.to_str().unwrap())?;

    // ищем по *игры* только в tags[0] (в V1/V2 это "tags[0]")
    let grams = required_grams_from_wildcard("*игры*")?;
    let b1 = r1.prefilter(BooleanOp::And, &grams, Some("tags[0]"))?;
    let b2 = r2.prefilter(BooleanOp::And, &grams, Some("tags[0]"))?;
    assert_eq!(b1.cardinality(), 1);
    assert_eq!(b2.cardinality(), 1);
    assert_eq!(b1.iter().collect::<Vec<_>>(), b2.iter().collect::<Vec<_>>());
    Ok(())
}
