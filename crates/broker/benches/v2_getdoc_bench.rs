use criterion::{black_box, criterion_group, criterion_main, Criterion};
use grepzilla_segment::v2::reader::BinSegmentReader;
use grepzilla_segment::v2::writer::BinSegmentWriter;
use grepzilla_segment::{SegmentReader, SegmentWriter};
use std::fs::File;
use std::io::Write;

fn build_segment(tmp: &tempfile::TempDir) -> String {
    let injson = tmp.path().join("data.jsonl");
    let mut f = File::create(&injson).unwrap();
    for i in 0..5000 {
        writeln!(
            f,
            r#"{{"_id":"doc-{i}","text":{{"title":"t{i}","body":"щенок играет {i} lorem ipsum"}}}}"#
        )
        .unwrap();
    }
    let seg = tmp.path().join("seg");
    BinSegmentWriter::default()
        .write_segment(injson.to_str().unwrap(), seg.to_str().unwrap())
        .unwrap();
    seg.to_str().unwrap().to_string()
}

fn bench_getdoc(c: &mut Criterion) {
    let td = tempfile::tempdir().unwrap();
    let seg = build_segment(&td);
    let r = BinSegmentReader::open_segment(&seg).unwrap();

    // холодный доступ
    c.bench_function("v2_getdoc_cold_1000", |b| {
        b.iter(|| {
            let mut sum = 0usize;
            for id in 0..1000u32 {
                if let Some(d) = r.get_doc(id) {
                    sum += d.ext_id.len();
                }
            }
            black_box(sum)
        })
    });

    // с прогревом
    r.prefetch_docs(0..2000u32);
    c.bench_function("v2_getdoc_prefetch_1000", |b| {
        b.iter(|| {
            let mut sum = 0usize;
            for id in 0..1000u32 {
                if let Some(d) = r.get_doc(id) {
                    sum += d.ext_id.len();
                }
            }
            black_box(sum)
        })
    });
}

criterion_group!(benches, bench_getdoc);
criterion_main!(benches);
