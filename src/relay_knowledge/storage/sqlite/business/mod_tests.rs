use crate::{
    domain::{
        BusinessAlias, BusinessAliasKind, BusinessDomainDefinition, BusinessGlossary,
        BusinessKnowledgeProjectionInput, BusinessKnowledgeQueryKind,
        BusinessKnowledgeQueryRequest, BusinessKnowledgeSource, BusinessMappingRelation,
        BusinessTechnicalMappingDefinition, BusinessTermDefinition, BusinessTermStatus,
        CodeRepositorySelector, FreshnessPolicy, TechnicalTargetKind,
    },
    storage::{BusinessKnowledgeStore, CodeRepositoryStore, SqliteGraphStore},
};

#[tokio::test]
async fn projection_preserves_homonyms_conflicts_and_unresolved_hints() {
    let store = SqliteGraphStore::open_in_memory().expect("store");
    store
        .upsert_code_repository(registration())
        .await
        .expect("repository");
    let status = store
        .replace_business_knowledge_projection(projection())
        .await
        .expect("projection");
    assert!(!status.stale);

    let request = BusinessKnowledgeQueryRequest::new(
        CodeRepositorySelector::new("repository-1", "commit-1", Vec::new(), Vec::new())
            .expect("selector"),
        None,
        Some("CVR".to_owned()),
        BusinessKnowledgeQueryKind::All,
        FreshnessPolicy::AllowStale,
        20,
    )
    .expect("request");
    let result = store
        .run_read_snapshot(move |connection| {
            super::projection_for_scope(connection, "scope-1", request)
        })
        .await
        .expect("query");

    assert_eq!(
        result.resolution,
        crate::domain::BusinessKnowledgeResolution::Ambiguous
    );
    assert_eq!(result.terms.len(), 2);
    assert!(result.terms.iter().any(|term| !term.conflicts.is_empty()));
    assert!(
        result
            .terms
            .iter()
            .all(|term| term.mappings[0].resolution_state == "unresolved")
    );
    assert_eq!(result.terms[0].mappings[0].target_hint, "missing::symbol");

    store
        .run(|connection| {
            connection.execute(
                "INSERT INTO software_components VALUES (?1, ?2, ?3, ?4, ?5, NULL, NULL, ?6, ?7, ?8, ?9, ?10, 1, 1, 9000, 1)",
                rusqlite::params![
                    "component-1",
                    "repository-1",
                    "scope-1",
                    "cargo",
                    "checkout-service",
                    "runtime",
                    "manifest",
                    "resolved",
                    "rust",
                    "Cargo.toml",
                ],
            )?;
            super::refresh_mapping_resolutions(connection, "scope-1")
        })
        .await
        .expect("software-owned mapping should be rebound");
    let rebound = store
        .run_read_snapshot(|connection| {
            super::projection_for_scope(
                connection,
                "scope-1",
                BusinessKnowledgeQueryRequest::new(
                    CodeRepositorySelector::new("repository-1", "commit-1", Vec::new(), Vec::new())
                        .expect("selector"),
                    Some("sales".to_owned()),
                    Some("CVR".to_owned()),
                    BusinessKnowledgeQueryKind::Mappings,
                    FreshnessPolicy::AllowStale,
                    20,
                )
                .expect("request"),
            )
        })
        .await
        .expect("rebound mapping query");
    assert_eq!(rebound.terms[0].mappings[1].resolution_state, "resolved");
    assert_eq!(
        rebound.terms[0].mappings[1].resolved_id.as_deref(),
        Some("component-1")
    );
}

fn registration() -> crate::domain::CodeRepositoryRegistration {
    crate::domain::CodeRepositoryRegistration::new(
        "repository-1",
        "repo",
        ".",
        Vec::new(),
        Vec::new(),
    )
    .expect("registration")
}

fn projection() -> BusinessKnowledgeProjectionInput {
    let domains = vec![
        BusinessDomainDefinition {
            id: "sales".to_owned(),
            name: "Sales".to_owned(),
            description: None,
        },
        BusinessDomainDefinition {
            id: "support".to_owned(),
            name: "Support".to_owned(),
            description: None,
        },
    ];
    let terms = ["sales", "support"]
        .into_iter()
        .map(|domain| BusinessTermDefinition {
            id: "conversion".to_owned(),
            domain: domain.to_owned(),
            canonical_name: "Conversion".to_owned(),
            definition: format!("{domain} definition"),
            language: "en".to_owned(),
            status: BusinessTermStatus::Active,
            aliases: vec![BusinessAlias {
                value: "CVR".to_owned(),
                kind: BusinessAliasKind::Abbreviation,
                language: Some("en".to_owned()),
            }],
            semantics: None,
            mappings: vec![
                BusinessTechnicalMappingDefinition {
                    relation: BusinessMappingRelation::RepresentedBy,
                    target_kind: TechnicalTargetKind::Symbol,
                    target: "missing::symbol".to_owned(),
                    path: None,
                    source_scope: None,
                },
                BusinessTechnicalMappingDefinition {
                    relation: BusinessMappingRelation::CalculatedFrom,
                    target_kind: TechnicalTargetKind::SoftwareComponent,
                    target: "checkout-service".to_owned(),
                    path: None,
                    source_scope: None,
                },
            ],
        })
        .collect();
    BusinessKnowledgeProjectionInput {
        repository_id: "repository-1".to_owned(),
        source_scope: "scope-1".to_owned(),
        resolved_commit_sha: "commit-1".to_owned(),
        sources: vec![
            BusinessKnowledgeSource {
                source_id: "glossary".to_owned(),
                source_path: ".knowledge/business-glossary.yaml".to_owned(),
                authority_rank: 0,
                content_digest: "a".repeat(64),
                glossary: BusinessGlossary {
                    schema_version: 1,
                    domains,
                    terms,
                },
            },
            BusinessKnowledgeSource {
                source_id: "glossary-secondary".to_owned(),
                source_path: ".knowledge/business-glossary-secondary.yaml".to_owned(),
                authority_rank: 1,
                content_digest: "b".repeat(64),
                glossary: BusinessGlossary {
                    schema_version: 1,
                    domains: vec![BusinessDomainDefinition {
                        id: "sales".to_owned(),
                        name: "Sales".to_owned(),
                        description: None,
                    }],
                    terms: vec![BusinessTermDefinition {
                        id: "conversion".to_owned(),
                        domain: "sales".to_owned(),
                        canonical_name: "Conversion".to_owned(),
                        definition: "competing sales definition".to_owned(),
                        language: "en".to_owned(),
                        status: BusinessTermStatus::Active,
                        aliases: Vec::new(),
                        semantics: None,
                        mappings: Vec::new(),
                    }],
                },
            },
        ],
    }
}
