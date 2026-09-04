use super::render_text;

#[test]
fn map_show_reports_complete_v1_history_window() {
    let rendered = render_text(
        "knowledge.map.show",
        &serde_json::json!({
            "path": ".knowledge/knowledge-map.yaml",
            "map": {
                "topics": [{"id": "software-model"}],
                "sources": [{"id": "repository-software-model"}],
                "routes": [{"topic": "software-model"}],
                "history": {
                    "omitted_through": 0,
                    "complete": true,
                    "recent": [{"version": 1}]
                }
            }
        }),
    )
    .expect("v1 show should render");

    assert_eq!(
        rendered,
        "knowledge_map=.knowledge/knowledge-map.yaml topics=1 sources=1 routes=1 history_complete=true history_omitted_through=0 history_recent=1\n"
    );
    assert!(!rendered.contains("map history"));
}

#[test]
fn map_show_reports_a_recent_only_history_window() {
    let rendered = render_text(
        "knowledge.map.show",
        &serde_json::json!({
            "path": ".knowledge/knowledge-map.yaml",
            "map": {
                "topics": [{"id": "software-model"}],
                "sources": [{"id": "repository-software-model"}],
                "routes": [{"topic": "software-model"}],
                "history": {
                    "omitted_through": 24,
                    "complete": false,
                    "recent": [{"version": 25}, {"version": 26}]
                }
            }
        }),
    )
    .expect("v2 show should render");

    assert_eq!(
        rendered,
        "knowledge_map=.knowledge/knowledge-map.yaml topics=1 sources=1 routes=1 history_complete=false history_omitted_through=24 history_recent=2\nhistory_notice=entries through version 24 are not retained; run relay-knowledge map history without --from to read the retained window\n"
    );
}

#[test]
fn render_text_covers_operational_and_code_repository_summaries() {
    let cases = [
        (
            "knowledge.map.history",
            serde_json::json!({
                "path": ".knowledge/knowledge-map.yaml",
                "map_version": 9,
                "earliest_available_version": 5,
                "omitted_through": 4,
                "from_version": 5,
                "through_version": 6,
                "next_from_version": 7,
                "entries": [
                    {"version": 5, "action": "add\ninjected", "actor": "cli\rspoofed", "summary": "Added\r\nsource"},
                    {"version": 6, "action": "update", "actor": "cli", "summary": "Updated source"},
                ],
            }),
            "knowledge_map=.knowledge/knowledge-map.yaml map_version=9 earliest=5 omitted_through=4 from=5 through=6 next=7\nversion=5 action=add injected actor=cli spoofed summary=Added  source\nversion=6 action=update actor=cli summary=Updated source\n",
        ),
        (
            "files.content",
            serde_json::json!({
                "results": [
                    {
                        "path": "/docs/runbook.md",
                        "content_role": "user_source",
                    },
                    {
                        "path": "/docs/schema.sql",
                        "content_role": "user_source",
                    },
                ],
                "truncated": true,
                "duration_ms": 17,
            }),
            "results=2 truncated=true duration_ms=17\n",
        ),
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
            "code.repo.view",
            serde_json::json!({
                "request": {"view_kind": "dependency_tour"},
                "nodes": [{"id": "module:api"}],
                "edges": [{"id": "depends_on:module:api->module:domain"}],
                "sections": [{"id": "section:dependency_tour"}],
                "evidence": [{"id": "evidence:1"}],
                "metadata": {"stale": true},
                "freshness": {"scope_stale": true},
                "degraded_reason": null,
            }),
            "view=dependency_tour nodes=1 edges=1 sections=1 evidence=1 stale=true degraded=none\n",
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
            "code.repo.list",
            serde_json::json!({
                "repositories": [
                    {
                        "alias": "core",
                        "root_path": "/work/core",
                        "last_indexed_commit": "abc123",
                        "state": "fresh",
                        "indexed_file_count": 2,
                        "symbol_count": 3,
                        "stale": false,
                    },
                    {
                        "alias": "web",
                        "root_path": "/work/web",
                        "last_indexed_commit": "def456",
                        "state": "indexing",
                        "indexed_file_count": 5,
                        "symbol_count": 8,
                        "stale": true,
                    },
                ],
            }),
            "repositories=2\nrepo=core state=fresh files=2 symbols=3 stale=false commit=abc123 root=/work/core\nrepo=web state=indexing files=5 symbols=8 stale=true commit=def456 root=/work/web\n",
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
                "active_task": {
                    "state": "retrying",
                    "last_error_kind": "code_index",
                    "last_error_message": "publication fence expired while finalizing",
                },
                "checkpoint": {
                    "state": "finalizing:rebuild_calls",
                },
            }),
            "repo=repo files=2 symbols=3 stale=false task=retrying checkpoint=finalizing:rebuild_calls error_kind=code_index error=\"publication fence expired while finalizing\"\n",
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
                "build_targets": [{"target_id": "build_target:1"}],
                "iac_resources": [{"resource_id": "iac_resource:1"}],
                "design_elements": [{"element_id": "design_element:1"}],
            }),
            "software scope=scope-1 components=1 dependency_usages=1 sdk_usages=2 files=1 topics=1 relationships=1 build_targets=1 iac_resources=1 design_elements=1 stale=false\n",
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
