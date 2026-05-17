use std::{
    fs,
    path::{Path, PathBuf},
    sync::Arc,
    time::{SystemTime, UNIX_EPOCH},
};

use relay_knowledge::{
    api::{FileIndexRequest, FileQueryRequest, InterfaceKind, RequestContext},
    application::{RelayKnowledgeService, RuntimeConfiguration},
    env::{EnvironmentConfig, PlatformKind},
    storage::{KnowledgeStore, SqliteGraphStore},
};

#[tokio::test]
async fn indexes_and_queries_configured_local_file_roots() {
    let fixture = TempFixture::new("local-files-query");
    fixture.write("docs/quarterly-design.pdf", "pdf");
    fixture.write("docs/quarterly-design-notes.md", "notes");
    fixture.write("noise/quarterly-budget.xlsx", "budget");
    let service = service_for_root(fixture.path()).await;

    let indexed = service
        .index_files(
            FileIndexRequest {
                source_scope: Some("local-files".to_owned()),
                roots: vec![fixture.path().to_string_lossy().to_string()],
            },
            RequestContext::with_ids(InterfaceKind::Cli, "req-index", "trace-index"),
        )
        .await
        .expect("file index should run");

    assert_eq!(indexed.summary.root_count, 1);
    assert_eq!(indexed.summary.indexed_file_count, 3);

    let response = service
        .query_files(
            FileQueryRequest {
                query: "quarterly design pdf".to_owned(),
                source_scope: Some("local-files".to_owned()),
                root_id: None,
                limit: 5,
            },
            RequestContext::with_ids(InterfaceKind::Cli, "req-query", "trace-query"),
        )
        .await
        .expect("file query should run");

    assert_eq!(response.results.len(), 1);
    assert!(response.results[0].path.ends_with("quarterly-design.pdf"));
    assert_eq!(response.results[0].extension.as_deref(), Some("pdf"));
    assert!(!response.truncated);
}

#[tokio::test]
async fn reindex_marks_removed_files_missing_and_filters_queries() {
    let fixture = TempFixture::new("local-files-missing");
    let removed = fixture.write("docs/remove-me.md", "remove");
    fixture.write("docs/keep-me.md", "keep");
    let service = service_for_root(fixture.path()).await;

    service
        .index_files(
            FileIndexRequest {
                source_scope: Some("local-files".to_owned()),
                roots: vec![fixture.path().to_string_lossy().to_string()],
            },
            RequestContext::with_ids(InterfaceKind::Cli, "req-index-1", "trace-index-1"),
        )
        .await
        .expect("first file index should run");
    fs::remove_file(removed).expect("fixture file should be removable");
    let indexed = service
        .index_files(
            FileIndexRequest {
                source_scope: Some("local-files".to_owned()),
                roots: vec![fixture.path().to_string_lossy().to_string()],
            },
            RequestContext::with_ids(InterfaceKind::Cli, "req-index-2", "trace-index-2"),
        )
        .await
        .expect("second file index should run");

    assert_eq!(indexed.summary.indexed_file_count, 1);
    assert_eq!(indexed.summary.missing_file_count, 1);

    let missing = service
        .query_files(
            FileQueryRequest {
                query: "remove me".to_owned(),
                source_scope: Some("local-files".to_owned()),
                root_id: None,
                limit: 5,
            },
            RequestContext::with_ids(InterfaceKind::Cli, "req-query", "trace-query"),
        )
        .await
        .expect("file query should run");

    assert!(missing.results.is_empty());
}

async fn service_for_root(root: &Path) -> RelayKnowledgeService {
    let home = root.join("home");
    fs::create_dir_all(&home).expect("home should be created");
    let relay_home = root.join("relay");
    let home = home.to_string_lossy().to_string();
    let relay_home = relay_home.to_string_lossy().to_string();
    let root = root.to_string_lossy().to_string();
    let environment = EnvironmentConfig::from_pairs(
        PlatformKind::Unix,
        vec![
            ("HOME", home),
            ("TMPDIR", "/tmp".to_owned()),
            ("RELAY_KNOWLEDGE_HOME", relay_home),
            ("RELAY_KNOWLEDGE_FILE_INDEX_ROOTS", root),
            (
                "RELAY_KNOWLEDGE_FILE_INDEX_MAX_FILES_PER_ROOT",
                "1000".to_owned(),
            ),
        ],
    )
    .expect("environment should parse");
    let runtime = RuntimeConfiguration::from_environment(&environment)
        .await
        .expect("runtime should compose");
    let store = Arc::new(SqliteGraphStore::open_in_memory().expect("store should open"))
        as Arc<dyn KnowledgeStore>;

    RelayKnowledgeService::with_store(runtime, store)
}

struct TempFixture {
    root: PathBuf,
}

impl TempFixture {
    fn new(name: &str) -> Self {
        let suffix = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("time should be valid")
            .as_nanos();
        let root = std::env::temp_dir().join(format!(
            "relay-knowledge-{name}-{}-{suffix}",
            std::process::id()
        ));
        fs::create_dir_all(&root).expect("fixture root should be created");

        Self { root }
    }

    fn path(&self) -> &Path {
        &self.root
    }

    fn write(&self, relative: &str, content: &str) -> PathBuf {
        let path = self.root.join(relative);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).expect("fixture parent should be created");
        }
        fs::write(&path, content).expect("fixture file should be written");

        path
    }
}

impl Drop for TempFixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}
