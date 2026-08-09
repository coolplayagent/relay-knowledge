from __future__ import annotations

import asyncio
import gzip
import json
import shutil
import subprocess
import time
from pathlib import Path
from types import SimpleNamespace
from typing import cast

import pytest

from relay_knowledge_skill_eval.deep_swe_agent import (
    DEEP_SWE_OUTPUT_BUDGET_BYTES,
    DEEPSEEK_ENVIRONMENT_COMPOSE,
    STDERR_REDACTOR_SCRIPT,
    TRACE_CAPTURE_SCRIPT,
    DeepSweConfigurationError,
    DeepSweTransportError,
    PiDeepSweAgent,
    _agent_exec_environment,
    _build_deep_swe_prompt,
    _merge_summaries,
    _pi_shell_command,
    _recoverable,
)
from relay_knowledge_skill_eval.deep_swe_runner import (
    AGENT_TIMEOUT_SECONDS,
    DEEP_SWE_COMMIT,
    DEEP_SWE_REPOSITORY,
    INDEX_TIMEOUT_SECONDS,
    PIER_CLEANUP_GRACE_SECONDS,
    PIER_SETUP_GRACE_SECONDS,
    _agent_config,
    _job_config,
    _prepare_task_inputs,
    _run,
    ensure_deep_swe_tasks,
    validate_deep_swe_tasks,
)
from relay_knowledge_skill_eval.models import Condition


def test_deep_swe_treatment_requires_and_loads_skill() -> None:
    prompt = _build_deep_swe_prompt(
        "fix it",
        condition=Condition.SKILL,
        require_skill_use=True,
    )
    command = _pi_shell_command(
        condition=Condition.SKILL,
        model="deepseek-v4-flash",
        thinking="high",
        prompt_path="/logs/agent/prompt.txt",
        trace_path="/logs/agent/trace.gz",
        summary_path="/logs/agent/summary.json",
        stderr_path="/logs/agent/stderr.log",
        continue_session=False,
    )

    assert "requirement is mandatory" in prompt
    assert "Required workflow:" in prompt
    assert "continue iterating" in prompt
    assert "Inspect git status and git diff" in prompt
    assert "<task>\nfix it\n</task>" in prompt
    assert "--skill" in command
    assert "/opt/pi-eval/skill/SKILL.md" in command
    assert "prompt=$(cat /logs/agent/prompt.txt)" in command
    assert '"$prompt"' in command
    assert "< /logs/agent/prompt.txt" not in command


def test_deep_swe_baseline_does_not_load_skill() -> None:
    prompt = _build_deep_swe_prompt(
        "fix it",
        condition=Condition.BASELINE,
        require_skill_use=False,
    )
    command = _pi_shell_command(
        condition=Condition.BASELINE,
        model="deepseek-v4-flash",
        thinking="high",
        prompt_path="/logs/agent/prompt.txt",
        trace_path="/logs/agent/trace.gz",
        summary_path="/logs/agent/summary.json",
        stderr_path="/logs/agent/stderr.log",
        continue_session=False,
    )

    assert "requirement is mandatory" not in prompt
    assert "Required workflow:" in prompt
    assert "continue iterating" in prompt
    assert "Inspect git status and git diff" in prompt
    assert "<task>\nfix it\n</task>" in prompt
    assert "--skill" not in command
    assert "--no-skills" in command
    assert command.startswith("setsid bash -lc ")
    assert "pi-deepswe-agent.pgid" in command
    assert f"head -c {DEEP_SWE_OUTPUT_BUDGET_BYTES}" in command
    assert "stderr-redactor.mjs" in command


def test_deep_swe_trace_capture_redacts_and_bounds_persisted_output() -> None:
    assert "process.env.DEEPSEEK_API_KEY" in TRACE_CAPTURE_SCRIPT
    assert "[REDACTED]" in TRACE_CAPTURE_SCRIPT
    assert "maxOutputBytes = 64 * 1024 * 1024" in TRACE_CAPTURE_SCRIPT
    assert 'import readline from "node:readline"' not in TRACE_CAPTURE_SCRIPT
    assert "for await (const chunkValue of process.stdin)" in TRACE_CAPTURE_SCRIPT
    assert "remaining = maxOutputBytes - inputBytes" in TRACE_CAPTURE_SCRIPT
    assert "Buffer.concat(lineSegments, lineBytes)" in TRACE_CAPTURE_SCRIPT
    assert "pendingRelayCommands.set(key, kind)" in TRACE_CAPTURE_SCRIPT
    assert "relayKind && event.isError !== true" in TRACE_CAPTURE_SCRIPT
    assert "output_limited:outputLimited" in TRACE_CAPTURE_SCRIPT
    assert "process.exitCode = 75" in TRACE_CAPTURE_SCRIPT
    assert "process.env.DEEPSEEK_API_KEY" in STDERR_REDACTOR_SCRIPT
    assert 'import readline from "node:readline"' not in STDERR_REDACTOR_SCRIPT
    assert "for await (const chunkValue of process.stdin)" in STDERR_REDACTOR_SCRIPT
    assert "stderr-output-limited" in _pi_shell_command(
        condition=Condition.BASELINE,
        model="deepseek-v4-flash",
        thinking="high",
        prompt_path="/logs/agent/prompt.txt",
        trace_path="/logs/agent/trace.gz",
        summary_path="/logs/agent/summary.json",
        stderr_path="/logs/agent/stderr.log",
        continue_session=False,
    )
    assert "output.split(apiKey)" in STDERR_REDACTOR_SCRIPT
    assert "sk-[A-Za-z0-9_-]{20,}" in STDERR_REDACTOR_SCRIPT


def test_stderr_redactor_marks_data_after_exact_newline_boundary(
    tmp_path: Path,
) -> None:
    node = shutil.which("node")
    if node is None:
        pytest.skip("Node.js is required to execute the bundled redactor")
    script = tmp_path / "stderr-redactor.mjs"
    marker = tmp_path / "stderr-output-limited"
    script.write_text(
        STDERR_REDACTOR_SCRIPT.replace("64 * 1024 * 1024", "8"),
        encoding="utf-8",
    )

    result = subprocess.run(
        [node, str(script), str(marker)],
        input=b"1234567\nZ",
        capture_output=True,
        check=False,
    )

    assert result.returncode == 75
    assert result.stdout == b"1234567\n"
    assert marker.read_text(encoding="utf-8") == "limited\n"


def test_deep_swe_key_is_not_forwarded_in_exec_arguments() -> None:
    environment = _agent_exec_environment(
        {"DEEPSEEK_API_KEY": "secret", "SAFE_SETTING": "value"}
    )

    assert "DEEPSEEK_API_KEY" not in environment
    assert environment["SAFE_SETTING"] == "value"
    encoded_compose = str(DEEPSEEK_ENVIRONMENT_COMPOSE)
    assert "secret" not in encoded_compose
    assert "${DEEPSEEK_API_KEY}" in encoded_compose


def test_deep_swe_exec_deadline_commits_partial_work_and_reports_timeout(
    tmp_path: Path,
) -> None:
    class DeadlineEnvironment:
        def __init__(self) -> None:
            self.commands: list[str] = []
            self.agent_envs: list[dict[str, str]] = []

        def agent_process_env(self, env: dict[str, str]) -> dict[str, str]:
            merged = {"HTTPS_PROXY": "http://egress-proxy:8080", **env}
            self.agent_envs.append(merged)
            return merged

        async def exec(self, command: str, **kwargs: object) -> SimpleNamespace:
            self.commands.append(command)
            if len(self.commands) == 1:
                assert kwargs["env"] == self.agent_envs[-1]
                (tmp_path / "pi-summary-01.json").write_text(
                    '{"tokens":{"input":7,"cache_read":11,"output":5,'
                    '"total":23,"cost_usd":0.2,"requests":2},'
                    '"tools":{"calls":3}}\n',
                    encoding="utf-8",
                )
                raise RuntimeError("Command timed out after 1 seconds")
            return SimpleNamespace(return_code=0, stderr="")

    environment = DeadlineEnvironment()
    agent = PiDeepSweAgent(
        logs_dir=tmp_path,
        model_name="deepseek/deepseek-v4-flash",
        condition="baseline",
        require_skill_use=False,
        agent_timeout_seconds=1,
        max_continuations=0,
    )

    context = SimpleNamespace()
    agent.preindex_seconds = 12.5
    with pytest.raises(TimeoutError, match="configured timeout"):
        asyncio.run(
            agent.run(
                "fix it",
                cast(object, environment),
                cast(object, context),
            )
        )

    assert len(environment.commands) == 3
    assert "kill -TERM" in environment.commands[1]
    assert "kill -KILL" in environment.commands[1]
    assert "git add -A" in environment.commands[2]
    assert environment.agent_envs == [
        {
            "HTTPS_PROXY": "http://egress-proxy:8080",
            "RELAY_KNOWLEDGE_HOME": "/tmp/relay-knowledge-home",
            "RELAY_KNOWLEDGE_SEMANTIC_BACKEND": "local",
            "RELAY_KNOWLEDGE_VECTOR_BACKEND": "local",
        }
    ]
    assert context.n_input_tokens == 18
    assert context.n_cache_tokens == 11
    assert context.n_output_tokens == 5
    assert context.cost_usd == 0.2
    assert context.n_agent_steps == 2
    assert context.metadata["tools"]["calls"] == 3
    assert context.metadata["preindex_seconds"] == 12.5


def test_deep_swe_trace_limit_commits_partial_work_and_fails_final(
    tmp_path: Path,
) -> None:
    class LimitedEnvironment:
        def __init__(self) -> None:
            self.commands: list[str] = []

        @staticmethod
        def agent_process_env(env: dict[str, str]) -> dict[str, str]:
            return env

        async def exec(self, command: str, **_kwargs: object) -> SimpleNamespace:
            self.commands.append(command)
            if "pi-eval" in command:
                (tmp_path / "pi-stderr-01.log").write_text("", encoding="utf-8")
                (tmp_path / "pi-summary-01.json").write_text(
                    '{"tokens":{},"tools":{},"output_limited":true}\n',
                    encoding="utf-8",
                )
                return SimpleNamespace(return_code=75, stderr="")
            return SimpleNamespace(return_code=0, stderr="")

    environment = LimitedEnvironment()
    agent = PiDeepSweAgent(
        logs_dir=tmp_path,
        model_name="deepseek/deepseek-v4-flash",
        condition="baseline",
        require_skill_use=False,
        max_continuations=3,
    )

    with pytest.raises(RuntimeError, match="trace exceeded"):
        asyncio.run(
            agent.run(
                "fix it",
                cast(object, environment),
                cast(object, SimpleNamespace()),
            )
        )

    assert len(environment.commands) == 2
    assert "git add -A" in environment.commands[1]


def test_deep_swe_mandatory_skill_success_requires_repository_query(
    tmp_path: Path,
) -> None:
    class NoQueryEnvironment:
        @staticmethod
        def agent_process_env(env: dict[str, str]) -> dict[str, str]:
            return env

        async def exec(self, command: str, **_kwargs: object) -> SimpleNamespace:
            if "pi-eval" in command:
                (tmp_path / "pi-stderr-01.log").write_text("", encoding="utf-8")
                (tmp_path / "pi-summary-01.json").write_text(
                    '{"tokens":{},"tools":{"relay_commands":{}},'
                    '"output_limited":false}\n',
                    encoding="utf-8",
                )
            return SimpleNamespace(return_code=0, stderr="", stdout="")

    agent = PiDeepSweAgent(
        logs_dir=tmp_path,
        model_name="deepseek/deepseek-v4-flash",
        condition="skill",
        require_skill_use=True,
    )

    with pytest.raises(RuntimeError, match="repository query was not observed"):
        asyncio.run(
            agent.run(
                "fix it",
                cast(object, NoQueryEnvironment()),
                cast(object, SimpleNamespace()),
            )
        )


def test_deep_swe_mandatory_query_survives_a_successful_continuation(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    class ContinuedEnvironment:
        def __init__(self) -> None:
            self.pi_attempts = 0

        @staticmethod
        def agent_process_env(env: dict[str, str]) -> dict[str, str]:
            return env

        async def exec(self, command: str, **_kwargs: object) -> SimpleNamespace:
            if "pi-eval" not in command:
                return SimpleNamespace(return_code=0, stderr="", stdout="")
            self.pi_attempts += 1
            attempt = self.pi_attempts
            (tmp_path / f"pi-stderr-{attempt:02d}.log").write_text(
                "temporary transport failure" if attempt == 1 else "",
                encoding="utf-8",
            )
            relay_commands = {"repo query": 1} if attempt == 1 else {}
            (tmp_path / f"pi-summary-{attempt:02d}.json").write_text(
                json.dumps(
                    {
                        "tokens": {},
                        "tools": {"relay_commands": relay_commands},
                        "output_limited": False,
                    }
                ),
                encoding="utf-8",
            )
            return SimpleNamespace(
                return_code=75 if attempt == 1 else 0,
                stderr="",
                stdout="",
            )

    async def no_delay(*_args: object, **_kwargs: object) -> None:
        return None

    monkeypatch.setattr(
        "relay_knowledge_skill_eval.deep_swe_agent.asyncio.sleep", no_delay
    )
    environment = ContinuedEnvironment()
    context = SimpleNamespace()
    agent = PiDeepSweAgent(
        logs_dir=tmp_path,
        model_name="deepseek/deepseek-v4-flash",
        condition="skill",
        require_skill_use=True,
        max_continuations=1,
    )

    asyncio.run(
        agent.run(
            "fix it",
            cast(object, environment),
            cast(object, context),
        )
    )

    assert environment.pi_attempts == 2
    assert context.metadata["tools"]["relay_commands"]["repo query"] == 1
    assert context.metadata["tools"]["harness_continuations"] == 1


def test_deep_swe_context_recovers_metrics_when_timeout_truncates_summary(
    tmp_path: Path,
) -> None:
    event = {
        "type": "message_end",
        "message": {
            "role": "assistant",
            "timestamp": 1,
            "model": "deepseek-v4-flash",
            "usage": {
                "input": 7,
                "output": 5,
                "cacheRead": 11,
                "totalTokens": 23,
                "cost": {"total": 0.2},
            },
        },
    }
    compressed = gzip.compress(
        (json.dumps(event, separators=(",", ":")) + "\n").encode()
    )
    (tmp_path / "pi-trace-01.jsonl.gz").write_bytes(compressed[:-8])
    agent = PiDeepSweAgent(
        logs_dir=tmp_path,
        model_name="deepseek/deepseek-v4-flash",
        condition="baseline",
        require_skill_use=False,
    )
    agent._harness_continuations = 0
    context = SimpleNamespace()

    agent._populate_context(context, started=time.monotonic())

    assert context.n_input_tokens == 18
    assert context.n_cache_tokens == 11
    assert context.n_output_tokens == 5
    assert context.n_agent_steps == 1
    assert context.metadata["tokens"]["total"] == 23
    assert context.metadata["metrics_recovered_from_truncated_trace"] is True


def test_deep_swe_missing_exec_log_is_retryable_infrastructure(
    tmp_path: Path,
) -> None:
    class MissingLogEnvironment:
        @staticmethod
        def agent_process_env(env: dict[str, str]) -> dict[str, str]:
            return env

        async def exec(self, _command: str, **_kwargs: object) -> SimpleNamespace:
            return SimpleNamespace(
                return_code=125,
                stderr="",
                stdout="Docker daemon disconnected before exec",
            )

    agent = PiDeepSweAgent(
        logs_dir=tmp_path,
        model_name="deepseek/deepseek-v4-flash",
        condition="baseline",
        require_skill_use=False,
    )

    with pytest.raises(DeepSweTransportError, match="stderr artifact"):
        asyncio.run(
            agent.run(
                "fix it",
                cast(object, MissingLogEnvironment()),
                cast(object, SimpleNamespace()),
            )
        )


def test_blank_stderr_is_retryable_only_for_known_transient_exit() -> None:
    assert _recoverable(75, "") is True
    assert _recoverable(137, "") is True
    assert _recoverable(1, "") is False
    assert _recoverable(1, "connection reset") is True


def test_blank_stderr_nontransient_exit_is_final_agent_error(tmp_path: Path) -> None:
    class BlankFailureEnvironment:
        @staticmethod
        def agent_process_env(env: dict[str, str]) -> dict[str, str]:
            return env

        async def exec(self, command: str, **_kwargs: object) -> SimpleNamespace:
            if "pi-eval" in command:
                (tmp_path / "pi-stderr-01.log").write_text("", encoding="utf-8")
                return SimpleNamespace(return_code=1, stderr="", stdout="")
            return SimpleNamespace(return_code=0, stderr="", stdout="")

    agent = PiDeepSweAgent(
        logs_dir=tmp_path,
        model_name="deepseek/deepseek-v4-flash",
        condition="baseline",
        require_skill_use=False,
        max_continuations=0,
    )

    with pytest.raises(RuntimeError, match="Pi agent failed") as failure:
        asyncio.run(
            agent.run(
                "fix it",
                cast(object, BlankFailureEnvironment()),
                cast(object, SimpleNamespace()),
            )
        )
    assert not isinstance(failure.value, DeepSweTransportError)


def test_deep_swe_provider_configuration_failure_is_retryable(
    tmp_path: Path,
) -> None:
    class InvalidProviderEnvironment:
        @staticmethod
        def agent_process_env(env: dict[str, str]) -> dict[str, str]:
            return env

        async def exec(self, command: str, **_kwargs: object) -> SimpleNamespace:
            if "pi-eval" in command:
                (tmp_path / "pi-stderr-01.log").write_text(
                    "Invalid API key\n", encoding="utf-8"
                )
                return SimpleNamespace(return_code=1, stderr="", stdout="")
            return SimpleNamespace(return_code=0, stderr="", stdout="")

    agent = PiDeepSweAgent(
        logs_dir=tmp_path,
        model_name="deepseek/deepseek-v4-flash",
        condition="baseline",
        require_skill_use=False,
        max_continuations=0,
    )

    with pytest.raises(DeepSweConfigurationError, match="provider configuration"):
        asyncio.run(
            agent.run(
                "fix it",
                cast(object, InvalidProviderEnvironment()),
                cast(object, SimpleNamespace()),
            )
        )


def test_final_agent_error_is_not_transport_due_to_earlier_attempt(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    class MixedFailureEnvironment:
        def __init__(self) -> None:
            self.attempt = 0

        @staticmethod
        def agent_process_env(env: dict[str, str]) -> dict[str, str]:
            return env

        async def exec(self, command: str, **_kwargs: object) -> SimpleNamespace:
            if "pi-eval" in command:
                self.attempt += 1
                message = (
                    "connection reset by peer"
                    if self.attempt == 1
                    else "deterministic agent command failure"
                )
                (tmp_path / f"pi-stderr-{self.attempt:02d}.log").write_text(
                    message + "\n", encoding="utf-8"
                )
                return SimpleNamespace(return_code=1, stderr="", stdout="")
            return SimpleNamespace(return_code=0, stderr="", stdout="")

    async def no_backoff(_: float) -> None:
        return None

    monkeypatch.setattr(
        "relay_knowledge_skill_eval.deep_swe_agent.asyncio.sleep", no_backoff
    )
    agent = PiDeepSweAgent(
        logs_dir=tmp_path,
        model_name="deepseek/deepseek-v4-flash",
        condition="baseline",
        require_skill_use=False,
        max_continuations=1,
    )

    with pytest.raises(RuntimeError, match="Pi agent failed after continuation"):
        asyncio.run(
            agent.run(
                "fix it",
                cast(object, MixedFailureEnvironment()),
                cast(object, SimpleNamespace()),
            )
        )


def test_deep_swe_backoff_stops_at_deadline_and_commits_partial_work(
    tmp_path: Path, monkeypatch
) -> None:
    clock = [0.0]
    sleeps: list[float] = []

    async def advance_clock(delay: float) -> None:
        sleeps.append(delay)
        clock[0] += delay

    class BackoffEnvironment:
        def __init__(self) -> None:
            self.commands: list[str] = []

        @staticmethod
        def agent_process_env(env: dict[str, str]) -> dict[str, str]:
            return env

        async def exec(self, command: str, **_kwargs: object) -> SimpleNamespace:
            self.commands.append(command)
            if "pi-eval" in command:
                (tmp_path / "pi-stderr-01.log").write_text(
                    "connection reset\n", encoding="utf-8"
                )
                return SimpleNamespace(return_code=1, stderr="", stdout="")
            return SimpleNamespace(return_code=0, stderr="", stdout="")

    monkeypatch.setattr(
        "relay_knowledge_skill_eval.deep_swe_agent.time.monotonic",
        lambda: clock[0],
    )
    monkeypatch.setattr(
        "relay_knowledge_skill_eval.deep_swe_agent.asyncio.sleep", advance_clock
    )
    environment = BackoffEnvironment()
    agent = PiDeepSweAgent(
        logs_dir=tmp_path,
        model_name="deepseek/deepseek-v4-flash",
        condition="baseline",
        require_skill_use=False,
        agent_timeout_seconds=1,
        max_continuations=3,
    )

    with pytest.raises(TimeoutError, match="configured timeout"):
        asyncio.run(
            agent.run(
                "fix it",
                cast(object, environment),
                cast(object, SimpleNamespace()),
            )
        )

    assert sleeps == [1.0]
    assert "kill -TERM" in environment.commands[1]
    assert "git add -A" in environment.commands[2]


def test_deep_swe_conditions_share_the_same_common_workflow() -> None:
    baseline = _build_deep_swe_prompt(
        "fix it",
        condition=Condition.BASELINE,
        require_skill_use=False,
    )
    skill = _build_deep_swe_prompt(
        "fix it",
        condition=Condition.SKILL,
        require_skill_use=True,
    )

    skill_requirement = (
        "Before editing, you must follow the loaded relay-knowledge-cli skill "
        "and execute its bundled CLI to investigate relevant definitions, "
        "references, callers, dependencies, or repository context. Use the "
        "retrieved evidence to guide the implementation, and perform additional "
        "queries when the implementation path remains unclear. This requirement "
        "is mandatory.\n\n"
    )
    assert skill.replace(skill_requirement, "") == baseline


def test_deep_swe_pair_job_runs_only_one_task_with_two_conditions() -> None:
    agents = [
        _agent_config("baseline", require_skill_use=False),
        _agent_config("skill", require_skill_use=True),
    ]
    runtime_image = "relay-knowledge-skill-eval:content-addressed"
    config = _job_config(Path("task-a"), Path("jobs"), agents, runtime_image)

    assert config.n_concurrent_trials == 2
    assert len(config.tasks) == 1
    assert [agent.kwargs["condition"] for agent in config.agents] == [
        "baseline",
        "skill",
    ]
    assert all(
        agent.override_timeout_sec == AGENT_TIMEOUT_SECONDS + PIER_CLEANUP_GRACE_SECONDS
        for agent in config.agents
    )
    assert all(
        agent.override_timeout_sec > agent.kwargs["agent_timeout_seconds"]
        for agent in config.agents
    )
    assert all(
        agent.override_setup_timeout_sec
        == INDEX_TIMEOUT_SECONDS + PIER_SETUP_GRACE_SECONDS
        for agent in config.agents
    )
    assert all(
        agent.override_setup_timeout_sec > agent.kwargs["index_timeout_seconds"]
        for agent in config.agents
    )
    assert config.environment.kwargs["runtime_image"] == runtime_image
    assert config.retry.include_exceptions == {"DeepSweTransportError"}
    assert "DeepSweConfigurationError" not in config.retry.include_exceptions


def test_trace_summaries_preserve_cache_and_relay_metrics() -> None:
    tokens, tools = _merge_summaries(
        [
            {
                "tokens": {
                    "input": 10,
                    "output": 20,
                    "reasoning": 5,
                    "cache_read": 30,
                    "cache_write": 0,
                    "total": 60,
                    "cost_usd": 0.01,
                    "requests": 2,
                },
                "tools": {
                    "calls": 3,
                    "errors": 1,
                    "cumulative_seconds": 4.5,
                    "by_name": {"bash": 3},
                    "relay_commands": {"repo query": 2},
                },
            }
        ]
    )

    assert tokens.cache_read == 30
    assert tokens.total == 60
    assert tools.relay_commands == {"repo query": 2}


def test_deep_swe_task_scripts_use_posix_line_endings(tmp_path: Path) -> None:
    source = tmp_path / "official" / "task-a"
    source.mkdir(parents=True)
    (source / "task.toml").write_text(
        "version = '1'\r\n"
        "[[verifier.collect]]\r\n"
        'command = "mkdir -p /logs/artifacts && '
        'git diff HEAD > /logs/artifacts/model.patch"\r\n'
        "timeout_sec = 120\r\n",
        encoding="utf-8",
    )
    tests = source / "tests"
    tests.mkdir()
    (tests / "test.sh").write_bytes(b"#!/bin/bash\r\necho ok\r\n")
    (tests / "test.patch").write_bytes(b"--- a/file\r\n+++ b/file\r\n")
    (tests / "fixture.txt").write_bytes(b"one\r\ntwo\r\n")

    prepared = _prepare_task_inputs([source], tmp_path / "run")

    assert prepared == [tmp_path / "run" / "task-inputs" / "task-a"]
    assert (prepared[0] / "tests" / "test.sh").read_bytes() == (
        b"#!/bin/bash\necho ok\n"
    )
    assert (prepared[0] / "tests" / "test.patch").read_bytes() == (
        b"--- a/file\n+++ b/file\n"
    )
    assert (prepared[0] / "tests" / "fixture.txt").read_bytes() == (b"one\r\ntwo\r\n")
    assert "[[verifier.collect]]" in (prepared[0] / "task.toml").read_text(
        encoding="utf-8"
    )
    assert not (prepared[0] / "pre_artifacts.sh").exists()

    resumed = _prepare_task_inputs([source], tmp_path / "run")
    assert resumed == prepared
    assert not (resumed[0] / "pre_artifacts.sh").exists()


def test_existing_pinned_deep_swe_checkout_is_reused(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    checkout = tmp_path / "deep-swe"
    (checkout / ".git").mkdir(parents=True)
    tasks = checkout / "tasks"
    for index in range(113):
        task = tasks / f"task-{index:03d}"
        task.mkdir(parents=True)
        (task / "task.toml").write_text("version = '1'\n", encoding="utf-8")

    commands: list[list[str]] = []

    def fake_git(arguments: list[str], *, cwd: Path) -> str:
        assert cwd == checkout
        commands.append(arguments)
        if arguments[:3] == ["remote", "get-url", "origin"]:
            return DEEP_SWE_REPOSITORY
        if arguments[:2] == ["rev-parse", "HEAD"]:
            return DEEP_SWE_COMMIT
        if arguments[0] in {"reset", "clean"}:
            return ""
        raise AssertionError(arguments)

    monkeypatch.setattr("relay_knowledge_skill_eval.deep_swe_runner._run_git", fake_git)

    assert ensure_deep_swe_tasks(checkout) == tasks
    assert commands == [
        ["remote", "get-url", "origin"],
        ["rev-parse", "HEAD"],
        ["reset", "--hard", DEEP_SWE_COMMIT],
        ["clean", "-fdx"],
    ]


def test_deep_swe_run_fails_after_infrastructure_retries_are_exhausted(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    task = tmp_path / "task-a"
    task.mkdir()
    jobs: list[object] = []
    report_calls: list[dict[str, object]] = []

    class FakeJob:
        def on_trial_ended(self, callback: object) -> None:
            assert callback is not None

        async def run(self) -> None:
            return None

    async def fake_create(_: object) -> FakeJob:
        job = FakeJob()
        jobs.append(job)
        return job

    monkeypatch.setattr(
        "relay_knowledge_skill_eval.deep_swe_runner._prepare_task_inputs",
        lambda task_paths, _: task_paths,
    )
    monkeypatch.setattr(
        "relay_knowledge_skill_eval.deep_swe_runner.write_deep_swe_report",
        lambda *_, **kwargs: report_calls.append(kwargs),
    )
    monkeypatch.setattr(
        "relay_knowledge_skill_eval.deep_swe_runner._archive_infrastructure_failures",
        lambda *_, **__: False,
    )
    monkeypatch.setattr(
        "relay_knowledge_skill_eval.deep_swe_runner._job_config",
        lambda *_, **__: object(),
    )
    monkeypatch.setattr(
        "relay_knowledge_skill_eval.deep_swe_runner._has_infrastructure_failure",
        lambda _: True,
    )
    monkeypatch.setattr(
        "relay_knowledge_skill_eval.deep_swe_runner.Job.create", fake_create
    )

    with pytest.raises(RuntimeError, match=r"task-a.*after three attempts"):
        asyncio.run(
            _run(
                [task],
                tmp_path / "run",
                "runtime:latest",
                skill_version="test-version",
                skill_sha256="test-sha",
            )
        )

    assert len(jobs) == 3
    assert report_calls
    assert all(call["skill_version"] == "test-version" for call in report_calls)
    assert all(call["skill_sha256"] == "test-sha" for call in report_calls)


def test_deep_swe_repairs_existing_job_before_validation_and_report(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    task = tmp_path / "task-a"
    task.mkdir()
    output_dir = tmp_path / "run"
    (output_dir / "tasks" / task.name).mkdir(parents=True)
    events: list[str] = []

    class FakeJob:
        def _close_logger_handlers(self) -> None:
            events.append("close")

        def on_trial_ended(self, callback: object) -> None:
            assert callback is not None

        async def run(self) -> None:
            events.append("run")

    async def fake_create(_: object) -> FakeJob:
        events.append("validate")
        return FakeJob()

    monkeypatch.setattr(
        "relay_knowledge_skill_eval.deep_swe_runner._prepare_task_inputs",
        lambda task_paths, _: task_paths,
    )
    monkeypatch.setattr(
        "relay_knowledge_skill_eval.deep_swe_runner.write_deep_swe_report",
        lambda *_, **__: events.append("report"),
    )
    monkeypatch.setattr(
        "relay_knowledge_skill_eval.deep_swe_runner._archive_infrastructure_failures",
        lambda *_, **__: events.append("archive") or False,
    )
    monkeypatch.setattr(
        "relay_knowledge_skill_eval.deep_swe_runner._has_infrastructure_failure",
        lambda _: False,
    )
    monkeypatch.setattr(
        "relay_knowledge_skill_eval.deep_swe_runner.Job.create", fake_create
    )

    asyncio.run(
        _run(
            [task],
            output_dir,
            "runtime:latest",
            skill_version="test-version",
            skill_sha256="test-sha",
        )
    )

    assert events[0:4] == ["archive", "validate", "close", "report"]
    assert events.count("archive") == 2
    assert events.count("validate") == 2
    assert events.count("close") == 1


def test_explicit_deep_swe_tasks_require_clean_pinned_official_checkout(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    checkout = tmp_path / "deep-swe"
    tasks = checkout / "tasks"
    for index in range(113):
        task = tasks / f"task-{index:03d}"
        task.mkdir(parents=True)
        (task / "task.toml").write_text("version = '1'\n", encoding="utf-8")

    commands: list[tuple[Path, list[str]]] = []

    def fake_git(arguments: list[str], *, cwd: Path) -> str:
        commands.append((cwd, arguments))
        if arguments == ["rev-parse", "--show-toplevel"]:
            return str(checkout)
        if arguments == ["remote", "get-url", "origin"]:
            return DEEP_SWE_REPOSITORY + ".git"
        if arguments == ["rev-parse", "HEAD"]:
            return DEEP_SWE_COMMIT
        if arguments == ["status", "--porcelain", "--untracked-files=all"]:
            return ""
        raise AssertionError(arguments)

    monkeypatch.setattr("relay_knowledge_skill_eval.deep_swe_runner._run_git", fake_git)

    assert validate_deep_swe_tasks(tasks) == tasks
    assert commands == [
        (tasks, ["rev-parse", "--show-toplevel"]),
        (checkout, ["remote", "get-url", "origin"]),
        (checkout, ["rev-parse", "HEAD"]),
        (checkout, ["status", "--porcelain", "--untracked-files=all"]),
    ]


@pytest.mark.parametrize(
    ("command", "output", "message"),
    [
        (["remote", "get-url", "origin"], "https://example.com/fork", "origin"),
        (["rev-parse", "HEAD"], "0" * 40, "expected"),
        (
            ["status", "--porcelain", "--untracked-files=all"],
            " M tasks/task-000/task.toml",
            "modified or untracked",
        ),
    ],
)
def test_explicit_deep_swe_tasks_reject_unofficial_or_modified_checkout(
    tmp_path: Path,
    monkeypatch: pytest.MonkeyPatch,
    command: list[str],
    output: str,
    message: str,
) -> None:
    checkout = tmp_path / "deep-swe"
    tasks = checkout / "tasks"
    tasks.mkdir(parents=True)

    def fake_git(arguments: list[str], *, cwd: Path) -> str:
        if arguments == ["rev-parse", "--show-toplevel"]:
            return str(checkout)
        if arguments == command:
            return output
        if arguments == ["remote", "get-url", "origin"]:
            return DEEP_SWE_REPOSITORY
        if arguments == ["rev-parse", "HEAD"]:
            return DEEP_SWE_COMMIT
        if arguments == ["status", "--porcelain", "--untracked-files=all"]:
            return ""
        raise AssertionError((cwd, arguments))

    monkeypatch.setattr("relay_knowledge_skill_eval.deep_swe_runner._run_git", fake_git)

    with pytest.raises(RuntimeError, match=message):
        validate_deep_swe_tasks(tasks)
