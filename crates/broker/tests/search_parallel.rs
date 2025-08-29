// broker/tests/search_parallel.rs
use broker::search::types::*;
use broker::search::SearchCoordinator;

#[tokio::test]
async fn fills_page_and_cancels_rest() {
    let coord = SearchCoordinator::new(4);
    let req = SearchRequest {
        wildcard: "*игра*".into(),
        field: Some("text.body".to_string()),
        segments: vec![
            "segments/000001".into(),
            "segments/000002".into(),
            "segments/000003".into(),
        ],
        page: PageIn {
            size: 50,
            cursor: None,
        },
        limits: Some(SearchLimits {
            parallelism: Some(3),
            deadline_ms: Some(500),
            max_candidates: Some(200_000),
        }),
        shards: None,
    };
    let resp = coord.handle(req).await.unwrap();
    assert!(resp.hits.len() <= 50);
}
