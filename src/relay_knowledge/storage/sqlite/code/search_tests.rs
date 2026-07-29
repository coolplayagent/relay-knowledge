use super::{search_document_content, search_document_content_into};

#[test]
fn symbol_search_content_preserves_identifier_expansion() {
    let content = search_document_content(
        "symbol",
        [
            "NewLRUCache",
            "",
            "leveldb::NewLRUCache",
            "function",
            "db/cache.cc",
        ],
    );

    assert_eq!(
        content,
        "NewLRUCache leveldb::NewLRUCache function db/cache.cc cache leveldb lru new newlrucache"
    );
}

#[test]
fn route_search_content_expands_handler_identifier_terms() {
    let content = search_document_content(
        "route",
        [
            "route endpoint http",
            "/api/users",
            "get",
            "listUsers",
            "express",
            "src/routes.ts",
        ],
    );

    assert_eq!(
        content,
        "route endpoint http /api/users get listUsers express src/routes.ts list listusers users"
    );
}

#[test]
fn non_symbol_search_content_keeps_only_nonempty_fields() {
    let content = search_document_content("chunk", ["", "body text", "  ", "src/lib.rs"]);

    assert_eq!(content, "body text src/lib.rs");
}

#[test]
fn reusable_search_content_buffers_do_not_leak_previous_terms() {
    let mut content = String::from("stale content");
    let mut symbol_terms = vec!["stale".to_owned()];
    search_document_content_into(
        &mut content,
        &mut symbol_terms,
        "symbol",
        ["GraphIndex", "relay_knowledge::GraphIndex"],
    );
    assert_eq!(
        content,
        "GraphIndex relay_knowledge::GraphIndex graph graphindex index knowledge relay relay_knowledge"
    );

    search_document_content_into(&mut content, &mut symbol_terms, "chunk", ["new chunk"]);
    assert_eq!(content, "new chunk");
    assert!(symbol_terms.is_empty());
}
