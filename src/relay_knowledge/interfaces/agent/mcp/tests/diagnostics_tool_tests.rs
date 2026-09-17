use super::*;
#[tokio::test]
async fn diagnostics_mcp_paginates_and_enforces_access_policy() {
    let repo = FixtureRepo::create("mcp-diagnostics");
    for i in 0..3 {
        repo.write(
            &format!("knowledge/bad{i}.py"),
            "def broken():\n    return (\n",
        );
    }
    repo.git(["add", "."]);
    repo.git(["commit", "-m", "diagnostics"]);
    let fixture_home = repo.path.to_str().unwrap();
    let (server, service) = server_and_service([
        ("HOME", fixture_home),
        ("TMPDIR", fixture_home),
        ("RELAY_KNOWLEDGE_HOME", fixture_home),
        ("RELAY_KNOWLEDGE_MCP_ALLOWED_SCOPES", "fixture"),
        ("RELAY_KNOWLEDGE_MCP_MAX_LIMIT", "2"),
    ])
    .await;
    register_and_index_fixture(&service, &repo).await;
    let mut router = server.router();
    let response = tool_call(
        &mut router,
        "diagnostics",
        "relay_code_diagnostics",
        json!({"repository":"fixture"}),
    )
    .await;
    assert_eq!(response["result"]["isError"], false, "{response:#}");
    let page = &response["result"]["structuredContent"];
    assert_eq!(page["degraded_file_count"], 3);
    assert_eq!(page["diagnostics"].as_array().unwrap().len(), 2);
    let next = tool_call(
        &mut router,
        "diagnostics-next",
        "relay_code_diagnostics",
        json!({"repository":"fixture","cursor":page["next_cursor"]}),
    )
    .await;
    assert_eq!(
        next["result"]["structuredContent"]["diagnostics"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    let denied = tool_call(
        &mut router,
        "diagnostics-limit",
        "relay_code_diagnostics",
        json!({"repository":"fixture","limit":3}),
    )
    .await;
    assert_eq!(
        denied["result"]["structuredContent"]["error_kind"],
        "limit_exceeded"
    );
    let denied = tool_call(
        &mut router,
        "diagnostics-scope",
        "relay_code_diagnostics",
        json!({"repository":"other"}),
    )
    .await;
    assert_eq!(denied["result"]["isError"], true);
}
