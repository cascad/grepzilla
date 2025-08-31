use criterion::{Criterion, black_box, criterion_group, criterion_main};
use grepzilla_segment::SegmentReader;
use grepzilla_segment::SegmentWriter;
use grepzilla_segment::v2::reader::BinSegmentReader;
use grepzilla_segment::v2::writer::BinSegmentWriter;
use std::fs::File;
use std::io::Write;
use tempfile::TempDir;

fn write_jsonl(path: &std::path::Path, lines: &[&str]) {
    let mut f = File::create(path).unwrap();
    for l in lines {
        writeln!(f, "{}", l).unwrap();
    }
}

fn build_small_segment(td: &TempDir) -> std::path::PathBuf {
    let out = td.path().join("seg");
    std::fs::create_dir_all(&out).unwrap();
    let in_jsonl = td.path().join("in.jsonl");

    // 10k простых документов
    let mut lines = Vec::new();
    for i in 0..10_000 {
        lines.push(format!(
            r#"{{"_id":"{i}","text":{{"title":"T{i}","body":"doc{i} играет в поле"}}}}"#
        ));
    }
    write_jsonl(
        &in_jsonl,
        &lines.iter().map(|s| s.as_str()).collect::<Vec<_>>(),
    );

    BinSegmentWriter::default()
        .write_segment(in_jsonl.to_str().unwrap(), out.to_str().unwrap())
        .unwrap();
    out
}

fn bench_getdoc(c: &mut Criterion) {
    let td = TempDir::new().unwrap();
    let seg_dir = build_small_segment(&td);
    let rdr = BinSegmentReader::open_segment(seg_dir.to_str().unwrap()).unwrap();

    c.bench_function("v2_get_doc_10k", |b| {
        b.iter(|| {
            // читаем по кругу первые 1000 доков
            for id in 0u32..1000 {
                let d = rdr.get_doc(id).unwrap();
                black_box(&d.ext_id);
            }
        })
    });
}

criterion_group!(benches, bench_getdoc);
criterion_main!(benches);
