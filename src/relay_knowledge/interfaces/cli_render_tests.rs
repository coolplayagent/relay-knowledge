use super::render_text;

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
            "code.repo.index",
            serde_json::json!({
                "task": {
                    "task_id": "task-1",
                    "state": "queued",
                    "source_scope": "scope-1",
                },
            }),
            "index task=task-1 state=queued scope=scope-1\n",
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
            "code.repo.feature_flags",
            serde_json::json!({
                "flags": [{"feature_flag_id": "flag:1"}],
                "degraded_reason": null,
            }),
            "feature_flags=1 degraded=none\n",
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
        (
            "code.repo.software",
            serde_json::json!({
                "status": {
                    "source_scope": "scope-1",
                    "stale": false,
                },
                "components": [{"component_id": "component:1"}],
                "dependency_usages": [{"usage_id": "dependency_usage:1"}],
                "sdk_usages": [{"usage_id": "sdk_usage:1"}, {"usage_id": "sdk_usage:2"}],
                "files": [{"software_file_id": "file:1"}],
                "topics": [{"topic_id": "topic:1"}],
                "relationships": [{"relationship_id": "relationship:1"}],
            }),
            "software scope=scope-1 components=1 dependency_usages=1 sdk_usages=2 files=1 topics=1 relationships=1 stale=false\n",
        ),
        (
            "setup.doctor",
            serde_json::json!({
                "configuration_ready": true,
                "live_health_checked": false,
                "checks": [{ "name": "runtime_paths" }],
                "recommended_actions": [],
            }),
            "setup_configuration_ready=true live_health_checked=false checks=1 actions=0\n",
        ),
        (
            "setup.profile",
            serde_json::json!({
                "profile": "agent-readonly",
                "environment": [{"name": "RELAY_KNOWLEDGE_MCP_ALLOWED_SCOPES"}],
                "commands": ["relay-knowledge service run --mcp streamable-http"],
            }),
            "setup_profile=agent-readonly env_vars=1 commands=1\n",
        ),
    ];

    for (operation, payload, expected) in cases {
        let rendered = render_text(operation, &payload).expect("render should succeed");

        assert_eq!(rendered, expected);
    }
}
