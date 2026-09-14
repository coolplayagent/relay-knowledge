use super::*;
use crate::domain::{BusinessGlossary, CodeRepositorySelector, GraphVersion};

#[test]
fn bootstrap_example_is_valid_and_guides_committed_authoring() {
    let bootstrap = BusinessKnowledgeBootstrap::default();
    let glossary = BusinessGlossary::parse(bootstrap.schema_example.as_bytes()).unwrap();
    assert_eq!(glossary.schema_version, bootstrap.schema_version);
    assert_eq!(glossary.terms.len(), 1);
    assert_eq!(glossary.terms[0].mappings.len(), 1);
    assert_eq!(
        bootstrap.default_glossary_path,
        crate::project::BUSINESS_GLOSSARY_RELATIVE_PATH
    );
    assert!(
        bootstrap
            .next_steps
            .iter()
            .any(|step| step.contains("Commit"))
    );
    assert!(
        bootstrap
            .next_steps
            .iter()
            .any(|step| step.contains("map route"))
    );
    let json = serde_json::to_value(&bootstrap).unwrap();
    assert_eq!(
        serde_json::from_value::<BusinessKnowledgeBootstrap>(json).unwrap(),
        bootstrap
    );
}

#[test]
fn diagnostics_explain_independent_readiness_and_result_states() {
    use BusinessKnowledgeDiagnosticReason as Reason;
    use BusinessKnowledgeQueryKind as Kind;
    use BusinessKnowledgeResultStatus as Status;
    use BusinessKnowledgeState as State;
    for (state, status, kind, count, stale, expected, authoring) in [
        (
            State::Unknown,
            Status::Unavailable,
            Kind::All,
            0,
            true,
            Some(Reason::GraphOnly),
            false,
        ),
        (
            State::Mapped,
            Status::Matched,
            Kind::All,
            1,
            true,
            Some(Reason::StaleProjection),
            false,
        ),
        (
            State::NoSources,
            Status::Unavailable,
            Kind::All,
            0,
            false,
            Some(Reason::NoBusinessSources),
            true,
        ),
        (
            State::EmptyGlossary,
            Status::NoMatch,
            Kind::All,
            0,
            false,
            Some(Reason::EmptyGlossary),
            true,
        ),
        (
            State::TermsOnly,
            Status::Matched,
            Kind::All,
            0,
            false,
            Some(Reason::NoMappings),
            true,
        ),
        (
            State::TermsOnly,
            Status::Matched,
            Kind::Terms,
            0,
            false,
            None,
            false,
        ),
        (
            State::TermsOnly,
            Status::NoMatch,
            Kind::Mappings,
            0,
            false,
            Some(Reason::NoMappings),
            true,
        ),
        (
            State::Mapped,
            Status::NoMatch,
            Kind::Mappings,
            0,
            false,
            Some(Reason::NoMatch),
            false,
        ),
        (
            State::Mapped,
            Status::Matched,
            Kind::All,
            0,
            false,
            Some(Reason::NoMappings),
            true,
        ),
        (
            State::Mapped,
            Status::Matched,
            Kind::All,
            1,
            false,
            None,
            false,
        ),
        (
            State::Mapped,
            Status::Ambiguous,
            Kind::All,
            1,
            false,
            Some(Reason::Ambiguous),
            false,
        ),
    ] {
        let knowledge = BusinessKnowledgeSummary {
            state,
            projection: crate::domain::BusinessKnowledgeStatus {
                repository_id: "repo".into(),
                source_scope: "scope".into(),
                resolved_commit_sha: "commit".into(),
                projected_graph_version: GraphVersion::new(1),
                stale,
                source_count: 1,
                domain_count: 1,
                term_count: 1,
                mapping_count: count,
                last_error: None,
            },
        };
        let request = BusinessKnowledgeQueryRequest::new(
            CodeRepositorySelector::new("repo", "HEAD", Vec::new(), Vec::new()).unwrap(),
            None,
            None,
            kind,
            crate::domain::FreshnessPolicy::AllowStale,
            100,
        )
        .unwrap();
        let result = BusinessKnowledgeResult {
            status,
            match_type: None,
            returned_term_count: 1,
            returned_mapping_count: count,
            truncated: false,
        };
        let diagnostic = BusinessKnowledgeDiagnostics::for_query(&knowledge, &request, &result);
        assert_eq!(diagnostic.as_ref().map(|value| value.reason), expected);
        if let Some(value) = diagnostic {
            assert_eq!(value.bootstrap.is_some(), authoring);
            assert!(!value.next_steps.is_empty());
            let json = serde_json::to_value(&value).unwrap();
            assert_eq!(
                serde_json::from_value::<BusinessKnowledgeDiagnostics>(json).unwrap(),
                value
            );
        }
    }
}
