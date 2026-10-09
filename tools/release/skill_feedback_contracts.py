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
    byte_limits = {
        "report": {"intent": 4096, "expected": 4096, "actual": 4096, "impact": 4096,
                   "trace_id": 256, "request_id": 256},
        "observation": {"text": 4096},
        "evidence": {"label": 256, "content": 16384},
        "reproduction": {"scenario": 4096, "expected": 4096},
        "policy": {"validation_runner": 128},
        "fix": {"reference": 1024, "target_version": 128},
        "validation": {"runner": 128, "actual": 4096, "version": 128,
                       "environment": 1024, "run_id": 256},
    }
    for definition, fields in byte_limits.items():
        for field, limit in fields.items():
            node = schema_property(definitions[definition], field)
            if "anyOf" in node:
                node = node["anyOf"][0]
            require_schema_value(path, node.get("x-maxUtf8Bytes"), limit, f"{definition}.{field} UTF-8 budget")
            # Exercise every byte bound with ASCII, CJK, astral and mixed-width text.
            for character in ("a", "界", "😀"):
                size = len(character.encode("utf-8"))
                boundary = character * (limit // size) + "a" * (limit % size)
                validate_schema_instance(node, boundary)
                try:
                    validate_schema_instance(node, boundary + "a")
                except ValueError:
                    pass
                else:
                    raise ValueError(f"{path} accepts oversized UTF-8 {definition}.{field}")
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
    # Match the Rust DTO's exact component boundary; three dots are legal.
    for target in (
        ".../repo", "owner/...", ".../...", "owner.name/.repo", "owner../repo..",
        f"{'a' * 100}/{'b' * 100}",
    ):
        validate_schema_instance(schema, {**policy, "target_repository": target})
    invalid = [
        {**example, "target_repository": "attacker/repository"},
        {**example, "schema_version": 2},
        {**example, "intent": "界" * 2000},
        {**example, "observations": [{"origin": "proven", "text": "claim"}]},
        {**example, "evidence": [{"label": "raw", "content": "x", "public": True}]},
        {**policy, "target_repository": None},
        {**policy, "allowed_kinds": []},
        {**policy, "allowed_kinds": ["bug", "bug"]},
        {**policy, "daily_quota": 101},
        {**validation, "passed": True},
        {key: value for key, value in validation.items() if key != "runner"},
    ]
    invalid.extend({**policy, "target_repository": target} for target in (
        "./repo", "../repo", "owner/.", "owner/..", "./.", "../..", ".../..", "../...",
    ))
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
        source = load_schema(path)
        privacy_drift = copy.deepcopy(source)
        privacy_drift["$defs"]["report"]["additionalProperties"] = True
        missing_bytes = copy.deepcopy(source)
        del missing_bytes["$defs"]["report"]["properties"]["intent"]["x-maxUtf8Bytes"]
        missing_guard = copy.deepcopy(source)
        del missing_guard["$defs"]["policy"]["properties"]["target_repository"]["anyOf"][0]["not"]
        overbroad_guard = copy.deepcopy(source)
        overbroad_guard["$defs"]["policy"]["properties"]["target_repository"]["anyOf"][0]["not"] = {
            "pattern": r"(^|/)\.+(/|$)",
        }
        for name, drift, expected in (
            ("privacy", privacy_drift, "reject unknown feedback fields"),
            ("missing-utf8-budget", missing_bytes, "UTF-8 budget"),
            ("missing-dot-guard", missing_guard, "accepts invalid feedback input"),
            ("overbroad-dot-guard", overbroad_guard, "oneOf"),
        ):
            drift_path = Path(temporary) / f"{name}.json"
            drift_path.write_text(json.dumps(drift), encoding="utf-8")
            try:
                check_feedback_schema(drift_path, example)
            except ValueError as error:
                if expected not in str(error):
                    raise
            else:
                raise AssertionError(f"feedback schema {name} contract drift was accepted")
