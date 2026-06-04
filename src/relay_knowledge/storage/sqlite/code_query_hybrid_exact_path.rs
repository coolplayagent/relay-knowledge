use crate::domain::{CodeQueryKind, CodeRetrievalHit, CodeRetrievalRequest};

use super::code_query_hybrid_planning::{
    hybrid_query_requires_chunk_first_before_symbols, hybrid_sequence_terms,
};

pub(super) fn hybrid_exact_path_query_can_defer_to_source_fallback(
    request: &CodeRetrievalRequest,
    hits: &[CodeRetrievalHit],
) -> bool {
    hybrid_query_can_skip_graph_expansion(request, hits)
        && exact_path_hits_cover_query_identities(&request.query, hits)
        && !hybrid_query_mentions_type_surface(&request.query)
}

pub(super) fn hybrid_query_can_skip_graph_expansion(
    request: &CodeRetrievalRequest,
    hits: &[CodeRetrievalHit],
) -> bool {
    request.code_query_kind == CodeQueryKind::Hybrid
        && request_has_exact_file_filter(request)
        && !hits.is_empty()
        && !hybrid_query_mentions_graph_expansion(&request.query)
}

pub(super) fn hybrid_exact_path_query_should_skip_chunk_first(
    request: &CodeRetrievalRequest,
) -> bool {
    request.code_query_kind == CodeQueryKind::Hybrid
        && request_has_exact_file_filter(request)
        && !hybrid_query_mentions_graph_expansion(&request.query)
}

pub(super) fn hybrid_query_should_use_layered_chunk_search(request: &CodeRetrievalRequest) -> bool {
    request.code_query_kind == CodeQueryKind::Hybrid
        && !hybrid_exact_path_query_should_skip_chunk_first(request)
        && (hybrid_query_requires_chunk_first_before_symbols(request)
            || hybrid_query_mentions_graph_expansion(&request.query))
}

pub(super) fn request_has_exact_file_filter(request: &CodeRetrievalRequest) -> bool {
    request
        .repository
        .path_filters
        .iter()
        .any(|path| exact_file_filter(path))
}

fn hybrid_query_mentions_graph_expansion(query: &str) -> bool {
    hybrid_sequence_terms(query).iter().any(|term| {
        matches!(
            term.as_str(),
            "call"
                | "calls"
                | "caller"
                | "callers"
                | "callee"
                | "callees"
                | "dependencies"
                | "dependency"
                | "execution"
                | "inheritance"
                | "inherits"
                | "reference"
                | "references"
                | "referenced"
                | "import"
                | "imports"
                | "importer"
                | "importers"
        )
    })
}

fn exact_file_filter(path: &str) -> bool {
    let path = normalize_filter_path(path);
    !path.is_empty()
        && path
            .rsplit('/')
            .next()
            .is_some_and(|name| name.contains('.'))
        && !path.ends_with('/')
}

fn normalize_filter_path(path: &str) -> &str {
    let mut path = path.trim_end_matches(['/', '\\']);
    while let Some(stripped) = path.strip_prefix("./") {
        path = stripped;
    }

    path
}

fn exact_path_hits_cover_query_identities(query: &str, hits: &[CodeRetrievalHit]) -> bool {
    let identities = identifier_like_query_terms(query);
    identities.is_empty()
        || identities
            .iter()
            .all(|identity| hits.iter().any(|hit| hit_mentions_identity(hit, identity)))
}

fn hybrid_query_mentions_type_surface(query: &str) -> bool {
    hybrid_sequence_terms(query).iter().any(|term| {
        matches!(
            term.as_str(),
            "class"
                | "struct"
                | "interface"
                | "interfaces"
                | "trait"
                | "traits"
                | "public"
                | "extends"
                | "implements"
                | "override"
                | "overrides"
        )
    })
}

fn identifier_like_query_terms(query: &str) -> Vec<String> {
    let mut identities = Vec::new();
    for raw_token in query.split_whitespace().map(str::trim) {
        if raw_token.contains('/') || raw_token.contains('\\') {
            continue;
        }
        for term in raw_token
            .split(|character: char| !(character.is_ascii_alphanumeric() || character == '_'))
            .filter(|term| term.len() >= 3)
        {
            if identifier_like_query_term(term)
                && !identities
                    .iter()
                    .any(|existing: &String| existing.eq_ignore_ascii_case(term))
            {
                identities.push(term.to_owned());
            }
        }
    }

    identities
}

fn identifier_like_query_term(term: &str) -> bool {
    term.contains('_') || camel_case_boundary_count(term) > 0 || capitalized_identifier(term)
}

fn camel_case_boundary_count(term: &str) -> usize {
    let mut previous: Option<char> = None;
    let chars = term.chars().collect::<Vec<_>>();
    let mut boundaries = 0usize;
    for (index, character) in chars.iter().enumerate() {
        let next = chars.get(index + 1).copied();
        let starts_upper_word = character.is_ascii_uppercase()
            && previous.is_some_and(|previous| {
                previous.is_ascii_lowercase()
                    || previous.is_ascii_digit()
                    || next.is_some_and(|next| next.is_ascii_lowercase())
            });
        if starts_upper_word {
            boundaries += 1;
        }
        previous = Some(*character);
    }

    boundaries
}

fn capitalized_identifier(term: &str) -> bool {
    let mut chars = term.chars();
    chars.next().is_some_and(|first| first.is_ascii_uppercase())
        && chars.any(|character| character.is_ascii_lowercase())
}

fn hit_mentions_identity(hit: &CodeRetrievalHit, identity: &str) -> bool {
    let identity = identity.to_ascii_lowercase();
    text_mentions_identity(&hit.excerpt, &identity)
        || hit
            .canonical_symbol_id
            .as_deref()
            .is_some_and(|symbol| text_mentions_identity(symbol, &identity))
}

fn text_mentions_identity(text: &str, identity: &str) -> bool {
    let text = text.to_ascii_lowercase();
    text.contains(identity)
        || compact_identifier_text(&text).contains(&compact_identifier_text(identity))
}

fn compact_identifier_text(text: &str) -> String {
    text.chars()
        .filter(|character| character.is_ascii_alphanumeric())
        .collect()
}

#[cfg(test)]
mod tests {
    use crate::domain::{
        CodeRepositorySelector, CodeRetrievalLayer, CodeRetrievalRequest, FreshnessPolicy,
        RepositoryCodeRange,
    };

    use super::*;

    #[test]
    fn exact_path_hybrid_without_graph_intent_skips_chunk_first() {
        let request = request(
            "std function compression lambda input output db bench",
            CodeQueryKind::Hybrid,
            vec!["benchmarks/db_bench.cc".to_owned()],
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
    fn exact_path_hybrid_with_partial_hits_can_defer_to_source_fallback() {
        let request = request(
            "NoDestructor variadic constructor template instance type",
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
            canonical_symbol_id: Some(
                "repo://repo/db::version_set::leveldb.Builder.SaveTo".to_owned(),
            ),
            ..hit()
        };

        assert!(!hybrid_exact_path_query_can_defer_to_source_fallback(
            &request,
            &[save_to_hit]
        ));
    }

    #[test]
    fn exact_path_hybrid_with_all_identifier_identities_can_defer() {
        let request = request(
            "VersionSet Builder SaveTo compact pointers deleted files",
            CodeQueryKind::Hybrid,
            vec!["db/version_set.cc".to_owned()],
        );
        let save_to_hit = CodeRetrievalHit {
            excerpt: "Builder.SaveTo: Save the current state in *v. void SaveTo(Version* v) {"
                .to_owned(),
            canonical_symbol_id: Some(
                "repo://repo/db::version_set::leveldb.Builder.SaveTo".to_owned(),
            ),
            ..hit()
        };

        assert!(hybrid_exact_path_query_can_defer_to_source_fallback(
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

    fn request(
        query: &str,
        kind: CodeQueryKind,
        path_filters: Vec<String>,
    ) -> CodeRetrievalRequest {
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
}
