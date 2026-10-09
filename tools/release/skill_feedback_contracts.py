"""Strict feedback DTO/schema and packaged-example release checks."""

from __future__ import annotations

import copy
import json
import tempfile
from pathlib import Path

from skill_schema_contracts import (
    SCHEMA_DRAFT,
    load_schema,
    require_schema_value,
    schema_object_nodes,
    schema_property,
    validate_schema_instance,
)


FEEDBACK_SCHEMA = Path("references/feedback.schema.json")
FEEDBACK_EXAMPLE = Path("references/feedback-report.example.json")


def check_feedback_schema(path: Path, example_path: Path) -> None:
    schema = load_schema(path)
    require_schema_value(path, schema.get("$schema"), SCHEMA_DRAFT, "draft")
    definitions = schema.get("$defs")
    if not isinstance(definitions, dict):
        raise ValueError(f"{path} is missing feedback definitions")
    for name in ("kind", "report", "policy", "fix", "validation", "observation", "evidence", "reproduction", "diagnostics"):
        if not isinstance(definitions.get(name), dict):
            raise ValueError(f"{path} is missing feedback definition {name}")
    require_schema_value(path, schema.get("oneOf"), [
        {"$ref": f"#/$defs/{name}"} for name in ("report", "policy", "fix", "validation")
    ], "input document alternatives")
    if any(node.get("additionalProperties") is not False for node in schema_object_nodes(schema)):
        raise ValueError(f"{path} must reject unknown feedback fields")
    for name in ("report", "policy"):
        require_schema_value(path, schema_property(definitions[name], "schema_version").get("const"), 1, f"{name} version")
    require_schema_value(path, definitions["kind"].get("enum"), [
        "bug", "missing-capability", "poor-result", "workflow-friction", "performance", "documentation"
    ], "feedback kinds")
    require_schema_value(path, definitions["validation"].get("required"), [
        "runner", "scenario_digest", "actual", "version", "environment", "run_id"
    ], "validation evidence requirements")
    for field in ("observations", "evidence"):
        require_schema_value(path, schema_property(definitions["report"], field).get("maxItems"), 16, f"{field} budget")
    require_schema_value(path, schema_property(definitions["policy"], "daily_quota").get("maximum"), 100, "quota maximum")
    example = load_schema(example_path)
    validate_schema_instance(schema, example)
    policy = {
        "schema_version": 1, "mode": "auto-submit", "target_repository": "owner/repository",
        "allowed_kinds": ["bug"], "daily_quota": 5, "validation_runner": "regression-ci",
    }
    validation = {
        "runner": "regression-ci", "scenario_digest": "a" * 64, "actual": "expected result",
        "version": "1.2.0", "environment": "isolated deterministic fixture", "run_id": "run-1",
    }
    for accepted in (policy, validation, {"reference": "owner/repository#1", "target_version": "1.2.0"}):
        validate_schema_instance(schema, accepted)
    invalid = [
        {**example, "target_repository": "attacker/repository"},
        {**example, "schema_version": 2},
        {**example, "observations": [{"origin": "proven", "text": "claim"}]},
        {**example, "evidence": [{"label": "raw", "content": "x", "public": True}]},
        {**policy, "target_repository": None},
        {**policy, "allowed_kinds": []},
        {**policy, "allowed_kinds": ["bug", "bug"]},
        {**policy, "daily_quota": 101},
        {**validation, "passed": True},
        {key: value for key, value in validation.items() if key != "runner"},
    ]
    for rejected in invalid:
        try:
            validate_schema_instance(schema, rejected)
        except ValueError:
            continue
        raise ValueError(f"{path} accepts invalid feedback input")


def self_test_feedback_schema(skill_root: Path) -> None:
    path = skill_root / FEEDBACK_SCHEMA
    example = skill_root / FEEDBACK_EXAMPLE
    check_feedback_schema(path, example)
    with tempfile.TemporaryDirectory(prefix="relay-feedback-schema-") as temporary:
        drift = copy.deepcopy(load_schema(path))
        drift["$defs"]["report"]["additionalProperties"] = True
        drift_path = Path(temporary) / "drift.json"
        drift_path.write_text(json.dumps(drift), encoding="utf-8")
        try:
            check_feedback_schema(drift_path, example)
        except ValueError as error:
            if "reject unknown feedback fields" not in str(error):
                raise
        else:
            raise AssertionError("feedback schema privacy contract drift was accepted")
