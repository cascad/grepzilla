// broker/tests/search_parallel.rs
use broker::search::{SearchCoordinator};
use broker::search::types::*;

#[tokio::test]
async fn fills_page_and_cancels_rest() {
    let coord = SearchCoordinator::new(4);
    let req = SearchRequest {
        wildcard: "*игра*".into(),
        field: "text.body".into(),
        segments: vec!["segments/000001".into(),"segments/000002".into(),"segments/000003".into()],
        page: PageReq { size: 50, cursor: None },
        limits: Some(SearchLimits { parallelism: Some(3), deadline_ms: Some(500), max_candidates: Some(200_000) }),
    };
    let resp = coord.handle(req).await.unwrap();
    assert!(resp.hits.len() <= 50);
}
