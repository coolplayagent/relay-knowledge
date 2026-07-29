use super::public_interface_chunk_bonus;
use crate::domain::{CodeQueryKind, CodeRepositorySelector, CodeRetrievalRequest, FreshnessPolicy};

#[test]
fn interface_intent_boosts_public_header_declarations() {
    let request = hybrid_request("cache interface lookup insert total charge");
    let bonus = public_interface_chunk_bonus(
        4.0,
        &request.query,
        "class LEVELDB_EXPORT Cache {\n public:\n  virtual Handle* Insert() = 0;\n};",
        "include/leveldb/cache.h",
        &request,
    );

    assert!(bonus > 0.0);
}

#[test]
fn interface_bonus_ignores_implementation_and_non_interface_queries() {
    let request = hybrid_request("cache lookup insert total charge");

    assert_eq!(
        public_interface_chunk_bonus(
            4.0,
            &request.query,
            "class Cache { public: Handle* Insert(); };",
            "include/leveldb/cache.h",
            &request,
        ),
        0.0
    );
    let interface_request = hybrid_request("cache interface lookup insert total charge");
    assert_eq!(
        public_interface_chunk_bonus(
            4.0,
            &interface_request.query,
            "class LRUCache { public: Handle* Insert(); };",
            "util/cache.cc",
            &interface_request,
        ),
        0.0
    );
}

fn hybrid_request(query: &str) -> CodeRetrievalRequest {
    let selector = CodeRepositorySelector::new("repo", "commit", Vec::new(), Vec::new())
        .expect("selector should validate");
    CodeRetrievalRequest::new(
        query,
        selector,
        CodeQueryKind::Hybrid,
        10,
        FreshnessPolicy::AllowStale,
    )
    .expect("request should validate")
}
