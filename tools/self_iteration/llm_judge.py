"""LLM or coding-agent judge support for research-style self-iteration review."""

from __future__ import annotations

import json
import shlex
import subprocess
import time
import urllib.error
import urllib.parse
import urllib.request
from dataclasses import dataclass
from pathlib import Path
from typing import Any

from scoring import CaseObservation, GateObservation


DEFAULT_DIMENSIONS = [
    "research_alignment",
    "architecture_soundness",
    "reliability_resilience",
    "performance_generalization",
    "implementation_actionability",
    "anti_fixture_special_casing",
]
DEFAULT_CLI_JUDGE_COMMAND = (
    'opencode run --file {prompt_file} '
    '"Read the attached relay-knowledge judge prompt and return only the strict JSON object it requests."'
)
DISABLED_BACKENDS = {"none", "off", "disabled", "skip", "false"}


@dataclass(frozen=True)
class JudgeSettings:
    backend: str
    enabled: bool
    missing: list[str]
    timeout_seconds: int
    http_base_url: str = ""
    http_api_key: str = ""
    http_model: str = ""
    cli_command: str = ""

    @property
    def configured(self) -> bool:
        return self.enabled and not self.missing


@dataclass(frozen=True)
class RawJudgeResponse:
    exit_code: int
    duration_ms: int
    text: str
    message: str

    @property
    def passed(self) -> bool:
        return self.exit_code == 0


def evaluate_research_judge_suite(
    *,
    workspace: Path,
    run_home: Path,
    env: dict[str, str],
    suite_config: dict[str, Any],
    generated_diff: bool,
    candidate_diff: str,
    gates: list[GateObservation],
    cases: list[CaseObservation],
    metrics: list[Any],
    repo_reports: list[dict[str, Any]],
) -> dict[str, Any]:
    settings = judge_settings_from_env(env)
    report: dict[str, Any] = {
        "repository": "research_judge",
        "commands": [],
        "cases": [],
        "metrics": [],
        "scope": "judge",
        "index_summary": judge_settings_summary(settings),
    }

    if not settings.enabled:
        gate = GateObservation(
            name="research_judge",
            passed=True,
            message="judge skipped: backend disabled",
        )
        report["gates"] = [gate]
        report["index_summary"]["status"] = "skipped"
        return report

    if settings.missing:
        gate = GateObservation(
            name="research_judge",
            passed=False,
            message="judge misconfigured: missing " + ", ".join(settings.missing),
        )
        report["gates"] = [gate]
        report["index_summary"]["status"] = "misconfigured"
        return report

    prompt = build_judge_prompt(
        workspace=workspace,
        suite_config=suite_config,
        generated_diff=generated_diff,
        candidate_diff=candidate_diff,
        gates=gates,
        cases=cases,
        metrics=metrics,
        repo_reports=repo_reports,
    )
    raw = run_judge(settings, workspace, run_home, env, prompt)
    report["index_summary"].update(
        {
            "status": "completed" if raw.passed else "failed",
            "duration_ms": raw.duration_ms,
            "message": raw.message,
        }
    )

    if not raw.passed:
        gate = GateObservation(
            name="research_judge",
            passed=False,
            duration_ms=raw.duration_ms,
            message=raw.message,
        )
        report["gates"] = [gate]
        return report

    outcome = judge_outcome(raw.text, suite_config)
    gate = GateObservation(
        name="research_judge",
        passed=outcome["gate_passed"],
        duration_ms=raw.duration_ms,
        message=outcome["message"],
    )
    case = CaseObservation(
        case_id="research_judge",
        repository="research_judge",
        passed=outcome["case_passed"],
        rank=1 if outcome["case_passed"] else None,
        max_rank=1,
        message=outcome["message"],
        objective="research_judge",
        score_override=outcome["score"],
    )
    report["gates"] = [gate]
    report["cases"] = [case]
    report["index_summary"].update(outcome["summary"])
    return report


def judge_settings_from_env(env: dict[str, str]) -> JudgeSettings:
    backend = normalized(env.get("RELAY_KNOWLEDGE_JUDGE_BACKEND", ""))
    timeout_seconds = parse_timeout_seconds(
        env.get("RELAY_KNOWLEDGE_JUDGE_TIMEOUT_SECONDS", "120")
    )
    cli_command = first_env(
        env,
        "RELAY_KNOWLEDGE_JUDGE_COMMAND",
        "RELAY_KNOWLEDGE_JUDGE_AGENT_COMMAND",
        "RELAY_KNOWLEDGE_JUDGE_CLI_COMMAND",
    )
    http_base_url = env.get("RELAY_KNOWLEDGE_JUDGE_BASE_URL", "").strip()
    http_api_key = env.get("RELAY_KNOWLEDGE_JUDGE_API_KEY", "").strip()
    http_model = env.get("RELAY_KNOWLEDGE_JUDGE_MODEL", "").strip()
    http_present = any([http_base_url, http_api_key, http_model])

    if backend in DISABLED_BACKENDS:
        return JudgeSettings(
            backend="none",
            enabled=False,
            missing=[],
            timeout_seconds=timeout_seconds,
        )
    if backend in {"agent", "coding_agent", "cli_agent", "opencode", "open_code"}:
        backend = "cli"
    if not backend:
        if cli_command:
            backend = "cli"
        elif http_present:
            backend = "http"
        else:
            backend = "cli"
            cli_command = DEFAULT_CLI_JUDGE_COMMAND

    if backend == "cli":
        if not cli_command:
            cli_command = DEFAULT_CLI_JUDGE_COMMAND
        missing = [] if cli_command else ["RELAY_KNOWLEDGE_JUDGE_COMMAND"]
        return JudgeSettings(
            backend=backend,
            enabled=True,
            missing=missing,
            timeout_seconds=timeout_seconds,
            cli_command=cli_command,
        )
    if backend == "http":
        missing = [
            name
            for name, value in (
                ("RELAY_KNOWLEDGE_JUDGE_BASE_URL", http_base_url),
                ("RELAY_KNOWLEDGE_JUDGE_API_KEY", http_api_key),
                ("RELAY_KNOWLEDGE_JUDGE_MODEL", http_model),
            )
            if not value
        ]
        return JudgeSettings(
            backend=backend,
            enabled=True,
            missing=missing,
            timeout_seconds=timeout_seconds,
            http_base_url=http_base_url,
            http_api_key=http_api_key,
            http_model=http_model,
        )
    return JudgeSettings(
        backend=backend,
        enabled=True,
        missing=[f"unsupported RELAY_KNOWLEDGE_JUDGE_BACKEND={backend}"],
        timeout_seconds=timeout_seconds,
    )


def first_env(env: dict[str, str], *names: str) -> str:
    for name in names:
        value = env.get(name, "").strip()
        if value:
            return value
    return ""


def normalized(value: str) -> str:
    return value.strip().lower().replace("-", "_")


def parse_timeout_seconds(value: str) -> int:
    try:
        parsed = int(value or "120")
    except ValueError:
        return 120
    return max(1, parsed)


def judge_settings_summary(settings: JudgeSettings) -> dict[str, Any]:
    return {
        "backend": settings.backend,
        "enabled": settings.enabled,
        "configured": settings.configured,
        "missing": settings.missing,
        "timeout_seconds": settings.timeout_seconds,
        "http_base_url_configured": bool(settings.http_base_url),
        "http_api_key_configured": bool(settings.http_api_key),
        "http_model_configured": bool(settings.http_model),
        "cli_command_configured": bool(settings.cli_command),
        "cli_command_name": command_name(settings.cli_command),
    }


def command_name(command: str) -> str:
    if not command:
        return ""
    try:
        parts = shlex.split(command)
    except ValueError:
        return ""
    return Path(parts[0]).name if parts else ""


def build_judge_prompt(
    *,
    workspace: Path,
    suite_config: dict[str, Any],
    generated_diff: bool,
    candidate_diff: str,
    gates: list[GateObservation],
    cases: list[CaseObservation],
    metrics: list[Any],
    repo_reports: list[dict[str, Any]],
) -> str:
    dimensions = suite_config.get("rubric_dimensions", DEFAULT_DIMENSIONS)
    max_doc_chars = int(suite_config.get("max_doc_chars", 4000))
    max_diff_chars = int(suite_config.get("max_diff_chars", 30000))
    docs = document_excerpts(workspace, suite_config.get("documents", []), max_doc_chars)
    diff_text = candidate_diff.strip() or "(no candidate diff captured)"
    if len(diff_text) > max_diff_chars:
        diff_text = diff_text[:max_diff_chars] + "\n...diff truncated..."
    return f"""You are the relay-knowledge research judge.

Evaluate whether this candidate improves relay-knowledge according to the
project capability docs, architecture specs, and research notes. Prefer
general mechanisms over benchmark fixture special-casing.

Return only one strict JSON object with this schema:
{{
  "passed": true,
  "confidence": 0.0,
  "overall_score": 0.0,
  "scores": {{
    {", ".join(json.dumps(str(name)) + ": 0.0" for name in dimensions)}
  }},
  "summary": "short verdict",
  "evidence": ["specific evidence"],
  "risks": ["specific risk"],
  "recommended_cases": ["next deterministic or judge case to add"]
}}

All scores and confidence must be numbers from 0.0 to 1.0.

Rubric:
- research_alignment: matches 02 capabilities, 03 architecture contracts, and 04 research direction.
- architecture_soundness: preserves env/paths/net boundaries, async/resource boundaries, acyclic design, and documentation expectations.
- reliability_resilience: improves or protects freshness, recovery, backpressure, cancellation, degraded states, and diagnostics.
- performance_generalization: uses candidate narrowing, indexing, batching, query planning, or resource budgets rather than local hacks.
- implementation_actionability: change is concrete, maintainable, tested, and scoped.
- anti_fixture_special_casing: no known query/path/symbol/repository enumeration unless it is a documented product contract.

Deterministic evaluation summary:
{deterministic_summary(gates, cases, metrics, repo_reports, generated_diff)}

Candidate diff:
```diff
{diff_text}
```

Reference document excerpts:
{docs}
"""


def document_excerpts(workspace: Path, documents: Any, max_doc_chars: int) -> str:
    if not isinstance(documents, list) or not documents:
        documents = [
            "docs/zh/02-capabilities/15-evaluation-and-quality-gates.md",
            "docs/zh/03-architecture-specs/02-engineering-hard-constraints.md",
            "docs/zh/04-research/08-competitive-performance-research-2026.md",
        ]
    chunks: list[str] = []
    for item in documents:
        relative = str(item)
        path = workspace / relative
        try:
            text = path.read_text(encoding="utf-8")
        except OSError:
            text = "(missing)"
        chunks.append(f"## {relative}\n{text[:max_doc_chars]}")
    return "\n\n".join(chunks)


def deterministic_summary(
    gates: list[GateObservation],
    cases: list[CaseObservation],
    metrics: list[Any],
    repo_reports: list[dict[str, Any]],
    generated_diff: bool,
) -> str:
    failed_gates = [gate.name for gate in gates if not gate.passed]
    failed_cases = [case.case_id for case in cases if not case.passed][:16]
    metric_lines = [
        f"{getattr(metric, 'name', '')}={getattr(metric, 'value', '')}"
        for metric in metrics[:16]
    ]
    repo_names = [str(report.get("repository", "")) for report in repo_reports[:12]]
    return json.dumps(
        {
            "generated_diff": generated_diff,
            "gate_count": len(gates),
            "failed_gates": failed_gates,
            "case_count": len(cases),
            "failed_cases": failed_cases,
            "metrics": metric_lines,
            "report_sections": repo_names,
        },
        ensure_ascii=False,
        sort_keys=True,
    )


def run_judge(
    settings: JudgeSettings,
    workspace: Path,
    run_home: Path,
    env: dict[str, str],
    prompt: str,
) -> RawJudgeResponse:
    if settings.backend == "cli":
        return run_cli_judge(settings, workspace, run_home, env, prompt)
    return run_http_judge(settings, prompt)


def run_cli_judge(
    settings: JudgeSettings,
    workspace: Path,
    run_home: Path,
    env: dict[str, str],
    prompt: str,
) -> RawJudgeResponse:
    started = time.monotonic()
    prompt_file = run_home / "judge-prompt.txt"
    prompt_file.parent.mkdir(parents=True, exist_ok=True)
    prompt_file.write_text(prompt, encoding="utf-8")
    try:
        command, stdin = cli_command(settings.cli_command, workspace, prompt_file, prompt)
        completed = subprocess.run(
            command,
            cwd=workspace,
            env=env,
            input=stdin,
            text=True,
            capture_output=True,
            timeout=settings.timeout_seconds,
            check=False,
        )
        duration_ms = int((time.monotonic() - started) * 1000)
        output = "\n".join(part for part in [completed.stdout, completed.stderr] if part)
        return RawJudgeResponse(
            exit_code=completed.returncode,
            duration_ms=duration_ms,
            text=output,
            message=last_output_line(completed.stdout, completed.stderr),
        )
    except (OSError, ValueError, subprocess.TimeoutExpired) as error:
        return RawJudgeResponse(
            exit_code=124 if isinstance(error, subprocess.TimeoutExpired) else 1,
            duration_ms=int((time.monotonic() - started) * 1000),
            text="",
            message=str(error),
        )


def cli_command(
    template: str,
    workspace: Path,
    prompt_file: Path,
    prompt: str,
) -> tuple[list[str], str | None]:
    parts = shlex.split(template)
    if not parts:
        raise ValueError("empty judge CLI command")
    stdin_prompt = prompt
    command: list[str] = []
    used_inline_prompt = False
    used_prompt_file = False
    for part in parts:
        if "{prompt}" in part:
            used_inline_prompt = True
            part = part.replace("{prompt}", prompt)
        if "{prompt_file}" in part:
            used_prompt_file = True
            part = part.replace("{prompt_file}", str(prompt_file))
        part = part.replace("{workspace}", str(workspace))
        command.append(part)
    if used_inline_prompt or used_prompt_file:
        stdin_prompt = None
    return command, stdin_prompt


def run_http_judge(settings: JudgeSettings, prompt: str) -> RawJudgeResponse:
    started = time.monotonic()
    payload = {
        "model": settings.http_model,
        "messages": [
            {
                "role": "system",
                "content": "Return only strict JSON. Do not include markdown.",
            },
            {"role": "user", "content": prompt},
        ],
        "temperature": 0,
    }
    request = urllib.request.Request(
        normalize_chat_completions_url(settings.http_base_url),
        data=json.dumps(payload).encode("utf-8"),
        headers={
            "Authorization": f"Bearer {settings.http_api_key}",
            "Content-Type": "application/json",
        },
        method="POST",
    )
    try:
        with urllib.request.urlopen(request, timeout=settings.timeout_seconds) as response:
            body = response.read().decode("utf-8", errors="replace")
        duration_ms = int((time.monotonic() - started) * 1000)
        text = extract_http_content(body)
        return RawJudgeResponse(0, duration_ms, text, "judge completed")
    except (
        urllib.error.URLError,
        TimeoutError,
        OSError,
        ValueError,
        json.JSONDecodeError,
    ) as error:
        return RawJudgeResponse(
            exit_code=1,
            duration_ms=int((time.monotonic() - started) * 1000),
            text="",
            message=str(error),
        )


def normalize_chat_completions_url(base_url: str) -> str:
    cleaned = base_url.strip().rstrip("/")
    if cleaned.endswith("/chat/completions"):
        return cleaned
    path = urllib.parse.urlparse(cleaned).path.rstrip("/")
    last = path.rsplit("/", 1)[-1]
    if last.startswith("v") and last[1:].isdigit():
        return cleaned + "/chat/completions"
    return cleaned + "/v1/chat/completions"


def extract_http_content(body: str) -> str:
    payload = json.loads(body)
    choices = payload.get("choices")
    if isinstance(choices, list) and choices:
        first = choices[0]
        if isinstance(first, dict):
            message = first.get("message")
            if isinstance(message, dict) and isinstance(message.get("content"), str):
                return message["content"]
            if isinstance(first.get("text"), str):
                return first["text"]
    if isinstance(payload.get("output_text"), str):
        return payload["output_text"]
    return body


def judge_outcome(text: str, suite_config: dict[str, Any]) -> dict[str, Any]:
    try:
        payload = parse_json_object(text)
        dimensions = suite_config.get("rubric_dimensions", DEFAULT_DIMENSIONS)
        scores = normalized_dimension_scores(payload, dimensions)
        confidence = normalized_score(payload["confidence"])
        score = normalized_score(payload.get("overall_score", average(list(scores.values()))))
    except (KeyError, TypeError, ValueError, json.JSONDecodeError) as error:
        return {
            "gate_passed": False,
            "case_passed": False,
            "score": 0.0,
            "message": f"judge returned invalid JSON: {error}",
            "summary": {"status": "invalid", "raw_tail": text[-1000:]},
        }

    min_score = float(suite_config.get("min_score", 0.75))
    min_confidence = float(suite_config.get("min_confidence", 0.60))
    anti_threshold = float(suite_config.get("min_anti_fixture_special_casing", 0.75))
    anti_score = scores.get("anti_fixture_special_casing", 0.0)
    judge_passed = bool(payload.get("passed", False))
    passed = (
        judge_passed
        and score >= min_score
        and confidence >= min_confidence
        and anti_score >= anti_threshold
    )
    summary_text = str(payload.get("summary", "")).strip()
    reasons = []
    if not judge_passed:
        reasons.append("judge marked candidate failed")
    if score < min_score:
        reasons.append(f"score {score:.3f} below {min_score:.3f}")
    if confidence < min_confidence:
        reasons.append(f"confidence {confidence:.3f} below {min_confidence:.3f}")
    if anti_score < anti_threshold:
        reasons.append(
            f"anti_fixture_special_casing {anti_score:.3f} below {anti_threshold:.3f}"
        )
    message = summary_text or "judge completed"
    if reasons:
        message = message + " | " + "; ".join(reasons)
    return {
        "gate_passed": passed,
        "case_passed": passed,
        "score": score,
        "message": message[:400],
        "summary": {
            "status": "passed" if passed else "failed",
            "score": score,
            "confidence": confidence,
            "scores": scores,
            "summary": summary_text,
            "evidence": string_list(payload.get("evidence", []), 8),
            "risks": string_list(payload.get("risks", []), 8),
            "recommended_cases": string_list(payload.get("recommended_cases", []), 8),
        },
    }


def parse_json_object(text: str) -> dict[str, Any]:
    stripped = text.strip()
    if stripped.startswith("```"):
        stripped = stripped.strip("`")
        if stripped.lower().startswith("json"):
            stripped = stripped[4:].strip()
    try:
        payload = json.loads(stripped)
        if isinstance(payload, dict):
            return payload
    except json.JSONDecodeError:
        # Intentionally ignore the direct parse failure and try extraction fallbacks below.
        pass
    for line in reversed(stripped.splitlines()):
        line = line.strip()
        if not line:
            continue
        try:
            payload = json.loads(line)
            if isinstance(payload, dict):
                return payload
        except json.JSONDecodeError:
            continue
    start = stripped.find("{")
    end = stripped.rfind("}")
    if start >= 0 and end > start:
        payload = json.loads(stripped[start : end + 1])
        if isinstance(payload, dict):
            return payload
    raise json.JSONDecodeError("no JSON object found", stripped, 0)


def normalized_dimension_scores(
    payload: dict[str, Any],
    dimensions: list[str],
) -> dict[str, float]:
    raw = payload.get("scores")
    if not isinstance(raw, dict):
        raise ValueError("missing scores object")
    scores: dict[str, float] = {}
    for dimension in dimensions:
        if dimension not in raw:
            raise ValueError(f"missing score dimension {dimension}")
        scores[str(dimension)] = normalized_score(raw[dimension])
    return scores


def normalized_score(value: Any) -> float:
    number = float(value)
    if number > 10.0 and number <= 100.0:
        number = number / 100.0
    elif number > 1.0 and number <= 10.0:
        number = number / 10.0
    if not 0.0 <= number <= 1.0:
        raise ValueError(f"score out of range: {value}")
    return number


def average(values: list[float]) -> float:
    if not values:
        return 0.0
    return sum(values) / len(values)


def string_list(value: Any, limit: int) -> list[str]:
    if not isinstance(value, list):
        return []
    return [str(item)[:400] for item in value[:limit]]


def last_output_line(stdout: str, stderr: str) -> str:
    for output in (stderr, stdout):
        lines = [line.strip() for line in output.splitlines() if line.strip()]
        if lines:
            return lines[-1][:400]
    return ""
