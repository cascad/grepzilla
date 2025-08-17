pub mod index;
pub mod query;
pub mod util;

#[cfg(test)]
mod tests {
    use super::index::InMemoryIndex;
    use super::query::parse_query;
    use serde_json::json;

    #[test]
    fn test_basic_insert_search() {
        let mut idx = InMemoryIndex::new();
        let doc = json!({
            "_id": "1",
            "text.title": "Кошки",
            "text.body": "котёнок играет с клубком"
        });
        idx.add_json_doc(doc).unwrap();

        let (plan, opts) = parse_query("*кот*").unwrap();
        let hits = idx.search(&plan, opts.limit, opts.offset).unwrap();
        assert!(!hits.is_empty());
    }
}
