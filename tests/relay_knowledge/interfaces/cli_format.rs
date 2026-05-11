use std::process::Command;

use relay_knowledge::{
    env::{
        ALL_PROXY, ALL_PROXY_LOWER, HTTP_PROXY, HTTP_PROXY_LOWER, HTTPS_PROXY, HTTPS_PROXY_LOWER,
        NO_PROXY, NO_PROXY_LOWER, RELAY_KNOWLEDGE_CACHE_DIR, RELAY_KNOWLEDGE_CONFIG_DIR,
        RELAY_KNOWLEDGE_DATA_DIR, RELAY_KNOWLEDGE_HOME, RELAY_KNOWLEDGE_HTTP_BIND,
        RELAY_KNOWLEDGE_HTTP_MAX_BODY_BYTES, RELAY_KNOWLEDGE_HTTP_REQUEST_TIMEOUT_MS,
        RELAY_KNOWLEDGE_HTTP_SHUTDOWN_TIMEOUT_MS, RELAY_KNOWLEDGE_LOG_DIR,
        RELAY_KNOWLEDGE_QOS_MAX_CONNECTIONS, RELAY_KNOWLEDGE_QOS_MAX_IN_FLIGHT_REQUESTS,
        RELAY_KNOWLEDGE_QOS_MAX_QUEUE_DEPTH, RELAY_KNOWLEDGE_RUNTIME_DIR,
        RELAY_KNOWLEDGE_SERVICE_DIR, RELAY_KNOWLEDGE_STATE_DIR, RELAY_KNOWLEDGE_TEMP_DIR,
        SSL_VERIFY, SSL_VERIFY_LOWER,
    },
    interfaces::cli::{CliCommand, OutputFormat},
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
}

#[test]
fn rejects_unknown_cli_output_format() {
    let error = CliCommand::parse(["--format", "xml"]).expect_err("format should be rejected");

    assert_eq!(error.exit_code(), 2);
    assert_eq!(
        error.to_string(),
        "invalid --format value 'xml', expected text, json, or streaming-json"
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
    assert_eq!(events[3]["event"], "completed");

    for event in events {
        assert_eq!(event["operation"], "project.status");
        assert_eq!(event["metadata"]["graph_version"], 0);
    }
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

    command
}
