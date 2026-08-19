from __future__ import annotations

import io
import json
import subprocess

from relay_knowledge_skill_eval.indexer import (
    RepositoryIndexer,
    _active_task,
    _has_indexed_scope,
)
from relay_knowledge_skill_eval.security import SecretRedactor


def test_real_status_shape_is_recognized_as_completed_index() -> None:
    status = {
        "status": {
            "last_indexed_scope_id": "git_snapshot:123",
            "last_indexed_commit": "abc",
            "tree_hash": "def",
            "state": "fresh",
            "indexed_file_count": 1837,
        },
        "checkpoint": {"state": "completed"},
    }
    assert _active_task(status) is None
    assert _has_indexed_scope(status) is True


def test_nested_active_task_is_found() -> None:
    task = {"task_id": "task-1", "state": "queued"}
    assert _active_task({"nested": {"active_task": task}}) == task


def test_retrying_index_task_waits_until_retry_window_before_worker(
    monkeypatch,
) -> None:
    wall_clock = [1.0]
    monotonic_clock = [0.0]
    sleeps: list[float] = []

    def sleep(delay: float) -> None:
        sleeps.append(delay)
        wall_clock[0] += delay
        monotonic_clock[0] += delay

    class Runtime:
        def __init__(self) -> None:
            self.status_calls = 0
            self.worker_calls = 0

        def exec(self, _container, command, **_kwargs):
            if "status" in command:
                self.status_calls += 1
                state = "retrying" if self.status_calls < 3 else "succeeded"
                task = {
                    "task_id": "task-1",
                    "state": state,
                    "next_retry_at_ms": 2000,
                }
                return subprocess.CompletedProcess(
                    command,
                    0,
                    json.dumps({"active_task": task}),
                    "",
                )
            if "index-worker" in command:
                self.worker_calls += 1
                return subprocess.CompletedProcess(command, 0, "{}", "")
            raise AssertionError(command)

    runtime = Runtime()
    monkeypatch.setattr(
        "relay_knowledge_skill_eval.indexer.time.time", lambda: wall_clock[0]
    )
    monkeypatch.setattr(
        "relay_knowledge_skill_eval.indexer.time.monotonic",
        lambda: monotonic_clock[0],
    )
    monkeypatch.setattr("relay_knowledge_skill_eval.indexer.time.sleep", sleep)
    indexer = RepositoryIndexer(
        runtime,  # type: ignore[arg-type]
        timeout_seconds=10,
        redactor=SecretRedactor(),
    )

    indexer._drain_task("container", 10.0, io.StringIO())

    assert sleeps == [1.0]
    assert runtime.worker_calls == 1
