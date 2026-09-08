use std::{
    fs,
    sync::Arc,
    time::{SystemTime, UNIX_EPOCH},
};

use relay_knowledge::{
    adapters::SqliteKnowledgeStoreFactory,
    api::{HybridRetrievalRequest, IngestEvidence, IngestRequest, InterfaceKind, RequestContext},
    application::{RelayKnowledgeService, RuntimeConfiguration},
    domain::FreshnessPolicy,
    env::{EnvironmentConfig, PlatformKind},
};

#[tokio::test]
async fn windows_upgrade_reopens_existing_graph_for_cli_web_and_pinned_service() {
    for topology in ["single_sqlite", "partitioned_sqlite"] {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!(
            "relay-windows-upgrade-{}-{nonce}",
            std::process::id()
        ));
        let legacy = root.join("local/relay-knowledge/data");
        let mut environment = EnvironmentConfig::from_pairs(
            PlatformKind::Windows,
            [
                ("APPDATA", root.join("roaming").display().to_string()),
                ("LOCALAPPDATA", root.join("local").display().to_string()),
                ("TEMP", root.join("tmp").display().to_string()),
                ("RELAY_KNOWLEDGE_DATA_DIR", legacy.display().to_string()),
                ("RELAY_KNOWLEDGE_STORAGE_TOPOLOGY", topology.to_owned()),
            ],
        )
        .expect("isolated Windows environment");
        let runtime = RuntimeConfiguration::from_environment(&environment)
            .await
            .unwrap();
        let factory = Arc::new(SqliteKnowledgeStoreFactory::new(
            runtime.paths.clone(),
            runtime.storage.topology,
        ));
        let old_service =
            RelayKnowledgeService::with_runtime_adapters(runtime, factory, None, None);
        old_service
            .ingest(
                IngestRequest {
                    source_scope: "upgrade".to_owned(),
                    evidence: vec![IngestEvidence {
                        id: Some("existing-evidence".to_owned()),
                        source_path: None,
                        span: None,
                        confidence: None,
                        status: None,
                        content: "Preserved SQLite knowledge".to_owned(),
                        entity_labels: Vec::new(),
                        extraction: None,
                    }],
                    relations: Vec::new(),
                    claims: Vec::new(),
                    events: Vec::new(),
                },
                RequestContext::with_ids(InterfaceKind::Cli, "seed", "seed"),
            )
            .await
            .unwrap();
        drop(old_service);

        // An upgraded CLI and Web runtime with no override must select the same
        // store as the installed service's retained explicit data path.
        for interface in [InterfaceKind::Cli, InterfaceKind::Web] {
            environment.paths.data_dir = None;
            let runtime = RuntimeConfiguration::from_environment(&environment)
                .await
                .unwrap();
            assert_eq!(runtime.paths.data_dir, legacy);
            let factory = Arc::new(SqliteKnowledgeStoreFactory::new(
                runtime.paths.clone(),
                runtime.storage.topology,
            ));
            let service =
                RelayKnowledgeService::with_runtime_adapters(runtime, factory, None, None);
            let status = service
                .project_status(RequestContext::with_ids(interface, "status", "status"))
                .await
                .unwrap();
            assert_eq!(status.metadata.graph_version, 1);
            let response = service
                .retrieve_context(
                    HybridRetrievalRequest {
                        query: "SQLite".to_owned(),
                        source_scope: Some("upgrade".to_owned()),
                        limit: 5,
                        freshness: FreshnessPolicy::WaitUntilFresh,
                    },
                    RequestContext::with_ids(interface, "query", "query"),
                )
                .await
                .unwrap();
            assert!(
                !response.context_pack.items.is_empty(),
                "existing graph must remain queryable"
            );
            environment.paths.data_dir = Some(legacy.clone());
            let pinned_service = RuntimeConfiguration::from_environment(&environment)
                .await
                .unwrap();
            assert_eq!(pinned_service.paths.data_dir, legacy);
        }
        fs::remove_dir_all(root).expect("remove isolated storage");
    }
}
