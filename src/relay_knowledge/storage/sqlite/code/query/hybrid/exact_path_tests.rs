use crate::domain::{
    CodeRepositorySelector, CodeRetrievalLayer, CodeRetrievalRequest, FreshnessPolicy,
    RepositoryCodeRange,
};

use super::*;

#[test]
fn exact_path_hybrid_without_graph_intent_skips_chunk_first() {
    let request = request(
        "NoDestructor",
        CodeQueryKind::Hybrid,
        vec!["util/no_destructor.h".to_owned()],
    );

    assert!(hybrid_exact_path_query_should_skip_chunk_first(&request));
}

#[test]
fn exact_path_hybrid_with_graph_intent_keeps_chunk_first() {
    let request = request(
        "std function compression lambda input output db bench callers",
        CodeQueryKind::Hybrid,
        vec!["benchmarks/db_bench.cc".to_owned()],
    );

    assert!(!hybrid_exact_path_query_should_skip_chunk_first(&request));
    assert!(hybrid_query_should_use_layered_chunk_search(&request));
}

#[test]
fn broad_hybrid_without_graph_intent_uses_single_chunk_pass() {
    let request = request(
        "cache interface lookup insert total charge lru",
        CodeQueryKind::Hybrid,
        Vec::new(),
    );

    assert!(!hybrid_query_should_use_layered_chunk_search(&request));
}

#[test]
fn workflow_language_hybrid_uses_layered_chunk_search() {
    let selector =
        CodeRepositorySelector::new("repo", "commit", Vec::new(), vec!["python".to_owned()])
            .expect("selector should validate");
    let request = CodeRetrievalRequest::new(
        "lambda payload filter normalize service runner",
        selector,
        CodeQueryKind::Hybrid,
        12,
        FreshnessPolicy::AllowStale,
    )
    .expect("request should validate");

    assert!(hybrid_query_should_use_layered_chunk_search(&request));
}

#[test]
fn dense_api_hybrid_keeps_layered_chunk_search_before_symbols() {
    let request = request(
        "worker.New RegisterWorkflow RegisterActivity InterruptCh task queue",
        CodeQueryKind::Hybrid,
        Vec::new(),
    );

    assert!(hybrid_query_should_use_layered_chunk_search(&request));
}

#[test]
fn exact_path_hybrid_single_identity_can_defer_to_source_fallback() {
    let request = request(
        "NoDestructor",
        CodeQueryKind::Hybrid,
        vec!["./util/no_destructor.h".to_owned()],
    );

    assert!(hybrid_exact_path_query_can_defer_to_source_fallback(
        &request,
        &[hit()]
    ));
}

#[test]
fn exact_path_hybrid_with_uncovered_member_identity_runs_chunk_layer() {
    let request = request(
        "VersionSet Builder Apply compact pointers deleted files added files SaveTo",
        CodeQueryKind::Hybrid,
        vec!["db/version_set.cc".to_owned()],
    );
    let save_to_hit = CodeRetrievalHit {
        excerpt: "Builder.SaveTo: Save the current state in *v. void SaveTo(Version* v) {"
            .to_owned(),
        canonical_symbol_id: Some("repo://repo/db::version_set::leveldb.Builder.SaveTo".to_owned()),
        ..hit()
    };

    assert!(!hybrid_exact_path_query_can_defer_to_source_fallback(
        &request,
        &[save_to_hit]
    ));
}

#[test]
fn exact_path_hybrid_contextual_query_runs_chunk_layer_even_with_symbol_coverage() {
    let request = request(
        "VersionSet Builder SaveTo compact pointers deleted files",
        CodeQueryKind::Hybrid,
        vec!["db/version_set.cc".to_owned()],
    );
    let save_to_hit = CodeRetrievalHit {
        excerpt: "Builder.SaveTo: Save the current state in *v. void SaveTo(Version* v) {"
            .to_owned(),
        canonical_symbol_id: Some("repo://repo/db::version_set::leveldb.Builder.SaveTo".to_owned()),
        ..hit()
    };

    assert!(!hybrid_exact_path_query_can_defer_to_source_fallback(
        &request,
        &[save_to_hit]
    ));
}

#[test]
fn exact_path_hybrid_type_surface_query_needs_type_declaration_hit_to_defer() {
    let request = request(
        "DBImpl public DB interface override Put Delete Write Get",
        CodeQueryKind::Hybrid,
        vec!["db/db_impl.h".to_owned()],
    );
    let member_hits = db_impl_member_hits();

    assert!(!hybrid_exact_path_query_can_defer_to_source_fallback(
        &request,
        &member_hits
    ));
}

#[test]
fn graph_expansion_intent_keeps_hybrid_graph_layers_enabled() {
    for query in [
        "NoDestructor callers",
        "NoDestructor references",
        "NoDestructor imports",
        "worker dependency flow",
        "execution flow builder",
        "agent inheritance context",
    ] {
        let request = request(
            query,
            CodeQueryKind::Hybrid,
            vec!["util/no_destructor.h".to_owned()],
        );

        assert!(
            !hybrid_exact_path_query_can_defer_to_source_fallback(&request, &[hit()]),
            "{query}"
        );
        assert!(!hybrid_query_can_skip_graph_expansion(&request, &[hit()]));
    }
}

#[test]
fn broad_hybrid_without_graph_intent_can_skip_graph_expansion_after_hits() {
    let request = request(
        "function literal notify payload goroutine callback",
        CodeQueryKind::Hybrid,
        Vec::new(),
    );

    assert!(!hybrid_query_can_skip_graph_expansion(&request, &[hit()]));
    assert!(!hybrid_exact_path_query_can_defer_to_source_fallback(
        &request,
        &[hit()]
    ));
}

#[test]
fn non_file_filters_do_not_defer_hybrid_graph_layers() {
    let request = request(
        "NoDestructor variadic constructor template instance type",
        CodeQueryKind::Hybrid,
        vec!["util/".to_owned()],
    );

    assert!(!hybrid_exact_path_query_can_defer_to_source_fallback(
        &request,
        &[hit()]
    ));
}

fn request(query: &str, kind: CodeQueryKind, path_filters: Vec<String>) -> CodeRetrievalRequest {
    let selector = CodeRepositorySelector::new("repo", "commit", path_filters, Vec::new())
        .expect("selector should validate");
    CodeRetrievalRequest::new(query, selector, kind, 10, FreshnessPolicy::AllowStale)
        .expect("request should validate")
}

fn hit() -> CodeRetrievalHit {
    CodeRetrievalHit {
        repository_id: "repo".to_owned(),
        scope_id: "scope".to_owned(),
        resolved_commit_sha: "commit".to_owned(),
        tree_hash: "tree".to_owned(),
        path: "util/no_destructor.h".to_owned(),
        language_id: "c".to_owned(),
        byte_range: RepositoryCodeRange { start: 0, end: 1 },
        line_range: RepositoryCodeRange { start: 1, end: 1 },
        symbol_snapshot_id: Some("symbol".to_owned()),
        canonical_symbol_id: Some("repo://repo/util::no_destructor::NoDestructor".to_owned()),
        file_id: Some("file".to_owned()),
        retrieval_layers: vec![CodeRetrievalLayer::Symbol],
        index_versions: vec!["code:scope:tree".to_owned()],
        stale: false,
        staleness_hint: None,
        degraded_reason: None,
        edge_kind: None,
        edge_resolution_state: None,
        edge_target_hint: None,
        edge_confidence_basis_points: None,
        edge_confidence_tier: None,
        score: 2.0,
        excerpt: "NoDestructor.alignas: alignas(InstanceType)".to_owned(),
    }
}

fn db_impl_member_hits() -> Vec<CodeRetrievalHit> {
    ["Put", "Delete", "Write", "Get"]
        .into_iter()
        .map(|member| CodeRetrievalHit {
            excerpt: format!("DBImpl.{member}: Status {member}(const WriteOptions&) override;"),
            canonical_symbol_id: Some(format!("repo://repo/db::db_impl::DBImpl.{member}")),
            ..hit()
        })
        .collect()
}
