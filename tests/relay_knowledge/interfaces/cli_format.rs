use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

use relay_knowledge::{
    PROJECT_NAME,
    env::{
        ALL_PROXY, ALL_PROXY_LOWER, HTTP_PROXY, HTTP_PROXY_LOWER, HTTPS_PROXY, HTTPS_PROXY_LOWER,
        NO_PROXY, NO_PROXY_LOWER, RELAY_KNOWLEDGE_CACHE_DIR, RELAY_KNOWLEDGE_CONFIG_DIR,
        RELAY_KNOWLEDGE_DATA_DIR, RELAY_KNOWLEDGE_HOME, RELAY_KNOWLEDGE_HTTP_BIND,
        RELAY_KNOWLEDGE_HTTP_MAX_BODY_BYTES, RELAY_KNOWLEDGE_HTTP_REQUEST_TIMEOUT_MS,
        RELAY_KNOWLEDGE_HTTP_SHUTDOWN_TIMEOUT_MS, RELAY_KNOWLEDGE_LOG_DIR,
        RELAY_KNOWLEDGE_MCP_ALLOW_REMOTE_CLIENTS, RELAY_KNOWLEDGE_MCP_ALLOW_UNSPECIFIED_SCOPE,
        RELAY_KNOWLEDGE_MCP_ALLOWED_ORIGINS, RELAY_KNOWLEDGE_MCP_ALLOWED_SCOPES,
        RELAY_KNOWLEDGE_MCP_ENDPOINT, RELAY_KNOWLEDGE_MCP_MAX_CONTEXT_BYTES,
        RELAY_KNOWLEDGE_MCP_MAX_LIMIT, RELAY_KNOWLEDGE_MCP_STREAMABLE_HTTP_ENABLED,
        RELAY_KNOWLEDGE_QOS_MAX_CONNECTIONS, RELAY_KNOWLEDGE_QOS_MAX_IN_FLIGHT_REQUESTS,
        RELAY_KNOWLEDGE_QOS_MAX_QUEUE_DEPTH, RELAY_KNOWLEDGE_RUNTIME_DIR,
        RELAY_KNOWLEDGE_SERVICE_DIR, RELAY_KNOWLEDGE_STATE_DIR, RELAY_KNOWLEDGE_TEMP_DIR,
        SSL_VERIFY, SSL_VERIFY_LOWER,
    },
    interfaces::cli::{CliAction, CliCommand, OutputFormat},
};
use serde_json::Value;

#[test]
fn parses_cli_output_formats() {
    let text = CliCommand::parse(["--format", "text"]).expect("text format should parse");
    let json = CliCommand::parse(["--format=json"]).expect("json format should parse");
    let streaming_json = CliCommand::parse(["--format", "streaming-json"])
        .expect("streaming-json format should parse");

    assert_eq!(text.format, OutputFormat::Text);
    assert_eq!(json.format, OutputFormat::Json);
    assert_eq!(streaming_json.format, OutputFormat::StreamingJson);
    assert_eq!(text.action, CliAction::Status);
}

#[test]
fn rejects_unknown_cli_output_format() {
    let error = CliCommand::parse(["--format", "xml"]).expect_err("format should be rejected");

    assert_eq!(error.exit_code(), 2);
    assert_eq!(
        error.to_string(),
        "invalid --format value 'xml', expected text, json, markdown, or streaming-json"
    );
}

#[test]
fn binary_outputs_text_by_default() {
    let output = relay_command().output().expect("binary should run");

    assert!(output.status.success());
    assert_eq!(String::from_utf8_lossy(&output.stdout), "relay-knowledge\n");
    assert!(output.stderr.is_empty());
}

#[test]
fn binary_outputs_single_json_object() {
    let output = relay_command()
        .args(["--format", "json"])
        .output()
        .expect("binary should run");

    assert!(output.status.success());
    assert!(output.stderr.is_empty());

    let value: Value = serde_json::from_slice(&output.stdout).expect("stdout should be JSON");

    assert_eq!(value["project_name"], "relay-knowledge");
    assert_eq!(value["metadata"]["graph_version"], 0);
    assert_eq!(value["metadata"]["stale"], false);
    assert_eq!(value["runtime"]["http_bind"], "127.0.0.1:8791");
    assert_eq!(value["runtime"]["http_proxy_configured"], false);
    assert_eq!(value["runtime"]["http_no_proxy_rules"], 0);
    assert_eq!(value["runtime"]["http_ssl_verify"], true);
    assert_eq!(value["runtime"]["qos_max_connections"], 1024);
    assert!(value["metadata"]["trace_id"].as_str().is_some());
    assert!(value["metadata"]["request_id"].as_str().is_some());
}

#[test]
fn binary_outputs_streaming_json_as_ndjson_events() {
    let output = relay_command()
        .args(["--format=streaming-json"])
        .output()
        .expect("binary should run");

    assert!(output.status.success());
    assert!(output.stderr.is_empty());

    let stdout = String::from_utf8(output.stdout).expect("stdout should be UTF-8");
    let lines = stdout.lines().collect::<Vec<_>>();

    assert_eq!(lines.len(), 4);

    let events = lines
        .iter()
        .map(|line| serde_json::from_str::<Value>(line).expect("line should be JSON"))
        .collect::<Vec<_>>();

    assert_eq!(events[0]["event"], "started");
    assert_eq!(events[1]["event"], "progress");
    assert_eq!(events[2]["event"], "item");
    assert_eq!(events[2]["project_name"], "relay-knowledge");
    assert_eq!(events[2]["runtime"]["http_bind"], "127.0.0.1:8791");
    assert!(events[2]["payload"].is_null());
    assert_eq!(events[3]["event"], "completed");

    for event in events {
        assert_eq!(event["operation"], "project.status");
        assert_eq!(event["metadata"]["graph_version"], 0);
    }
}

#[test]
fn binary_outputs_version_without_runtime_configuration() {
    let output = relay_command()
        .env(RELAY_KNOWLEDGE_HOME, "")
        .args(["version"])
        .output()
        .expect("binary should run");

    assert!(output.status.success());
    assert_eq!(
        String::from_utf8_lossy(&output.stdout),
        format!("relay-knowledge {}\n", env!("CARGO_PKG_VERSION"))
    );
    assert!(output.stderr.is_empty());
}

#[test]
fn binary_outputs_version_json_from_flag_alias() {
    let output = relay_command()
        .env(RELAY_KNOWLEDGE_HOME, "")
        .args(["--version", "--format", "json"])
        .output()
        .expect("binary should run");

    assert!(output.status.success());
    assert!(output.stderr.is_empty());

    let value: Value = serde_json::from_slice(&output.stdout).expect("version JSON");

    assert_eq!(value["project_name"], "relay-knowledge");
    assert_eq!(value["version"], env!("CARGO_PKG_VERSION"));
}

#[test]
fn binary_outputs_root_help_without_runtime_configuration() {
    let output = relay_command()
        .env(RELAY_KNOWLEDGE_HOME, "")
        .args(["--help"])
        .output()
        .expect("binary should run");

    assert!(output.status.success());
    assert!(output.stderr.is_empty());

    let stdout = String::from_utf8_lossy(&output.stdout);

    assert!(stdout.contains("Usage: relay-knowledge <command>"));
    assert!(stdout.contains("help"));
    assert!(stdout.contains("machine-readable parameter metadata"));
}

#[test]
fn binary_outputs_command_help_text() {
    let output = relay_command()
        .env(RELAY_KNOWLEDGE_HOME, "")
        .args(["repo", "query", "core", "--help"])
        .output()
        .expect("binary should run");

    assert!(output.status.success());
    assert!(output.stderr.is_empty());

    let stdout = String::from_utf8_lossy(&output.stdout);

    assert!(stdout.contains("Usage: relay-knowledge repo query"));
    assert!(stdout.contains("--kind"));
    assert!(stdout.contains("definition"));
}

#[test]
fn binary_outputs_namespace_help_text() {
    let output = relay_command()
        .env(RELAY_KNOWLEDGE_HOME, "")
        .args(["repo", "--help"])
        .output()
        .expect("binary should run");

    assert!(output.status.success());
    assert!(output.stderr.is_empty());

    let stdout = String::from_utf8_lossy(&output.stdout);

    assert!(stdout.contains("Usage: relay-knowledge repo <subcommand>"));
    assert!(stdout.contains("repo query"));
    assert!(stdout.contains("repo index"));
}

#[test]
fn binary_outputs_machine_readable_help() {
    let output = relay_command()
        .env(RELAY_KNOWLEDGE_HOME, "")
        .args(["help", "repo", "query", "--format", "json"])
        .output()
        .expect("binary should run");
    let software = relay_command()
        .env(RELAY_KNOWLEDGE_HOME, "")
        .args(["help", "repo", "software", "--format", "json"])
        .output()
        .expect("binary should run");

    assert!(output.status.success());
    assert!(software.status.success());
    assert!(output.stderr.is_empty());
    assert!(software.stderr.is_empty());

    let value: Value = serde_json::from_slice(&output.stdout).expect("help JSON");
    let software_value: Value =
        serde_json::from_slice(&software.stdout).expect("software help JSON");

    assert_eq!(value["schema_version"], 2);
    assert_eq!(value["path"], serde_json::json!(["repo", "query"]));
    assert_eq!(value["operation"], "code.repo.query");
    assert_eq!(value["effect"], "read-only");
    assert_eq!(value["syntax"]["kind"], "command");
    assert!(
        value["options"]
            .as_array()
            .expect("options")
            .iter()
            .any(|option| option["flag"] == "--kind"
                && option["allowed_values"]
                    .as_array()
                    .expect("values")
                    .iter()
                    .any(|value| value == "definition"))
    );
    assert!(
        software_value["options"]
            .as_array()
            .expect("software options")
            .iter()
            .any(|option| option["flag"] == "--kind"
                && option["allowed_values"]
                    == serde_json::json!([
                        "dependencies",
                        "sdks",
                        "files",
                        "topics",
                        "relationships",
                        "all"
                    ]))
    );
}

#[test]
fn binary_help_metadata_uses_canonical_cli_name() {
    let root = relay_command()
        .args(["help", "--format", "json"])
        .output()
        .expect("binary should run");
    let repo = relay_command()
        .args(["help", "repo", "--format", "json"])
        .output()
        .expect("binary should run");
    let repo_query = relay_command()
        .args(["help", "repo", "query", "--format", "json"])
        .output()
        .expect("binary should run");

    assert!(root.status.success());
    assert!(repo.status.success());
    assert!(repo_query.status.success());

    let root_json: Value = serde_json::from_slice(&root.stdout).expect("root help JSON");
    let repo_json: Value = serde_json::from_slice(&repo.stdout).expect("repo help JSON");
    let repo_query_json: Value =
        serde_json::from_slice(&repo_query.stdout).expect("repo query help JSON");

    assert_eq!(root_json["binary"], PROJECT_NAME);
    assert_eq!(repo_json["binary"], PROJECT_NAME);
    assert_eq!(repo_query_json["binary"], PROJECT_NAME);
    assert_eq!(repo_json["kind"], "namespace");

    for command in root_json["commands"].as_array().expect("root commands") {
        assert_command_metadata_uses_project_name(command);
    }
    for command in repo_json["commands"].as_array().expect("repo commands") {
        assert_command_metadata_uses_project_name(command);
    }
    assert_command_metadata_uses_project_name(&repo_query_json);
}

#[test]
fn binary_rejects_streaming_json_version_format() {
    let output = relay_command()
        .args(["version", "--format", "streaming-json"])
        .output()
        .expect("binary should run");

    assert_eq!(output.status.code(), Some(2));
    assert!(output.stdout.is_empty());
    assert_eq!(
        String::from_utf8_lossy(&output.stderr).trim(),
        "version does not support --format streaming-json"
    );
}

#[test]
fn binary_ingests_queries_and_inspects_isolated_graph() {
    let home = isolated_home("binary-ingests-queries");
    let ingest = relay_command()
        .env(RELAY_KNOWLEDGE_HOME, &home)
        .args([
            "ingest",
            "--source",
            "docs",
            "--content",
            "Rust async services isolate blocking SQLite work",
            "--entity",
            "Rust",
            "--format",
            "json",
        ])
        .output()
        .expect("ingest command should run");

    assert!(ingest.status.success());
    assert!(ingest.stderr.is_empty());
    let ingest_json: Value = serde_json::from_slice(&ingest.stdout).expect("ingest JSON");
    assert_eq!(ingest_json["metadata"]["graph_version"], 1);
    assert_eq!(ingest_json["receipt"]["evidence_count"], 1);
    assert_eq!(ingest_json["indexes"].as_array().expect("indexes").len(), 3);

    let query = relay_command()
        .env(RELAY_KNOWLEDGE_HOME, &home)
        .args([
            "query",
            "SQLite",
            "--source",
            "docs",
            "--freshness",
            "wait-until-fresh",
            "--format",
            "json",
        ])
        .output()
        .expect("query command should run");

    assert!(query.status.success());
    assert!(query.stderr.is_empty());
    let query_json: Value = serde_json::from_slice(&query.stdout).expect("query JSON");
    assert_eq!(query_json["metadata"]["graph_version"], 1);
    assert_eq!(query_json["metadata"]["stale"], false);
    assert_eq!(query_json["results"].as_array().expect("results").len(), 1);

    let inspect = relay_command()
        .env(RELAY_KNOWLEDGE_HOME, &home)
        .args(["graph", "inspect", "--format", "json"])
        .output()
        .expect("inspect command should run");

    assert!(inspect.status.success());
    let inspect_json: Value = serde_json::from_slice(&inspect.stdout).expect("inspect JSON");
    assert_eq!(inspect_json["graph"]["entity_count"], 1);
    assert_eq!(inspect_json["graph"]["evidence_count"], 1);
}

#[test]
fn binary_queries_dash_prefixed_text_after_delimiter() {
    let output = relay_command()
        .args(["--format", "json", "query", "--", "--help"])
        .output()
        .expect("query command should run");

    assert!(output.status.success());
    assert!(output.stderr.is_empty());

    let value: Value = serde_json::from_slice(&output.stdout).expect("query JSON");

    assert_eq!(value["metadata"]["graph_version"], 0);
    assert_eq!(value["results"].as_array().expect("results").len(), 0);
}

#[test]
fn binary_queries_dash_prefixed_text_with_trailing_global_format() {
    let output = relay_command()
        .args(["query", "--", "--help", "--format", "json"])
        .output()
        .expect("query command should run");

    assert!(output.status.success());
    assert!(output.stderr.is_empty());

    let value: Value = serde_json::from_slice(&output.stdout).expect("query JSON");

    assert_eq!(value["metadata"]["graph_version"], 0);
    assert_eq!(value["results"].as_array().expect("results").len(), 0);
}

#[test]
fn binary_ingests_dash_prefixed_content_value() {
    let home = isolated_home("binary-ingests-dash-prefixed-content");
    let output = relay_command()
        .env(RELAY_KNOWLEDGE_HOME, &home)
        .args([
            "ingest",
            "--source",
            "docs",
            "--content",
            "--version",
            "--format",
            "json",
        ])
        .output()
        .expect("ingest command should run");

    assert!(output.status.success());
    assert!(output.stderr.is_empty());

    let value: Value = serde_json::from_slice(&output.stdout).expect("ingest JSON");

    assert_eq!(value["metadata"]["graph_version"], 1);
    assert_eq!(value["receipt"]["evidence_count"], 1);
}

#[test]
fn binary_reports_health_and_service_status() {
    let home = isolated_home("binary-health-service");

    let health = relay_command()
        .env(RELAY_KNOWLEDGE_HOME, &home)
        .args(["health", "--format", "json"])
        .output()
        .expect("health command should run");

    assert!(health.status.success());
    let health_json: Value = serde_json::from_slice(&health.stdout).expect("health JSON");
    assert_eq!(health_json["healthy"], true);
    assert_eq!(health_json["graph"]["graph_version"], 0);

    let service = relay_command()
        .env(RELAY_KNOWLEDGE_HOME, &home)
        .args(["service", "doctor", "--format", "json"])
        .output()
        .expect("service command should run");

    assert!(service.status.success());
    let service_json: Value = serde_json::from_slice(&service.stdout).expect("service JSON");
    assert_eq!(service_json["service_name"], "relay-knowledge");
    assert_eq!(service_json["mode"], "disabled");
}

#[test]
fn binary_reports_setup_doctor_and_profiles() {
    let home = isolated_home("binary-setup");

    let doctor = relay_command()
        .env(RELAY_KNOWLEDGE_HOME, &home)
        .args(["setup", "doctor", "--format", "json"])
        .output()
        .expect("setup doctor should run");

    assert!(doctor.status.success());
    assert!(doctor.stderr.is_empty());
    let doctor_json: Value = serde_json::from_slice(&doctor.stdout).expect("doctor JSON");
    assert_eq!(doctor_json["configuration_ready"], true);
    assert_eq!(doctor_json["live_health_checked"], false);
    assert!(
        doctor_json["checks"]
            .as_array()
            .expect("checks")
            .iter()
            .any(|check| check["name"] == "network_budget")
    );
    assert!(
        doctor_json["live_health_commands"]
            .as_array()
            .expect("live health commands")
            .iter()
            .any(|command| command == "relay-knowledge health --format json")
    );

    let profile = relay_command()
        .env(RELAY_KNOWLEDGE_HOME, &home)
        .args(["setup", "profile", "agent-readonly", "--format", "json"])
        .output()
        .expect("setup profile should run");

    assert!(profile.status.success());
    assert!(profile.stderr.is_empty());
    let profile_json: Value = serde_json::from_slice(&profile.stdout).expect("profile JSON");
    assert_eq!(profile_json["profile"], "agent-readonly");
    assert!(
        profile_json["environment"]
            .as_array()
            .expect("environment")
            .iter()
            .any(|variable| variable["name"] == "RELAY_KNOWLEDGE_MCP_ALLOWED_SCOPES")
    );
}

#[test]
fn binary_rejects_flag_style_actions_and_extra_command_words() {
    let flag_action = relay_command()
        .args(["--ingest", "--source", "docs", "--content", "x"])
        .output()
        .expect("binary should run");
    let extra = relay_command()
        .args(["status", "health"])
        .output()
        .expect("binary should run");

    assert_eq!(flag_action.status.code(), Some(2));
    let flag_stderr = String::from_utf8_lossy(&flag_action.stderr);
    assert!(flag_stderr.contains("unknown option '--ingest'; commands are positional"));
    assert!(flag_stderr.contains("relay-knowledge ingest"));
    assert_eq!(extra.status.code(), Some(2));
    assert!(
        String::from_utf8_lossy(&extra.stderr)
            .contains("unexpected argument 'health' for 'status'")
    );
}

#[test]
fn binary_diagnostics_use_canonical_cli_name() {
    let typo = relay_command()
        .args(["repo", "qurey", "core", "--query", "rust"])
        .output()
        .expect("binary should run");
    let json = relay_command()
        .args(["--format", "json", "query", "--query", "SQLite"])
        .output()
        .expect("binary should run");

    assert_eq!(typo.status.code(), Some(2));
    let stderr = String::from_utf8_lossy(&typo.stderr);
    assert!(stderr.contains("did you mean 'repo query'"));
    assert!(stderr.contains(&format!("Try: {PROJECT_NAME} repo query")));
    assert!(stderr.contains(&format!("Usage: {PROJECT_NAME} repo <subcommand>")));

    assert_eq!(json.status.code(), Some(2));
    let value: Value = serde_json::from_slice(&json.stderr).expect("diagnostic JSON");
    assert_eq!(value["suggestion"], format!("{PROJECT_NAME} query SQLite"));
    assert_eq!(
        value["usage"],
        format!(
            "{PROJECT_NAME} query <text> [--source <scope>] [--limit <n>] [--freshness <policy>]"
        )
    );
}

#[test]
fn binary_outputs_json_parse_diagnostic_on_json_format_errors() {
    let output = relay_command()
        .args(["--format", "json", "query", "--query", "SQLite"])
        .output()
        .expect("binary should run");

    assert_eq!(output.status.code(), Some(2));
    assert!(output.stdout.is_empty());

    let value: Value = serde_json::from_slice(&output.stderr).expect("stderr diagnostic JSON");

    assert_eq!(value["matched_path"], serde_json::json!(["query"]));
    assert_eq!(value["unexpected_token"], "--query");
    assert_eq!(value["suggestion"], format!("{PROJECT_NAME} query SQLite"));
}

fn assert_command_metadata_uses_project_name(command: &Value) {
    let path = command["path"]
        .as_array()
        .expect("path should be an array")
        .iter()
        .map(|segment| {
            segment
                .as_str()
                .expect("path segment should be a string")
                .to_owned()
        })
        .collect::<Vec<_>>();
    let usage = command["usage"].as_str().expect("usage should exist");

    assert_usage_invokes_path(usage, &path);

    for example in command["examples"]
        .as_array()
        .expect("examples should be an array")
    {
        assert_invocation_uses_project_name(example.as_str().expect("example should be a string"));
    }
}

fn assert_usage_invokes_path(usage: &str, path: &[String]) {
    assert_invocation_uses_project_name(usage);

    let tokens = usage.split_whitespace().collect::<Vec<_>>();
    assert_eq!(tokens.first().copied(), Some(PROJECT_NAME));

    for (index, segment) in path.iter().enumerate() {
        let usage_segment = tokens
            .get(index + 1)
            .unwrap_or_else(|| panic!("usage is missing path segment '{segment}': {usage}"));
        assert!(
            usage_segment
                .split('|')
                .any(|candidate| candidate == segment.as_str()),
            "usage segment '{usage_segment}' should include path segment '{segment}' in {usage}"
        );
    }
}

fn assert_invocation_uses_project_name(invocation: &str) {
    assert!(
        invocation
            .split_whitespace()
            .any(|token| token == PROJECT_NAME),
        "invocation should include canonical CLI name '{PROJECT_NAME}': {invocation}"
    );
}

fn relay_command() -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_relay-knowledge"));

    for variable in [
        RELAY_KNOWLEDGE_HOME,
        RELAY_KNOWLEDGE_CONFIG_DIR,
        RELAY_KNOWLEDGE_DATA_DIR,
        RELAY_KNOWLEDGE_STATE_DIR,
        RELAY_KNOWLEDGE_CACHE_DIR,
        RELAY_KNOWLEDGE_LOG_DIR,
        RELAY_KNOWLEDGE_TEMP_DIR,
        RELAY_KNOWLEDGE_RUNTIME_DIR,
        RELAY_KNOWLEDGE_SERVICE_DIR,
        RELAY_KNOWLEDGE_HTTP_BIND,
        RELAY_KNOWLEDGE_HTTP_REQUEST_TIMEOUT_MS,
        RELAY_KNOWLEDGE_HTTP_SHUTDOWN_TIMEOUT_MS,
        RELAY_KNOWLEDGE_HTTP_MAX_BODY_BYTES,
        RELAY_KNOWLEDGE_MCP_STREAMABLE_HTTP_ENABLED,
        RELAY_KNOWLEDGE_MCP_ENDPOINT,
        RELAY_KNOWLEDGE_MCP_ALLOWED_ORIGINS,
        RELAY_KNOWLEDGE_MCP_ALLOWED_SCOPES,
        RELAY_KNOWLEDGE_MCP_ALLOW_UNSPECIFIED_SCOPE,
        RELAY_KNOWLEDGE_MCP_MAX_LIMIT,
        RELAY_KNOWLEDGE_MCP_MAX_CONTEXT_BYTES,
        RELAY_KNOWLEDGE_MCP_ALLOW_REMOTE_CLIENTS,
        RELAY_KNOWLEDGE_QOS_MAX_CONNECTIONS,
        RELAY_KNOWLEDGE_QOS_MAX_IN_FLIGHT_REQUESTS,
        RELAY_KNOWLEDGE_QOS_MAX_QUEUE_DEPTH,
        HTTPS_PROXY,
        HTTPS_PROXY_LOWER,
        HTTP_PROXY,
        HTTP_PROXY_LOWER,
        ALL_PROXY,
        ALL_PROXY_LOWER,
        NO_PROXY,
        NO_PROXY_LOWER,
        SSL_VERIFY,
        SSL_VERIFY_LOWER,
    ] {
        command.env_remove(variable);
    }
    command.env(RELAY_KNOWLEDGE_HOME, isolated_home("relay-command"));

    command
}

fn isolated_home(test_name: &str) -> String {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock should be after epoch")
        .as_nanos();
    let path = std::env::temp_dir().join(format!("relay-knowledge-{test_name}-{nanos}"));

    path.display().to_string()
}
