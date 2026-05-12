use crate::{
    api::{CodeRepositoryRegisterRequest, RequestContext},
    application::RelayKnowledgeService,
    domain::{
        CodeImpactRequest, CodeIndexMode, CodeIndexRequest, CodeQueryKind, CodeRepositorySelector,
        CodeRetrievalRequest, FreshnessPolicy,
    },
};

use super::{CliError, OutputFormat, parse_freshness, render_response, value_after};

/// Parsed `repo` CLI command.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RepoCommand {
    Register {
        root_path: String,
        alias: String,
        path_filters: Vec<String>,
        language_filters: Vec<String>,
    },
    Index {
        alias: String,
        ref_selector: String,
    },
    Update {
        alias: String,
        base_ref: String,
        head_ref: String,
    },
    Query {
        alias: String,
        query: String,
        kind: CodeQueryKind,
        limit: usize,
        ref_selector: String,
        path_filters: Vec<String>,
        language_filters: Vec<String>,
        freshness: FreshnessPolicy,
    },
    Impact {
        alias: String,
        base_ref: String,
        head_ref: String,
        limit: usize,
    },
    Status {
        alias: String,
    },
}

pub fn parse_repo(tokens: &[String]) -> Result<RepoCommand, CliError> {
    match tokens.first().map(String::as_str) {
        Some("register") => parse_register(&tokens[1..]),
        Some("index") => parse_index(&tokens[1..]),
        Some("update") => parse_update(&tokens[1..]),
        Some("query") => parse_query(&tokens[1..]),
        Some("impact") => parse_impact(&tokens[1..]),
        Some("status") => parse_status(&tokens[1..]),
        Some(other) => Err(CliError::UnexpectedArgument(other.to_owned())),
        None => Err(CliError::UnexpectedArgument("repo".to_owned())),
    }
}

pub async fn run_repo(
    service: &RelayKnowledgeService,
    command: RepoCommand,
    context: RequestContext,
    format: OutputFormat,
) -> Result<String, CliError> {
    match command {
        RepoCommand::Register {
            root_path,
            alias,
            path_filters,
            language_filters,
        } => {
            let response = service
                .register_code_repository(
                    CodeRepositoryRegisterRequest {
                        root_path,
                        alias,
                        path_filters,
                        language_filters,
                    },
                    context,
                )
                .await
                .map_err(|error| CliError::ApiFailed(error.message))?;

            render_response(
                "code.repo.register",
                response.metadata.clone(),
                &response,
                format,
            )
        }
        RepoCommand::Index {
            alias,
            ref_selector,
        } => {
            let response = service
                .index_code_repository(
                    CodeIndexRequest {
                        repository: selector(alias, ref_selector, Vec::new(), Vec::new())?,
                        mode: CodeIndexMode::Full,
                        freshness_policy: FreshnessPolicy::WaitUntilFresh,
                    },
                    context,
                )
                .await
                .map_err(|error| CliError::ApiFailed(error.message))?;

            render_response(
                "code.repo.index",
                response.metadata.clone(),
                &response,
                format,
            )
        }
        RepoCommand::Update {
            alias,
            base_ref,
            head_ref,
        } => {
            let response = service
                .index_code_repository(
                    CodeIndexRequest {
                        repository: selector(alias, head_ref.clone(), Vec::new(), Vec::new())?,
                        mode: CodeIndexMode::incremental(base_ref, head_ref)
                            .map_err(|error| CliError::ApiFailed(error.to_string()))?,
                        freshness_policy: FreshnessPolicy::WaitUntilFresh,
                    },
                    context,
                )
                .await
                .map_err(|error| CliError::ApiFailed(error.message))?;

            render_response(
                "code.repo.update",
                response.metadata.clone(),
                &response,
                format,
            )
        }
        RepoCommand::Query {
            alias,
            query,
            kind,
            limit,
            ref_selector,
            path_filters,
            language_filters,
            freshness,
        } => {
            let request = CodeRetrievalRequest::new(
                query,
                selector(alias, ref_selector, path_filters, language_filters)?,
                kind,
                limit,
                freshness,
            )
            .map_err(|error| CliError::ApiFailed(error.to_string()))?;
            let response = service
                .query_code_repository(request, context)
                .await
                .map_err(|error| CliError::ApiFailed(error.message))?;

            render_response(
                "code.repo.query",
                response.metadata.clone(),
                &response,
                format,
            )
        }
        RepoCommand::Impact {
            alias,
            base_ref,
            head_ref,
            limit,
        } => {
            let request = CodeImpactRequest::new(
                selector(alias, head_ref.clone(), Vec::new(), Vec::new())?,
                base_ref,
                head_ref,
                limit,
            )
            .map_err(|error| CliError::ApiFailed(error.to_string()))?;
            let response = service
                .impact_code_repository(request, context)
                .await
                .map_err(|error| CliError::ApiFailed(error.message))?;

            render_response(
                "code.repo.impact",
                response.metadata.clone(),
                &response,
                format,
            )
        }
        RepoCommand::Status { alias } => {
            let response = service
                .code_repository_status(selector(alias, "HEAD", Vec::new(), Vec::new())?, context)
                .await
                .map_err(|error| CliError::ApiFailed(error.message))?;

            render_response(
                "code.repo.status",
                response.metadata.clone(),
                &response,
                format,
            )
        }
    }
}

fn parse_register(tokens: &[String]) -> Result<RepoCommand, CliError> {
    let root_path = tokens
        .first()
        .filter(|value| !value.starts_with('-'))
        .cloned()
        .ok_or(CliError::MissingValue("<path>"))?;
    let mut alias = None;
    let mut path_filters = Vec::new();
    let mut language_filters = Vec::new();
    let mut index = 1;
    while index < tokens.len() {
        match tokens[index].as_str() {
            "--alias" => {
                alias = Some(value_after(tokens, index, "--alias")?);
                index += 2;
            }
            "--path" => {
                path_filters.push(value_after(tokens, index, "--path")?);
                index += 2;
            }
            "--language" => {
                language_filters.push(value_after(tokens, index, "--language")?);
                index += 2;
            }
            other => return Err(CliError::UnexpectedArgument(other.to_owned())),
        }
    }

    Ok(RepoCommand::Register {
        root_path,
        alias: alias.ok_or(CliError::MissingValue("--alias"))?,
        path_filters,
        language_filters,
    })
}

fn parse_index(tokens: &[String]) -> Result<RepoCommand, CliError> {
    let alias = positional_alias(tokens)?;
    let mut ref_selector = "HEAD".to_owned();
    let mut index = 1;
    while index < tokens.len() {
        match tokens[index].as_str() {
            "--ref" => {
                ref_selector = value_after(tokens, index, "--ref")?;
                index += 2;
            }
            other => return Err(CliError::UnexpectedArgument(other.to_owned())),
        }
    }

    Ok(RepoCommand::Index {
        alias,
        ref_selector,
    })
}

fn parse_update(tokens: &[String]) -> Result<RepoCommand, CliError> {
    let alias = positional_alias(tokens)?;
    let (base_ref, head_ref, _) = parse_base_head_limit(tokens, 1, 50)?;

    Ok(RepoCommand::Update {
        alias,
        base_ref: base_ref.ok_or(CliError::MissingValue("--base"))?,
        head_ref: head_ref.ok_or(CliError::MissingValue("--head"))?,
    })
}

fn parse_query(tokens: &[String]) -> Result<RepoCommand, CliError> {
    let alias = positional_alias(tokens)?;
    let mut query = None;
    let mut kind = CodeQueryKind::Hybrid;
    let mut limit = 10;
    let mut ref_selector = "HEAD".to_owned();
    let mut path_filters = Vec::new();
    let mut language_filters = Vec::new();
    let mut freshness = FreshnessPolicy::AllowStale;
    let mut index = 1;
    while index < tokens.len() {
        match tokens[index].as_str() {
            "--query" => {
                query = Some(value_after(tokens, index, "--query")?);
                index += 2;
            }
            "--kind" => {
                kind = parse_query_kind(&value_after(tokens, index, "--kind")?)?;
                index += 2;
            }
            "--limit" => {
                let value = value_after(tokens, index, "--limit")?;
                limit = value
                    .parse::<usize>()
                    .map_err(|_| CliError::InvalidLimit(value.clone()))?;
                index += 2;
            }
            "--ref" => {
                ref_selector = value_after(tokens, index, "--ref")?;
                index += 2;
            }
            "--path" => {
                path_filters.push(value_after(tokens, index, "--path")?);
                index += 2;
            }
            "--language" => {
                language_filters.push(value_after(tokens, index, "--language")?);
                index += 2;
            }
            "--freshness" => {
                freshness = parse_freshness(&value_after(tokens, index, "--freshness")?)?;
                index += 2;
            }
            other if !other.starts_with('-') && query.is_none() => {
                query = Some(other.to_owned());
                index += 1;
            }
            other => return Err(CliError::UnexpectedArgument(other.to_owned())),
        }
    }

    Ok(RepoCommand::Query {
        alias,
        query: query.ok_or(CliError::MissingValue("--query"))?,
        kind,
        limit,
        ref_selector,
        path_filters,
        language_filters,
        freshness,
    })
}

fn parse_impact(tokens: &[String]) -> Result<RepoCommand, CliError> {
    let alias = positional_alias(tokens)?;
    let (base_ref, head_ref, limit) = parse_base_head_limit(tokens, 1, 100)?;

    Ok(RepoCommand::Impact {
        alias,
        base_ref: base_ref.ok_or(CliError::MissingValue("--base"))?,
        head_ref: head_ref.ok_or(CliError::MissingValue("--head"))?,
        limit,
    })
}

fn parse_status(tokens: &[String]) -> Result<RepoCommand, CliError> {
    Ok(RepoCommand::Status {
        alias: positional_alias(tokens)?,
    })
}

fn parse_base_head_limit(
    tokens: &[String],
    start_index: usize,
    default_limit: usize,
) -> Result<(Option<String>, Option<String>, usize), CliError> {
    let mut base_ref = None;
    let mut head_ref = None;
    let mut limit = default_limit;
    let mut index = start_index;
    while index < tokens.len() {
        match tokens[index].as_str() {
            "--base" => {
                base_ref = Some(value_after(tokens, index, "--base")?);
                index += 2;
            }
            "--head" => {
                head_ref = Some(value_after(tokens, index, "--head")?);
                index += 2;
            }
            "--limit" => {
                let value = value_after(tokens, index, "--limit")?;
                limit = value
                    .parse::<usize>()
                    .map_err(|_| CliError::InvalidLimit(value.clone()))?;
                index += 2;
            }
            other => return Err(CliError::UnexpectedArgument(other.to_owned())),
        }
    }

    Ok((base_ref, head_ref, limit))
}

fn positional_alias(tokens: &[String]) -> Result<String, CliError> {
    tokens
        .first()
        .filter(|value| !value.starts_with('-'))
        .cloned()
        .ok_or(CliError::MissingValue("<alias>"))
}

fn parse_query_kind(value: &str) -> Result<CodeQueryKind, CliError> {
    match value {
        "hybrid" => Ok(CodeQueryKind::Hybrid),
        "symbol" => Ok(CodeQueryKind::Symbol),
        "definition" => Ok(CodeQueryKind::Definition),
        "references" => Ok(CodeQueryKind::References),
        "callers" => Ok(CodeQueryKind::Callers),
        "callees" => Ok(CodeQueryKind::Callees),
        "imports" => Ok(CodeQueryKind::Imports),
        other => Err(CliError::InvalidCodeQueryKind(other.to_owned())),
    }
}

fn selector(
    alias: String,
    ref_selector: impl Into<String>,
    path_filters: Vec<String>,
    language_filters: Vec<String>,
) -> Result<CodeRepositorySelector, CliError> {
    CodeRepositorySelector::new(alias, ref_selector, path_filters, language_filters)
        .map_err(|error| CliError::ApiFailed(error.to_string()))
}

#[cfg(test)]
mod tests {
    use std::{
        fs,
        path::{Path, PathBuf},
        process::Command,
        sync::Arc,
        time::{SystemTime, UNIX_EPOCH},
    };

    use super::*;
    use crate::{
        api::InterfaceKind,
        application::RuntimeConfiguration,
        env::{EnvironmentConfig, PlatformKind},
        storage::SqliteGraphStore,
    };

    #[test]
    fn parses_repo_query_with_kind_filters_and_freshness() {
        let command = parse_repo(&[
            "query".to_owned(),
            "core".to_owned(),
            "--query".to_owned(),
            "RetryPolicy".to_owned(),
            "--kind".to_owned(),
            "references".to_owned(),
            "--path".to_owned(),
            "src".to_owned(),
            "--language".to_owned(),
            "rust".to_owned(),
            "--freshness".to_owned(),
            "wait-until-fresh".to_owned(),
        ])
        .expect("repo query should parse");

        assert_eq!(
            command,
            RepoCommand::Query {
                alias: "core".to_owned(),
                query: "RetryPolicy".to_owned(),
                kind: CodeQueryKind::References,
                limit: 10,
                ref_selector: "HEAD".to_owned(),
                path_filters: vec!["src".to_owned()],
                language_filters: vec!["rust".to_owned()],
                freshness: FreshnessPolicy::WaitUntilFresh,
            }
        );
    }

    #[test]
    fn parses_repo_command_forms_and_validation_errors() {
        let register = parse_repo(&[
            "register".to_owned(),
            "/work/repo".to_owned(),
            "--alias".to_owned(),
            "core".to_owned(),
            "--path".to_owned(),
            "src".to_owned(),
            "--language".to_owned(),
            "rust".to_owned(),
        ])
        .expect("register command should parse");
        assert_eq!(
            register,
            RepoCommand::Register {
                root_path: "/work/repo".to_owned(),
                alias: "core".to_owned(),
                path_filters: vec!["src".to_owned()],
                language_filters: vec!["rust".to_owned()],
            }
        );

        assert_eq!(
            parse_repo(&["index".to_owned(), "core".to_owned()])
                .expect("index command should parse"),
            RepoCommand::Index {
                alias: "core".to_owned(),
                ref_selector: "HEAD".to_owned(),
            }
        );
        assert_eq!(
            parse_repo(&[
                "update".to_owned(),
                "core".to_owned(),
                "--base".to_owned(),
                "main".to_owned(),
                "--head".to_owned(),
                "feature".to_owned(),
            ])
            .expect("update command should parse"),
            RepoCommand::Update {
                alias: "core".to_owned(),
                base_ref: "main".to_owned(),
                head_ref: "feature".to_owned(),
            }
        );
        assert_eq!(
            parse_repo(&[
                "impact".to_owned(),
                "core".to_owned(),
                "--base".to_owned(),
                "main".to_owned(),
                "--head".to_owned(),
                "feature".to_owned(),
                "--limit".to_owned(),
                "7".to_owned(),
            ])
            .expect("impact command should parse"),
            RepoCommand::Impact {
                alias: "core".to_owned(),
                base_ref: "main".to_owned(),
                head_ref: "feature".to_owned(),
                limit: 7,
            }
        );
        assert_eq!(
            parse_repo(&["status".to_owned(), "core".to_owned()])
                .expect("status command should parse"),
            RepoCommand::Status {
                alias: "core".to_owned()
            }
        );

        assert_eq!(parse_query_kind("hybrid").unwrap(), CodeQueryKind::Hybrid);
        assert_eq!(parse_query_kind("symbol").unwrap(), CodeQueryKind::Symbol);
        assert_eq!(
            parse_query_kind("definition").unwrap(),
            CodeQueryKind::Definition
        );
        assert_eq!(parse_query_kind("callers").unwrap(), CodeQueryKind::Callers);
        assert_eq!(parse_query_kind("callees").unwrap(), CodeQueryKind::Callees);
        assert_eq!(parse_query_kind("imports").unwrap(), CodeQueryKind::Imports);
        assert_eq!(
            parse_query_kind("impact").unwrap_err(),
            CliError::InvalidCodeQueryKind("impact".to_owned())
        );

        let positional_query = parse_repo(&[
            "query".to_owned(),
            "core".to_owned(),
            "RetryPolicy".to_owned(),
            "--kind".to_owned(),
            "symbol".to_owned(),
        ])
        .expect("positional query should parse");
        assert!(matches!(
            positional_query,
            RepoCommand::Query {
                kind: CodeQueryKind::Symbol,
                ..
            }
        ));

        assert_eq!(
            parse_repo(&[]).expect_err("empty repo command should fail"),
            CliError::UnexpectedArgument("repo".to_owned())
        );
        assert_eq!(
            parse_repo(&["query".to_owned(), "core".to_owned()])
                .expect_err("missing query should fail"),
            CliError::MissingValue("--query")
        );
        assert_eq!(
            parse_repo(&[
                "impact".to_owned(),
                "core".to_owned(),
                "--base".to_owned(),
                "main".to_owned(),
            ])
            .expect_err("missing head should fail"),
            CliError::MissingValue("--head")
        );
        assert_eq!(
            parse_repo(&[
                "query".to_owned(),
                "core".to_owned(),
                "--query".to_owned(),
                "RetryPolicy".to_owned(),
                "--kind".to_owned(),
                "unknown".to_owned(),
            ])
            .expect_err("unknown query kind should fail"),
            CliError::InvalidCodeQueryKind("unknown".to_owned())
        );
        assert_eq!(
            parse_repo(&[
                "impact".to_owned(),
                "core".to_owned(),
                "--base".to_owned(),
                "main".to_owned(),
                "--head".to_owned(),
                "feature".to_owned(),
                "--limit".to_owned(),
                "many".to_owned(),
            ])
            .expect_err("bad limit should fail"),
            CliError::InvalidLimit("many".to_owned())
        );
        assert_eq!(
            parse_repo(&["unknown".to_owned()]).expect_err("unknown subcommand should fail"),
            CliError::UnexpectedArgument("unknown".to_owned())
        );
    }

    #[tokio::test]
    async fn runs_repo_commands_against_shared_service() {
        let repo = FixtureRepo::create("repo-cli");
        repo.write(
            "src/lib.rs",
            r#"
/// Selects the retry budget.
pub fn retry_policy() -> u32 {
    3
}
"#,
        );
        repo.write(
            "src/main.rs",
            r#"
use crate::retry_policy;

fn run_worker() {
    retry_policy();
}
"#,
        );
        repo.git(["add", "."]);
        repo.git(["commit", "-m", "initial"]);
        let base_ref = repo.git_text(["rev-parse", "HEAD"]);
        let service = service_with_memory_store().await;

        let registered = run_repo(
            &service,
            RepoCommand::Register {
                root_path: repo.path.display().to_string(),
                alias: "fixture".to_owned(),
                path_filters: vec!["src".to_owned()],
                language_filters: vec!["rust".to_owned()],
            },
            context("register"),
            OutputFormat::Json,
        )
        .await
        .expect("register should run");
        let value = json_value(&registered);
        assert_eq!(value["registration"]["alias"], "fixture");

        let indexed = run_repo(
            &service,
            RepoCommand::Index {
                alias: "fixture".to_owned(),
                ref_selector: "HEAD".to_owned(),
            },
            context("index"),
            OutputFormat::StreamingJson,
        )
        .await
        .expect("index should run");
        assert!(indexed.contains("code.repo.index"));

        let definitions = run_repo(
            &service,
            RepoCommand::Query {
                alias: "fixture".to_owned(),
                query: "retry_policy".to_owned(),
                kind: CodeQueryKind::Definition,
                limit: 5,
                ref_selector: "HEAD".to_owned(),
                path_filters: Vec::new(),
                language_filters: Vec::new(),
                freshness: FreshnessPolicy::AllowStale,
            },
            context("query"),
            OutputFormat::Json,
        )
        .await
        .expect("query should run");
        assert_eq!(json_value(&definitions)["results"][0]["path"], "src/lib.rs");

        repo.write(
            "src/lib.rs",
            r#"
/// Selects the retry budget.
pub fn retry_policy() -> u32 {
    5
}

pub fn retry_policy_v2() -> u32 {
    retry_policy()
}
"#,
        );
        repo.git(["add", "."]);
        repo.git(["commit", "-m", "update policy"]);
        let head_ref = repo.git_text(["rev-parse", "HEAD"]);

        let updated = run_repo(
            &service,
            RepoCommand::Update {
                alias: "fixture".to_owned(),
                base_ref: base_ref.clone(),
                head_ref: head_ref.clone(),
            },
            context("update"),
            OutputFormat::Text,
        )
        .await
        .expect("update should run");
        assert_eq!(updated, "code.repo.update\n");

        let impact = run_repo(
            &service,
            RepoCommand::Impact {
                alias: "fixture".to_owned(),
                base_ref,
                head_ref,
                limit: 10,
            },
            context("impact"),
            OutputFormat::Json,
        )
        .await
        .expect("impact should run");
        assert_eq!(json_value(&impact)["changed_paths"][0], "src/lib.rs");

        let status = run_repo(
            &service,
            RepoCommand::Status {
                alias: "fixture".to_owned(),
            },
            context("status"),
            OutputFormat::StreamingJson,
        )
        .await
        .expect("status should run");
        assert!(status.contains("code.repo.status"));
    }

    fn json_value(output: &str) -> serde_json::Value {
        serde_json::from_str(output.trim()).expect("json output should parse")
    }

    fn context(name: &str) -> RequestContext {
        RequestContext::with_ids(
            InterfaceKind::Cli,
            format!("req-{name}"),
            format!("trace-{name}"),
        )
    }

    async fn service_with_memory_store() -> RelayKnowledgeService {
        let environment = EnvironmentConfig::from_pairs(
            PlatformKind::Unix,
            [
                ("HOME", "/home/alice"),
                ("TMPDIR", "/tmp"),
                ("RELAY_KNOWLEDGE_HOME", "/srv/relay"),
            ],
        )
        .expect("environment should parse");
        let runtime = RuntimeConfiguration::from_environment(&environment)
            .await
            .expect("runtime should compose");
        let store = Arc::new(SqliteGraphStore::open_in_memory().expect("store should open"));

        RelayKnowledgeService::with_store(runtime, store)
    }

    struct FixtureRepo {
        path: PathBuf,
    }

    impl FixtureRepo {
        fn create(name: &str) -> Self {
            let nanos = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .expect("clock should be after epoch")
                .as_nanos();
            let path = std::env::temp_dir().join(format!("relay-knowledge-{name}-{nanos}"));
            fs::create_dir_all(path.join("src")).expect("repo directory should be created");
            let repo = Self { path };
            repo.git(["init"]);
            repo.git(["config", "user.email", "relay@example.invalid"]);
            repo.git(["config", "user.name", "Relay Test"]);
            repo
        }

        fn write(&self, relative: &str, content: &str) {
            let path = self.path.join(relative);
            if let Some(parent) = path.parent() {
                fs::create_dir_all(parent).expect("parent directory should exist");
            }
            fs::write(path, content).expect("fixture file should be written");
        }

        fn git<const N: usize>(&self, args: [&str; N]) {
            let output = git_command(&self.path, args)
                .output()
                .expect("git should run");
            assert!(
                output.status.success(),
                "git failed: {}",
                String::from_utf8_lossy(&output.stderr)
            );
        }

        fn git_text<const N: usize>(&self, args: [&str; N]) -> String {
            let output = git_command(&self.path, args)
                .output()
                .expect("git should run");
            assert!(output.status.success());
            String::from_utf8_lossy(&output.stdout).trim().to_owned()
        }
    }

    impl Drop for FixtureRepo {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.path);
        }
    }

    fn git_command<const N: usize>(path: &Path, args: [&str; N]) -> Command {
        let mut command = Command::new("git");
        command.current_dir(path).args(args);
        command
    }
}
