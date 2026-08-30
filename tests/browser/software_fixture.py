from __future__ import annotations


def software_response(kind: str) -> dict:
    entities = software_entities()
    statements = software_statements()
    conflict_statements = [
        statement for statement in statements if statement["fact_state"] == "conflicting"
    ]
    diagnostics = [
        {
            "diagnostic_id": "diagnostic-invalid-range",
            "shape_id": "software:RelationShape",
            "code": "invalid_range",
            "severity": "error",
            "statement_id": "statement-rejected",
            "field": "object_id",
            "message": "predicate range does not allow the object entity kind",
        }
    ]
    is_conflicts = kind == "conflicts"
    return {
        "metadata": {
            "trace_id": f"trace-software-{kind}",
            "request_id": f"req-software-{kind}",
            "graph_version": 7,
            "indexed_graph_version": 7,
            "stale": False,
        },
        "scope": {
            "scope_id": "git_snapshot:software-fixture",
            "repository_id": "repo-relay",
            "alias": "relay",
            "requested_ref": "abc123def456",
            "resolved_commit_sha": "abc123def456",
            "tree_hash": "tree-software",
            "path_filters": [],
            "language_filters": [],
            "indexed_file_count": 12,
            "index_versions": ["code:git_snapshot:software-fixture:tree-software"],
            "stale": False,
        },
        "request": {"kind": kind, "limit": 180},
        "status": {
            "repository_id": "repo-relay",
            "source_scope": "git_snapshot:software-fixture",
            "projected_graph_version": 7,
            "stale": False,
            "ontology_version": "1.0.0",
            "projection_schema_version": 6,
            "source_coverage": {
                "source_kinds": ["manifest", "lockfile", "build_file", "iac", "test"],
                "source_path_count": 8,
                "evidence_ref_count": len(statements),
            },
            "completeness_basis_points": 10000,
            "freshness": "fresh",
            "conflict_count": 1,
            "entity_count": len(entities),
            "statement_count": len(statements),
            "diagnostic_count": len(diagnostics),
            "component_count": 2,
            "sdk_usage_count": 0,
            "file_count": 12,
            "topic_count": 1,
            "relationship_count": len(statements),
            "build_target_count": 1,
            "iac_resource_count": 1,
            "design_element_count": 2,
        },
        "entities": [] if is_conflicts else entities,
        "statements": conflict_statements if is_conflicts else statements,
        "diagnostics": diagnostics if is_conflicts else [],
    }


def software_entities() -> list[dict]:
    return [
        software_entity(
            "software_entity:snapshot",
            "repository_snapshot",
            "software fixture",
            "Cargo.toml",
            "code",
        ),
        software_entity(
            "software_entity:system",
            "software_system",
            "Checkout system",
            "README.md",
            "documentation",
        ),
        software_entity(
            "software_entity:component",
            "component",
            "Checkout core",
            "src/lib.rs",
            "code",
        ),
        software_entity(
            "software_entity:api", "api", "CheckoutApi", "src/lib.rs", "code"
        ),
        software_entity(
            "software_entity:build",
            "build_definition",
            "container",
            "Dockerfile",
            "build_file",
        ),
        software_entity(
            "software_entity:artifact",
            "release_artifact",
            "checkout:latest",
            "Dockerfile",
            "build_file",
        ),
        software_entity(
            "software_entity:deployment",
            "deployment_unit",
            "deploy/app.yaml",
            "deploy/app.yaml",
            "iac",
        ),
        software_entity(
            "software_entity:service",
            "runtime_service",
            "checkout.service",
            "deploy/checkout.service",
            "service_definition",
        ),
        software_entity(
            "software_entity:test",
            "test_case",
            "checkout_smoke_test",
            "tests/smoke.rs",
            "test",
        ),
        software_entity(
            "software_entity:database-v1",
            "package_component",
            "database v1",
            "Cargo.toml",
            "manifest",
        ),
        software_entity(
            "software_entity:database-v2",
            "package_component",
            "database v2",
            "Cargo.lock",
            "lockfile",
        ),
    ]


def software_entity(
    entity_key: str, entity_kind: str, name: str, path: str, source_kind: str
) -> dict:
    evidence_refs = [] if entity_kind == "repository_snapshot" else [software_evidence(path)]
    return {
        "entity_key": entity_key,
        "occurrence_id": f"occurrence:{entity_key}",
        "repository_id": "repo-relay",
        "source_scope": "git_snapshot:software-fixture",
        "entity_kind": entity_kind,
        "name": name,
        "source_kind": source_kind,
        "evidence_refs": evidence_refs,
        "attributes": {},
        "created_graph_version": 7,
    }


def software_statements() -> list[dict]:
    return [
        software_statement(
            "statement-system",
            "software_entity:snapshot",
            "contains",
            "software_entity:system",
            "README.md",
            "documentation",
        ),
        software_statement(
            "statement-component",
            "software_entity:system",
            "contains",
            "software_entity:component",
            "README.md",
            "documentation",
        ),
        software_statement(
            "statement-api",
            "software_entity:component",
            "provides_api",
            "software_entity:api",
            "src/lib.rs",
            "code",
        ),
        software_statement(
            "statement-build",
            "software_entity:build",
            "builds",
            "software_entity:artifact",
            "Dockerfile",
            "build_file",
        ),
        software_statement(
            "statement-deploy",
            "software_entity:deployment",
            "deploys",
            "software_entity:artifact",
            "deploy/app.yaml",
            "iac",
        ),
        software_statement(
            "statement-service",
            "software_entity:deployment",
            "runs_as",
            "software_entity:service",
            "deploy/checkout.service",
            "service_definition",
        ),
        software_statement(
            "statement-test",
            "software_entity:test",
            "tests",
            "software_entity:api",
            "tests/smoke.rs",
            "test",
        ),
        software_statement(
            "statement-conflict-v1",
            "software_entity:component",
            "depends_on",
            "software_entity:database-v1",
            "Cargo.toml",
            "manifest",
            conflicting=True,
        ),
        software_statement(
            "statement-conflict-v2",
            "software_entity:component",
            "depends_on",
            "software_entity:database-v2",
            "Cargo.lock",
            "lockfile",
            conflicting=True,
        ),
    ]


def software_statement(
    statement_id: str,
    subject_id: str,
    predicate: str,
    object_id: str,
    path: str,
    source_kind: str,
    *,
    conflicting: bool = False,
) -> dict:
    return {
        "statement_id": statement_id,
        "subject_id": subject_id,
        "predicate": predicate,
        "object_id": object_id,
        "source_scope": "git_snapshot:software-fixture",
        "source_kind": source_kind,
        "evidence_refs": [software_evidence(path)],
        "assertion_mode": "verified" if source_kind == "lockfile" else "declared",
        "resolution_state": "conflicting" if conflicting else "resolved",
        "extractor_id": "relay-knowledge/software-ontology",
        "extractor_version": "1.0.0",
        "confidence_basis_points": 9500,
        "fact_state": "conflicting" if conflicting else "active",
    }


def software_evidence(path: str) -> dict:
    return {
        "evidence_id": f"evidence:{path}",
        "source_scope": "git_snapshot:software-fixture",
        "path": path,
        "line_range": {"start": 1, "end": 1},
    }


CODE_REPOSITORY_LIST_RESPONSE = {
    "metadata": {
        "trace_id": "trace-repositories",
        "request_id": "req-repositories",
        "graph_version": 7,
        "indexed_graph_version": 7,
        "stale": False,
    },
    "repositories": [
        {
            "repository_id": "repo-relay",
            "alias": "relay",
            "root_path": "/srv/relay/repository",
            "path_filters": [],
            "language_filters": [],
            "last_indexed_scope_id": "git_snapshot:software-fixture",
            "last_indexed_commit": "abc123def456",
            "tree_hash": "tree-software",
            "state": "ready",
            "indexed_file_count": 12,
            "symbol_count": 18,
            "reference_count": 24,
            "chunk_count": 30,
            "stale": False,
        }
    ],
}
