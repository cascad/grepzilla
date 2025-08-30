use grepzilla_segment::gram::{BooleanOp, required_grams_from_wildcard};
use grepzilla_segment::segjson::{JsonSegmentReader, JsonSegmentWriter};
use grepzilla_segment::{SegmentReader, SegmentWriter};
use std::fs::File;
use std::io::Write;
use tempfile::tempdir;

#[test]
fn smoke_ingest_and_search() {
    // Временная папка для сегмента и входных данных
    let tmp = tempdir().unwrap();

    // Готовим самодостаточный input JSONL во временном файле
    let input_path = tmp.path().join("data.jsonl");
    let mut f = File::create(&input_path).unwrap();
    writeln!(f, "{{\"_id\":\"1\",\"text.title\":\"Кошки\",\"text.body\":\"котёнок играет с клубком\",\"tags\":[\"pets\"],\"lang\":\"ru\"}} ").unwrap();
    writeln!(f, "{{\"_id\":\"2\",\"text.title\":\"Собаки\",\"text.body\":\"щенок играет с мячиком\",\"tags\":[\"pets\"],\"lang\":\"ru\"}} ").unwrap();
    writeln!(f, "{{\"_id\":\"3\",\"text.title\":\"Птицы\",\"text.body\":\"воробей поёт утром\",\"tags\":[\"wildlife\"],\"lang\":\"ru\"}} ").unwrap();

    // Папка сегмента
    let out = tmp.path().join("seg");

    // Build сегмент
    let mut w = JsonSegmentWriter::default();
    w.write_segment(input_path.to_str().unwrap(), out.to_str().unwrap())
        .unwrap();

    // Открываем и проверяем
    let reader = JsonSegmentReader::open_segment(out.to_str().unwrap()).unwrap();
    assert_eq!(reader.doc_count(), 3);

    // Префильтр по обязательным граммам для *играет*
    let grams = required_grams_from_wildcard("*играет*").unwrap();
    let bm = reader.prefilter(BooleanOp::And, &grams, None).unwrap();

    // Собираем ext_id кандидатов (без финальной verify, нам важен coverage)
    let mut found = Vec::new();
    for doc_id in bm.iter() {
        let doc = reader.get_doc(doc_id).unwrap();
        found.push(doc.ext_id.clone());
    }

    // Ожидаем, что «играет» нашли доки 1 и 2, но не 3
    assert!(found.contains(&"1".to_string()));
    assert!(found.contains(&"2".to_string()));
    assert!(!found.contains(&"3".to_string()));
}
