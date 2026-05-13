use std::sync::Arc;

use serde_json::Value;

use super::*;
use crate::{
    application::RuntimeConfiguration,
    env::{EnvironmentConfig, PlatformKind},
    storage::SqliteGraphStore,
};

#[test]
fn parses_ingest_command_with_entities() {
    let command = CliCommand::parse([
        "ingest",
        "--source",
        "docs",
        "--content",
        "Rust async storage",
        "--entity",
        "Rust",
        "--entity",
        "SQLite",
        "--format",
        "json",
    ])
    .expect("ingest command should parse");

    assert_eq!(command.format, OutputFormat::Json);
    assert_eq!(
        command.action,
        CliAction::Ingest {
            source_scope: "docs".to_owned(),
            content: "Rust async storage".to_owned(),
            entity_labels: vec!["Rust".to_owned(), "SQLite".to_owned()],
        }
    );
}

#[test]
fn parses_query_action_with_options() {
    let command = CliCommand::parse([
        "query",
        "SQLite",
        "--source",
        "docs",
        "--limit",
        "3",
        "--freshness",
        "graph-only",
    ])
    .expect("query command should parse");

    assert_eq!(
        command.action,
        CliAction::Query {
            query: "SQLite".to_owned(),
            source_scope: Some("docs".to_owned()),
            limit: 3,
            freshness: FreshnessPolicy::GraphOnly,
        }
    );
}

#[test]
fn parses_dash_prefixed_query_after_delimiter() {
    let command = CliCommand::parse(["query", "--source", "docs", "--", "--help"])
        .expect("dash-prefixed query should parse after delimiter");

    assert_eq!(
        command.action,
        CliAction::Query {
            query: "--help".to_owned(),
            source_scope: Some("docs".to_owned()),
            limit: 10,
            freshness: FreshnessPolicy::AllowStale,
        }
    );
    assert!(!command.help);
}

#[test]
fn parses_global_format_after_dash_prefixed_query_delimiter() {
    let command = CliCommand::parse(["query", "--", "--help", "--format", "json"])
        .expect("dash-prefixed query and trailing format should parse");

    assert_eq!(command.format, OutputFormat::Json);
    assert_eq!(
        command.action,
        CliAction::Query {
            query: "--help".to_owned(),
            source_scope: None,
            limit: 10,
            freshness: FreshnessPolicy::AllowStale,
        }
    );
    assert!(!command.help);
}

#[test]
fn parses_dash_prefixed_ingest_content_as_value() {
    let command = CliCommand::parse([
        "ingest",
        "--source",
        "docs",
        "--content",
        "--version",
        "--format",
        "json",
    ])
    .expect("dash-prefixed ingest content should parse");

    assert_eq!(command.format, OutputFormat::Json);
    assert_eq!(
        command.action,
        CliAction::Ingest {
            source_scope: "docs".to_owned(),
            content: "--version".to_owned(),
            entity_labels: Vec::new(),
        }
    );
}

#[test]
fn parses_index_and_service_actions() {
    let index = CliCommand::parse(["index", "refresh", "--kind", "bm25"])
        .expect("index command should parse");
    let service = CliCommand::parse(["service", "doctor"]).expect("service command should parse");
    let run = CliCommand::parse(["service", "run", "--mcp", "streamable-http"])
        .expect("service run should parse");

    assert_eq!(
        index.action,
        CliAction::IndexRefresh {
            kinds: vec![IndexKind::Bm25],
        }
    );
    assert_eq!(service.action, CliAction::ServiceStatus);
    assert_eq!(
        run.action,
        CliAction::ServiceRun {
            mcp: ServiceMcpTransport::StreamableHttp,
            web: false,
        }
    );
}

#[test]
fn parses_service_run_with_web_and_mcp() {
    let web = CliCommand::parse(["service", "run", "--web"]).expect("web service should parse");
    let combined = CliCommand::parse(["service", "run", "--web", "--mcp", "streamable-http"])
        .expect("combined service should parse");

    assert_eq!(
        web.action,
        CliAction::ServiceRun {
            mcp: ServiceMcpTransport::Configured,
            web: true,
        }
    );
    assert_eq!(
        combined.action,
        CliAction::ServiceRun {
            mcp: ServiceMcpTransport::StreamableHttp,
            web: true,
        }
    );
}

#[test]
fn parses_operational_worker_proposal_audit_and_service_actions() {
    let worker = CliCommand::parse(["worker", "run-once", "--kind", "vision"])
        .expect("worker command should parse");
    let proposals = CliCommand::parse(["proposal", "list", "--state", "proposed", "--limit", "7"])
        .expect("proposal list should parse");
    let accept = CliCommand::parse([
        "proposal",
        "accept",
        "proposal:1",
        "--by",
        "reviewer",
        "--reason",
        "valid",
    ])
    .expect("proposal accept should parse");
    let audit = CliCommand::parse(["audit", "query", "--operation", "worker.run_once"])
        .expect("audit command should parse");
    let service_plan =
        CliCommand::parse(["service", "plan", "uninstall"]).expect("service plan should parse");
    let operator = CliCommand::parse(["service", "operator", "resume"])
        .expect("operator command should parse");

    assert_eq!(
        worker.action,
        CliAction::WorkerRunOnce {
            kind: Some(WorkerKind::Vision),
        }
    );
    assert_eq!(
        proposals.action,
        CliAction::ProposalList {
            state: Some(ProposalState::Proposed),
            limit: 7,
        }
    );
    assert_eq!(
        accept.action,
        CliAction::ProposalAccept {
            proposal_id: "proposal:1".to_owned(),
            actor: "reviewer".to_owned(),
            reason: Some("valid".to_owned()),
        }
    );
    assert_eq!(
        audit.action,
        CliAction::AuditQuery {
            operation: Some("worker.run_once".to_owned()),
            limit: 100,
        }
    );
    assert_eq!(
        service_plan.action,
        CliAction::ServicePlan {
            action: ServiceManagerAction::Uninstall,
        }
    );
    assert_eq!(operator.action, CliAction::ServiceOperatorResume);

    assert!(matches!(
        CliCommand::parse(["worker", "run-once", "--kind", "gpu"]),
        Err(CliError::InvalidWorkerKind(_))
    ));
    assert!(matches!(
        CliCommand::parse(["proposal", "list", "--state", "merged"]),
        Err(CliError::InvalidProposalState(_))
    ));
    assert!(matches!(
        CliCommand::parse(["service", "plan", "restart"]),
        Err(CliError::InvalidServiceAction(_))
    ));
}

#[test]
fn rejects_invalid_query_limit_and_freshness() {
    let limit =
        CliCommand::parse(["query", "x", "--limit", "nope"]).expect_err("limit should fail");
    let freshness = CliCommand::parse(["query", "x", "--freshness", "fresh-now"])
        .expect_err("freshness should fail");

    assert_eq!(limit.exit_code(), 2);
    assert_eq!(freshness.exit_code(), 2);
}

#[test]
fn rejects_missing_ingest_values_and_bad_index_kind() {
    let source =
        CliCommand::parse(["ingest", "--content", "x"]).expect_err("missing source should fail");
    let kind =
        CliCommand::parse(["index", "refresh", "--kind", "other"]).expect_err("kind should fail");

    assert_eq!(source.to_string(), "missing value for --source");
    assert_eq!(kind.exit_code(), 2);
}

#[test]
fn rejects_flag_style_actions_and_extra_command_words() {
    let flag_action = CliCommand::parse(["--ingest", "--source", "docs", "--content", "x"])
        .expect_err("flag-style actions should fail");
    let extra =
        CliCommand::parse(["status", "health"]).expect_err("extra command words should fail");

    assert_eq!(flag_action.to_string(), "unexpected argument '--ingest'");
    assert_eq!(extra.to_string(), "unexpected argument 'health'");
    assert_eq!(flag_action.exit_code(), 2);
}

#[test]
fn rejects_legacy_query_flag_form() {
    let error = CliCommand::parse(["query", "--query", "SQLite"])
        .expect_err("query text should be positional");

    assert_eq!(error.to_string(), "unexpected argument '--query'");
    assert_eq!(error.exit_code(), 2);
}

#[test]
fn parses_version_without_other_arguments() {
    let command = CliCommand::parse(["version"]).expect("version should parse");
    let flag_alias = CliCommand::parse(["--version"]).expect("version flag should parse");

    assert_eq!(command.action, CliAction::Version);
    assert_eq!(flag_alias.action, CliAction::Version);
}

#[test]
fn render_text_covers_operational_and_code_repository_summaries() {
    let cases = [
        (
            "worker.run_once",
            serde_json::json!({
                "task": {"task_id": "task:1"},
                "proposals": [{"proposal_id": "proposal:1"}],
            }),
            "task=task:1 proposals=1\n",
        ),
        (
            "proposal.show",
            serde_json::json!({
                "proposal": {"proposal_id": "proposal:1"},
                "conflicts": [{"conflict_id": "conflict:1"}],
            }),
            "proposal=proposal:1 conflicts=1\n",
        ),
        (
            "proposal.supersede",
            serde_json::json!({
                "proposal": {"proposal_id": "proposal:1", "state": "superseded"},
            }),
            "proposal=proposal:1 state=superseded\n",
        ),
        (
            "service.definition.write",
            serde_json::json!({"written": true}),
            "service_definition_written=true\n",
        ),
        (
            "service.operator.status",
            serde_json::json!({"operator": {"state": "paused"}}),
            "operator=paused\n",
        ),
        (
            "code.repo.index",
            serde_json::json!({
                "summary": {
                    "indexed_file_count": 2,
                    "symbol_count": 3,
                    "reference_count": 4,
                    "chunk_count": 5,
                    "degraded_file_count": 1,
                },
            }),
            "indexed files=2 symbols=3 references=4 chunks=5 degraded=1\n",
        ),
        (
            "code.repo.scope_preview",
            serde_json::json!({
                "preview": {
                    "selected_file_count": 2,
                    "selected_byte_count": 128,
                    "unsupported_file_count": 1,
                    "expected_degraded_file_count": 1,
                },
            }),
            "preview files=2 bytes=128 unsupported=1 expected_degraded=1\n",
        ),
        (
            "code.repo.impact",
            serde_json::json!({
                "path_groups": {"in_scope_changed_paths": ["src/lib.rs"]},
                "results": [{"symbol_id": "sym:1"}],
            }),
            "changed_in_scope=1 results=1\n",
        ),
        (
            "code.repo.status",
            serde_json::json!({
                "status": {
                    "alias": "repo",
                    "indexed_file_count": 2,
                    "symbol_count": 3,
                    "stale": false,
                },
            }),
            "repo=repo files=2 symbols=3 stale=false\n",
        ),
        (
            "code.repo.report",
            serde_json::json!({
                "report": {
                    "alias": "repo",
                    "indexed_file_count": 2,
                    "freshness_state": "fresh",
                },
            }),
            "repo=repo files=2 freshness=fresh\n",
        ),
    ];

    for (operation, payload, expected) in cases {
        let rendered =
            super::cli_render::render_text(operation, &payload).expect("render should succeed");

        assert_eq!(rendered, expected);
    }
}

#[tokio::test]
async fn run_version_honors_json_and_rejects_streaming_json_format() {
    let service = service_with_memory_store().await;
    let json = run_with_service(
        &service,
        CliCommand {
            action: CliAction::Version,
            format: OutputFormat::Json,
            help: false,
        },
        context("version-json"),
    )
    .await
    .expect("version should render JSON");
    let value: Value = serde_json::from_str(&json).expect("version should be JSON");

    let streaming = run_with_service(
        &service,
        CliCommand {
            action: CliAction::Version,
            format: OutputFormat::StreamingJson,
            help: false,
        },
        context("version-streaming"),
    )
    .await
    .expect_err("streaming-json should be rejected");

    assert_eq!(value["project_name"], "relay-knowledge");
    assert_eq!(value["version"], env!("CARGO_PKG_VERSION"));
    assert_eq!(
        streaming.to_string(),
        "version does not support --format streaming-json"
    );
    assert_eq!(streaming.exit_code(), 2);
}

#[tokio::test]
async fn run_with_service_covers_ingest_query_and_diagnostics() {
    let service = service_with_memory_store().await;
    let ingest = run_with_service(
        &service,
        CliCommand {
            action: CliAction::Ingest {
                source_scope: "docs".to_owned(),
                content: "Rust async SQLite storage".to_owned(),
                entity_labels: vec!["Rust".to_owned()],
            },
            format: OutputFormat::Text,
            help: false,
        },
        context("ingest"),
    )
    .await
    .expect("ingest should run");

    assert_eq!(ingest, "ingested graph_version=1 evidence_count=1\n");

    let query = run_with_service(
        &service,
        CliCommand {
            action: CliAction::Query {
                query: "SQLite".to_owned(),
                source_scope: Some("docs".to_owned()),
                limit: 10,
                freshness: FreshnessPolicy::WaitUntilFresh,
            },
            format: OutputFormat::Text,
            help: false,
        },
        context("query"),
    )
    .await
    .expect("query should run");

    assert_eq!(query, "results=1\n");

    let graph = run_with_service(
        &service,
        CliCommand {
            action: CliAction::GraphInspect,
            format: OutputFormat::Text,
            help: false,
        },
        context("graph"),
    )
    .await
    .expect("graph inspect should run");
    let index = run_with_service(
        &service,
        CliCommand {
            action: CliAction::IndexRefresh {
                kinds: vec![IndexKind::Semantic, IndexKind::Semantic],
            },
            format: OutputFormat::Text,
            help: false,
        },
        context("index"),
    )
    .await
    .expect("index refresh should run");
    let health = run_with_service(
        &service,
        CliCommand {
            action: CliAction::Health,
            format: OutputFormat::Text,
            help: false,
        },
        context("health"),
    )
    .await
    .expect("health should run");
    let service_status = run_with_service(
        &service,
        CliCommand {
            action: CliAction::ServiceStatus,
            format: OutputFormat::Text,
            help: false,
        },
        context("service"),
    )
    .await
    .expect("service status should run");

    assert_eq!(
        graph,
        "graph_version=1 entities=1 evidence=1 code_files=0 code_symbols=0 repo_code_files=0 repo_code_symbols=0\n"
    );
    assert_eq!(index, "refreshed_indexes=1\n");
    assert_eq!(
        health,
        "healthy=true repo_code_files=0 repo_code_symbols=0\n"
    );
    assert_eq!(service_status, "service=relay-knowledge mode=disabled\n");
}

#[tokio::test]
async fn run_with_service_covers_operational_lifecycle_commands() {
    let service = service_with_memory_store().await;
    run_with_service(
        &service,
        CliCommand {
            action: CliAction::Ingest {
                source_scope: "docs".to_owned(),
                content: "Operational text queues extractor and embedding work".to_owned(),
                entity_labels: Vec::new(),
            },
            format: OutputFormat::Text,
            help: false,
        },
        context("ops-ingest"),
    )
    .await
    .expect("ingest should queue worker tasks");

    let worker_status = run_with_service(
        &service,
        CliCommand {
            action: CliAction::WorkerStatus {
                kind: Some(WorkerKind::Extractor),
            },
            format: OutputFormat::Text,
            help: false,
        },
        context("worker-status"),
    )
    .await
    .expect("worker status should run");
    assert_eq!(worker_status, "workers=1\n");

    let first_run = run_with_service(
        &service,
        CliCommand {
            action: CliAction::WorkerRunOnce {
                kind: Some(WorkerKind::Extractor),
            },
            format: OutputFormat::Json,
            help: false,
        },
        context("worker-run-extractor"),
    )
    .await
    .expect("worker run should create proposal");
    let first: Value = serde_json::from_str(&first_run).expect("run output should be JSON");
    let first_id = first["proposals"][0]["proposal_id"]
        .as_str()
        .expect("proposal id should exist")
        .to_owned();

    let show = run_with_service(
        &service,
        CliCommand {
            action: CliAction::ProposalShow {
                proposal_id: first_id.clone(),
            },
            format: OutputFormat::Text,
            help: false,
        },
        context("proposal-show"),
    )
    .await
    .expect("proposal show should run");
    assert!(show.contains("conflicts=0"));

    let accepted = run_with_service(
        &service,
        CliCommand {
            action: CliAction::ProposalAccept {
                proposal_id: first_id,
                actor: "reviewer".to_owned(),
                reason: Some("accepted".to_owned()),
            },
            format: OutputFormat::Text,
            help: false,
        },
        context("proposal-accept"),
    )
    .await
    .expect("proposal accept should run");
    assert!(accepted.contains("state=accepted"));

    let second_run = run_with_service(
        &service,
        CliCommand {
            action: CliAction::WorkerRunOnce {
                kind: Some(WorkerKind::Embedding),
            },
            format: OutputFormat::Json,
            help: false,
        },
        context("worker-run-embedding"),
    )
    .await
    .expect("embedding worker run should create proposal");
    let second: Value = serde_json::from_str(&second_run).expect("run output should be JSON");
    let second_id = second["proposals"][0]["proposal_id"]
        .as_str()
        .expect("proposal id should exist")
        .to_owned();

    let rejected = run_with_service(
        &service,
        CliCommand {
            action: CliAction::ProposalReject {
                proposal_id: second_id,
                actor: "reviewer".to_owned(),
                reason: Some("not needed".to_owned()),
            },
            format: OutputFormat::Text,
            help: false,
        },
        context("proposal-reject"),
    )
    .await
    .expect("proposal reject should run");
    assert!(rejected.contains("state=rejected"));

    let proposal_list = run_with_service(
        &service,
        CliCommand {
            action: CliAction::ProposalList {
                state: None,
                limit: 10,
            },
            format: OutputFormat::Text,
            help: false,
        },
        context("proposal-list"),
    )
    .await
    .expect("proposal list should run");
    let audit = run_with_service(
        &service,
        CliCommand {
            action: CliAction::AuditQuery {
                operation: Some("worker.run_once".to_owned()),
                limit: 10,
            },
            format: OutputFormat::Text,
            help: false,
        },
        context("audit-query"),
    )
    .await
    .expect("audit query should run");
    let service_plan = run_with_service(
        &service,
        CliCommand {
            action: CliAction::ServicePlan {
                action: ServiceManagerAction::Install,
            },
            format: OutputFormat::Text,
            help: false,
        },
        context("service-plan"),
    )
    .await
    .expect("service plan should run");
    let paused = run_with_service(
        &service,
        CliCommand {
            action: CliAction::ServiceOperatorPause,
            format: OutputFormat::Text,
            help: false,
        },
        context("service-pause"),
    )
    .await
    .expect("service operator pause should run");
    let resumed = run_with_service(
        &service,
        CliCommand {
            action: CliAction::ServiceOperatorResume,
            format: OutputFormat::Text,
            help: false,
        },
        context("service-resume"),
    )
    .await
    .expect("service operator resume should run");

    assert!(proposal_list.starts_with("proposals="));
    assert!(audit.starts_with("audit_events="));
    assert!(service_plan.contains("service_plan=install"));
    assert_eq!(paused, "operator=paused\n");
    assert_eq!(resumed, "operator=enabled\n");
}

#[tokio::test]
async fn run_with_service_streams_generic_payloads() {
    let service = service_with_memory_store().await;
    let output = run_with_service(
        &service,
        CliCommand {
            action: CliAction::Health,
            format: OutputFormat::StreamingJson,
            help: false,
        },
        context("stream"),
    )
    .await
    .expect("health should stream");
    let lines = output.lines().collect::<Vec<_>>();
    let item: Value = serde_json::from_str(lines[1]).expect("event should be JSON");

    assert_eq!(lines.len(), 3);
    assert_eq!(item["event"], "item");
    assert_eq!(item["payload"]["healthy"], true);
}

#[tokio::test]
async fn run_with_service_streams_project_status_contract() {
    let service = service_with_memory_store().await;
    let output = run_with_service(
        &service,
        CliCommand {
            action: CliAction::Status,
            format: OutputFormat::StreamingJson,
            help: false,
        },
        context("status-stream"),
    )
    .await
    .expect("status should stream");
    let events = output
        .lines()
        .map(|line| serde_json::from_str::<Value>(line).expect("event should be JSON"))
        .collect::<Vec<_>>();

    assert_eq!(events.len(), 4);
    assert_eq!(events[0]["event"], "started");
    assert_eq!(events[1]["event"], "progress");
    assert_eq!(events[2]["event"], "item");
    assert_eq!(events[2]["project_name"], "relay-knowledge");
    assert_eq!(events[2]["runtime"]["http_bind"], "127.0.0.1:8791");
    assert!(events[2]["payload"].is_null());
    assert_eq!(events[3]["event"], "completed");
}

#[tokio::test]
async fn run_with_service_maps_api_errors_to_cli_errors() {
    let service = service_with_memory_store().await;
    let error = run_with_service(
        &service,
        CliCommand {
            action: CliAction::Query {
                query: " ".to_owned(),
                source_scope: None,
                limit: 10,
                freshness: FreshnessPolicy::AllowStale,
            },
            format: OutputFormat::Json,
            help: false,
        },
        context("error"),
    )
    .await
    .expect_err("empty query should fail");

    assert_eq!(error.exit_code(), 1);
    assert_eq!(error.to_string(), "query must not be empty");
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
    let store = SqliteGraphStore::open_in_memory().expect("store should open");

    RelayKnowledgeService::with_store(runtime, Arc::new(store))
}

fn context(operation: &str) -> RequestContext {
    RequestContext::with_ids(
        InterfaceKind::Cli,
        format!("req-{operation}"),
        format!("trace-{operation}"),
    )
}
