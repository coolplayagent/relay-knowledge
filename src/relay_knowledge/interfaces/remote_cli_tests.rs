use serde_json::{Value, json};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

use super::*;
use crate::{domain::CodeQueryKind, env::NetworkEnvOverrides};

#[test]
fn parses_global_remote_service_url() {
    let command = CliCommand::parse([
        "repo",
        "query",
        "core",
        "--query",
        "retry_policy",
        "--remote",
        "http://127.0.0.1:8791",
        "--format",
        "json",
    ])
    .expect("remote repo query command should parse");

    assert_eq!(command.format, OutputFormat::Json);
    assert_eq!(
        command.remote_base_url,
        Some("http://127.0.0.1:8791".to_owned())
    );
    assert!(matches!(command.action, CliAction::Repo(_)));
}

#[tokio::test]
async fn remote_repo_query_posts_stable_code_api_and_renders_response() {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("listener should bind");
    let addr = listener.local_addr().expect("listener addr should resolve");
    let server = tokio::spawn(async move {
        let (mut stream, _) = listener.accept().await.expect("client should connect");
        let mut buffer = vec![0; 4096];
        let count = stream.read(&mut buffer).await.expect("request should read");
        let request = String::from_utf8_lossy(&buffer[..count]);
        assert!(request.starts_with("POST /api/v1/code/repositories/org%2Frepo/query HTTP/1.1"));
        assert!(request.contains("x-relay-request-id: req-remote-query"));
        assert!(request.contains("\"query\":\"retry_policy\""));
        assert!(request.contains("\"repository\":\"org/repo\""));
        let response = json!({
            "metadata": {
                "trace_id": "trace-remote-query",
                "request_id": "req-remote-query",
                "graph_version": 1,
                "stale": false
            },
            "scope": {
                "scope_id": "git_snapshot:0000000000000001",
                "repository_id": "repo:org/repo",
                "alias": "org/repo",
                "requested_ref": "HEAD",
                "resolved_commit_sha": "abc",
                "tree_hash": "tree",
                "path_filters": [],
                "language_filters": [],
                "index_versions": ["code:git_snapshot:0000000000000001:tree"],
                "stale": false
            },
            "request": {
                "query": "retry_policy",
                "repository": {
                    "repository": "org/repo",
                    "ref_selector": "HEAD",
                    "path_filters": [],
                    "language_filters": []
                },
                "code_query_kind": "definition",
                "limit": 5,
                "freshness_policy": "allow_stale"
            },
            "results": []
        })
        .to_string();
        let response_head = format!(
            "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n",
            response.len()
        );
        stream
            .write_all(response_head.as_bytes())
            .await
            .expect("response head should write");
        stream
            .write_all(response.as_bytes())
            .await
            .expect("response body should write");
    });
    let action = CliAction::Repo(repo_cli::RepoCommand::Query {
        alias: "org/repo".to_owned(),
        query: "retry_policy".to_owned(),
        kind: CodeQueryKind::Definition,
        limit: 5,
        ref_selector: "HEAD".to_owned(),
        path_filters: Vec::new(),
        language_filters: Vec::new(),
        freshness: FreshnessPolicy::AllowStale,
    });

    let output = remote_cli::run_remote(
        &NetworkEnvOverrides::default(),
        &format!("http://{addr}"),
        &action,
        context("remote-query"),
        OutputFormat::Json,
    )
    .await
    .expect("remote query should run")
    .expect("repo query should be supported");

    let value: Value = serde_json::from_str(output.trim()).expect("remote output should be JSON");
    assert_eq!(value["request"]["query"], "retry_policy");
    assert_eq!(value["results"].as_array().expect("results array").len(), 0);
    server.await.expect("server task should finish");
}

fn context(name: &str) -> RequestContext {
    RequestContext::with_ids(
        InterfaceKind::Cli,
        format!("req-{name}"),
        format!("trace-{name}"),
    )
}
