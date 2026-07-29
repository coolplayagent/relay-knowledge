use std::{
    fs,
    path::{Path, PathBuf},
    sync::Arc,
    time::{SystemTime, UNIX_EPOCH},
};

use crate::{
    api::{InterfaceKind, RequestContext},
    application::{RelayKnowledgeService, RuntimeConfiguration},
    env::{EnvironmentConfig, PlatformKind},
    storage::{KnowledgeStore, SqliteGraphStore},
};

use super::*;

#[test]
fn parses_files_index_roots_and_scope() {
    let action = parse_files(&[
        "index".to_owned(),
        "--source".to_owned(),
        "local-files".to_owned(),
        "--root".to_owned(),
        "/opt/docs".to_owned(),
        "--root".to_owned(),
        "D:\\Archive".to_owned(),
    ])
    .expect("files index should parse");

    assert_eq!(
        action,
        CliAction::FilesIndex {
            source_scope: Some("local-files".to_owned()),
            roots: vec!["/opt/docs".to_owned(), "D:\\Archive".to_owned()]
        }
    );
}

#[test]
fn parses_files_query_forms_and_errors() {
    let action = parse_files(&[
        "query".to_owned(),
        "quarterly".to_owned(),
        "design".to_owned(),
        "--source".to_owned(),
        "local-files".to_owned(),
        "--root".to_owned(),
        "root-1".to_owned(),
        "--limit".to_owned(),
        "7".to_owned(),
    ])
    .expect("positional query should parse");
    assert_eq!(
        action,
        CliAction::FilesQuery {
            query: "quarterly design".to_owned(),
            source_scope: Some("local-files".to_owned()),
            root_id: Some("root-1".to_owned()),
            limit: 7,
            freshness: crate::domain::FreshnessPolicy::AllowStale
        }
    );

    let delimited = parse_files(&["query".to_owned(), "--".to_owned(), "--dash".to_owned()])
        .expect("delimiter query should parse");
    assert_eq!(
        delimited,
        CliAction::FilesQuery {
            query: "--dash".to_owned(),
            source_scope: None,
            root_id: None,
            limit: DEFAULT_FILE_QUERY_LIMIT,
            freshness: crate::domain::FreshnessPolicy::AllowStale
        }
    );

    let fresh = parse_files(&[
        "query".to_owned(),
        "design".to_owned(),
        "--freshness".to_owned(),
        "wait-until-fresh".to_owned(),
    ])
    .expect("freshness should parse");
    assert_eq!(
        fresh,
        CliAction::FilesQuery {
            query: "design".to_owned(),
            source_scope: None,
            root_id: None,
            limit: DEFAULT_FILE_QUERY_LIMIT,
            freshness: crate::domain::FreshnessPolicy::WaitUntilFresh
        }
    );

    let content = parse_files(&[
        "content".to_owned(),
        "database".to_owned(),
        "runbook".to_owned(),
        "--source".to_owned(),
        "local-files".to_owned(),
    ])
    .expect("content query should parse");
    assert_eq!(
        content,
        CliAction::FilesContentQuery {
            query: "database runbook".to_owned(),
            source_scope: Some("local-files".to_owned()),
            root_id: None,
            limit: DEFAULT_FILE_QUERY_LIMIT,
            freshness: crate::domain::FreshnessPolicy::AllowStale
        }
    );

    assert!(matches!(
        parse_files(&[
            "query".to_owned(),
            "name".to_owned(),
            "--limit".to_owned(),
            "wide".to_owned()
        ]),
        Err(CliError::InvalidLimit(value)) if value == "wide"
    ));
    assert_eq!(
        parse_files(&["query".to_owned()]).expect_err("query is required"),
        CliError::MissingValue("query")
    );
    assert_eq!(
        parse_files(&["remove".to_owned()]).expect_err("subcommand is required"),
        CliError::UnexpectedArgument("remove".to_owned())
    );
}

#[tokio::test]
async fn run_files_dispatches_index_query_and_non_file_actions() {
    let fixture = TempFixture::new("files-cli");
    fixture.write("docs/quarterly-design.pdf", "pdf");
    let service = service_for_root(fixture.path()).await;
    let context = RequestContext::with_ids(InterfaceKind::Cli, "req-files", "trace-files");

    let indexed = run_files(
        &service,
        &CliAction::FilesIndex {
            source_scope: Some("local-files".to_owned()),
            roots: vec![fixture.path().to_string_lossy().to_string()],
        },
        context.clone(),
        OutputFormat::Json,
    )
    .await
    .expect("index command should run")
    .expect("index command should render");
    assert!(indexed.contains("\"root_count\":1"));

    let queried = run_files(
        &service,
        &CliAction::FilesQuery {
            query: "quarterly design".to_owned(),
            source_scope: Some("local-files".to_owned()),
            root_id: None,
            limit: 5,
            freshness: crate::domain::FreshnessPolicy::AllowStale,
        },
        context,
        OutputFormat::Json,
    )
    .await
    .expect("query command should run")
    .expect("query command should render");
    assert!(queried.contains("quarterly-design.pdf"));

    let content = run_files(
        &service,
        &CliAction::FilesContentQuery {
            query: "pdf".to_owned(),
            source_scope: Some("local-files".to_owned()),
            root_id: None,
            limit: 5,
            freshness: crate::domain::FreshnessPolicy::AllowStale,
        },
        RequestContext::with_ids(InterfaceKind::Cli, "req-files-content", "trace-files"),
        OutputFormat::Json,
    )
    .await
    .expect("content command should run")
    .expect("content command should render");
    assert!(content.contains("\"results\""));

    assert!(
        run_files(
            &service,
            &CliAction::Status,
            RequestContext::for_interface(InterfaceKind::Cli),
            OutputFormat::Json,
        )
        .await
        .expect("non-file command should be ignored")
        .is_none()
    );
}

#[tokio::test]
async fn file_index_loop_exits_when_shutdown_is_signaled() {
    let fixture = TempFixture::new("files-loop-shutdown");
    fixture.write("docs/quarterly-design.pdf", "pdf");
    let service = service_for_root(fixture.path()).await;
    let (shutdown, receiver) = tokio::sync::watch::channel(false);
    let task = tokio::spawn(run_file_index_loop(
        service,
        std::time::Duration::from_secs(60),
        receiver,
    ));

    shutdown
        .send(true)
        .expect("shutdown signal should be delivered");
    tokio::time::timeout(std::time::Duration::from_secs(2), task)
        .await
        .expect("file index loop should stop promptly")
        .expect("file index loop task should not panic");
}

async fn service_for_root(root: &Path) -> RelayKnowledgeService {
    let home = root.join("home");
    fs::create_dir_all(&home).expect("home should be created");
    let relay_home = root.join("relay");
    let environment = EnvironmentConfig::from_pairs(
        PlatformKind::Unix,
        [
            ("HOME", home.to_string_lossy().to_string()),
            ("TMPDIR", "/tmp".to_owned()),
            (
                "RELAY_KNOWLEDGE_HOME",
                relay_home.to_string_lossy().to_string(),
            ),
            (
                "RELAY_KNOWLEDGE_FILE_INDEX_ROOTS",
                root.to_string_lossy().to_string(),
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

    fn write(&self, relative: &str, content: &str) {
        let path = self.root.join(relative);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).expect("fixture parent should be created");
        }
        fs::write(path, content).expect("fixture file should be written");
    }
}

impl Drop for TempFixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}
