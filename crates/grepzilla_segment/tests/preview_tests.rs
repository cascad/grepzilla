use grepzilla_segment::StoredDoc;
use grepzilla_segment::common::preview::{PreviewOpts, build_preview};
use std::collections::BTreeMap;

fn mk_doc(map: &[(&str, &str)]) -> StoredDoc {
    let mut fields = BTreeMap::new();
    for (k, v) in map {
        fields.insert((*k).to_string(), (*v).to_string());
    }
    StoredDoc {
        doc_id: 0,
        ext_id: "x".into(),
        fields,
    }
}

#[test]
fn preview_picks_preferred_and_highlights() {
    let doc = mk_doc(&[
        ("text.title", "Заголовок про игру"),
        ("text.body", "Тут основной текст без совпадения"),
    ]);

    let out = build_preview(
        &doc,
        PreviewOpts {
            preferred_fields: &["text.title", "text.body"],
            max_len: 22,                    // укладываемся в окно
            highlight_needle: Some("игру"), // нормализованная игла
        },
    );

    assert!(
        out.contains('[') && out.contains(']'),
        "no highlight: {out}"
    );
    // Проверяем, что не развалились по байтовым границам (UTF-8)
    assert!(out.chars().count() <= 22 + 3, "too long: {}", out); // небольшой запас под «…»
}

#[test]
fn preview_fallbacks_to_first_field() {
    let doc = mk_doc(&[
        ("body", "какой-то текст без матчей"),
        ("title", "другой текст"),
    ]);

    let out = build_preview(
        &doc,
        PreviewOpts {
            preferred_fields: &["text.title", "text.body", "title", "body"],
            max_len: 16,
            highlight_needle: Some("не_найдется"),
        },
    );

    // без иглы или при её отсутствии — просто усечение
    assert!(!out.contains('['), "unexpected highlight: {out}");
    assert!(out.ends_with('…') || out.chars().count() <= 16);
}
