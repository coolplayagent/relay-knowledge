use super::*;

use std::{
    path::PathBuf,
    time::{SystemTime, UNIX_EPOCH},
};

use tokio::fs;

use crate::{
    api::{InterfaceKind, RequestContext},
    domain::{
        BusinessGlossary, KnowledgeMap, KnowledgeMapChange, KnowledgeMapSource,
        KnowledgeMapSourceKind, RepositoryMapType,
    },
    project::{
        AGENT_CONTRACT_DIR_NAME, BUSINESS_GLOSSARY_RELATIVE_PATH, CODESPEC_DIR_NAME,
        KNOWLEDGE_MAP_RELATIVE_PATH, LEGACY_AGENT_CONTRACT_DIR_NAME,
        LEGACY_BUSINESS_GLOSSARY_RELATIVE_PATH, LEGACY_KNOWLEDGE_MAP_RELATIVE_PATH,
    },
};

#[tokio::test]
async fn validate_rejects_a_missing_reserved_software_model_route() {
    let (root, service, context) = initialized_repository("missing-software-route").await;
    remove_topic_ref(&service, "software-model").await;

    let validation = service
        .validate(&context)
        .await
        .expect("validation should report contract diagnostics");

    assert!(!validation.valid);
    assert!(
        validation
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.contains("repository-software-model"))
    );
    let _ = fs::remove_dir_all(root).await;
}

#[tokio::test]
async fn validate_rejects_a_missing_reserved_business_glossary_route() {
    let (root, service, context) = initialized_repository("missing-business-route").await;
    remove_topic_ref(&service, "business-knowledge").await;

    let validation = service
        .validate(&context)
        .await
        .expect("validation should report contract diagnostics");

    assert!(!validation.valid);
    assert!(
        validation
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.contains("repository-business-glossary"))
    );
    let _ = fs::remove_dir_all(root).await;
}

#[tokio::test]
async fn validate_allows_additional_business_knowledge_sources() {
    let (root, service, context) = initialized_repository("additional-business-source").await;
    service
        .add_source(
            &context,
            KnowledgeMapSourceAddRequest {
                id: "business-handbook".to_owned(),
                topic: "business-knowledge".to_owned(),
                kind: KnowledgeMapSourceKind::Doc,
                uri: "docs/business-handbook.md".to_owned(),
                source_scope: Some("docs".to_owned()),
                description: Some("Reviewed business guidance.".to_owned()),
            },
        )
        .await
        .expect("ordinary business source should add");

    let validation = service
        .validate(&context)
        .await
        .expect("validation should run");

    assert!(validation.valid, "{:?}", validation.diagnostics);
    let _ = fs::remove_dir_all(root).await;
}

#[tokio::test]
async fn source_remove_rejects_both_reserved_routes_without_publication() {
    let (root, service, context) = initialized_repository("remove-reserved-route").await;
    let original = fs::read(service.map_path())
        .await
        .expect("original map should read");

    for reserved_id in ["repository-software-model", "repository-business-glossary"] {
        let error = service
            .remove_source(&context, reserved_id.to_owned())
            .await
            .expect_err("reserved source removal must fail closed");
        assert!(matches!(error, KnowledgeMapServiceError::Domain(_)));
        assert_eq!(
            fs::read(service.map_path())
                .await
                .expect("map should remain readable"),
            original,
            "reserved removal must not publish a partial map"
        );
    }
    let _ = fs::remove_dir_all(root).await;
}

#[tokio::test]
async fn first_source_add_bootstraps_a_complete_valid_knowledge_contract() {
    let root = temp_root("source-add-bootstrap");
    fs::create_dir_all(&root).await.expect("root should create");
    write_agents_contract(&root).await;
    let service = KnowledgeMapService::new(root.clone());
    let context = RequestContext::for_interface(InterfaceKind::Cli);

    service
        .add_source(
            &context,
            KnowledgeMapSourceAddRequest {
                id: "architecture-guide".to_owned(),
                topic: "architecture".to_owned(),
                kind: KnowledgeMapSourceKind::Doc,
                uri: "docs/architecture.md".to_owned(),
                source_scope: Some("docs".to_owned()),
                description: None,
            },
        )
        .await
        .expect("first source add should bootstrap the map");

    for directory in contracts::baseline_directories(RepositoryMapType::Knowledge) {
        for key_file in directory.key_files {
            assert!(
                fs::try_exists(root.join(&key_file))
                    .await
                    .expect("baseline file should be probed"),
                "missing baseline key file {key_file}"
            );
        }
    }
    let glossary = fs::read(root.join(BUSINESS_GLOSSARY_RELATIVE_PATH))
        .await
        .expect("business glossary should be created");
    BusinessGlossary::parse(&glossary).expect("created business glossary should validate");
    let validation = service
        .validate(&context)
        .await
        .expect("bootstrapped contract should validate");
    assert!(validation.valid, "{:?}", validation.diagnostics);
    let _ = fs::remove_dir_all(root).await;
}

#[tokio::test]
async fn invalid_first_source_adds_leave_no_repository_map_contracts() {
    let cases = [
        (
            "blank-id",
            KnowledgeMapSourceAddRequest {
                id: "  ".to_owned(),
                topic: "architecture".to_owned(),
                kind: KnowledgeMapSourceKind::Doc,
                uri: "docs/architecture.md".to_owned(),
                source_scope: Some("docs".to_owned()),
                description: None,
            },
        ),
        (
            "reserved-conflict",
            KnowledgeMapSourceAddRequest {
                id: "repository-software-model".to_owned(),
                topic: "architecture".to_owned(),
                kind: KnowledgeMapSourceKind::Doc,
                uri: "docs/generated-model.md".to_owned(),
                source_scope: Some("docs".to_owned()),
                description: None,
            },
        ),
    ];

    for (label, request) in cases {
        let root = temp_root(label);
        let service = KnowledgeMapService::new(root.clone());
        let context = RequestContext::for_interface(InterfaceKind::Cli);

        let error = service
            .add_source(&context, request)
            .await
            .expect_err("invalid first source add should fail before publication");

        assert!(matches!(error, KnowledgeMapServiceError::Domain(_)));
        for directory in [
            AGENT_CONTRACT_DIR_NAME,
            LEGACY_AGENT_CONTRACT_DIR_NAME,
            CODESPEC_DIR_NAME,
        ] {
            assert!(
                !fs::try_exists(root.join(directory))
                    .await
                    .expect("contract directory should be probed"),
                "invalid case {label} created {directory}"
            );
        }
        assert!(
            !fs::try_exists(root.join(BUSINESS_GLOSSARY_RELATIVE_PATH))
                .await
                .expect("glossary should be probed"),
            "invalid case {label} created the glossary"
        );
    }
}

#[tokio::test]
async fn source_add_migrates_a_legacy_only_map_and_rollback_restores_exact_bytes() {
    let root = temp_root("legacy-source-add");
    fs::create_dir_all(root.join(LEGACY_AGENT_CONTRACT_DIR_NAME))
        .await
        .expect("legacy contract directory should create");
    write_agents_contract(&root).await;
    let mut legacy = KnowledgeMap::initial("unix:1".to_owned());
    legacy
        .remove_source("repository-software-model")
        .expect("old fixture should omit the software route");
    legacy
        .remove_source("repository-business-glossary")
        .expect("old fixture should omit the business route");
    let original = serde_norway::to_string(&legacy).expect("legacy map should serialize");
    fs::write(root.join(LEGACY_KNOWLEDGE_MAP_RELATIVE_PATH), &original)
        .await
        .expect("legacy root should write");
    let service = KnowledgeMapService::new(root.clone());
    let context = RequestContext::for_interface(InterfaceKind::Cli);

    let error = service
        .add_source(
            &context,
            KnowledgeMapSourceAddRequest {
                id: "repository-software-model".to_owned(),
                topic: "software-model".to_owned(),
                kind: KnowledgeMapSourceKind::Repo,
                uri: ".".to_owned(),
                source_scope: Some("repository".to_owned()),
                description: None,
            },
        )
        .await
        .expect_err("duplicate legacy source add should fail before migration");
    assert!(matches!(error, KnowledgeMapServiceError::Domain(_)));
    assert_eq!(
        fs::read_to_string(service.legacy_map_path())
            .await
            .expect("legacy root should remain readable"),
        original
    );
    assert!(!fs::try_exists(service.map_path()).await.unwrap());
    assert!(!fs::try_exists(service.legacy_backup_path()).await.unwrap());

    service
        .add_source(
            &context,
            KnowledgeMapSourceAddRequest {
                id: "architecture-guide".to_owned(),
                topic: "architecture".to_owned(),
                kind: KnowledgeMapSourceKind::Doc,
                uri: "docs/architecture.md".to_owned(),
                source_scope: Some("docs".to_owned()),
                description: None,
            },
        )
        .await
        .expect("controlled mutation should migrate and repair the legacy map");

    assert!(
        fs::read_to_string(service.map_path())
            .await
            .expect("visible root should read")
            .contains("schema_version: 4")
    );
    assert!(
        fs::read_to_string(service.legacy_map_path())
            .await
            .expect("legacy redirect should read")
            .contains("artifact_kind: redirect")
    );
    assert_eq!(
        fs::read_to_string(service.legacy_backup_path())
            .await
            .expect("rollback backup should read"),
        original
    );
    let validation = service
        .validate(&context)
        .await
        .expect("migrated mutation should validate");
    assert!(validation.valid, "{:?}", validation.diagnostics);

    service
        .rollback_v3(&context)
        .await
        .expect("valid legacy backup should remain rollback-compatible");
    assert_eq!(
        fs::read_to_string(service.legacy_map_path())
            .await
            .expect("restored legacy root should read"),
        original
    );
    assert!(!fs::try_exists(service.map_path()).await.unwrap());
    let rolled_back_validation = service
        .validate(&context)
        .await
        .expect("restored old map should return current diagnostics");
    assert!(!rolled_back_validation.valid);
    assert!(
        rolled_back_validation
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.contains("repository-software-model"))
    );
    let _ = fs::remove_dir_all(root).await;
}

#[tokio::test]
async fn source_update_migrates_and_repairs_a_legacy_only_map_before_publication() {
    let (root, service, context, original) =
        legacy_only_repository_with_ordinary_source("legacy-source-update").await;

    let error = service
        .update_source(
            &context,
            KnowledgeMapChange {
                id: "missing-source".to_owned(),
                topic: None,
                kind: None,
                uri: None,
                source_scope: None,
                description: Some("This update must not publish migration state.".to_owned()),
            },
        )
        .await
        .expect_err("missing legacy source update should fail before migration");
    assert!(matches!(error, KnowledgeMapServiceError::Domain(_)));
    assert_eq!(
        fs::read_to_string(service.legacy_map_path())
            .await
            .expect("legacy root should remain readable"),
        original
    );
    assert!(!fs::try_exists(service.map_path()).await.unwrap());
    assert!(!fs::try_exists(service.legacy_backup_path()).await.unwrap());

    service
        .update_source(
            &context,
            KnowledgeMapChange {
                id: "architecture-guide".to_owned(),
                topic: None,
                kind: None,
                uri: None,
                source_scope: None,
                description: Some("Reviewed architecture guidance.".to_owned()),
            },
        )
        .await
        .expect("controlled update should migrate and repair the legacy map");

    assert_migrated_contract_is_valid_and_rollbackable(&root, &service, &context, &original).await;
    let _ = fs::remove_dir_all(root).await;
}

#[tokio::test]
async fn source_remove_migrates_and_repairs_a_legacy_only_map_before_publication() {
    let (root, service, context, original) =
        legacy_only_repository_with_ordinary_source("legacy-source-remove").await;

    let error = service
        .remove_source(&context, "missing-source".to_owned())
        .await
        .expect_err("missing legacy source removal should fail before migration");
    assert!(matches!(error, KnowledgeMapServiceError::Domain(_)));
    assert_eq!(
        fs::read_to_string(service.legacy_map_path())
            .await
            .expect("legacy root should remain readable"),
        original
    );
    assert!(!fs::try_exists(service.map_path()).await.unwrap());
    assert!(!fs::try_exists(service.legacy_backup_path()).await.unwrap());

    service
        .remove_source(&context, "architecture-guide".to_owned())
        .await
        .expect("controlled removal should migrate and repair the legacy map");

    assert_migrated_contract_is_valid_and_rollbackable(&root, &service, &context, &original).await;
    let _ = fs::remove_dir_all(root).await;
}

#[tokio::test]
async fn validate_accepts_the_legacy_glossary_uri_in_the_legacy_namespace() {
    let root = temp_root("legacy-glossary-uri");
    fs::create_dir_all(root.join(LEGACY_AGENT_CONTRACT_DIR_NAME))
        .await
        .expect("legacy contract directory should create");
    write_agents_contract(&root).await;
    let mut map = KnowledgeMap::initial("unix:1".to_owned());
    map.sources
        .iter_mut()
        .find(|source| source.id == "repository-business-glossary")
        .expect("reserved glossary source should exist")
        .uri = LEGACY_BUSINESS_GLOSSARY_RELATIVE_PATH.to_owned();
    fs::write(
        root.join(LEGACY_KNOWLEDGE_MAP_RELATIVE_PATH),
        serde_norway::to_string(&map).expect("legacy map should serialize"),
    )
    .await
    .expect("legacy map should write");
    fs::write(
        root.join(LEGACY_BUSINESS_GLOSSARY_RELATIVE_PATH),
        serialize_yaml(&BusinessGlossary::empty_v1()).expect("glossary should serialize"),
    )
    .await
    .expect("legacy glossary should write");
    let service = KnowledgeMapService::new(root.clone());
    let context = RequestContext::for_interface(InterfaceKind::Cli);

    let validation = service
        .validate(&context)
        .await
        .expect("legacy contract should validate");

    assert!(validation.valid, "{:?}", validation.diagnostics);
    let _ = fs::remove_dir_all(root).await;
}

#[tokio::test]
async fn validate_legacy_root_reads_its_canonical_routed_glossary() {
    let root = temp_root("legacy-canonical-glossary-uri");
    fs::create_dir_all(root.join(LEGACY_AGENT_CONTRACT_DIR_NAME))
        .await
        .expect("legacy contract directory should create");
    fs::create_dir_all(root.join("knowledge/glossary"))
        .await
        .expect("canonical glossary directory should create");
    write_agents_contract(&root).await;
    let map = KnowledgeMap::initial("unix:1".to_owned());
    fs::write(
        root.join(LEGACY_KNOWLEDGE_MAP_RELATIVE_PATH),
        serde_norway::to_string(&map).expect("legacy map should serialize"),
    )
    .await
    .expect("legacy map should write");
    fs::write(
        root.join(BUSINESS_GLOSSARY_RELATIVE_PATH),
        serialize_yaml(&BusinessGlossary::empty_v1()).expect("glossary should serialize"),
    )
    .await
    .expect("canonical glossary should write");
    let service = KnowledgeMapService::new(root.clone());
    let context = RequestContext::for_interface(InterfaceKind::Cli);

    let validation = service
        .validate(&context)
        .await
        .expect("legacy contract should validate");

    assert!(validation.valid, "{:?}", validation.diagnostics);
    let _ = fs::remove_dir_all(root).await;
}

#[tokio::test]
async fn source_update_rejects_the_legacy_glossary_uri_before_publication() {
    let (root, service, context) =
        initialized_repository("reject-visible-legacy-glossary-uri").await;
    let original = fs::read(service.map_path())
        .await
        .expect("visible v3 root should read");

    let error = service
        .update_source(
            &context,
            KnowledgeMapChange {
                id: "repository-business-glossary".to_owned(),
                topic: None,
                kind: None,
                uri: Some(LEGACY_BUSINESS_GLOSSARY_RELATIVE_PATH.to_owned()),
                source_scope: None,
                description: None,
            },
        )
        .await
        .expect_err("visible v3 mutations must reject the legacy glossary URI");

    assert!(matches!(error, KnowledgeMapServiceError::Domain(_)));
    assert_eq!(
        fs::read(service.map_path())
            .await
            .expect("rejected mutation must not rewrite the root"),
        original
    );
    let _ = fs::remove_dir_all(root).await;
}

#[tokio::test]
async fn init_records_legacy_glossary_uri_migration_in_a_visible_v4_map() {
    let (root, service, context) = initialized_repository("visible-legacy-glossary-uri").await;
    let mut manifest = parse_manifest(
        &fs::read_to_string(service.map_path())
            .await
            .expect("manifest should read"),
    )
    .expect("manifest should parse");
    let topic_ref = manifest
        .topics
        .iter_mut()
        .find(|topic| topic.id == "business-knowledge")
        .expect("business topic should exist");
    let mut shard: KnowledgeMapTopicShard = serde_norway::from_str(
        &fs::read_to_string(root.join(AGENT_CONTRACT_DIR_NAME).join(&topic_ref.r#ref))
            .await
            .expect("business shard should read"),
    )
    .expect("business shard should parse");
    shard
        .sources
        .iter_mut()
        .find(|source| source.id == "repository-business-glossary")
        .expect("reserved glossary source should exist")
        .uri = LEGACY_BUSINESS_GLOSSARY_RELATIVE_PATH.to_owned();
    let yaml = serialize_yaml(&shard).expect("tampered shard should serialize");
    topic_ref.digest = content_digest(yaml.as_bytes());
    topic_ref.r#ref = format!(
        "{KNOWLEDGE_MAP_TOPICS_DIR_NAME}/topic-{}-{}.yaml",
        stable_id(&topic_ref.id),
        topic_ref.digest
    );
    fs::write(
        root.join(AGENT_CONTRACT_DIR_NAME).join(&topic_ref.r#ref),
        yaml,
    )
    .await
    .expect("tampered shard should write");
    fs::write(
        service.map_path(),
        serialize_yaml(&manifest).expect("manifest should serialize"),
    )
    .await
    .expect("manifest should write");

    let validation = service
        .validate(&context)
        .await
        .expect("validation should report the namespace mismatch");

    assert!(!validation.valid);
    assert!(
        validation
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.contains(BUSINESS_GLOSSARY_RELATIVE_PATH))
    );

    let initialized = service
        .init(&context)
        .await
        .expect("writer initialization should publish the canonical glossary URI");
    assert!(initialized.summary.contains("canonical artifact"));
    assert!(
        service
            .validate(&context)
            .await
            .expect("validation should run after the migration")
            .valid
    );
    let repaired_manifest = parse_manifest(
        &fs::read_to_string(service.map_path())
            .await
            .expect("repaired manifest should read"),
    )
    .expect("repaired manifest should parse");
    let migration_entry = repaired_manifest
        .history
        .recent
        .last()
        .expect("glossary migration should append history");
    assert_eq!(migration_entry.action, "source.migrate");
    assert!(migration_entry.summary.contains("business glossary"));
    assert!(!migration_entry.summary.contains("history"));
    let repaired_topic_ref = repaired_manifest
        .topics
        .iter()
        .find(|topic| topic.id == "business-knowledge")
        .expect("repaired business topic should exist");
    let repaired_shard: KnowledgeMapTopicShard = serde_norway::from_str(
        &fs::read_to_string(
            root.join(AGENT_CONTRACT_DIR_NAME)
                .join(&repaired_topic_ref.r#ref),
        )
        .await
        .expect("repaired business shard should read"),
    )
    .expect("repaired business shard should parse");
    assert_eq!(
        repaired_shard
            .sources
            .iter()
            .find(|source| source.id == "repository-business-glossary")
            .expect("repaired glossary source should exist")
            .uri,
        BUSINESS_GLOSSARY_RELATIVE_PATH
    );
    let _ = fs::remove_dir_all(root).await;
}

async fn initialized_repository(label: &str) -> (PathBuf, KnowledgeMapService, RequestContext) {
    let root = temp_root(label);
    fs::create_dir_all(&root).await.expect("root should create");
    write_agents_contract(&root).await;
    let service = KnowledgeMapService::new(root.clone());
    let context = RequestContext::for_interface(InterfaceKind::Cli);
    service.init(&context).await.expect("map should initialize");
    (root, service, context)
}

async fn legacy_only_repository_with_ordinary_source(
    label: &str,
) -> (PathBuf, KnowledgeMapService, RequestContext, String) {
    let root = temp_root(label);
    fs::create_dir_all(root.join(LEGACY_AGENT_CONTRACT_DIR_NAME))
        .await
        .expect("legacy contract directory should create");
    write_agents_contract(&root).await;
    let mut legacy = KnowledgeMap::initial("unix:1".to_owned());
    legacy
        .remove_source("repository-software-model")
        .expect("old fixture should omit the software route");
    legacy
        .remove_source("repository-business-glossary")
        .expect("old fixture should omit the business route");
    legacy
        .add_source(
            KnowledgeMapSource::new(
                "architecture-guide".to_owned(),
                "architecture".to_owned(),
                KnowledgeMapSourceKind::Doc,
                "docs/architecture.md".to_owned(),
                Some("docs".to_owned()),
                None,
            )
            .expect("ordinary source should construct"),
        )
        .expect("ordinary source should seed the legacy map");
    let original = serde_norway::to_string(&legacy).expect("legacy map should serialize");
    fs::write(root.join(LEGACY_KNOWLEDGE_MAP_RELATIVE_PATH), &original)
        .await
        .expect("legacy root should write");
    let service = KnowledgeMapService::new(root.clone());
    let context = RequestContext::for_interface(InterfaceKind::Cli);
    (root, service, context, original)
}

async fn assert_migrated_contract_is_valid_and_rollbackable(
    root: &std::path::Path,
    service: &KnowledgeMapService,
    context: &RequestContext,
    original: &str,
) {
    let validation = service
        .validate(context)
        .await
        .expect("migrated mutation should validate");
    assert!(validation.valid, "{:?}", validation.diagnostics);
    assert!(
        fs::read_to_string(service.legacy_map_path())
            .await
            .expect("legacy redirect should read")
            .contains("artifact_kind: redirect")
    );
    assert_eq!(
        fs::read_to_string(service.legacy_backup_path())
            .await
            .expect("rollback backup should read"),
        original
    );

    service
        .rollback_v3(context)
        .await
        .expect("controlled mutation must preserve rollback");
    assert_eq!(
        fs::read_to_string(service.legacy_map_path())
            .await
            .expect("restored legacy root should read"),
        original
    );
    assert!(!fs::try_exists(service.map_path()).await.unwrap());
    assert_eq!(
        service
            .read_contract_dir_name()
            .await
            .expect("rolled-back namespace should resolve"),
        LEGACY_AGENT_CONTRACT_DIR_NAME
    );
    assert!(root.join(LEGACY_AGENT_CONTRACT_DIR_NAME).is_dir());
}

async fn write_agents_contract(root: &std::path::Path) {
    fs::write(
        root.join("AGENTS.md"),
        format!("Knowledge map: {KNOWLEDGE_MAP_RELATIVE_PATH}\n"),
    )
    .await
    .expect("AGENTS contract should write");
}

async fn remove_topic_ref(service: &KnowledgeMapService, topic: &str) {
    let mut manifest = parse_manifest(
        &fs::read_to_string(service.map_path())
            .await
            .expect("manifest should read"),
    )
    .expect("manifest should parse");
    manifest.topics.retain(|entry| entry.id != topic);
    fs::write(
        service.map_path(),
        serialize_yaml(&manifest).expect("manifest should serialize"),
    )
    .await
    .expect("manifest should write");
}

fn temp_root(label: &str) -> PathBuf {
    std::env::temp_dir().join(format!(
        "relay-knowledge-map-reserved-{label}-{}",
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("time should work")
            .as_nanos()
    ))
}
