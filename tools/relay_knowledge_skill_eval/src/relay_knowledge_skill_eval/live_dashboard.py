from __future__ import annotations

import argparse
import json
import os
import time
import urllib.error
from pathlib import Path

from relay_knowledge_skill_eval.checkpoint import CheckpointStore
from relay_knowledge_skill_eval.models import EvalResult, RunOutcome
from relay_knowledge_skill_eval.reporting import write_reports
from relay_knowledge_skill_eval.site_sync import upload_report


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("--output-dir", type=Path, required=True)
    parser.add_argument("--expected-results", type=int, required=True)
    parser.add_argument("--interval", type=float, default=5.0)
    parser.add_argument("--site-url", default=os.environ.get("EVAL_SITE_URL", ""))
    arguments = parser.parse_args()
    checkpoint = CheckpointStore(arguments.output_dir)
    last_signature: tuple[int, int] | None = None
    while True:
        path = checkpoint.results_path
        signature = (
            path.stat().st_mtime_ns if path.exists() else 0,
            path.stat().st_size if path.exists() else 0,
        )
        metadata_path = arguments.output_dir / "checkpoint.meta.json"
        if signature != last_signature and metadata_path.exists():
            results = list(checkpoint.load_results().values())
            metadata = checkpoint.load_meta().model_dump(mode="json")
            active_suite = _load_active_suite(arguments.output_dir)
            metadata.update(
                {
                    "expected_results": arguments.expected_results,
                    "completed_results": _completed_result_count(results),
                }
            )
            if active_suite:
                metadata["active_suite"] = active_suite
            report = write_reports(results, arguments.output_dir, metadata=metadata)
            site_current = _upload_site_snapshot(report, arguments.site_url)
            if site_current:
                last_signature = signature
            if _results_are_final(results, arguments.expected_results) and site_current:
                return
        time.sleep(arguments.interval)


def _results_are_final(results: list[EvalResult], expected_results: int) -> bool:
    return _completed_result_count(results) >= expected_results


def _completed_result_count(results: list[EvalResult]) -> int:
    """Count final rows while leaving retryable infrastructure rows pending."""
    return sum(result.outcome is not RunOutcome.INFRA_ERROR for result in results)


def _upload_site_snapshot(report: dict[str, object], site_url: str) -> bool:
    ingest_token = os.environ.get("EVAL_SITE_INGEST_TOKEN", "")
    if not site_url or not ingest_token:
        return True
    try:
        upload_report(report, site_url, ingest_token)
    except (OSError, RuntimeError, urllib.error.URLError) as error:
        print(f"Site sync failed: {error}", flush=True)
        return False
    return True


def _load_active_suite(output_dir: Path) -> str:
    try:
        report = json.loads((output_dir / "report.json").read_text(encoding="utf-8"))
    except (OSError, json.JSONDecodeError):
        return ""
    metadata = report.get("metadata", {})
    if not isinstance(metadata, dict):
        return ""
    active_suite = metadata.get("active_suite", "")
    return active_suite if isinstance(active_suite, str) else ""


if __name__ == "__main__":
    main()
