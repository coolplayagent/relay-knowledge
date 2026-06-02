use crate::domain::{CodeQueryKind, CodeRetrievalHit, CodeRetrievalLayer, CodeRetrievalRequest};

use super::{
    code_query_api_identities::{ApiSymbolIdentity, hybrid_api_symbol_identities},
    code_query_hybrid_planning::hybrid_sequence_terms,
};

pub(super) fn hybrid_direct_results_can_answer_without_graph_expansion(
    request: &CodeRetrievalRequest,
    hits: &[CodeRetrievalHit],
) -> bool {
    if request.code_query_kind != CodeQueryKind::Hybrid {
        return false;
    }
    let terms = hybrid_sequence_terms(&request.query);
    if terms.len() < 3 {
        return false;
    }
    if hybrid_query_has_graph_expansion_intent(&terms) {
        return false;
    }

    if hits
        .iter()
        .take(request.limit.max(1))
        .any(|hit| hybrid_direct_hit_covers_query(hit, &terms))
    {
        return true;
    }

    hybrid_api_identity_symbol_hits_cover_query(request, hits)
}

fn hybrid_direct_hit_covers_query(hit: &CodeRetrievalHit, terms: &[String]) -> bool {
    hybrid_direct_hit_can_answer(hit)
        && hybrid_sequence_match_count(&hit.excerpt, terms)
            >= hybrid_direct_required_match_count(terms.len())
}

fn hybrid_direct_required_match_count(term_count: usize) -> usize {
    term_count
        .saturating_mul(4)
        .div_ceil(5)
        .clamp(4, 6)
        .min(term_count)
}

fn hybrid_query_has_graph_expansion_intent(terms: &[String]) -> bool {
    terms.iter().any(|term| {
        matches!(
            term.as_str(),
            "caller"
                | "callers"
                | "callee"
                | "callees"
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

fn hybrid_direct_hit_can_answer(hit: &CodeRetrievalHit) -> bool {
    if hit.edge_kind.is_some()
        || hit
            .retrieval_layers
            .contains(&CodeRetrievalLayer::TextFallback)
    {
        return false;
    }

    hit.retrieval_layers.contains(&CodeRetrievalLayer::Lexical)
        || (hit.retrieval_layers.contains(&CodeRetrievalLayer::Symbol)
            && hit
                .retrieval_layers
                .contains(&CodeRetrievalLayer::Definition))
}

fn hybrid_sequence_match_count(excerpt: &str, terms: &[String]) -> usize {
    let excerpt = excerpt.to_ascii_lowercase();
    terms
        .iter()
        .filter(|term| excerpt.contains(term.as_str()))
        .count()
}

fn hybrid_api_identity_symbol_hits_cover_query(
    request: &CodeRetrievalRequest,
    hits: &[CodeRetrievalHit],
) -> bool {
    let identities = hybrid_api_symbol_identities(&request.query, request);
    if identities.len() < 2 || identities.len() > request.limit.max(1) {
        return false;
    }

    identities.iter().all(|identity| {
        hits.iter()
            .any(|hit| api_identity_symbol_hit_matches(hit, identity))
    })
}

fn api_identity_symbol_hit_matches(hit: &CodeRetrievalHit, identity: &ApiSymbolIdentity) -> bool {
    if !hit.retrieval_layers.contains(&CodeRetrievalLayer::Symbol)
        || !hit
            .retrieval_layers
            .contains(&CodeRetrievalLayer::Definition)
        || hit
            .retrieval_layers
            .contains(&CodeRetrievalLayer::TextFallback)
        || hit.edge_kind.is_some()
    {
        return false;
    }
    let Some(canonical_symbol_id) = hit.canonical_symbol_id.as_deref() else {
        return false;
    };
    let Some(leaf_name) = canonical_symbol_leaf(canonical_symbol_id) else {
        return false;
    };

    identity.matches_symbol(leaf_name, &hit.excerpt, &hit.excerpt, canonical_symbol_id)
}

fn canonical_symbol_leaf(canonical_symbol_id: &str) -> Option<&str> {
    canonical_symbol_id
        .rsplit(|character: char| !(character.is_ascii_alphanumeric() || character == '_'))
        .find(|part| !part.is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::{CodeRepositorySelector, FreshnessPolicy, RepositoryCodeRange};

    #[test]
    fn hybrid_direct_gate_accepts_collective_api_identity_symbol_coverage() {
        let request = request(
            "worker.New RegisterWorkflow InterruptCh task queue",
            CodeQueryKind::Hybrid,
            5,
        );
        let hits = vec![
            symbol_hit("worker.New", "fn New(client Client) Worker"),
            symbol_hit(
                "worker.RegisterWorkflow",
                "fn RegisterWorkflow(workflow interface{})",
            ),
            symbol_hit("worker.InterruptCh", "fn InterruptCh() <-chan interface{}"),
        ];

        assert!(hybrid_direct_results_can_answer_without_graph_expansion(
            &request, &hits
        ));
    }

    #[test]
    fn hybrid_direct_gate_keeps_graph_expansion_for_api_graph_intent_or_underfilled_limits() {
        let hits = vec![
            symbol_hit("worker.New", "fn New(client Client) Worker"),
            symbol_hit(
                "worker.RegisterWorkflow",
                "fn RegisterWorkflow(workflow interface{})",
            ),
        ];

        assert!(!hybrid_direct_results_can_answer_without_graph_expansion(
            &request(
                "worker.New RegisterWorkflow callers",
                CodeQueryKind::Hybrid,
                5,
            ),
            &hits,
        ));
        assert!(!hybrid_direct_results_can_answer_without_graph_expansion(
            &request("worker.New RegisterWorkflow", CodeQueryKind::Hybrid, 1),
            &hits,
        ));
    }

    #[test]
    fn hybrid_direct_gate_requires_scoped_api_identity_symbol_match() {
        let request = request("worker.New RegisterWorkflow", CodeQueryKind::Hybrid, 5);
        let hits = vec![
            symbol_hit("internal.New", "fn New(client Client) Worker"),
            symbol_hit(
                "worker.RegisterWorkflow",
                "fn RegisterWorkflow(workflow interface{})",
            ),
        ];

        assert!(!hybrid_direct_results_can_answer_without_graph_expansion(
            &request, &hits,
        ));
    }

    fn request(query: &str, kind: CodeQueryKind, limit: usize) -> CodeRetrievalRequest {
        CodeRetrievalRequest::new(
            query,
            CodeRepositorySelector::new("repo", "commit", Vec::new(), Vec::new())
                .expect("selector should validate"),
            kind,
            limit,
            FreshnessPolicy::AllowStale,
        )
        .expect("request should validate")
    }

    fn symbol_hit(canonical_symbol_id: &str, excerpt: &str) -> CodeRetrievalHit {
        CodeRetrievalHit {
            repository_id: "repo".to_owned(),
            scope_id: "code:test:hybrid-direct-gate:commit:tree".to_owned(),
            resolved_commit_sha: "commit".to_owned(),
            tree_hash: "tree".to_owned(),
            path: "src/worker.go".to_owned(),
            language_id: "go".to_owned(),
            byte_range: range(1, 1),
            line_range: range(1, 1),
            symbol_snapshot_id: Some(format!("{canonical_symbol_id}-symbol")),
            canonical_symbol_id: Some(format!("repo://repo/{canonical_symbol_id}")),
            file_id: Some("worker-file".to_owned()),
            retrieval_layers: vec![CodeRetrievalLayer::Symbol, CodeRetrievalLayer::Definition],
            index_versions: Vec::new(),
            stale: false,
            degraded_reason: None,
            edge_kind: None,
            edge_resolution_state: None,
            edge_target_hint: None,
            edge_confidence_basis_points: None,
            edge_confidence_tier: None,
            score: 10.0,
            excerpt: excerpt.to_owned(),
        }
    }

    fn range(start: u32, end: u32) -> RepositoryCodeRange {
        RepositoryCodeRange { start, end }
    }
}
