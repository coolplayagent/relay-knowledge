from __future__ import annotations

import json
import time
from collections.abc import Mapping
from dataclasses import dataclass
from pathlib import Path

from relay_knowledge_skill_eval.docker_runtime import (
    RELAY_KNOWLEDGE_ENVIRONMENT,
    DockerRuntime,
)
from relay_knowledge_skill_eval.security import SecretRedactor

_BINARY = "/opt/pi-eval/skill/assets/linux-x86_64/relay-knowledge"
_ENVIRONMENT = tuple(
    f"{variable}={value}" for variable, value in RELAY_KNOWLEDGE_ENVIRONMENT
)


@dataclass(frozen=True)
class IndexProgress:
    """Normalized durable index-task state shared by both benchmark runners."""

    state: str
    task_id: str
    indexed_scope_present: bool
    next_retry_at_ms: int = 0


def parse_index_progress(status_output: str) -> IndexProgress:
    status = _json_object(status_output)
    task = _active_task(status)
    return IndexProgress(
        state=_string(task.get("state")) if task is not None else "",
        task_id=_task_id(task) if task is not None else "",
        indexed_scope_present=_has_indexed_scope(status),
        next_retry_at_ms=(
            _non_negative_integer(task.get("next_retry_at_ms"))
            if task is not None
            else 0
        ),
    )


class RepositoryIndexer:
    def __init__(
        self,
        runtime: DockerRuntime,
        *,
        timeout_seconds: int,
        redactor: SecretRedactor,
    ) -> None:
        self._runtime = runtime
        self._timeout_seconds = timeout_seconds
        self._redactor = redactor

    def prepare(self, container: str, log_path: Path) -> float:
        started = time.monotonic()
        deadline = started + self._timeout_seconds
        log_path.parent.mkdir(parents=True, exist_ok=True)
        with log_path.open("w", encoding="utf-8", newline="\n") as log:
            self._run(
                container,
                [_BINARY, "version", "--format", "json"],
                deadline,
                log,
            )
            self._run(
                container,
                [
                    _BINARY,
                    "repo",
                    "register",
                    "/testbed",
                    "--alias",
                    "swebench",
                    "--format",
                    "json",
                ],
                deadline,
                log,
            )
            self._run(
                container,
                [
                    _BINARY,
                    "repo",
                    "index",
                    "swebench",
                    "--ref",
                    "HEAD",
                    "--format",
                    "json",
                ],
                deadline,
                log,
            )
            self._drain_task(container, deadline, log)
        return time.monotonic() - started

    def _drain_task(self, container: str, deadline: float, log: object) -> None:
        attempts = 0
        while attempts < 100:
            status_output = self._run(
                container,
                [_BINARY, "repo", "status", "swebench", "--format", "json"],
                deadline,
                log,
            )
            progress = parse_index_progress(status_output)
            if not progress.state:
                if progress.indexed_scope_present:
                    return
                raise RuntimeError("Repository index ended without an indexed scope")
            state = progress.state
            if state == "succeeded":
                return
            if state in {"failed", "dead_letter", "cancelled"}:
                raise RuntimeError(
                    f"Repository index task reached terminal state {state}"
                )
            if state in {"queued", "retrying", "pending"}:
                if state == "retrying" and self._wait_for_retry_window(
                    progress.next_retry_at_ms, deadline
                ):
                    attempts += 1
                    continue
                command = [_BINARY, "repo", "index-worker"]
                if progress.task_id:
                    command.extend(["--task-id", progress.task_id])
                command.extend(["--format", "json"])
                self._run(container, command, deadline, log)
                attempts += 1
                continue
            if state == "running":
                if time.monotonic() >= deadline:
                    raise TimeoutError(
                        "Repository index lease did not complete before timeout"
                    )
                time.sleep(min(2.0, max(0.0, deadline - time.monotonic())))
                attempts += 1
                continue
            raise RuntimeError(f"Unsupported repository index task state: {state!r}")
        raise RuntimeError(
            "Repository index exceeded 100 bounded worker/status attempts"
        )

    @staticmethod
    def _wait_for_retry_window(next_retry_at_ms: int, deadline: float) -> bool:
        delay = max(0.0, (next_retry_at_ms / 1000) - time.time())
        if delay <= 0:
            return False
        remaining = deadline - time.monotonic()
        if remaining <= 0:
            raise TimeoutError("Repository pre-index timeout expired")
        time.sleep(min(delay, remaining))
        if time.monotonic() >= deadline:
            raise TimeoutError("Repository pre-index timeout expired")
        return True

    def _run(
        self,
        container: str,
        command: list[str],
        deadline: float,
        log: object,
    ) -> str:
        remaining = deadline - time.monotonic()
        if remaining <= 0:
            raise TimeoutError("Repository pre-index timeout expired")
        result = self._runtime.exec(
            container,
            command,
            timeout=remaining,
            environment=_ENVIRONMENT,
        )
        entry = {
            "command": command[1:4],
            "returncode": result.returncode,
            "stdout": self._redactor.redact(result.stdout),
            "stderr": self._redactor.redact(result.stderr),
        }
        write = getattr(log, "write", None)
        if callable(write):
            write(json.dumps(entry, ensure_ascii=False) + "\n")
        return result.stdout


def _json_object(value: str) -> Mapping[str, object]:
    try:
        parsed = json.loads(value)
    except json.JSONDecodeError as exc:
        raise RuntimeError(f"relay-knowledge returned invalid JSON: {exc}") from exc
    if not isinstance(parsed, Mapping):
        raise RuntimeError("relay-knowledge JSON root is not an object")
    return parsed


def _active_task(value: Mapping[str, object]) -> Mapping[str, object] | None:
    direct = value.get("active_task")
    if isinstance(direct, Mapping):
        return direct
    for child in value.values():
        if isinstance(child, Mapping):
            found = _active_task(child)
            if found is not None:
                return found
    return None


def _has_indexed_scope(value: Mapping[str, object]) -> bool:
    indicators = (
        "indexed_commit",
        "indexed_ref",
        "indexed_tree",
        "last_indexed_scope_id",
        "last_indexed_commit",
        "latest_indexed_ref",
        "tree_hash",
        "scope_retention",
        "scopes",
    )
    if any(value.get(key) not in (None, "", [], {}) for key in indicators):
        return True
    return any(
        _has_indexed_scope(child)
        for child in value.values()
        if isinstance(child, Mapping)
    )


def _task_id(task: Mapping[str, object]) -> str:
    for key in ("task_id", "id"):
        value = task.get(key)
        if isinstance(value, str):
            return value
    return ""


def _string(value: object) -> str:
    return value.lower() if isinstance(value, str) else ""


def _non_negative_integer(value: object) -> int:
    return (
        value
        if isinstance(value, int) and not isinstance(value, bool) and value > 0
        else 0
    )
