//! Test-only loopback transport verifies query encoding without external services.
use super::*;
#[tokio::test]
async fn remote_diagnostics_preserves_cursor_filters_and_page_response() {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let server = tokio::spawn(async move {
        let (mut stream, _) = listener.accept().await.unwrap();
        let mut buffer = vec![0; 8192];
        let count = stream.read(&mut buffer).await.unwrap();
        let request = String::from_utf8_lossy(&buffer[..count]);
        let target = request
            .lines()
            .next()
            .unwrap()
            .split_whitespace()
            .nth(1)
            .unwrap();
        let url = reqwest::Url::parse(&format!("http://localhost{target}")).unwrap();
        assert_eq!(url.path(), "/api/v1/code/repositories/demo/diagnostics");
        let pairs = url
            .query_pairs()
            .collect::<std::collections::HashMap<_, _>>();
        assert_eq!(pairs["cursor"], "token +&?");
        assert_eq!(pairs["path_filters"], r#"["src"]"#);
        assert_eq!(pairs["limit"], "2");
        let response=json!({"metadata":{"trace_id":"trace","request_id":"req","graph_version":1,"stale":false},"scope":{"scope_id":"scope","repository_id":"repo","alias":"demo","requested_ref":"HEAD","resolved_commit_sha":"commit","tree_hash":"tree","path_filters":[],"language_filters":[],"index_versions":[],"stale":false},"degraded_file_count":1,"diagnostics":[{"repository_id":"repo","source_scope":"scope","path":"src/bad.py","parse_status":"partial","message":"syntax error"}],"next_cursor":null}).to_string();
        stream.write_all(format!("HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",response.len(),response).as_bytes()).await.unwrap();
    });
    let command = CliCommand::parse([
        "repo",
        "diagnostics",
        "demo",
        "--path",
        "src",
        "--limit",
        "2",
        "--cursor",
        "token +&?",
        "--format",
        "json",
    ])
    .unwrap();
    assert!(remote::supports(&command.action));
    let output = remote::run_remote(
        &NetworkEnvOverrides::default(),
        &format!("http://{addr}"),
        &command.action,
        context("diagnostics"),
        OutputFormat::Json,
    )
    .await
    .unwrap()
    .unwrap();
    let response: Value = serde_json::from_str(&output).unwrap();
    assert_eq!(response["degraded_file_count"], 1);
    assert_eq!(response["diagnostics"][0]["path"], "src/bad.py");
    server.await.unwrap();
}
