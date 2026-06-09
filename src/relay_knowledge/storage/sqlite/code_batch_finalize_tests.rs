use crate::{
    domain::{
        CodeImportRecord, CodeIndexBatch, CodeIndexResourceBudget, CodeIndexSession,
        CodeParseStatus, CodeQueryKind, CodeRepositoryRegistration, CodeRepositorySelector,
        CodeRetrievalRequest, FreshnessPolicy, RepositoryCodeChunkRecord, RepositoryCodeFileRecord,
        RepositoryCodeRange, RepositoryCodeReferenceRecord, RepositoryCodeSymbolRecord,
    },
    storage::{CodeRepositoryStore, SqliteGraphStore},
};
use std::collections::BTreeMap;

#[tokio::test]
async fn checkpointed_batches_finalize_cross_batch_call_edges() {
    let store = registered_store().await;
    let source_scope = "git_snapshot:call-finalize";
    let session = session_for_scope(source_scope, 2);
    let target_file = file(
        source_scope,
        "target-file",
        "src/target.rs",
        "rust",
        CodeParseStatus::Parsed,
    );
    let caller_file = file(
        source_scope,
        "caller-file",
        "src/caller.rs",
        "rust",
        CodeParseStatus::Parsed,
    );
    let target_symbol = symbol(
        source_scope,
        "target-symbol",
        "target-file",
        "src/target.rs",
        "target",
        "rust",
    );
    let target_reference = reference(
        source_scope,
        "target-reference",
        "caller-file",
        "src/caller.rs",
        "target",
    );

    store
        .begin_code_index_session(session.clone())
        .await
        .expect("session should begin");
    let checkpoint = store
        .apply_code_index_batch(CodeIndexBatch {
            files: vec![target_file],
            symbols: vec![target_symbol],
            ..batch(source_scope, 1)
        })
        .await
        .expect("first batch should persist");
    assert_eq!(checkpoint.batch_count, 1);
    let indexing_status = store
        .code_repository_status("fixture".to_owned())
        .await
        .expect("status should load")
        .expect("status should exist");
    assert_eq!(indexing_status.state, "indexing");
    assert_eq!(indexing_status.indexed_file_count, 1);
    store
        .apply_code_index_batch(CodeIndexBatch {
            files: vec![caller_file],
            references: vec![target_reference],
            ..batch(source_scope, 2)
        })
        .await
        .expect("second batch should persist");
    let summary = store
        .finalize_code_index_session(session)
        .await
        .expect("session should finalize");

    assert_eq!(summary.progress.batch_count, 2);
    assert_eq!(summary.progress.checkpoint_file_count, 2);
    let hits = search(&store, "target", CodeQueryKind::Callers).await;

    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0].edge_resolution_state.as_deref(), Some("resolved"));
    assert_eq!(search_document_count(&store, source_scope, "call").await, 1);
}

#[tokio::test]
async fn checkpointed_finalize_preserves_reference_resolution_rules() {
    let store = registered_store().await;
    let source_scope = "git_snapshot:reference-finalize-rules";
    let session = session_for_scope(source_scope, 3);
    let file_a = file(
        source_scope,
        "file-a",
        "src/a.rs",
        "rust",
        CodeParseStatus::Parsed,
    );
    let file_b = file(
        source_scope,
        "file-b",
        "src/b.rs",
        "rust",
        CodeParseStatus::Parsed,
    );
    let file_c = file(
        source_scope,
        "file-c",
        "src/c.rs",
        "rust",
        CodeParseStatus::Parsed,
    );
    let symbols = vec![
        symbol(
            source_scope,
            "global-symbol",
            "file-a",
            "src/a.rs",
            "global",
            "rust",
        ),
        symbol(
            source_scope,
            "shared-a",
            "file-a",
            "src/a.rs",
            "shared",
            "rust",
        ),
        symbol(
            source_scope,
            "shared-b",
            "file-b",
            "src/b.rs",
            "shared",
            "rust",
        ),
        symbol(
            source_scope,
            "ambiguous-a",
            "file-a",
            "src/a.rs",
            "ambiguous",
            "rust",
        ),
        symbol(
            source_scope,
            "ambiguous-b",
            "file-b",
            "src/b.rs",
            "ambiguous",
            "rust",
        ),
    ];
    let references = vec![
        reference(source_scope, "global-ref", "file-c", "src/c.rs", "global"),
        reference(source_scope, "shared-ref", "file-a", "src/a.rs", "shared"),
        reference(
            source_scope,
            "ambiguous-ref",
            "file-c",
            "src/c.rs",
            "ambiguous",
        ),
        reference(source_scope, "missing-ref", "file-c", "src/c.rs", "missing"),
    ];

    store
        .begin_code_index_session(session.clone())
        .await
        .expect("session should begin");
    store
        .apply_code_index_batch(CodeIndexBatch {
            parsed_byte_count: 60,
            files: vec![file_a, file_b, file_c],
            symbols,
            references,
            ..batch(source_scope, 1)
        })
        .await
        .expect("batch should persist");
    store
        .finalize_code_index_session(session)
        .await
        .expect("session should finalize");

    let resolved = reference_resolution_rows(&store, source_scope).await;

    assert_eq!(
        resolved.get("global-ref"),
        Some(&(
            "resolved".to_owned(),
            Some("global-symbol".to_owned()),
            8_000,
            "inferred".to_owned()
        ))
    );
    assert_eq!(
        resolved.get("shared-ref"),
        Some(&(
            "resolved".to_owned(),
            Some("shared-a".to_owned()),
            8_000,
            "inferred".to_owned()
        ))
    );
    assert_eq!(
        resolved.get("ambiguous-ref"),
        Some(&("ambiguous".to_owned(), None, 5_000, "ambiguous".to_owned()))
    );
    assert_eq!(
        resolved.get("missing-ref"),
        Some(&("unresolved".to_owned(), None, 2_500, "ambiguous".to_owned()))
    );
}

#[tokio::test]
async fn checkpointed_batches_finalize_python_import_edges() {
    let store = registered_store().await;
    let source_scope = "git_snapshot:python-imports";
    let session = session_for_scope(source_scope, 2);
    let model_file = file(
        source_scope,
        "model-file",
        "src/relay_teams/connector/w3_models.py",
        "python",
        CodeParseStatus::Parsed,
    );
    let service_file = file(
        source_scope,
        "service-file",
        "src/relay_teams/connector/service.py",
        "python",
        CodeParseStatus::Parsed,
    );
    let request_symbol = symbol(
        source_scope,
        "request-symbol",
        "model-file",
        "src/relay_teams/connector/w3_models.py",
        "W3ConnectorSaveRequest",
        "python",
    );
    let service_import = import(
        source_scope,
        "service-import",
        "service-file",
        "src/relay_teams/connector/service.py",
        "from relay_teams.connector.w3_models import W3ConnectorSaveRequest",
    );

    store
        .begin_code_index_session(session.clone())
        .await
        .expect("session should begin");
    store
        .apply_code_index_batch(CodeIndexBatch {
            files: vec![model_file],
            symbols: vec![request_symbol],
            ..batch(source_scope, 1)
        })
        .await
        .expect("model batch should persist");
    store
        .apply_code_index_batch(CodeIndexBatch {
            files: vec![service_file],
            imports: vec![service_import],
            ..batch(source_scope, 2)
        })
        .await
        .expect("service batch should persist");
    store
        .finalize_code_index_session(session)
        .await
        .expect("session should finalize");

    let hits = search(&store, "W3ConnectorSaveRequest", CodeQueryKind::Imports).await;

    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0].path, "src/relay_teams/connector/service.py");
    assert_eq!(hits[0].edge_resolution_state.as_deref(), Some("resolved"));
}

#[tokio::test]
async fn checkpointed_batches_finalize_relative_python_import_edges() {
    let store = registered_store().await;
    let source_scope = "git_snapshot:python-relative-imports";
    let session = session_for_scope(source_scope, 2);
    let model_file = file(
        source_scope,
        "model-file",
        "src/relay_teams/connector/w3_models.py",
        "python",
        CodeParseStatus::Parsed,
    );
    let service_file = file(
        source_scope,
        "service-file",
        "src/relay_teams/connector/service.py",
        "python",
        CodeParseStatus::Parsed,
    );
    let request_symbol = symbol(
        source_scope,
        "request-symbol",
        "model-file",
        "src/relay_teams/connector/w3_models.py",
        "W3ConnectorSaveRequest",
        "python",
    );
    let service_import = import(
        source_scope,
        "service-import",
        "service-file",
        "src/relay_teams/connector/service.py",
        "from .w3_models import W3ConnectorSaveRequest",
    );

    store
        .begin_code_index_session(session.clone())
        .await
        .expect("session should begin");
    store
        .apply_code_index_batch(CodeIndexBatch {
            files: vec![model_file],
            symbols: vec![request_symbol],
            ..batch(source_scope, 1)
        })
        .await
        .expect("model batch should persist");
    store
        .apply_code_index_batch(CodeIndexBatch {
            files: vec![service_file],
            imports: vec![service_import],
            ..batch(source_scope, 2)
        })
        .await
        .expect("service batch should persist");
    store
        .finalize_code_index_session(session)
        .await
        .expect("session should finalize");

    let hits = search(&store, "W3ConnectorSaveRequest", CodeQueryKind::Imports).await;

    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0].path, "src/relay_teams/connector/service.py");
    assert_eq!(hits[0].edge_resolution_state.as_deref(), Some("resolved"));
}

#[tokio::test]
async fn checkpointed_finalize_materializes_effective_maven_dependencies() {
    let store = registered_store().await;
    let source_scope = "git_snapshot:maven-effective-dependencies";
    let session = session_for_scope(source_scope, 1);
    let content = r#"<project>
  <modelVersion>4.0.0</modelVersion>
  <groupId>com.acme</groupId>
  <artifactId>service</artifactId>
  <version>1.0.0</version>
  <properties><slf4j.version>2.0.9</slf4j.version></properties>
  <dependencyManagement>
    <dependencies>
      <dependency>
        <groupId>org.slf4j</groupId>
        <artifactId>slf4j-api</artifactId>
        <version>${slf4j.version}</version>
      </dependency>
      <dependency>
        <groupId>org.junit</groupId>
        <artifactId>junit-bom</artifactId>
        <version>5.10.1</version>
        <type>pom</type>
        <scope>import</scope>
      </dependency>
    </dependencies>
  </dependencyManagement>
  <dependencies>
    <dependency>
      <groupId>org.slf4j</groupId>
      <artifactId>slf4j-api</artifactId>
    </dependency>
  </dependencies>
</project>"#;

    store
        .begin_code_index_session(session.clone())
        .await
        .expect("session should begin");
    store
        .apply_code_index_batch(CodeIndexBatch {
            files: vec![file(
                source_scope,
                "pom-file",
                "pom.xml",
                "xml",
                CodeParseStatus::Parsed,
            )],
            chunks: vec![chunk(
                source_scope,
                "pom-chunk",
                "pom-file",
                "pom.xml",
                content,
            )],
            ..batch(source_scope, 1)
        })
        .await
        .expect("batch should persist");
    store
        .finalize_code_index_session(session)
        .await
        .expect("session should finalize");

    let slf4j_hits = search(&store, "org.slf4j:slf4j-api", CodeQueryKind::Sbom).await;
    assert_eq!(slf4j_hits.len(), 1);
    assert!(slf4j_hits[0].excerpt.contains("2.0.9"));
    assert_eq!(
        slf4j_hits[0].edge_resolution_state.as_deref(),
        Some("declared")
    );

    let bom_hits = search(&store, "org.junit:junit-bom", CodeQueryKind::Sbom).await;
    assert_eq!(bom_hits.len(), 1);
    assert!(bom_hits[0].excerpt.contains("group=bom"));
}

#[tokio::test]
async fn checkpointed_batches_finalize_java_import_edges_under_maven_roots() {
    let store = registered_store().await;
    let source_scope = "git_snapshot:java-imports";
    let session = session_for_scope(source_scope, 2);
    let context_file = file(
        source_scope,
        "context-file",
        "src/main/java/org/springframework/context/ApplicationContext.java",
        "java",
        CodeParseStatus::Parsed,
    );
    let loader_file = file(
        source_scope,
        "loader-file",
        "src/main/java/org/springframework/context/support/ContextLoader.java",
        "java",
        CodeParseStatus::Parsed,
    );
    let context_symbol = symbol(
        source_scope,
        "context-symbol",
        "context-file",
        "src/main/java/org/springframework/context/ApplicationContext.java",
        "ApplicationContext",
        "java",
    );
    let loader_import = import(
        source_scope,
        "loader-import",
        "loader-file",
        "src/main/java/org/springframework/context/support/ContextLoader.java",
        "import org.springframework.context.ApplicationContext;",
    );

    store
        .begin_code_index_session(session.clone())
        .await
        .expect("session should begin");
    store
        .apply_code_index_batch(CodeIndexBatch {
            files: vec![context_file],
            symbols: vec![context_symbol],
            ..batch(source_scope, 1)
        })
        .await
        .expect("context batch should persist");
    store
        .apply_code_index_batch(CodeIndexBatch {
            files: vec![loader_file],
            imports: vec![loader_import],
            ..batch(source_scope, 2)
        })
        .await
        .expect("loader batch should persist");
    store
        .finalize_code_index_session(session)
        .await
        .expect("session should finalize");

    let hits = search(&store, "ApplicationContext", CodeQueryKind::Imports).await;

    assert_eq!(hits.len(), 1);
    assert_eq!(
        hits[0].path,
        "src/main/java/org/springframework/context/support/ContextLoader.java"
    );
    assert_eq!(hits[0].edge_resolution_state.as_deref(), Some("resolved"));
    assert_eq!(
        hits[0].edge_target_hint.as_deref(),
        Some("src/main/java/org/springframework/context/ApplicationContext.java")
    );
}

#[tokio::test]
async fn checkpointed_batches_finalize_java_wildcard_import_edges_for_fqn_symbol_queries() {
    let store = registered_store().await;
    let source_scope = "git_snapshot:java-wildcard-imports";
    let session = session_for_scope(source_scope, 2);
    let context_file = file(
        source_scope,
        "context-file",
        "src/main/java/org/springframework/context/ApplicationContext.java",
        "java",
        CodeParseStatus::Parsed,
    );
    let loader_file = file(
        source_scope,
        "loader-file",
        "src/main/java/org/springframework/context/support/ContextLoader.java",
        "java",
        CodeParseStatus::Parsed,
    );
    let context_symbol = symbol(
        source_scope,
        "context-symbol",
        "context-file",
        "src/main/java/org/springframework/context/ApplicationContext.java",
        "ApplicationContext",
        "java",
    );
    let loader_import = import(
        source_scope,
        "loader-import",
        "loader-file",
        "src/main/java/org/springframework/context/support/ContextLoader.java",
        "import org.springframework.context.*;",
    );

    store
        .begin_code_index_session(session.clone())
        .await
        .expect("session should begin");
    store
        .apply_code_index_batch(CodeIndexBatch {
            files: vec![context_file],
            symbols: vec![context_symbol],
            ..batch(source_scope, 1)
        })
        .await
        .expect("context batch should persist");
    store
        .apply_code_index_batch(CodeIndexBatch {
            files: vec![loader_file],
            imports: vec![loader_import],
            ..batch(source_scope, 2)
        })
        .await
        .expect("loader batch should persist");
    store
        .finalize_code_index_session(session)
        .await
        .expect("session should finalize");

    let hits = search(
        &store,
        "org.springframework.context.ApplicationContext",
        CodeQueryKind::Imports,
    )
    .await;

    assert_eq!(hits.len(), 1);
    assert_eq!(
        hits[0].path,
        "src/main/java/org/springframework/context/support/ContextLoader.java"
    );
    assert_eq!(hits[0].edge_resolution_state.as_deref(), Some("resolved"));
    assert_eq!(
        hits[0].edge_target_hint.as_deref(),
        Some("org/springframework/context")
    );
}

#[tokio::test]
async fn checkpointed_batches_finalize_go_package_import_edges_for_symbol_queries() {
    let store = registered_store().await;
    let source_scope = "git_snapshot:go-imports";
    let session = session_for_scope(source_scope, 2);
    let informer_file = file(
        source_scope,
        "informer-file",
        "staging/src/k8s.io/client-go/informers/factory.go",
        "go",
        CodeParseStatus::Parsed,
    );
    let importer_file = file(
        source_scope,
        "authorizer-file",
        "pkg/kubeapiserver/authorizer/config.go",
        "go",
        CodeParseStatus::Parsed,
    );
    let informer_symbol = symbol(
        source_scope,
        "shared-informer-factory",
        "informer-file",
        "staging/src/k8s.io/client-go/informers/factory.go",
        "SharedInformerFactory",
        "go",
    );
    let importer_import = import(
        source_scope,
        "informer-import",
        "authorizer-file",
        "pkg/kubeapiserver/authorizer/config.go",
        "informers k8s.io/client-go/informers",
    );

    store
        .begin_code_index_session(session.clone())
        .await
        .expect("session should begin");
    store
        .apply_code_index_batch(CodeIndexBatch {
            files: vec![informer_file],
            symbols: vec![informer_symbol],
            ..batch(source_scope, 1)
        })
        .await
        .expect("informer batch should persist");
    store
        .apply_code_index_batch(CodeIndexBatch {
            files: vec![importer_file],
            imports: vec![importer_import],
            ..batch(source_scope, 2)
        })
        .await
        .expect("importer batch should persist");
    store
        .finalize_code_index_session(session)
        .await
        .expect("session should finalize");

    let hits = search(&store, "SharedInformerFactory", CodeQueryKind::Imports).await;

    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0].path, "pkg/kubeapiserver/authorizer/config.go");
    assert_eq!(hits[0].edge_resolution_state.as_deref(), Some("resolved"));
    assert_eq!(
        hits[0].edge_target_hint.as_deref(),
        Some("staging/src/k8s.io/client-go/informers")
    );
}

#[tokio::test]
async fn checkpointed_batch_replay_keeps_progress_counts_stable() {
    let store = registered_store().await;
    let source_scope = "git_snapshot:batch-replay";
    let session = session_for_scope(source_scope, 1);
    let indexed_file = file(
        source_scope,
        "replayed-file",
        "src/lib.rs",
        "rust",
        CodeParseStatus::Parsed,
    );
    let indexed_symbol = symbol(
        source_scope,
        "replayed-symbol",
        "replayed-file",
        "src/lib.rs",
        "run",
        "rust",
    );
    let batch = CodeIndexBatch {
        files: vec![indexed_file],
        symbols: vec![indexed_symbol],
        ..batch(source_scope, 1)
    };

    store
        .begin_code_index_session(session)
        .await
        .expect("session should begin");
    let first = store
        .apply_code_index_batch(batch.clone())
        .await
        .expect("first batch should persist");
    let replayed = store
        .apply_code_index_batch(batch)
        .await
        .expect("batch replay should remain idempotent for progress");
    let status = store
        .code_repository_status("fixture".to_owned())
        .await
        .expect("status should load")
        .expect("status should exist");

    assert_eq!(first.committed_file_count, 1);
    assert_eq!(first.committed_symbol_count, 1);
    assert_eq!(replayed.committed_file_count, 1);
    assert_eq!(replayed.committed_symbol_count, 1);
    assert_eq!(replayed.batch_count, 1);
    assert_eq!(status.indexed_file_count, 1);
    assert_eq!(status.symbol_count, 1);
}

#[tokio::test]
async fn new_checkpoint_batch_replaces_colliding_path_rows() {
    let store = registered_store().await;
    let source_scope = "git_snapshot:batch-path-collision";
    let session = session_for_scope(source_scope, 2);
    let path = "src/lib.rs";
    let batch = |batch_index, file_id: &str, symbol_id: &str, name: &str| CodeIndexBatch {
        files: vec![file(
            source_scope,
            file_id,
            path,
            "rust",
            CodeParseStatus::Parsed,
        )],
        symbols: vec![symbol(source_scope, symbol_id, file_id, path, name, "rust")],
        ..batch(source_scope, batch_index)
    };

    store
        .begin_code_index_session(session.clone())
        .await
        .expect("session should begin");
    store
        .apply_code_index_batch(batch(1, "first-file", "legacy-symbol", "legacy_handler"))
        .await
        .expect("first batch should persist");
    store
        .apply_code_index_batch(batch(2, "second-file", "current-symbol", "current_handler"))
        .await
        .expect("colliding new batch should replace path rows");
    store
        .finalize_code_index_session(session)
        .await
        .expect("session should finalize");

    let old_hits = search(&store, "legacy_handler", CodeQueryKind::Symbol).await;
    let new_hits = search(&store, "current_handler", CodeQueryKind::Symbol).await;

    assert!(old_hits.is_empty());
    assert_eq!(new_hits.len(), 1);
    assert_eq!(new_hits[0].path, path);
}

async fn registered_store() -> SqliteGraphStore {
    let store = SqliteGraphStore::open_in_memory().expect("store should open");
    store
        .upsert_code_repository(
            CodeRepositoryRegistration::new("repo", "fixture", "/tmp/repo", Vec::new(), Vec::new())
                .expect("registration should validate"),
        )
        .await
        .expect("repository should persist");

    store
}

fn batch(source_scope: &str, batch_index: usize) -> CodeIndexBatch {
    CodeIndexBatch {
        repository_id: "repo".to_owned(),
        source_scope: source_scope.to_owned(),
        batch_index,
        parsed_byte_count: 20,
        files: Vec::new(),
        symbols: Vec::new(),
        references: Vec::new(),
        imports: Vec::new(),
        dependencies: Vec::new(),
        feature_flags: Vec::new(),
        chunks: Vec::new(),
        diagnostics: Vec::new(),
    }
}

async fn reference_resolution_rows(
    store: &SqliteGraphStore,
    source_scope: &str,
) -> BTreeMap<String, (String, Option<String>, u16, String)> {
    let source_scope = source_scope.to_owned();
    store
        .run(move |connection| {
            let mut statement = connection.prepare(
                "
                SELECT reference_id, resolution_state, target_symbol_snapshot_id,
                       confidence_basis_points, confidence_tier
                FROM code_repository_references
                WHERE source_scope = ?1
                ",
            )?;
            let rows = statement.query_map([source_scope], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    (
                        row.get::<_, String>(1)?,
                        row.get::<_, Option<String>>(2)?,
                        row.get::<_, u16>(3)?,
                        row.get::<_, String>(4)?,
                    ),
                ))
            })?;

            rows.collect::<Result<BTreeMap<_, _>, _>>()
                .map_err(crate::storage::StorageError::from)
        })
        .await
        .expect("reference rows should load")
}

fn file(
    source_scope: &str,
    file_id: &str,
    path: &str,
    language_id: &str,
    parse_status: CodeParseStatus,
) -> RepositoryCodeFileRecord {
    RepositoryCodeFileRecord {
        repository_id: "repo".to_owned(),
        source_scope: source_scope.to_owned(),
        file_id: file_id.to_owned(),
        path: path.to_owned(),
        language_id: language_id.to_owned(),
        blob_hash: format!("{file_id}-hash"),
        byte_len: 20,
        line_count: 1,
        parse_status,
        is_generated: false,
        degraded_reason: None,
    }
}

fn chunk(
    source_scope: &str,
    chunk_id: &str,
    file_id: &str,
    path: &str,
    content: &str,
) -> RepositoryCodeChunkRecord {
    RepositoryCodeChunkRecord {
        repository_id: "repo".to_owned(),
        source_scope: source_scope.to_owned(),
        chunk_id: chunk_id.to_owned(),
        file_id: file_id.to_owned(),
        path: path.to_owned(),
        language_id: "xml".to_owned(),
        content: content.to_owned(),
        byte_range: RepositoryCodeRange {
            start: 0,
            end: content.len() as u32,
        },
        line_range: RepositoryCodeRange {
            start: 1,
            end: content.lines().count() as u32,
        },
        symbol_snapshot_id: None,
    }
}

fn symbol(
    source_scope: &str,
    symbol_snapshot_id: &str,
    file_id: &str,
    path: &str,
    name: &str,
    language_id: &str,
) -> RepositoryCodeSymbolRecord {
    RepositoryCodeSymbolRecord {
        repository_id: "repo".to_owned(),
        source_scope: source_scope.to_owned(),
        symbol_snapshot_id: symbol_snapshot_id.to_owned(),
        canonical_symbol_id: format!("repo://repo/{}::{name}", path.replace('/', "::")),
        file_id: file_id.to_owned(),
        path: path.to_owned(),
        language_id: language_id.to_owned(),
        name: name.to_owned(),
        qualified_name: format!("{}::{name}", path.replace('/', "::")),
        kind: "function".to_owned(),
        signature: format!("fn {name}()"),
        doc_comment: None,
        byte_range: RepositoryCodeRange { start: 0, end: 8 },
        line_range: RepositoryCodeRange { start: 1, end: 1 },
    }
}

fn reference(
    source_scope: &str,
    reference_id: &str,
    file_id: &str,
    path: &str,
    name: &str,
) -> RepositoryCodeReferenceRecord {
    RepositoryCodeReferenceRecord {
        repository_id: "repo".to_owned(),
        source_scope: source_scope.to_owned(),
        reference_id: reference_id.to_owned(),
        file_id: file_id.to_owned(),
        path: path.to_owned(),
        name: name.to_owned(),
        kind: "call".to_owned(),
        target_symbol_snapshot_id: None,
        target_hint: Some(name.to_owned()),
        resolution_state: "unresolved".to_owned(),
        confidence_basis_points: 2_500,
        confidence_tier: "ambiguous".to_owned(),
        byte_range: RepositoryCodeRange { start: 0, end: 6 },
        line_range: RepositoryCodeRange { start: 1, end: 1 },
    }
}

fn import(
    source_scope: &str,
    import_id: &str,
    file_id: &str,
    path: &str,
    module: &str,
) -> CodeImportRecord {
    CodeImportRecord {
        repository_id: "repo".to_owned(),
        source_scope: source_scope.to_owned(),
        import_id: import_id.to_owned(),
        file_id: file_id.to_owned(),
        path: path.to_owned(),
        module: module.to_owned(),
        target_hint: Some(module.to_owned()),
        resolution_state: "unresolved".to_owned(),
        confidence_basis_points: 10_000,
        confidence_tier: "extracted".to_owned(),
        line_range: RepositoryCodeRange { start: 1, end: 1 },
    }
}

fn session_for_scope(source_scope: &str, total_path_count: usize) -> CodeIndexSession {
    CodeIndexSession {
        repository_id: "repo".to_owned(),
        source_scope: source_scope.to_owned(),
        base_resolved_commit_sha: None,
        resolved_commit_sha: "commit".to_owned(),
        tree_hash: "tree".to_owned(),
        path_filters: Vec::new(),
        language_filters: Vec::new(),
        full_replace: true,
        total_path_count,
        changed_path_count: total_path_count,
        skipped_unchanged_count: 0,
        deleted_paths: Vec::new(),
        tombstones: Vec::new(),
        workspaces: Vec::new(),
        resource_budget: CodeIndexResourceBudget::new(1, 1024, 1024).expect("budget"),
    }
}

async fn search(
    store: &SqliteGraphStore,
    query: &str,
    kind: CodeQueryKind,
) -> Vec<crate::domain::CodeRetrievalHit> {
    let selector = CodeRepositorySelector::new("fixture", "commit", Vec::new(), Vec::new())
        .expect("selector should validate");
    store
        .search_code(
            CodeRetrievalRequest::new(query, selector, kind, 5, FreshnessPolicy::AllowStale)
                .expect("request should validate"),
        )
        .await
        .expect("query should succeed")
}

async fn search_document_count(
    store: &SqliteGraphStore,
    source_scope: &str,
    document_kind: &str,
) -> usize {
    let source_scope = source_scope.to_owned();
    let document_kind = document_kind.to_owned();
    store
        .run(move |connection| {
            connection
                .query_row(
                    "
                    SELECT COUNT(*)
                    FROM code_repository_search
                    WHERE source_scope = ?1 AND document_kind = ?2
                    ",
                    (&source_scope, &document_kind),
                    |row| row.get(0),
                )
                .map_err(crate::storage::StorageError::from)
        })
        .await
        .expect("search document count should load")
}
