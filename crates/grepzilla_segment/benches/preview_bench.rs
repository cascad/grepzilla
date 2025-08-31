// crates/grepzilla_segment/benches/preview_bench.rs
use criterion::{Criterion, criterion_group, criterion_main};
use grepzilla_segment::StoredDoc;
use grepzilla_segment::common::preview::{PreviewOpts, build_preview};
use std::collections::BTreeMap;

fn mk_doc() -> StoredDoc {
    let mut fields = BTreeMap::new();
    fields.insert(
        "text.body".to_string(),
        "щенок играет с мячиком на большой-большой поляне".to_string(),
    );
    fields.insert("text.title".to_string(), "игровые заметки".to_string());
    StoredDoc {
        doc_id: 0,
        ext_id: "x".into(),
        fields,
    }
}

fn bench_preview(c: &mut Criterion) {
    let doc = mk_doc();
    c.bench_function("build_preview", |b| {
        b.iter(|| {
            let _ = build_preview(
                &doc,
                PreviewOpts {
                    preferred_fields: &["text.title", "text.body"],
                    max_len: 80,
                    highlight_needle: Some("игра"),
                },
            );
        })
    });
}

criterion_group!(benches, bench_preview);
criterion_main!(benches);
