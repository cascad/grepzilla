use criterion::{black_box, criterion_group, criterion_main, Criterion};
use grepzilla_segment::gram::{required_grams_from_wildcard, BooleanOp};
use grepzilla_segment::v2::reader::BinSegmentReader;
use grepzilla_segment::v2::writer::BinSegmentWriter;
use grepzilla_segment::{SegmentReader, SegmentWriter};
use std::fs::File;
use std::io::Write;

fn build_segment(tmp: &tempfile::TempDir) -> String {
    let injson = tmp.path().join("data.jsonl");
    let mut f = File::create(&injson).unwrap();
    for i in 0..2000 {
        // немного «шумного» текста
        writeln!(f, r#"{{"_id":"doc-{i}","text":{{"title":"t{i}","body":"щенок играет с мячиком {i} lorem ipsum dolor sit amet"}}}}"#).unwrap();
    }
    let segdir = tmp.path().join("seg");
    BinSegmentWriter::default()
        .write_segment(injson.to_str().unwrap(), segdir.to_str().unwrap())
        .unwrap();
    segdir.to_str().unwrap().to_string()
}

fn bench_prefilter(c: &mut Criterion) {
    let td = tempfile::tempdir().unwrap();
    let seg = build_segment(&td);
    let r = BinSegmentReader::open_segment(&seg).unwrap();

    let grams = required_grams_from_wildcard("*играет*").unwrap();

    c.bench_function("v2_prefilter_AND_играет", |b| {
        b.iter(|| {
            let bm = r
                .prefilter(BooleanOp::And, black_box(&grams), Some("text.body"))
                .unwrap();
            black_box(bm.cardinality())
        })
    });
}

criterion_group!(benches, bench_prefilter);
criterion_main!(benches);
