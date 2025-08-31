// crates/grepzilla_segment/tests/preview_tests.rs
use grepzilla_segment::common::preview::{build_preview, truncate_chars_with_ellipsis, PreviewOpts};
use grepzilla_segment::StoredDoc;
use std::collections::BTreeMap;

fn doc_with(fields: &[(&str, &str)]) -> StoredDoc {
    let mut m = BTreeMap::new();
    for (k, v) in fields {
        m.insert((*k).to_string(), (*v).to_string());
    }
    StoredDoc {
        doc_id: 0,
        ext_id: "test".into(),
        fields: m,
    }
}

#[test]
fn preview_picks_preferred_and_highlights() {
    // нормализованные строки (в ingest они уже нормализуются)
    let d = doc_with(&[
        ("text.title", "заголовок про игру"),
        ("text.body", "щенок играет с мячиком"),
    ]);

    // берём title как приоритетный, highlight = "игра"
    let out = build_preview(
        &d,
        PreviewOpts {
            preferred_fields: &["text.title", "text.body"],
            max_len: 22, // узкое окно, важно, что не ломаем UTF-8
            highlight_needle: Some("игра"),
        },
    );
    assert!(out.contains('[') && out.contains(']'), "no highlight: {}", out);
}

#[test]
fn preview_fallbacks_to_first_field() {
    // preferred полей нет — возьмём любое первое
    let d = doc_with(&[("foo", "обычный текст без совпадений")]);
    let out = build_preview(
        &d,
        PreviewOpts {
            preferred_fields: &["text.title", "text.body"],
            max_len: 10,
            highlight_needle: Some("игра"),
        },
    );
    // иглы нет — должен быть простой truncate
    assert!(out.ends_with('…') || out.len() <= 10, "{}", out);
}

#[test]
fn truncate_respects_utf8_boundaries() {
    let s = "привет мир";
    let out = truncate_chars_with_ellipsis(s, 7);
    // длина в символах не больше 8 (7 + «…»)
    assert!(out.chars().count() <= 8, "{out}");
}
