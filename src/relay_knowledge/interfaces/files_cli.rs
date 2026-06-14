use crate::{
    api::{FileContentQueryRequest, FileIndexRequest, FileQueryRequest, RequestContext},
    application::{DEFAULT_FILE_QUERY_LIMIT, RelayKnowledgeService},
};

use super::{
    CliAction, CliError, OutputFormat, cli_render::render_response, parse_freshness, value_after,
};

pub(super) fn parse_files(tokens: &[String]) -> Result<CliAction, CliError> {
    match tokens.first().map(String::as_str) {
        Some("index") => parse_files_index(&tokens[1..]),
        Some("query") => parse_files_query(&tokens[1..]),
        Some("content") => parse_files_content_query(&tokens[1..]),
        other => Err(CliError::UnexpectedArgument(
            other.unwrap_or("files").to_owned(),
        )),
    }
}

pub(super) async fn run_files(
    service: &RelayKnowledgeService,
    action: &CliAction,
    context: RequestContext,
    format: OutputFormat,
) -> Result<Option<String>, CliError> {
    match action {
        CliAction::FilesIndex {
            source_scope,
            roots,
        } => {
            let response = service
                .index_files(
                    FileIndexRequest {
                        source_scope: source_scope.clone(),
                        roots: roots.clone(),
                    },
                    context,
                )
                .await
                .map_err(|error| CliError::api_failed(error, format))?;

            render_response("files.index", response.metadata.clone(), &response, format).map(Some)
        }
        CliAction::FilesQuery {
            query,
            source_scope,
            root_id,
            limit,
            freshness,
        } => {
            let response = service
                .query_files(
                    FileQueryRequest {
                        query: query.clone(),
                        source_scope: source_scope.clone(),
                        root_id: root_id.clone(),
                        limit: *limit,
                        freshness_policy: *freshness,
                    },
                    context,
                )
                .await
                .map_err(|error| CliError::api_failed(error, format))?;

            render_response("files.query", response.metadata.clone(), &response, format).map(Some)
        }
        CliAction::FilesContentQuery {
            query,
            source_scope,
            root_id,
            limit,
            freshness,
        } => {
            let response = service
                .query_file_content(
                    FileContentQueryRequest {
                        query: query.clone(),
                        source_scope: source_scope.clone(),
                        root_id: root_id.clone(),
                        limit: *limit,
                        freshness_policy: *freshness,
                    },
                    context,
                )
                .await
                .map_err(|error| CliError::api_failed(error, format))?;

            render_response(
                "files.content",
                response.metadata.clone(),
                &response,
                format,
            )
            .map(Some)
        }
        _ => Ok(None),
    }
}

pub(super) async fn run_file_index_loop(
    service: RelayKnowledgeService,
    interval: std::time::Duration,
    mut shutdown: tokio::sync::watch::Receiver<bool>,
) {
    loop {
        tokio::select! {
            _ = shutdown.changed() => break,
            _ = service.index_configured_files_once() => {}
        }
        tokio::select! {
            _ = shutdown.changed() => break,
            _ = tokio::time::sleep(interval) => {}
        }
    }
}

fn parse_files_index(tokens: &[String]) -> Result<CliAction, CliError> {
    let mut source_scope = None;
    let mut roots = Vec::new();
    let mut index = 0;
    while index < tokens.len() {
        match tokens[index].as_str() {
            "--source" => {
                source_scope = Some(value_after(tokens, index, "--source")?);
                index += 2;
            }
            "--root" => {
                roots.push(value_after(tokens, index, "--root")?);
                index += 2;
            }
            other => return Err(CliError::UnexpectedArgument(other.to_owned())),
        }
    }

    Ok(CliAction::FilesIndex {
        source_scope,
        roots,
    })
}

fn parse_files_query(tokens: &[String]) -> Result<CliAction, CliError> {
    parse_file_text_query(tokens, false)
}

fn parse_files_content_query(tokens: &[String]) -> Result<CliAction, CliError> {
    parse_file_text_query(tokens, true)
}

fn parse_file_text_query(tokens: &[String], content: bool) -> Result<CliAction, CliError> {
    let mut query = None;
    let mut source_scope = None;
    let mut root_id = None;
    let mut limit = DEFAULT_FILE_QUERY_LIMIT;
    let mut freshness = crate::domain::FreshnessPolicy::AllowStale;
    let mut index = 0;

    while index < tokens.len() {
        match tokens[index].as_str() {
            "--" if query.is_none() => {
                query = Some(value_after(tokens, index, "query")?);
                index += 2;
            }
            "--source" => {
                source_scope = Some(value_after(tokens, index, "--source")?);
                index += 2;
            }
            "--root" => {
                root_id = Some(value_after(tokens, index, "--root")?);
                index += 2;
            }
            "--limit" => {
                let value = value_after(tokens, index, "--limit")?;
                limit = value
                    .parse::<usize>()
                    .map_err(|_| CliError::InvalidLimit(value.clone()))?;
                index += 2;
            }
            "--freshness" => {
                freshness = parse_freshness(&value_after(tokens, index, "--freshness")?)?;
                index += 2;
            }
            other if !other.starts_with('-') && query.is_none() => {
                let mut values = vec![other.to_owned()];
                index += 1;
                while index < tokens.len() && !tokens[index].starts_with('-') {
                    values.push(tokens[index].clone());
                    index += 1;
                }
                query = Some(values.join(" "));
            }
            other => return Err(CliError::UnexpectedArgument(other.to_owned())),
        }
    }

    let query = query.ok_or(CliError::MissingValue("query"))?;
    if content {
        Ok(CliAction::FilesContentQuery {
            query,
            source_scope,
            root_id,
            limit,
            freshness,
        })
    } else {
        Ok(CliAction::FilesQuery {
            query,
            source_scope,
            root_id,
            limit,
            freshness,
        })
    }
}

#[cfg(test)]
mod tests {
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
}
