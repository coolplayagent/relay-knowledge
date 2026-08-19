from __future__ import annotations

import json
import subprocess
import threading
from pathlib import Path
from types import SimpleNamespace

from relay_knowledge_skill_eval.checkpoint import CheckpointStore
from relay_knowledge_skill_eval.indexer import RepositoryIndexer
from relay_knowledge_skill_eval.models import (
    Condition,
    EvalResult,
    RunOutcome,
    RunSignature,
    SweBenchDiagnostics,
    SweBenchItem,
)
from relay_knowledge_skill_eval.runner import (
    PI_OUTPUT_BUDGET_BYTES,
    PI_STREAM_QUEUE_ITEMS,
    EvaluatorConfig,
    SkillEvaluator,
)
from relay_knowledge_skill_eval.security import SecretRedactor


def item(instance_id: str) -> SweBenchItem:
    return SweBenchItem(
        instance_id=instance_id,
        repo="org/repo",
        base_commit=f"base-{instance_id}",
        problem_statement=f"Fix {instance_id}",
    )


def signature() -> RunSignature:
    return RunSignature(
        dataset_name="verified",
        dataset_sha256="dataset",
        harness_version="1",
        swebench_version="4.1.0",
        node_version="22.19.0",
        pi_version="0.80.3",
        model="deepseek-v4-flash",
        thinking="high",
        skill_version="1.1.13",
        skill_sha256="skill",
        runtime_image="runtime",
        image_prefix="prefix",
        prompt_version="1",
        treatment_instruction="available",
        condition_execution_mode="stable-hash-sequential",
        tool_allowlist="read,bash,edit,write,grep,find,ls",
        agent_timeout_seconds=600,
        index_timeout_seconds=600,
        score_timeout_seconds=900,
    )


class FakeRuntime:
    def __init__(self, *, generated_patch: str = "diff --git a/x b/x\n") -> None:
        self.generated_patch = generated_patch
        self.starts = 0
        self.removed: list[str] = []
        self.diff_commands: list[list[str]] = []
        self.patch_files: dict[str, str] = {}

    def start_instance(self, instance_id: str, condition: str) -> tuple[str, float]:
        self.starts += 1
        return f"{instance_id}-{condition}", 0.25

    def exec(self, container: str, command: list[str], **kwargs):
        _ = (container, kwargs)
        if command[:3] == ["git", "rev-parse", "HEAD"]:
            instance_id = container.rsplit("-", 1)[0]
            return subprocess.CompletedProcess(command, 0, f"base-{instance_id}\n", "")
        if command[:2] == ["bash", "-c"] and "git diff" in command[2]:
            self.diff_commands.append(command)
            limit = int(command[-1])
            self.patch_files[command[-2]] = self.generated_patch[:limit]
            return subprocess.CompletedProcess(command, 0, "", "")
        if command[:2] == ["wc", "-c"]:
            patch = self.patch_files[command[-1]]
            return subprocess.CompletedProcess(
                command, 0, f"{len(patch.encode('utf-8'))} {command[-1]}\n", ""
            )
        if command[:1] == ["cat"]:
            return subprocess.CompletedProcess(
                command, 0, self.patch_files[command[-1]], ""
            )
        if command[:2] == ["rm", "-f"]:
            self.patch_files.pop(command[-1], None)
            return subprocess.CompletedProcess(command, 0, "", "")
        return subprocess.CompletedProcess(command, 0, "", "")

    def remove_container(self, name: str) -> None:
        self.removed.append(name)


class FakeScorer:
    def __init__(self, *, fail: bool = False, image_fail: bool = False) -> None:
        self.fail = fail
        self.image_fail = image_fail
        self.patches: list[str] = []

    def ensure_instance_image(self, eval_item: SweBenchItem) -> float:
        _ = eval_item
        if self.image_fail:
            raise RuntimeError("fake image build failure")
        return 2.0

    def score(self, *, item: SweBenchItem, condition: str, generated_patch: str):
        _ = (item, condition)
        if self.fail:
            raise RuntimeError("fake scorer failure")
        self.patches.append(generated_patch)
        return (
            SweBenchDiagnostics(
                completed=True,
                resolved=bool(generated_patch),
                patch_exists=bool(generated_patch),
                patch_applied=bool(generated_patch),
            ),
            3.0,
        )


def evaluator(
    tmp_path: Path,
    runtime: FakeRuntime,
    scorer: FakeScorer,
    *,
    resume: bool = False,
    parallel_conditions: bool = False,
    require_skill_use: bool = False,
) -> tuple[SkillEvaluator, CheckpointStore]:
    store = CheckpointStore(tmp_path)
    store.initialize(signature(), "commit")
    value = SkillEvaluator(
        runtime=runtime,  # type: ignore[arg-type]
        scorer=scorer,  # type: ignore[arg-type]
        checkpoint=store,
        config=EvaluatorConfig(
            output_dir=tmp_path,
            resume=resume,
            concurrency=2,
            parallel_conditions=parallel_conditions,
            require_skill_use=require_skill_use,
        ),
        redactor=SecretRedactor(),
    )
    return value, store


def install_fake_agent(monkeypatch, evaluation: SkillEvaluator, *, timeout=False):
    def fake_stream(**kwargs):
        trace_path = kwargs["trace_path"]
        trace_path.write_text('{"type":"agent_end"}\n', encoding="utf-8")
        return (137 if timeout else 0, timeout, False, False, False, "")

    monkeypatch.setattr(evaluation, "_stream_pi", fake_stream)
    monkeypatch.setattr(RepositoryIndexer, "prepare", lambda *args: 4.0)


def test_fake_ab_run_captures_patch_timings_and_timeout(tmp_path, monkeypatch) -> None:
    runtime = FakeRuntime()
    scorer = FakeScorer()
    evaluation, store = evaluator(tmp_path, runtime, scorer)
    install_fake_agent(monkeypatch, evaluation, timeout=True)
    results = evaluation.run([item("case")])
    assert len(results) == 2
    assert {result.outcome for result in results} == {RunOutcome.TIMED_OUT}
    assert runtime.starts == 2
    assert len(runtime.removed) == 2
    assert len(scorer.patches) == 2
    assert all(command[-3] == "base-case" for command in runtime.diff_commands)
    assert all(result.timings.image_prepare_seconds == 1 for result in results)
    treatment = next(
        result for result in results if result.condition is Condition.SKILL
    )
    assert treatment.timings.preindex_seconds == 4
    assert len(store.load_results()) == 2


def test_raw_patch_is_scored_while_persisted_patch_is_redacted(
    tmp_path, monkeypatch
) -> None:
    raw_patch = "diff --git a/x b/x\n+fixture = 'sk-1234567890abcdef'\n"
    runtime = FakeRuntime(generated_patch=raw_patch)
    scorer = FakeScorer()
    evaluation, _ = evaluator(tmp_path, runtime, scorer)
    install_fake_agent(monkeypatch, evaluation)

    results = evaluation.run([item("secret-shaped-fixture")])

    assert scorer.patches == [raw_patch, raw_patch]
    for result in results:
        persisted = Path(result.patch_path).read_text(encoding="utf-8")
        assert "sk-" not in persisted
        assert "[REDACTED]" in persisted


def test_mandatory_skill_run_without_repository_query_is_not_scored(
    tmp_path, monkeypatch
) -> None:
    runtime = FakeRuntime(generated_patch="diff --git a/x b/x\n+fixed\n")
    scorer = FakeScorer()
    evaluation, _ = evaluator(
        tmp_path,
        runtime,
        scorer,
        require_skill_use=True,
    )
    install_fake_agent(monkeypatch, evaluation)

    results = evaluation.run([item("mandatory-query")])

    baseline = next(
        result for result in results if result.condition is Condition.BASELINE
    )
    treatment = next(
        result for result in results if result.condition is Condition.SKILL
    )
    assert baseline.outcome is RunOutcome.COMPLETED
    assert treatment.outcome is RunOutcome.AGENT_ERROR
    assert "repository query was not observed" in treatment.error
    assert len(scorer.patches) == 1


def test_mandatory_skill_run_with_repository_query_is_scored(
    tmp_path, monkeypatch
) -> None:
    runtime = FakeRuntime(generated_patch="diff --git a/x b/x\n+fixed\n")
    scorer = FakeScorer()
    evaluation, _ = evaluator(
        tmp_path,
        runtime,
        scorer,
        require_skill_use=True,
    )

    def fake_stream(**kwargs):
        if "--skill" in kwargs["command"]:
            kwargs["accumulator"].consume(
                {
                    "type": "tool_execution_start",
                    "toolCallId": "query",
                    "toolName": "bash",
                    "args": {
                        "command": "relay-knowledge repo query symbol --kind definition"
                    },
                },
                1.0,
            )
            kwargs["accumulator"].consume(
                {
                    "type": "tool_execution_end",
                    "toolCallId": "query",
                    "isError": False,
                },
                2.0,
            )
        kwargs["trace_path"].write_text('{"type":"agent_end"}\n', encoding="utf-8")
        return (0, False, False, False, False, "")

    monkeypatch.setattr(evaluation, "_stream_pi", fake_stream)
    monkeypatch.setattr(RepositoryIndexer, "prepare", lambda *args: 4.0)

    results = evaluation.run([item("observed-query")])

    assert all(result.outcome is RunOutcome.COMPLETED for result in results)
    assert len(scorer.patches) == 2


def test_mandatory_skill_run_with_failed_repository_query_is_not_scored(
    tmp_path, monkeypatch
) -> None:
    runtime = FakeRuntime(generated_patch="diff --git a/x b/x\n+fixed\n")
    scorer = FakeScorer()
    evaluation, _ = evaluator(
        tmp_path,
        runtime,
        scorer,
        require_skill_use=True,
    )

    def fake_stream(**kwargs):
        if "--skill" in kwargs["command"]:
            kwargs["accumulator"].consume(
                {
                    "type": "tool_execution_start",
                    "toolCallId": "failed-query",
                    "toolName": "bash",
                    "args": {"command": "relay-knowledge repo query symbol"},
                },
                1.0,
            )
            kwargs["accumulator"].consume(
                {
                    "type": "tool_execution_end",
                    "toolCallId": "failed-query",
                    "isError": True,
                },
                2.0,
            )
        kwargs["trace_path"].write_text('{"type":"agent_end"}\n', encoding="utf-8")
        return (0, False, False, False, False, "")

    monkeypatch.setattr(evaluation, "_stream_pi", fake_stream)
    monkeypatch.setattr(RepositoryIndexer, "prepare", lambda *args: 4.0)

    results = evaluation.run([item("failed-query")])

    treatment = next(
        result for result in results if result.condition is Condition.SKILL
    )
    assert treatment.outcome is RunOutcome.AGENT_ERROR
    assert treatment.tools.relay_commands == {}
    assert len(scorer.patches) == 1


def test_oversized_patch_is_bounded_and_not_scored(tmp_path, monkeypatch) -> None:
    runtime = FakeRuntime(generated_patch="oversized")
    scorer = FakeScorer()
    evaluation, _ = evaluator(tmp_path, runtime, scorer)
    install_fake_agent(monkeypatch, evaluation)
    monkeypatch.setattr(
        "relay_knowledge_skill_eval.runner.PATCH_OUTPUT_BUDGET_BYTES", 4
    )

    results = evaluation.run([item("oversized-patch")])

    assert len(results) == 2
    assert all(result.outcome is RunOutcome.AGENT_ERROR for result in results)
    assert all("64 MiB artifact budget" in result.error for result in results)
    assert scorer.patches == []
    assert runtime.patch_files == {}


def test_resume_expands_suite_without_rerunning_complete_pair(
    tmp_path, monkeypatch
) -> None:
    runtime = FakeRuntime(generated_patch="")
    scorer = FakeScorer()
    evaluation, store = evaluator(tmp_path, runtime, scorer, resume=True)
    for condition in Condition:
        store.append(
            EvalResult(
                instance_id="done",
                condition=condition,
                outcome=RunOutcome.COMPLETED,
            )
        )
    install_fake_agent(monkeypatch, evaluation)
    results = evaluation.run([item("done"), item("new")])
    assert len(results) == 4
    assert runtime.starts == 2
    new_results = [result for result in results if result.instance_id == "new"]
    assert all(not result.swebench.patch_exists for result in new_results)


def test_scoring_failure_is_checkpointed_as_infrastructure_error(
    tmp_path, monkeypatch
) -> None:
    runtime = FakeRuntime()
    evaluation, store = evaluator(tmp_path, runtime, FakeScorer(fail=True))
    install_fake_agent(monkeypatch, evaluation)
    results = evaluation.run([item("broken")])
    assert len(results) == 2
    assert all(result.outcome is RunOutcome.INFRA_ERROR for result in results)
    assert all("fake scorer failure" in result.error for result in results)
    assert len(store.load_results()) == 2
    report = json.loads((tmp_path / "report.json").read_text(encoding="utf-8"))
    assert report["metadata"]["recorded_results"] == 2
    assert report["metadata"]["completed_results"] == 0


def test_image_build_failure_is_checkpointed_without_starting_agents(
    tmp_path, monkeypatch
) -> None:
    runtime = FakeRuntime()
    evaluation, store = evaluator(
        tmp_path,
        runtime,
        FakeScorer(image_fail=True),
    )
    install_fake_agent(monkeypatch, evaluation)

    results = evaluation.run([item("missing-image")])

    assert len(results) == 2
    assert runtime.starts == 0
    assert all(result.outcome is RunOutcome.INFRA_ERROR for result in results)
    assert all("fake image build failure" in result.error for result in results)
    assert len(store.load_results()) == 2


def test_parallel_conditions_reach_agent_boundary_together(
    tmp_path, monkeypatch
) -> None:
    runtime = FakeRuntime()
    evaluation, store = evaluator(
        tmp_path,
        runtime,
        FakeScorer(),
        parallel_conditions=True,
    )
    barrier = threading.Barrier(2, timeout=2)

    def parallel_stream(**kwargs):
        barrier.wait()
        kwargs["trace_path"].write_text("trace\n", encoding="utf-8")
        return (0, False, False, False, False, "")

    monkeypatch.setattr(evaluation, "_stream_pi", parallel_stream)
    monkeypatch.setattr(RepositoryIndexer, "prepare", lambda *args: 0.0)
    results = evaluation.run([item("parallel")])
    assert len(results) == 2
    assert len(store.load_results()) == 2


def test_transport_failure_continues_same_pi_session(tmp_path, monkeypatch) -> None:
    runtime = FakeRuntime()
    evaluation, store = evaluator(tmp_path, runtime, FakeScorer())
    calls: list[dict[str, object]] = []
    attempts_by_container: dict[str, int] = {}

    def interrupted_stream(**kwargs):
        calls.append(kwargs)
        kwargs["trace_path"].write_text("trace\n", encoding="utf-8")
        container = str(kwargs["container"])
        attempts_by_container[container] = attempts_by_container.get(container, 0) + 1
        if attempts_by_container[container] == 1:
            return (1, False, False, True, False, "connection reset")
        return (0, False, False, False, False, "")

    monkeypatch.setattr(evaluation, "_stream_pi", interrupted_stream)
    monkeypatch.setattr(RepositoryIndexer, "prepare", lambda *args: 0.0)
    monkeypatch.setattr("relay_knowledge_skill_eval.runner.time.sleep", lambda _: None)
    results = evaluation.run([item("continued")])
    assert len(results) == 2
    assert len(calls) == 4
    assert all("--continue" not in calls[index]["command"] for index in (0, 2))
    assert all("--continue" in calls[index]["command"] for index in (1, 3))
    assert all(result.tools.harness_continuations == 1 for result in results)
    assert len(store.load_results()) == 2


def test_exhausted_recoverable_transport_failure_is_retried_on_resume(
    tmp_path, monkeypatch
) -> None:
    runtime = FakeRuntime()
    evaluation, store = evaluator(tmp_path, runtime, FakeScorer(), resume=True)

    def interrupted_stream(**kwargs):
        kwargs["trace_path"].write_text("trace\n", encoding="utf-8")
        return (1, False, False, True, False, "connection reset")

    monkeypatch.setattr(evaluation, "_stream_pi", interrupted_stream)
    monkeypatch.setattr(RepositoryIndexer, "prepare", lambda *args: 0.0)
    monkeypatch.setattr("relay_knowledge_skill_eval.runner.time.sleep", lambda _: None)

    failed = evaluation.run([item("transport-outage")])

    assert all(result.outcome is RunOutcome.INFRA_ERROR for result in failed)
    assert all(result.tools.harness_continuations == 3 for result in failed)
    assert runtime.starts == 2

    resumed, _ = evaluator(tmp_path, runtime, FakeScorer(), resume=True)
    install_fake_agent(monkeypatch, resumed)
    completed = resumed.run([item("transport-outage")])

    assert all(result.outcome is RunOutcome.COMPLETED for result in completed)
    assert runtime.starts == 4
    assert all(result.infrastructure_retries == 1 for result in completed)
    assert len(store.load_results()) == 2


def test_provider_configuration_failure_is_retried_after_configuration_fix(
    tmp_path, monkeypatch
) -> None:
    runtime = FakeRuntime()
    evaluation, _ = evaluator(tmp_path, runtime, FakeScorer(), resume=True)

    def invalid_key_stream(**kwargs):
        kwargs["trace_path"].write_text("trace\n", encoding="utf-8")
        return (1, False, False, False, False, "Invalid API key")

    monkeypatch.setattr(evaluation, "_stream_pi", invalid_key_stream)
    monkeypatch.setattr(RepositoryIndexer, "prepare", lambda *args: 0.0)

    failed = evaluation.run([item("provider-config")])

    assert all(result.outcome is RunOutcome.INFRA_ERROR for result in failed)
    assert all("provider configuration" in result.error for result in failed)
    assert runtime.starts == 2

    resumed, _ = evaluator(tmp_path, runtime, FakeScorer(), resume=True)
    install_fake_agent(monkeypatch, resumed)
    completed = resumed.run([item("provider-config")])

    assert all(result.outcome is RunOutcome.COMPLETED for result in completed)
    assert runtime.starts == 4


def test_scorer_outage_does_not_replace_final_agent_timeout(
    tmp_path, monkeypatch
) -> None:
    runtime = FakeRuntime()
    evaluation, store = evaluator(
        tmp_path,
        runtime,
        FakeScorer(fail=True),
        resume=True,
    )
    install_fake_agent(monkeypatch, evaluation, timeout=True)

    results = evaluation.run([item("timed-out-before-scoring")])

    assert all(result.outcome is RunOutcome.TIMED_OUT for result in results)
    assert all("scorer infrastructure error" in result.error for result in results)
    assert runtime.starts == 2

    resumed, _ = evaluator(tmp_path, runtime, FakeScorer(), resume=True)
    install_fake_agent(monkeypatch, resumed)
    assert resumed.run([item("timed-out-before-scoring")]) == results
    assert runtime.starts == 2
    assert len(store.load_results()) == 2


def test_agent_output_budget_is_bounded_and_final(tmp_path, monkeypatch) -> None:
    runtime = FakeRuntime()
    evaluation, store = evaluator(tmp_path, runtime, FakeScorer())

    def output_limited_stream(**kwargs):
        kwargs["trace_path"].write_text("trace\n", encoding="utf-8")
        return (0, False, False, False, True, "")

    monkeypatch.setattr(evaluation, "_stream_pi", output_limited_stream)
    monkeypatch.setattr(RepositoryIndexer, "prepare", lambda *args: 0.0)

    results = evaluation.run([item("verbose")])

    assert PI_STREAM_QUEUE_ITEMS == 256
    assert PI_OUTPUT_BUDGET_BYTES == 64 * 1024 * 1024
    assert all(result.outcome is RunOutcome.AGENT_ERROR for result in results)
    assert all("output budget" in result.error for result in results)
    assert all(result.tools.harness_continuations == 0 for result in results)
    assert len(store.load_results()) == 2


def test_container_agent_process_group_is_terminated_before_host_client(
    tmp_path, monkeypatch
) -> None:
    runtime = FakeRuntime()
    evaluation, _store = evaluator(tmp_path, runtime, FakeScorer())
    calls: list[dict[str, object]] = []

    def fake_run(command, **kwargs):
        calls.append({"command": command, **kwargs})
        return SimpleNamespace(returncode=0)

    monkeypatch.setattr("relay_knowledge_skill_eval.runner.subprocess.run", fake_run)

    evaluation._terminate_container_process("trial-container", "/tmp/pi.pid")

    assert calls[0]["command"][:4] == [
        "docker",
        "exec",
        "trial-container",
        "sh",
    ]
    script = calls[0]["command"][-1]
    assert 'kill -TERM -"$pid"' in script
    assert 'kill -KILL -"$pid"' in script
    assert calls[0]["timeout"] == 15
