from __future__ import annotations

import gzip
import hashlib
import json
import os
import queue
import shlex
import subprocess
import threading
import time
import uuid
from concurrent.futures import ThreadPoolExecutor, as_completed
from contextlib import suppress
from pathlib import Path

from pydantic import BaseModel, ConfigDict

from relay_knowledge_skill_eval.checkpoint import CheckpointStore
from relay_knowledge_skill_eval.docker_runtime import DockerRuntime
from relay_knowledge_skill_eval.indexer import RepositoryIndexer
from relay_knowledge_skill_eval.models import (
    Condition,
    EvalResult,
    RunOutcome,
    SweBenchItem,
    TimingMetrics,
)
from relay_knowledge_skill_eval.pi_events import (
    REPOSITORY_QUERY_COMMANDS,
    PiTraceAccumulator,
)
from relay_knowledge_skill_eval.reporting import write_reports
from relay_knowledge_skill_eval.security import SecretRedactor
from relay_knowledge_skill_eval.swebench_support import SweBenchHarness

PI_STREAM_CHUNK_CHARS = 64 * 1024
PI_STREAM_QUEUE_ITEMS = 256
PI_OUTPUT_BUDGET_BYTES = 64 * 1024 * 1024
PATCH_OUTPUT_BUDGET_BYTES = 64 * 1024 * 1024
PATCH_COLLECTION_TIMEOUT_SECONDS = 300

PROMPT_VERSION = "1"
FORCED_SKILL_PROMPT_VERSION = "2-forced-cli-use"
TOOL_ALLOWLIST = "read,bash,edit,write,grep,find,ls"
PI_SESSION_DIR = "/tmp/pi-eval-sessions"
CONTINUATION_PROMPT = (
    "The previous agent process was interrupted. Continue the same software task "
    "from the current repository and session state. Inspect the work already done, "
    "finish the implementation, run relevant tests, and leave the final patch in "
    "the working tree."
)
TRANSPORT_ERROR_MARKERS = (
    "connection reset",
    "connection refused",
    "connection closed",
    "socket hang up",
    "network error",
    "fetch failed",
    "econnreset",
    "etimedout",
    "request timed out",
    "request timeout",
    "timeout",
    "enotfound",
    "429",
    "502",
    "503",
    "504",
    "rate limit",
    "temporarily unavailable",
)
PROVIDER_CONFIGURATION_ERROR_MARKERS = (
    "invalid api key",
    "authentication failed",
    "unauthorized",
    "model not found",
)
FATAL_AGENT_ERROR_MARKERS = (
    *PROVIDER_CONFIGURATION_ERROR_MARKERS,
    "permission denied",
)
TRANSIENT_PROCESS_EXIT_CODES = frozenset({75, 137, 143})


class PatchOutputLimitError(RuntimeError):
    """The candidate patch exceeded the bounded scoring-artifact budget."""


class EvaluatorConfig(BaseModel):
    model_config = ConfigDict(extra="forbid", arbitrary_types_allowed=True)

    output_dir: Path
    model: str = "deepseek-v4-flash"
    thinking: str = "high"
    agent_timeout_seconds: int = 3600
    index_timeout_seconds: int = 600
    max_continuations: int = 3
    stall_timeout_seconds: int = 600
    concurrency: int = 1
    parallel_conditions: bool = False
    require_skill_use: bool = False
    resume: bool = False
    retry_infrastructure_failures: bool = True
    suite: str = "smoke-10"
    expected_results: int = 0


class SkillEvaluator:
    def __init__(
        self,
        *,
        runtime: DockerRuntime,
        scorer: SweBenchHarness,
        checkpoint: CheckpointStore,
        config: EvaluatorConfig,
        redactor: SecretRedactor,
    ) -> None:
        self._runtime = runtime
        self._scorer = scorer
        self._checkpoint = checkpoint
        self._config = config
        self._redactor = redactor
        self._image_lock = threading.Lock()
        self._results_lock = threading.Lock()
        self._report_lock = threading.Lock()

    def run(self, items: list[SweBenchItem]) -> list[EvalResult]:
        existing = self._checkpoint.load_results()
        pairs = [item for item in items if self._pending_conditions(item, existing)]
        if not pairs:
            return list(existing.values())
        with ThreadPoolExecutor(max_workers=self._config.concurrency) as executor:
            futures = {
                executor.submit(self._run_pair, item, existing): item.instance_id
                for item in pairs
            }
            for future in as_completed(futures):
                try:
                    future.result()
                except Exception as exc:  # paired worker boundary
                    raise RuntimeError(
                        f"Evaluation pair failed for {futures[future]}: {exc}"
                    ) from exc
        return list(existing.values())

    def _pending_conditions(
        self,
        item: SweBenchItem,
        existing: dict[str, EvalResult],
    ) -> tuple[Condition, ...]:
        pending: list[Condition] = []
        for condition in Condition:
            result = existing.get(f"{item.instance_id}:{condition.value}")
            retryable = (
                result is not None
                and result.outcome is RunOutcome.INFRA_ERROR
                and self._config.retry_infrastructure_failures
            )
            if not self._config.resume or result is None or retryable:
                pending.append(condition)
        return tuple(pending)

    def _run_pair(
        self,
        item: SweBenchItem,
        existing: dict[str, EvalResult],
    ) -> list[EvalResult]:
        pending = set(self._pending_conditions(item, existing))
        ordered = stable_condition_order(item.instance_id)
        selected = [condition for condition in ordered if condition in pending]
        image_started = time.monotonic()
        try:
            with self._image_lock:
                image_prepare_seconds = self._scorer.ensure_instance_image(item)
        except Exception as exc:
            shared_prepare = (time.monotonic() - image_started) / max(1, len(selected))
            results: list[EvalResult] = []
            for condition in selected:
                previous = existing.get(f"{item.instance_id}:{condition.value}")
                results.append(
                    EvalResult(
                        instance_id=item.instance_id,
                        condition=condition,
                        attempt=previous.attempt + 1 if previous is not None else 1,
                        infrastructure_retries=(
                            previous.infrastructure_retries + 1
                            if previous is not None
                            and previous.outcome is RunOutcome.INFRA_ERROR
                            else 0
                        ),
                        outcome=RunOutcome.INFRA_ERROR,
                        error=self._redactor.redact(
                            f"SWE-bench instance image preparation failed: {exc}"
                        ),
                        timings=TimingMetrics(
                            image_prepare_seconds=shared_prepare,
                            end_to_end_seconds=shared_prepare,
                        ),
                    )
                )
            for result in results:
                self._checkpoint_result(result, existing)
            return results
        shared_prepare = image_prepare_seconds / max(1, len(pending))
        if self._config.parallel_conditions and len(selected) > 1:
            with ThreadPoolExecutor(max_workers=2) as executor:
                futures = [
                    executor.submit(
                        self._run_and_checkpoint,
                        item,
                        condition,
                        shared_prepare,
                        existing.get(f"{item.instance_id}:{condition.value}"),
                        existing,
                    )
                    for condition in selected
                ]
                return [future.result() for future in futures]
        return [
            self._run_and_checkpoint(
                item,
                condition,
                shared_prepare,
                existing.get(f"{item.instance_id}:{condition.value}"),
                existing,
            )
            for condition in selected
        ]

    def _run_and_checkpoint(
        self,
        item: SweBenchItem,
        condition: Condition,
        image_prepare_seconds: float,
        previous: EvalResult | None,
        existing: dict[str, EvalResult],
    ) -> EvalResult:
        result = self._run_condition(
            item,
            condition,
            image_prepare_seconds=image_prepare_seconds,
            previous=previous,
        )
        self._checkpoint_result(result, existing)
        return result

    def _checkpoint_result(
        self,
        result: EvalResult,
        existing: dict[str, EvalResult],
    ) -> None:
        self._checkpoint.append(result)
        with self._results_lock:
            existing[result.checkpoint_key] = result
            snapshot = list(existing.values())
        with self._report_lock:
            metadata = self._checkpoint.load_meta().model_dump(mode="json")
            metadata.update(
                {
                    "active_suite": self._config.suite,
                    "expected_results": self._config.expected_results,
                    "recorded_results": len(snapshot),
                    "completed_results": sum(
                        result.outcome is not RunOutcome.INFRA_ERROR
                        for result in snapshot
                    ),
                }
            )
            write_reports(snapshot, self._config.output_dir, metadata=metadata)

    def _run_condition(
        self,
        item: SweBenchItem,
        condition: Condition,
        *,
        image_prepare_seconds: float,
        previous: EvalResult | None,
    ) -> EvalResult:
        end_to_end_started = time.monotonic()
        artifact_dir = (
            self._config.output_dir / "artifacts" / item.instance_id / condition.value
        )
        artifact_dir.mkdir(parents=True, exist_ok=True)
        prompt_path = artifact_dir / "prompt.txt"
        trace_path = artifact_dir / "pi-trace.jsonl.gz"
        patch_path = artifact_dir / "generated.patch"
        index_log_path = artifact_dir / "relay-index.jsonl"
        prompt = build_prompt(
            item.problem_statement,
            condition=condition,
            require_skill_use=self._config.require_skill_use,
        )
        prompt_path.write_text(prompt, encoding="utf-8", newline="\n")
        timings = TimingMetrics(image_prepare_seconds=image_prepare_seconds)
        container = ""
        accumulator = PiTraceAccumulator()
        outcome = RunOutcome.COMPLETED
        error = ""
        diagnostics = None
        try:
            container, timings.container_start_seconds = self._runtime.start_instance(
                item.instance_id, condition.value
            )
            self._reset_repository(container, item.base_commit)
            if condition is Condition.SKILL:
                indexer = RepositoryIndexer(
                    self._runtime,
                    timeout_seconds=self._config.index_timeout_seconds,
                    redactor=self._redactor,
                )
                timings.preindex_seconds = indexer.prepare(container, index_log_path)
            agent_started = time.monotonic()
            agent_deadline = agent_started + self._config.agent_timeout_seconds
            continuation_count = 0
            continue_session = False
            stderr_parts: list[str] = []
            while True:
                command = pi_command(
                    condition=condition,
                    model=self._config.model,
                    thinking=self._config.thinking,
                    continue_session=continue_session,
                )
                (
                    returncode,
                    timed_out,
                    stalled,
                    transport_error,
                    output_limited,
                    stderr,
                ) = self._stream_pi(
                    container=container,
                    command=command,
                    prompt=CONTINUATION_PROMPT if continue_session else prompt,
                    trace_path=trace_path,
                    accumulator=accumulator,
                    deadline=agent_deadline,
                    append_trace=continue_session,
                )
                stderr_parts.append(stderr)
                if output_limited:
                    outcome = RunOutcome.AGENT_ERROR
                    error = "Pi agent exceeded the bounded output budget"
                    break
                if timed_out:
                    outcome = RunOutcome.TIMED_OUT
                    error = "Pi agent exceeded the configured timeout"
                    break
                if returncode == 0:
                    break
                if contains_provider_configuration_error(stderr):
                    outcome = RunOutcome.INFRA_ERROR
                    error = (
                        "Pi provider configuration failed before task completion: "
                        f"{stderr[-2000:]}"
                    )
                    break
                recoverable = recoverable_agent_failure(
                    returncode=returncode,
                    stalled=stalled,
                    transport_error=transport_error,
                    stderr=stderr,
                )
                if not recoverable:
                    outcome = RunOutcome.AGENT_ERROR
                    error = (
                        f"Pi agent exited with status {returncode}: "
                        f"{''.join(stderr_parts)[-2000:]}"
                    )
                    break
                if continuation_count >= self._config.max_continuations:
                    outcome = RunOutcome.INFRA_ERROR
                    error = (
                        "Pi agent recoverable transport/process failure persisted "
                        f"after {continuation_count} continuations: "
                        f"{''.join(stderr_parts)[-2000:]}"
                    )
                    break
                continuation_count += 1
                accumulator.tools.harness_continuations += 1
                reason = "stalled output" if stalled else "transport/process error"
                append_trace_marker(
                    trace_path,
                    {
                        "type": "harness_continuation",
                        "continuation": continuation_count,
                        "reason": reason,
                    },
                )
                remaining = agent_deadline - time.monotonic()
                if remaining <= 0:
                    outcome = RunOutcome.TIMED_OUT
                    error = "Pi agent exceeded the configured timeout"
                    break
                time.sleep(min(5 * (2 ** (continuation_count - 1)), 30, remaining))
                continue_session = True
            timings.agent_seconds = time.monotonic() - agent_started
            try:
                generated_patch = self._collect_patch(container, item.base_commit)
            except PatchOutputLimitError as exc:
                outcome = RunOutcome.AGENT_ERROR
                error = str(exc)
                generated_patch = ""
                patch_path.write_text("", encoding="utf-8", newline="\n")
            else:
                patch_path.write_text(
                    self._redactor.redact(generated_patch),
                    encoding="utf-8",
                    newline="\n",
                )
                mandatory_query_observed = any(
                    accumulator.tools.relay_commands.get(kind, 0) > 0
                    for kind in REPOSITORY_QUERY_COMMANDS
                )
                if (
                    outcome is RunOutcome.COMPLETED
                    and condition is Condition.SKILL
                    and self._config.require_skill_use
                    and not mandatory_query_observed
                ):
                    outcome = RunOutcome.AGENT_ERROR
                    error = (
                        "Mandatory relay-knowledge CLI repository query was not "
                        "observed; treatment result was not scored"
                    )
                else:
                    diagnostics, timings.scoring_seconds = self._scorer.score(
                        item=item,
                        condition=condition.value,
                        generated_patch=generated_patch,
                    )
        except Exception as exc:  # persist the failed attempt for diagnosis/resume
            if outcome in {RunOutcome.TIMED_OUT, RunOutcome.AGENT_ERROR} and (
                patch_path.exists()
            ):
                error = f"{error}; scorer infrastructure error: {exc}"
            else:
                outcome = RunOutcome.INFRA_ERROR
                error = str(exc)
                if not patch_path.exists():
                    patch_path.write_text("", encoding="utf-8")
        finally:
            self._runtime.remove_container(container)
        timings.end_to_end_seconds = time.monotonic() - end_to_end_started
        return EvalResult(
            instance_id=item.instance_id,
            condition=condition,
            attempt=previous.attempt + 1 if previous is not None else 1,
            infrastructure_retries=(
                previous.infrastructure_retries + 1
                if previous is not None and previous.outcome is RunOutcome.INFRA_ERROR
                else 0
            ),
            outcome=outcome,
            error=self._redactor.redact(error),
            prompt_path=str(prompt_path),
            trace_path=str(trace_path),
            patch_path=str(patch_path),
            index_log_path=str(index_log_path) if condition is Condition.SKILL else "",
            tokens=accumulator.tokens,
            tools=accumulator.tools,
            timings=timings,
            swebench=diagnostics if diagnostics is not None else {},
        )

    def _reset_repository(self, container: str, base_commit: str) -> None:
        self._runtime.exec(container, ["git", "reset", "--hard", base_commit])
        self._runtime.exec(container, ["git", "clean", "-fdx"])
        result = self._runtime.exec(container, ["git", "rev-parse", "HEAD"])
        if result.stdout.strip() != base_commit:
            raise RuntimeError("SWE-bench container did not reset to the base commit")

    def _collect_patch(self, container: str, base_commit: str) -> str:
        patch_file = f"/tmp/relay-skill-eval-{uuid.uuid4().hex}.patch"
        self._runtime.exec(
            container,
            ["git", "add", "-N", "--all"],
            check=False,
            timeout=PATCH_COLLECTION_TIMEOUT_SECONDS,
        )
        try:
            generated = self._runtime.exec(
                container,
                [
                    "bash",
                    "-c",
                    (
                        'git diff --binary --no-ext-diff "$1" '
                        '| head -c "$3" > "$2"; '
                        "diff_status=${PIPESTATUS[0]}; "
                        'if [ "$diff_status" -ne 0 ] && '
                        '[ "$diff_status" -ne 141 ]; then exit "$diff_status"; fi'
                    ),
                    "relay-skill-eval-patch",
                    base_commit,
                    patch_file,
                    str(PATCH_OUTPUT_BUDGET_BYTES + 1),
                ],
                check=False,
                timeout=PATCH_COLLECTION_TIMEOUT_SECONDS,
            )
            if generated.returncode != 0:
                raise RuntimeError(
                    "Candidate patch collection failed: " + generated.stderr[-2000:]
                )
            size_result = self._runtime.exec(
                container,
                ["wc", "-c", patch_file],
                timeout=PATCH_COLLECTION_TIMEOUT_SECONDS,
            )
            try:
                patch_bytes = int(size_result.stdout.split()[0])
            except (IndexError, ValueError) as exc:
                raise RuntimeError("Candidate patch size was not parseable") from exc
            if patch_bytes > PATCH_OUTPUT_BUDGET_BYTES:
                raise PatchOutputLimitError(
                    "Candidate patch exceeded the bounded 64 MiB artifact budget"
                )
            return self._runtime.exec(
                container,
                ["cat", patch_file],
                timeout=PATCH_COLLECTION_TIMEOUT_SECONDS,
            ).stdout
        finally:
            self._runtime.exec(
                container,
                ["rm", "-f", patch_file],
                check=False,
                timeout=30,
            )

    def _stream_pi(
        self,
        *,
        container: str,
        command: list[str],
        prompt: str,
        trace_path: Path,
        accumulator: PiTraceAccumulator,
        deadline: float,
        append_trace: bool,
    ) -> tuple[int, bool, bool, bool, bool, str]:
        pid_file = f"/tmp/pi-eval-{uuid.uuid4().hex}.pid"
        container_command = (
            f"echo $$ > {shlex.quote(pid_file)}; "
            f"trap 'rm -f {shlex.quote(pid_file)}' EXIT; "
            f"{shlex.join(command)}"
        )
        docker_command = pi_docker_exec_command(container, container_command)
        process = subprocess.Popen(
            docker_command,
            stdin=subprocess.PIPE,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            text=True,
            encoding="utf-8",
            errors="replace",
            env=os.environ.copy(),
        )
        stream: queue.Queue[tuple[str, str | None, float]] = queue.Queue(
            maxsize=PI_STREAM_QUEUE_ITEMS
        )

        def drain(label: str, handle: object) -> None:
            while True:
                chunk = handle.readline(PI_STREAM_CHUNK_CHARS)
                if not chunk:
                    break
                stream.put((label, chunk, time.monotonic()))
            stream.put((label, None, time.monotonic()))

        stdout_thread = threading.Thread(
            target=drain, args=("stdout", process.stdout), daemon=True
        )
        stderr_thread = threading.Thread(
            target=drain, args=("stderr", process.stderr), daemon=True
        )
        stdout_thread.start()
        stderr_thread.start()
        if process.stdin is None:
            raise RuntimeError("Pi stdin pipe is unavailable")
        process.stdin.write(prompt)
        process.stdin.close()
        last_output = time.monotonic()
        ended_streams: set[str] = set()
        stderr_parts: list[str] = []
        timed_out = False
        stalled = False
        transport_error = False
        output_limited = False
        output_bytes = 0
        pending = {"stdout": "", "stderr": ""}
        trace_path.parent.mkdir(parents=True, exist_ok=True)
        trace_mode = "at" if append_trace and trace_path.exists() else "wt"

        def consume_line(label: str, line: str, observed_at: float) -> None:
            nonlocal last_output, transport_error
            last_output = observed_at
            redacted = self._redactor.redact(line)
            transport_error = transport_error or stream_has_transport_error(
                label, redacted
            )
            if label == "stdout":
                trace.write(redacted.rstrip("\r\n") + "\n")
                trace.flush()
                accumulator.consume_line(redacted, observed_at)
            else:
                stderr_parts.append(redacted)

        with gzip.open(trace_path, trace_mode, encoding="utf-8", newline="\n") as trace:
            while len(ended_streams) < 2:
                now = time.monotonic()
                remaining = deadline - now
                if remaining <= 0 and process.poll() is None and not timed_out:
                    timed_out = True
                    self._terminate_container_process(container, pid_file)
                    process.kill()
                elif (
                    now - last_output >= self._config.stall_timeout_seconds
                    and process.poll() is None
                    and not stalled
                ):
                    stalled = True
                    self._terminate_container_process(container, pid_file)
                    process.kill()
                try:
                    label, chunk, observed_at = stream.get(timeout=0.1)
                except queue.Empty:
                    if process.poll() is not None and not any(
                        thread.is_alive() for thread in (stdout_thread, stderr_thread)
                    ):
                        break
                    continue
                if chunk is None:
                    if pending[label]:
                        consume_line(label, pending[label], observed_at)
                        pending[label] = ""
                    ended_streams.add(label)
                    continue
                output_bytes += len(chunk.encode("utf-8", errors="replace"))
                if output_bytes > PI_OUTPUT_BUDGET_BYTES and not output_limited:
                    output_limited = True
                    self._terminate_container_process(container, pid_file)
                    process.kill()
                if output_limited:
                    continue
                pending[label] += chunk
                while "\n" in pending[label]:
                    line, pending[label] = pending[label].split("\n", 1)
                    consume_line(label, line + "\n", observed_at)
        try:
            returncode = process.wait(timeout=10)
        except subprocess.TimeoutExpired:
            self._terminate_container_process(container, pid_file)
            process.kill()
            returncode = process.wait()
            timed_out = True
        return (
            returncode,
            timed_out,
            stalled,
            transport_error,
            output_limited,
            "".join(stderr_parts),
        )

    def _terminate_container_process(self, container: str, pid_file: str) -> None:
        script = (
            f"if [ -s {shlex.quote(pid_file)} ]; then "
            f"pid=$(cat {shlex.quote(pid_file)}); "
            'kill -TERM -"$pid" 2>/dev/null || kill -TERM "$pid" 2>/dev/null || true; '
            "sleep 1; "
            'kill -KILL -"$pid" 2>/dev/null || kill -KILL "$pid" 2>/dev/null || true; '
            f"rm -f {shlex.quote(pid_file)}; fi"
        )
        # The container lifecycle cleanup remains the final safety boundary.
        with suppress(OSError, subprocess.TimeoutExpired):
            subprocess.run(
                ["docker", "exec", container, "sh", "-c", script],
                stdin=subprocess.DEVNULL,
                stdout=subprocess.DEVNULL,
                stderr=subprocess.DEVNULL,
                timeout=15,
                check=False,
            )


def stable_condition_order(instance_id: str) -> tuple[Condition, Condition]:
    digest = hashlib.sha256(instance_id.encode("utf-8")).digest()
    if digest[0] % 2 == 0:
        return (Condition.BASELINE, Condition.SKILL)
    return (Condition.SKILL, Condition.BASELINE)


def pi_docker_exec_command(container: str, shell_command: str) -> list[str]:
    """Build an unambiguous docker-exec boundary rooted at the task worktree."""
    if not container:
        raise ValueError("Pi docker exec requires a container name")
    return [
        "docker",
        "exec",
        "-i",
        "-w",
        "/testbed",
        container,
        "setsid",
        "sh",
        "-c",
        shell_command,
    ]


def build_prompt(
    problem_statement: str,
    *,
    condition: Condition = Condition.BASELINE,
    require_skill_use: bool = False,
) -> str:
    instructions = [
        "Work only in /testbed and solve the reported software issue.",
        "Inspect the repository, implement a general fix, and run relevant tests.",
        "Do not search for or use a reference patch, test patch, or gold answer.",
        "Leave the final implementation in the working tree for evaluation.",
    ]
    if condition is Condition.SKILL and require_skill_use:
        instructions.insert(
            1,
            "You must use the loaded relay-knowledge-cli skill before editing: "
            "follow its SKILL.md workflow and execute its bundled relay-knowledge "
            "CLI to query relevant repository definitions, references, callers, "
            "dependencies, or context. This requirement is mandatory.",
        )
    payload = {
        "task": problem_statement,
        "instructions": instructions,
    }
    return json.dumps(payload, ensure_ascii=False, indent=2) + "\n"


def contains_transport_error(text: str) -> bool:
    normalized = text.lower()
    return any(marker in normalized for marker in TRANSPORT_ERROR_MARKERS)


def stream_has_transport_error(label: str, text: str) -> bool:
    """Recognize provider transport failures without scanning ordinary stdout."""
    if label == "stderr":
        return contains_transport_error(text)
    if label != "stdout" or not contains_transport_error(text):
        return False
    try:
        payload = json.loads(text)
    except (TypeError, json.JSONDecodeError):
        return False
    return _contains_explicit_error_event(payload)


def _contains_explicit_error_event(value: object) -> bool:
    if isinstance(value, dict):
        for key in ("type", "event", "kind"):
            marker = value.get(key)
            if isinstance(marker, str):
                normalized = marker.strip().lower().replace("-", "_")
                if normalized == "error" or normalized.endswith("_error"):
                    return True
        return any(_contains_explicit_error_event(item) for item in value.values())
    if isinstance(value, list):
        return any(_contains_explicit_error_event(item) for item in value)
    return False


def contains_provider_configuration_error(text: str) -> bool:
    normalized = text.lower()
    return any(marker in normalized for marker in PROVIDER_CONFIGURATION_ERROR_MARKERS)


def recoverable_agent_failure(
    *,
    returncode: int,
    stalled: bool,
    transport_error: bool,
    stderr: str,
) -> bool:
    normalized = stderr.lower()
    if any(marker in normalized for marker in FATAL_AGENT_ERROR_MARKERS):
        return False
    return stalled or transport_error or returncode in TRANSIENT_PROCESS_EXIT_CODES


def append_trace_marker(trace_path: Path, payload: dict[str, object]) -> None:
    trace_path.parent.mkdir(parents=True, exist_ok=True)
    with gzip.open(trace_path, "at", encoding="utf-8", newline="\n") as trace:
        trace.write(json.dumps(payload, ensure_ascii=False) + "\n")


def pi_command(
    *,
    condition: Condition,
    model: str,
    thinking: str,
    continue_session: bool = False,
) -> list[str]:
    pi_arguments = [
        "/opt/pi-eval/bin/pi-eval",
        "--mode",
        "json",
        "--session-dir",
        PI_SESSION_DIR,
        "--provider",
        "deepseek",
        "--model",
        model,
        "--thinking",
        thinking,
        "--tools",
        TOOL_ALLOWLIST,
        "--no-extensions",
        "--no-skills",
        "--no-prompt-templates",
        "--no-themes",
        "--no-context-files",
        "--approve",
    ]
    if continue_session:
        pi_arguments.append("--continue")
    if condition is Condition.SKILL:
        pi_arguments.extend(["--skill", "/opt/pi-eval/skill/SKILL.md"])
    return [
        "bash",
        "-lc",
        'prompt="$(cat)"; exec "$@" "$prompt"',
        "pi-eval-message",
        *pi_arguments,
    ]
