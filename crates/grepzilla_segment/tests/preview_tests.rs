use grepzilla_segment::common::preview::{build_preview, PreviewOpts};
use grepzilla_segment::StoredDoc;
use std::collections::BTreeMap;

fn mk_doc(id: u32, title: &str, body: &str) -> StoredDoc {
    let mut fields = BTreeMap::new();
    fields.insert("text.title".to_string(), title.to_string());
    fields.insert("text.body".to_string(), body.to_string());
    StoredDoc {
        doc_id: id,
        ext_id: id.to_string(),
        fields,
    }
}

#[test]
fn preview_picks_preferred_and_highlights() {
    // Точное вхождение "игра" в ПРИОРИТЕТНОМ поле (title):
    let doc = mk_doc(
        1,
        "Заголовок: игра", // ← раньше было "Заголовок про игру" (нет подстроки "игра")
        "щенок играет с мячиком во дворе и очень доволен",
    );

    // приоритет: title → body, короткое окно
    let out = build_preview(
        &doc,
        PreviewOpts {
            preferred_fields: &["text.title", "text.body"],
            max_len: 20,
            highlight_needle: Some("игра"),
        },
    );

    // Должна быть подсветка
    assert!(out.contains('[') && out.contains(']'), "no highlight: {out}");

    // По символам, с небольшим запасом на скобки/многоточие
    let visible = out.chars().count();
    assert!(
        visible <= 24,
        "visible chars {} exceed bound; out={out}",
        visible
    );
}

#[test]
fn preview_fallbacks_to_first_field() {
    // без preferred_fields — должен взять первое доступное поле
    let mut fields = BTreeMap::new();
    fields.insert("foo".to_string(), "бар играет здесь".to_string());
    let doc = StoredDoc {
        doc_id: 0,
        ext_id: "x".into(),
        fields,
    };
    let out = build_preview(
        &doc,
        PreviewOpts {
            preferred_fields: &[],
            max_len: 30,
            highlight_needle: Some("игра"),
        },
    );
    assert!(out.contains('[') && out.contains(']'), "no highlight: {out}");
    let visible = out.chars().count();
    assert!(visible <= 34, "too long: {visible} chars; out={out}");
}
