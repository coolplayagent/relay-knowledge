use super::super::tests::{projection, registration};
use crate::{
    domain::{
        BusinessKnowledgeMatchType as Match, BusinessKnowledgeProjection,
        BusinessKnowledgeProjectionInput, BusinessKnowledgeQueryKind as Kind,
        BusinessKnowledgeQueryRequest, BusinessKnowledgeResultStatus as Status,
        CodeRepositorySelector, FreshnessPolicy,
    },
    storage::{BusinessKnowledgeStore, RepositoryCatalogStore as _, SqliteGraphStore},
};

#[tokio::test]
async fn business_query_distinguishes_unavailable_empty_and_terms_only() {
    let mut absent = projection();
    absent.sources.clear();
    let result = query(absent, Kind::All, None, None, 10).await;
    assert_eq!(result.result.status, Status::Unavailable);
    assert!(result.result.match_type.is_none());
    assert_eq!(result.result.returned_term_count, 0);
    let mut empty = projection();
    for source in &mut empty.sources {
        source.glossary.terms.clear();
    }
    assert_eq!(
        query(empty, Kind::All, None, None, 10).await.result.status,
        Status::NoMatch
    );
    let mut terms_only = projection();
    for source in &mut terms_only.sources {
        for term in &mut source.glossary.terms {
            term.mappings.clear();
        }
    }
    let terms = query(terms_only.clone(), Kind::Terms, None, None, 10).await;
    assert_eq!(terms.result.status, Status::Matched);
    assert!(terms.result.match_type.is_none());
    assert_eq!(terms.result.returned_term_count, 2);
    assert_eq!(terms.result.returned_mapping_count, 0);
    assert_eq!(
        query(terms_only, Kind::Mappings, None, None, 10)
            .await
            .result
            .status,
        Status::NoMatch
    );
}

#[tokio::test]
async fn business_query_keeps_ambiguity_before_limit_and_applies_domain() {
    let result = query(projection(), Kind::All, Some("CVR"), None, 1).await;
    assert_eq!(result.result.status, Status::Ambiguous);
    assert_eq!(result.result.match_type, Some(Match::Exact));
    assert!(result.result.truncated);
    assert_eq!(result.result.returned_term_count, 1);
    assert_eq!(
        result.result.returned_mapping_count,
        result.terms[0].mappings.len()
    );
    let scoped = query(projection(), Kind::Terms, Some("CVR"), Some("sales"), 1).await;
    assert_eq!(scoped.result.status, Status::Matched);
    assert_eq!(scoped.result.match_type, Some(Match::Exact));
    assert!(!scoped.result.truncated);
    assert!(scoped.terms[0].mappings.is_empty());
    for (text, domain) in [(Some("absent"), None), (None, Some("absent"))] {
        let missing = query(projection(), Kind::All, text, domain, 1).await;
        assert_eq!(missing.result.status, Status::NoMatch);
        assert!(missing.result.match_type.is_none());
    }
    let mut same_names = projection();
    for source in &mut same_names.sources {
        for domain in &mut source.glossary.domains {
            domain.name = "Shared".into();
        }
    }
    let still_ambiguous = query(same_names, Kind::All, Some("CVR"), Some("Shared"), 1).await;
    assert_eq!(still_ambiguous.result.status, Status::Ambiguous);
    let canonical = query(
        projection(),
        Kind::All,
        Some("Conversion"),
        Some("Sales"),
        10,
    )
    .await;
    assert_eq!(canonical.result.match_type, Some(Match::Exact));
    for text in ["convers", "CV", "competing", "missing::"] {
        let partial = query(projection(), Kind::All, Some(text), None, 10).await;
        assert_eq!(partial.result.status, Status::Matched);
        assert_eq!(partial.result.match_type, Some(Match::Partial));
    }
}

#[tokio::test]
async fn business_mapping_eligibility_precedes_exact_matching_and_limit() {
    let mut input = projection();
    for source in &mut input.sources {
        for term in &mut source.glossary.terms {
            if term.domain == "sales" {
                term.mappings.clear();
            }
        }
    }
    for text in [None, Some("CVR")] {
        let result = query(input.clone(), Kind::Mappings, text, None, 1).await;
        assert_eq!(result.result.status, Status::Matched);
        assert!(!result.result.truncated);
        assert_eq!(result.terms[0].domain_id, "support");
        assert_eq!(result.result.returned_mapping_count, 2);
        assert!(result.terms[0].definitions.is_empty());
    }
    for source in &mut input.sources {
        for term in &mut source.glossary.terms {
            term.canonical_name = if term.domain == "sales" {
                "Usage"
            } else {
                "Usage Rate"
            }
            .into();
            term.aliases.clear();
        }
    }
    let partial = query(input, Kind::Mappings, Some("Usage"), None, 1).await;
    assert_eq!(partial.result.status, Status::Matched);
    assert_eq!(partial.result.match_type, Some(Match::Partial));
    assert_eq!(partial.terms[0].domain_id, "support");
}

async fn query(
    input: BusinessKnowledgeProjectionInput,
    kind: Kind,
    text: Option<&str>,
    domain: Option<&str>,
    limit: usize,
) -> BusinessKnowledgeProjection {
    let store = SqliteGraphStore::open_in_memory().unwrap();
    store.upsert_code_repository(registration()).await.unwrap();
    store
        .replace_business_knowledge_projection(input)
        .await
        .unwrap();
    let request = BusinessKnowledgeQueryRequest::new(
        CodeRepositorySelector::new("repository-1", "commit-1", Vec::new(), Vec::new()).unwrap(),
        domain.map(str::to_owned),
        text.map(str::to_owned),
        kind,
        FreshnessPolicy::AllowStale,
        limit,
    )
    .unwrap();
    store
        .run_read_snapshot(move |connection| {
            super::super::projection_for_scope(connection, "scope-1", request)
        })
        .await
        .unwrap()
}
