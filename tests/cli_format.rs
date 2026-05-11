use std::process::Command;

use relay_knowledge::interfaces::cli::{CliCommand, OutputFormat};
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
    let output = Command::new(env!("CARGO_BIN_EXE_relay-knowledge"))
        .output()
        .expect("binary should run");

    assert!(output.status.success());
    assert_eq!(String::from_utf8_lossy(&output.stdout), "relay-knowledge\n");
    assert!(output.stderr.is_empty());
}

#[test]
fn binary_outputs_single_json_object() {
    let output = Command::new(env!("CARGO_BIN_EXE_relay-knowledge"))
        .args(["--format", "json"])
        .output()
        .expect("binary should run");

    assert!(output.status.success());
    assert!(output.stderr.is_empty());

    let value: Value = serde_json::from_slice(&output.stdout).expect("stdout should be JSON");

    assert_eq!(value["project_name"], "relay-knowledge");
    assert_eq!(value["metadata"]["graph_version"], 0);
    assert_eq!(value["metadata"]["stale"], false);
    assert!(value["metadata"]["trace_id"].as_str().is_some());
    assert!(value["metadata"]["request_id"].as_str().is_some());
}

#[test]
fn binary_outputs_streaming_json_as_ndjson_events() {
    let output = Command::new(env!("CARGO_BIN_EXE_relay-knowledge"))
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
    assert_eq!(events[3]["event"], "completed");

    for event in events {
        assert_eq!(event["operation"], "project.status");
        assert_eq!(event["metadata"]["graph_version"], 0);
    }
}
