from __future__ import annotations

import json
from collections.abc import Mapping
from urllib import request

SAFE_METADATA_KEYS = (
    "active_suite",
    "expected_results",
    "completed_results",
    "model",
    "pi_version",
    "skill_version",
    "prompt_version",
    "condition_execution_mode",
)
SAFE_TOKEN_KEYS = (
    "input",
    "output",
    "reasoning",
    "cache_read",
    "cache_write",
    "total",
    "cost_usd",
    "requests",
)
SAFE_TOOL_KEYS = (
    "calls",
    "errors",
    "cumulative_seconds",
    "by_name",
    "relay_commands",
    "auto_retries",
    "harness_continuations",
)
SAFE_TIMING_KEYS = (
    "image_prepare_seconds",
    "container_start_seconds",
    "preindex_seconds",
    "agent_seconds",
    "scoring_seconds",
    "end_to_end_seconds",
)
SAFE_SWEBENCH_KEYS = (
    "completed",
    "resolved",
    "resolution_status",
    "patch_exists",
    "patch_applied",
    "fail_to_pass",
    "pass_to_pass",
)


def _select_mapping(value: object, keys: tuple[str, ...]) -> dict[str, object]:
    if not isinstance(value, Mapping):
        return {}
    return {key: value[key] for key in keys if key in value}


def sanitize_report(report: Mapping[str, object]) -> dict[str, object]:
    metadata = report.get("metadata", {})
    safe_metadata = (
        {key: metadata[key] for key in SAFE_METADATA_KEYS if key in metadata}
        if isinstance(metadata, Mapping)
        else {}
    )
    safe_results: list[dict[str, object]] = []
    results = report.get("results", [])
    if isinstance(results, list):
        for result in results:
            if not isinstance(result, Mapping):
                continue
            safe_result = _select_mapping(
                result,
                (
                    "instance_id",
                    "condition",
                    "attempt",
                    "infrastructure_retries",
                    "outcome",
                    "created_at",
                ),
            )
            safe_result["tokens"] = _select_mapping(
                result.get("tokens"), SAFE_TOKEN_KEYS
            )
            safe_result["tools"] = _select_mapping(result.get("tools"), SAFE_TOOL_KEYS)
            safe_result["timings"] = _select_mapping(
                result.get("timings"), SAFE_TIMING_KEYS
            )
            safe_result["swebench"] = _select_mapping(
                result.get("swebench"), SAFE_SWEBENCH_KEYS
            )
            safe_results.append(safe_result)
    return {
        "schema_version": report.get("schema_version", 1),
        "primary_time_excludes_preindex": report.get(
            "primary_time_excludes_preindex", True
        ),
        "generated_at": report.get("generated_at", ""),
        "metadata": safe_metadata,
        "conditions": report.get("conditions", {}),
        "paired": report.get("paired", {}),
        "results": safe_results,
    }


def upload_report(
    report: Mapping[str, object], site_url: str, ingest_token: str
) -> None:
    endpoint = f"{site_url.rstrip('/')}/api/report"
    payload = json.dumps(sanitize_report(report), ensure_ascii=False).encode("utf-8")
    upload_request = request.Request(
        endpoint,
        data=payload,
        headers={
            "Authorization": f"Bearer {ingest_token}",
            "Content-Type": "application/json; charset=utf-8",
        },
        method="POST",
    )
    with request.urlopen(upload_request, timeout=15) as response:
        if response.status != 200:
            raise RuntimeError(f"site sync returned HTTP {response.status}")
