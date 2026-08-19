from __future__ import annotations

import asyncio
import json
import shlex
import time
from pathlib import Path
from typing import Any

from pier.agents.base import BaseAgent
from pier.environments.base import BaseEnvironment
from pier.environments.docker.docker import DockerEnvironment
from pier.models.agent.context import AgentContext
from pier.models.agent.network import NetworkAllowlist

from relay_knowledge_skill_eval.docker_runtime import merge_relay_environment
from relay_knowledge_skill_eval.indexer import parse_index_progress
from relay_knowledge_skill_eval.models import Condition, TokenUsage, ToolUsage
from relay_knowledge_skill_eval.pi_events import (
    REPOSITORY_QUERY_COMMANDS,
    recover_truncated_trace_usage,
)
from relay_knowledge_skill_eval.security import SecretRedactor

PI_RUNTIME_PATH = "/opt/pi-eval"
PI_TRACE_PATH = "/logs/agent/pi-trace.jsonl.gz"
PI_PROCESS_GROUP_FILE = "/tmp/pi-deepswe-agent.pgid"
DEEP_SWE_OUTPUT_BUDGET_BYTES = 64 * 1024 * 1024
RELAY_BINARY = "/opt/pi-eval/skill/assets/linux-x86_64/relay-knowledge"
STDERR_REDACTOR_PATH = "/logs/agent/stderr-redactor.mjs"
RELAY_ENV = {
    "RELAY_KNOWLEDGE_HOME": "/tmp/relay-knowledge-home",
    "RELAY_KNOWLEDGE_SEMANTIC_BACKEND": "local",
    "RELAY_KNOWLEDGE_VECTOR_BACKEND": "local",
}
TRANSPORT_MARKERS = (
    "connection reset",
    "connection refused",
    "connection aborted",
    "network error",
    "socket hang up",
    "timed out",
    "timeout",
    "econnreset",
    "econnrefused",
    "fetch failed",
    "tls",
    "http 429",
    "status 429",
    "http 5",
    "status 5",
)
PROVIDER_CONFIGURATION_MARKERS = (
    "invalid api key",
    "authentication failed",
    "unauthorized",
    "unknown model",
    "model not found",
)
TRANSIENT_PROCESS_EXIT_CODES = frozenset({75, 125, 137, 143})

TRACE_CAPTURE_SCRIPT = r"""
import fs from "node:fs";
import zlib from "node:zlib";
import { once } from "node:events";

const [tracePath, summaryPath] = process.argv.slice(2);
const maxOutputBytes = 64 * 1024 * 1024;
const apiKey = process.env.DEEPSEEK_API_KEY || "";
const redact = (value) => {
  let output = String(value);
  if (apiKey) output = output.split(apiKey).join("[REDACTED]");
  return output.replace(/sk-[A-Za-z0-9_-]{20,}/g, "[REDACTED]");
};
const output = fs.createWriteStream(tracePath, { flags: "w" });
const gzip = zlib.createGzip({ level: 6 });
gzip.pipe(output);

const tokens = {
  input:0, output:0, reasoning:0, cache_read:0, cache_write:0,
  total:0, cost_usd:0, requests:0
};
const tools = {
  calls:0, errors:0, cumulative_seconds:0, by_name:{},
  relay_commands:{}, auto_retries:0
};
const starts = new Map();
const pendingRelayCommands = new Map();
const messages = new Set();
const relayKinds = [
  "repo list", "repo register", "repo index-worker", "repo index",
  "repo status", "repo query", "repo context", "repo software",
  "repo feature-flags", "repo impact"
];
let inputBytes = 0;
let outputBytes = 0;
let outputLimited = false;
let lineBytes = 0;
let lineSegments = [];

const persistLine = (lineBuffer) => {
  const line = lineBuffer.toString("utf8").replace(/\r$/, "");
  const persisted = redact(line) + "\n";
  const persistedBytes = Buffer.byteLength(persisted, "utf8");
  if (outputBytes + persistedBytes > maxOutputBytes) return false;
  outputBytes += persistedBytes;
  gzip.write(persisted);
  let event;
  try { event = JSON.parse(line); } catch { return true; }
  if (event.type === "message_end" && event.message?.role === "assistant") {
    const usage = event.message.usage || {};
    const identity = JSON.stringify([
      event.message.timestamp, event.message.model, usage.totalTokens,
      usage.input, usage.output
    ]);
    if (!messages.has(identity)) {
      messages.add(identity);
      tokens.input += Number(usage.input || 0);
      tokens.output += Number(usage.output || 0);
      tokens.reasoning += Number(usage.reasoning || 0);
      tokens.cache_read += Number(usage.cacheRead || 0);
      tokens.cache_write += Number(usage.cacheWrite || 0);
      tokens.total += Number(usage.totalTokens || 0);
      tokens.cost_usd += Number(usage.cost?.total || 0);
      tokens.requests += 1;
    }
  } else if (event.type === "tool_execution_start") {
    const name = String(event.toolName || "unknown");
    tools.calls += 1;
    tools.by_name[name] = (tools.by_name[name] || 0) + 1;
    const key = String(event.toolCallId || "");
    starts.set(key, Date.now());
    const command = JSON.stringify(event.args || {}).replace(/\s+/g, " ");
    if (command.includes("relay-knowledge")) {
      const kind = relayKinds.find((value) => command.includes(value)) || "other";
      pendingRelayCommands.set(key, kind);
    }
  } else if (event.type === "tool_execution_end") {
    const key = String(event.toolCallId || "");
    if (starts.has(key)) {
      tools.cumulative_seconds += Math.max(0, (Date.now() - starts.get(key)) / 1000);
      starts.delete(key);
    }
    const relayKind = pendingRelayCommands.get(key);
    pendingRelayCommands.delete(key);
    if (relayKind && event.isError !== true) {
      tools.relay_commands[relayKind] = (tools.relay_commands[relayKind] || 0) + 1;
    }
    if (event.isError === true) tools.errors += 1;
  } else if (event.type === "auto_retry_start") {
    tools.auto_retries += 1;
  }
  return true;
};

inputLoop:
for await (const chunkValue of process.stdin) {
  const chunk = Buffer.isBuffer(chunkValue) ? chunkValue : Buffer.from(chunkValue);
  let offset = 0;
  while (offset < chunk.length) {
    const remaining = maxOutputBytes - inputBytes;
    if (remaining <= 0) {
      outputLimited = true;
      break inputLoop;
    }
    const scanEnd = Math.min(chunk.length, offset + remaining);
    const newline = chunk.indexOf(0x0a, offset);
    if (newline >= 0 && newline < scanEnd) {
      const segment = chunk.subarray(offset, newline);
      if (segment.length > 0) lineSegments.push(segment);
      lineBytes += segment.length;
      inputBytes += segment.length + 1;
      const lineBuffer = Buffer.concat(lineSegments, lineBytes);
      lineSegments = [];
      lineBytes = 0;
      if (!persistLine(lineBuffer)) {
        outputLimited = true;
        break inputLoop;
      }
      offset = newline + 1;
      continue;
    }
    const segment = chunk.subarray(offset, scanEnd);
    if (segment.length > 0) lineSegments.push(segment);
    lineBytes += segment.length;
    inputBytes += segment.length;
    offset = scanEnd;
    if (offset < chunk.length) {
      outputLimited = true;
      break inputLoop;
    }
  }
}
if (!outputLimited && lineBytes > 0) {
  if (!persistLine(Buffer.concat(lineSegments, lineBytes))) outputLimited = true;
}

gzip.end();
await once(output, "close");
fs.writeFileSync(
  summaryPath,
  JSON.stringify({tokens, tools, output_limited:outputLimited}) + "\n",
  "utf8"
);
if (outputLimited) process.exitCode = 75;
""".strip()

STDERR_REDACTOR_SCRIPT = r"""
import fs from "node:fs";
import { once } from "node:events";

const [limitMarkerPath] = process.argv.slice(2);
const maxOutputBytes = 64 * 1024 * 1024;
const apiKey = process.env.DEEPSEEK_API_KEY || "";
const redact = (value) => {
  let output = String(value);
  if (apiKey) output = output.split(apiKey).join("[REDACTED]");
  return output.replace(/sk-[A-Za-z0-9_-]{20,}/g, "[REDACTED]");
};
let inputBytes = 0;
let lineBytes = 0;
let lineSegments = [];
let outputLimited = false;

const writeLine = async (lineBuffer) => {
  const line = lineBuffer.toString("utf8").replace(/\r$/, "");
  if (!process.stdout.write(redact(line) + "\n")) {
    await once(process.stdout, "drain");
  }
};

inputLoop:
for await (const chunkValue of process.stdin) {
  const chunk = Buffer.isBuffer(chunkValue) ? chunkValue : Buffer.from(chunkValue);
  let offset = 0;
  while (offset < chunk.length) {
    const remaining = maxOutputBytes - inputBytes;
    if (remaining <= 0) {
      outputLimited = true;
      break inputLoop;
    }
    const scanEnd = Math.min(chunk.length, offset + remaining);
    const newline = chunk.indexOf(0x0a, offset);
    if (newline >= 0 && newline < scanEnd) {
      const segment = chunk.subarray(offset, newline);
      if (segment.length > 0) lineSegments.push(segment);
      lineBytes += segment.length;
      inputBytes += segment.length + 1;
      await writeLine(Buffer.concat(lineSegments, lineBytes));
      lineSegments = [];
      lineBytes = 0;
      offset = newline + 1;
      continue;
    }
    const segment = chunk.subarray(offset, scanEnd);
    if (segment.length > 0) lineSegments.push(segment);
    lineBytes += segment.length;
    inputBytes += segment.length;
    offset = scanEnd;
    if (offset < chunk.length) {
      outputLimited = true;
      break inputLoop;
    }
  }
}
if (lineBytes > 0 && !outputLimited) {
  await writeLine(Buffer.concat(lineSegments, lineBytes));
}
if (outputLimited) {
  fs.writeFileSync(limitMarkerPath, "limited\n", "utf8");
  process.exitCode = 75;
}
""".strip()

DEEPSEEK_ENVIRONMENT_COMPOSE = {
    "services": {"main": {"environment": {"DEEPSEEK_API_KEY": "${DEEPSEEK_API_KEY}"}}}
}


class DeepSweTransportError(RuntimeError):
    """Pi stopped after bounded transport/process continuation attempts."""


class DeepSweConfigurationError(RuntimeError):
    """Pi provider credentials or model configuration are invalid."""


class DeepSweOutputLimitError(RuntimeError):
    """Pi exceeded the bounded persisted-output budget."""


class DeepSweDockerEnvironment(DockerEnvironment):
    """Pier Docker environment with the immutable Pi/skill runtime mounted."""

    def __init__(self, *args: object, runtime_image: str, **kwargs: object) -> None:
        if not runtime_image.startswith("relay-knowledge-skill-eval:"):
            raise ValueError("DeepSWE runtime image has an unexpected identity")
        self._runtime_image = runtime_image
        super().__init__(*args, **kwargs)
        self._agent_environment_compose_path = (
            self.trial_paths.agent_dir / "docker-compose-agent-environment.json"
        )
        self._agent_environment_compose_path.parent.mkdir(parents=True, exist_ok=True)
        self._agent_environment_compose_path.write_text(
            json.dumps(DEEPSEEK_ENVIRONMENT_COMPOSE, indent=2) + "\n",
            encoding="utf-8",
        )

    @property
    def _docker_compose_paths(self) -> list[Path]:
        return [*super()._docker_compose_paths, self._agent_environment_compose_path]

    def _default_log_mounts(self) -> list[dict[str, Any]]:
        mounts: list[dict[str, Any]] = list(super()._default_log_mounts())
        mounts.append(
            {
                "type": "image",
                "source": self._runtime_image,
                "target": PI_RUNTIME_PATH,
                "read_only": True,
                "image": {"subpath": "opt/pi-eval"},
            }
        )
        return mounts


class PiDeepSweAgent(BaseAgent):
    """Run pinned Pi inside an official DeepSWE Pier task environment."""

    def __init__(
        self,
        *args: object,
        condition: str,
        require_skill_use: bool = True,
        thinking: str = "high",
        agent_timeout_seconds: int = 3600,
        index_timeout_seconds: int = 900,
        max_continuations: int = 3,
        extra_env: dict[str, str] | None = None,
        **kwargs: object,
    ) -> None:
        super().__init__(*args, **kwargs)
        self.condition = Condition(condition)
        self.require_skill_use = require_skill_use
        self.thinking = thinking
        self.agent_timeout_seconds = agent_timeout_seconds
        self.index_timeout_seconds = index_timeout_seconds
        self.max_continuations = max_continuations
        self.extra_env = extra_env or {}
        self.redactor = SecretRedactor(self.extra_env.get("DEEPSEEK_API_KEY", ""))
        self.preindex_seconds = 0.0

    @staticmethod
    def name() -> str:
        return "pi-deepswe"

    def version(self) -> str:
        return "0.80.3"

    def network_allowlist(self) -> NetworkAllowlist:
        return NetworkAllowlist(domains=["api.deepseek.com"])

    async def setup(self, environment: BaseEnvironment) -> None:
        check = await environment.exec(
            "test -x /opt/pi-eval/bin/pi-eval && "
            "test -x /opt/pi-eval/skill/assets/linux-x86_64/relay-knowledge",
            timeout_sec=30,
            user="root",
        )
        if check.return_code != 0:
            raise RuntimeError("Pinned Pi/skill runtime image mount is unavailable")
        await environment.exec(
            "git config --global --add safe.directory /app && "
            "git config --global user.email pi-eval@localhost && "
            "git config --global user.name 'Pi DeepSWE Eval'",
            cwd="/app",
            timeout_sec=30,
        )
        if self.condition is Condition.SKILL:
            self.preindex_seconds = await self._preindex(environment)

    async def run(
        self,
        instruction: str,
        environment: BaseEnvironment,
        context: AgentContext,
    ) -> None:
        started = time.monotonic()
        self._harness_continuations = 0
        try:
            await self._run_agent(instruction, environment, started=started)
        finally:
            self._populate_context(context, started=started)

    async def _run_agent(
        self,
        instruction: str,
        environment: BaseEnvironment,
        *,
        started: float,
    ) -> None:
        self.logs_dir.mkdir(parents=True, exist_ok=True)
        prompt = _build_deep_swe_prompt(
            instruction,
            condition=self.condition,
            require_skill_use=self.require_skill_use,
        )
        (self.logs_dir / "prompt.txt").write_text(prompt, encoding="utf-8")
        (self.logs_dir / "trace-capture.mjs").write_text(
            TRACE_CAPTURE_SCRIPT + "\n", encoding="utf-8"
        )
        (self.logs_dir / "stderr-redactor.mjs").write_text(
            STDERR_REDACTOR_SCRIPT + "\n", encoding="utf-8"
        )
        stderr_parts: list[str] = []
        mandatory_query_observed = False
        for attempt in range(self.max_continuations + 1):
            remaining = self.agent_timeout_seconds - (time.monotonic() - started)
            if remaining <= 0:
                await _preserve_timed_out_work(environment)
                raise TimeoutError("Pi agent exceeded the configured timeout")
            is_continuation = attempt > 0
            attempt_trace = self.logs_dir / f"pi-trace-{attempt + 1:02d}.jsonl.gz"
            summary_path = self.logs_dir / f"pi-summary-{attempt + 1:02d}.json"
            stderr_path = self.logs_dir / f"pi-stderr-{attempt + 1:02d}.log"
            stderr_limit_path = self.logs_dir / "stderr-output-limited"
            prompt_path = (
                self.logs_dir / "continuation.txt"
                if is_continuation
                else self.logs_dir / "prompt.txt"
            )
            if is_continuation:
                prompt_path.write_text(
                    "Continue the same task from the saved session. Inspect the "
                    "current worktree, finish the implementation, and run relevant "
                    "tests.\n",
                    encoding="utf-8",
                )
            command = _pi_shell_command(
                condition=self.condition,
                model=self._parsed_model_name or "deepseek-v4-flash",
                thinking=self.thinking,
                prompt_path=f"/logs/agent/{prompt_path.name}",
                trace_path=f"/logs/agent/{attempt_trace.name}",
                summary_path=f"/logs/agent/{summary_path.name}",
                stderr_path=f"/logs/agent/{stderr_path.name}",
                continue_session=is_continuation,
            )
            stderr_limit_path.unlink(missing_ok=True)
            try:
                result = await environment.exec(
                    command,
                    cwd="/app",
                    env=environment.agent_process_env(
                        _agent_exec_environment(self.extra_env)
                    ),
                    timeout_sec=max(1, int(remaining)),
                )
            except RuntimeError as error:
                if "command timed out after" not in str(error).lower():
                    raise
                # Pier stops only the host-side compose process on deadline.
                # Stop the isolated in-container Pi process group before
                # snapshotting its partial work for the official collector.
                await _preserve_timed_out_work(environment)
                raise TimeoutError(
                    "Pi agent exceeded the configured timeout"
                ) from error
            if stderr_limit_path.exists() or (
                stderr_path.exists()
                and stderr_path.stat().st_size >= DEEP_SWE_OUTPUT_BUDGET_BYTES
            ):
                await _commit_worktree(environment)
                raise DeepSweOutputLimitError(
                    "Pi stderr exceeded the bounded output budget"
                )
            if not stderr_path.exists():
                exec_output = self.redactor.redact(
                    (result.stderr or "") + (result.stdout or "")
                )
                raise DeepSweTransportError(
                    "Pi exec did not create its stderr artifact: " + exec_output[-2000:]
                )
            stderr = stderr_path.read_text(encoding="utf-8", errors="replace")
            stderr_parts.append(self.redactor.redact(stderr))
            if attempt_trace.exists():
                with (self.logs_dir / "pi-trace.jsonl.gz").open("ab") as output:
                    output.write(attempt_trace.read_bytes())
            summary: dict[str, object] = {}
            if summary_path.exists():
                summary = json.loads(summary_path.read_text(encoding="utf-8"))
                if summary.get("output_limited") is True:
                    await _commit_worktree(environment)
                    raise DeepSweOutputLimitError(
                        "Pi trace exceeded the bounded output budget"
                    )
            relay_commands = summary.get("tools", {})
            relay_commands = (
                relay_commands.get("relay_commands", {})
                if isinstance(relay_commands, dict)
                else {}
            )
            mandatory_query_observed = mandatory_query_observed or any(
                int(relay_commands.get(kind, 0) or 0) > 0
                for kind in REPOSITORY_QUERY_COMMANDS
            )
            if result.return_code == 0:
                if (
                    self.condition is Condition.SKILL
                    and self.require_skill_use
                    and not mandatory_query_observed
                ):
                    await _commit_worktree(environment)
                    raise RuntimeError(
                        "Mandatory relay-knowledge CLI repository query was not "
                        "observed"
                    )
                break
            if _provider_configuration_error(stderr):
                raise DeepSweConfigurationError(
                    "Pi provider configuration failed before task completion: "
                    + self.redactor.redact(stderr)[-2000:]
                )
            recoverable = _recoverable(result.return_code, stderr)
            if attempt >= self.max_continuations or not recoverable:
                combined_stderr = self.redactor.redact("".join(stderr_parts))
                if recoverable:
                    raise DeepSweTransportError(
                        "Pi transport/process failed after continuation attempts: "
                        + combined_stderr[-2000:]
                    )
                raise RuntimeError(
                    "Pi agent failed after continuation attempts: "
                    + combined_stderr[-2000:]
                )
            self._harness_continuations += 1
            remaining = self.agent_timeout_seconds - (time.monotonic() - started)
            if remaining <= 0:
                await _preserve_timed_out_work(environment)
                raise TimeoutError("Pi agent exceeded the configured timeout")
            await asyncio.sleep(min(5 * (2**attempt), 30, remaining))
            if time.monotonic() - started >= self.agent_timeout_seconds:
                await _preserve_timed_out_work(environment)
                raise TimeoutError("Pi agent exceeded the configured timeout")

        await _commit_worktree(environment)

    def _populate_context(self, context: AgentContext, *, started: float) -> None:
        summaries: list[dict[str, object]] = []
        summarized_attempts: set[str] = set()
        for summary_path in sorted(self.logs_dir.glob("pi-summary-*.json")):
            try:
                summary = json.loads(summary_path.read_text(encoding="utf-8"))
                if not isinstance(summary, dict):
                    continue
                _merge_summaries([summary])
            except (OSError, TypeError, ValueError):
                continue
            summaries.append(summary)
            summarized_attempts.add(summary_path.stem.removeprefix("pi-summary-"))
        missing_summaries = [
            trace_path
            for trace_path in sorted(self.logs_dir.glob("pi-trace-[0-9][0-9].jsonl.gz"))
            if trace_path.name.removeprefix("pi-trace-").removesuffix(".jsonl.gz")
            not in summarized_attempts
        ]
        recovered_metrics = False
        if missing_summaries:
            recovered_tokens, recovered_tools = recover_truncated_trace_usage(
                missing_summaries
            )
            if recovered_tokens.requests or recovered_tools.calls:
                summaries.append(
                    {
                        "tokens": recovered_tokens.model_dump(),
                        "tools": recovered_tools.model_dump(),
                    }
                )
                recovered_metrics = True
        tokens, tools = _merge_summaries(summaries)
        tools.harness_continuations = self._harness_continuations
        context.n_input_tokens = tokens.input + tokens.cache_read
        context.n_cache_tokens = tokens.cache_read
        context.n_output_tokens = tokens.output
        context.cost_usd = tokens.cost_usd
        context.n_agent_steps = tokens.requests
        context.metadata = {
            "condition": self.condition.value,
            "tokens": tokens.model_dump(),
            "tools": tools.model_dump(),
            "preindex_seconds": self.preindex_seconds,
            "agent_seconds": time.monotonic() - started,
            "trace_path": PI_TRACE_PATH,
            "metrics_recovered_from_truncated_trace": recovered_metrics,
        }

    async def _preindex(self, environment: BaseEnvironment) -> float:
        started = time.monotonic()
        deadline = started + self.index_timeout_seconds
        entries: list[dict[str, object]] = []
        commands = [
            f"{RELAY_BINARY} version --format json",
            f"{RELAY_BINARY} repo register /app --alias deepswe --format json",
            f"{RELAY_BINARY} repo index deepswe --ref HEAD --format json",
        ]
        for command in commands:
            await self._run_index_command(environment, command, deadline, entries)
        await self._drain_index(environment, deadline, entries)
        _write_jsonl(self.logs_dir / "relay-index.jsonl", entries)
        return time.monotonic() - started

    async def _drain_index(
        self,
        environment: BaseEnvironment,
        deadline: float,
        entries: list[dict[str, object]],
    ) -> None:
        for _ in range(100):
            output = await self._run_index_command(
                environment,
                f"{RELAY_BINARY} repo status deepswe --format json",
                deadline,
                entries,
            )
            progress = parse_index_progress(output)
            if not progress.state:
                if progress.indexed_scope_present:
                    return
                raise RuntimeError("Repository index ended without an indexed scope")
            if progress.state == "succeeded":
                return
            if progress.state in {"failed", "dead_letter", "cancelled"}:
                raise RuntimeError(
                    f"Repository index task reached terminal state {progress.state}"
                )
            if progress.state in {"queued", "retrying", "pending"}:
                if progress.state == "retrying":
                    delay = max(
                        0.0,
                        (progress.next_retry_at_ms / 1000) - time.time(),
                    )
                    if delay > 0:
                        remaining = deadline - time.monotonic()
                        if remaining <= 0:
                            raise TimeoutError("Repository pre-index timeout expired")
                        await asyncio.sleep(min(delay, remaining))
                        if time.monotonic() >= deadline:
                            raise TimeoutError("Repository pre-index timeout expired")
                        continue
                worker = f"{RELAY_BINARY} repo index-worker"
                if progress.task_id:
                    worker += f" --task-id {shlex.quote(progress.task_id)}"
                await self._run_index_command(
                    environment,
                    worker + " --format json",
                    deadline,
                    entries,
                )
                continue
            if progress.state == "running":
                remaining = deadline - time.monotonic()
                if remaining <= 0:
                    raise TimeoutError(
                        "Repository index lease did not complete before timeout"
                    )
                await asyncio.sleep(min(2.0, remaining))
                continue
            raise RuntimeError(
                f"Unsupported repository index task state: {progress.state!r}"
            )
        raise RuntimeError(
            "Repository index exceeded 100 bounded worker/status attempts"
        )

    async def _run_index_command(
        self,
        environment: BaseEnvironment,
        command: str,
        deadline: float,
        entries: list[dict[str, object]],
    ) -> str:
        remaining = deadline - time.monotonic()
        if remaining <= 0:
            raise TimeoutError("Repository pre-index timeout expired")
        result = await environment.exec(
            command,
            cwd="/app",
            env=RELAY_ENV,
            timeout_sec=max(1, int(remaining)),
        )
        entries.append(
            {
                "command": command.split()[1:4],
                "returncode": result.return_code,
                "stdout": self.redactor.redact(result.stdout or ""),
                "stderr": self.redactor.redact(result.stderr or ""),
            }
        )
        if result.return_code != 0:
            _write_jsonl(self.logs_dir / "relay-index.jsonl", entries)
            raise RuntimeError(
                f"Repository pre-index command failed ({result.return_code})"
            )
        return result.stdout or ""


def _build_deep_swe_prompt(
    instruction: str,
    *,
    condition: Condition,
    require_skill_use: bool,
) -> str:
    sections = [
        "You are a software engineer working inside /app. Complete the software "
        "task below by modifying the repository and validating the resulting "
        "behavior.",
        f"<task>\n{instruction.strip()}\n</task>",
        "Work autonomously until the implementation and relevant verification "
        "are complete.",
    ]
    if condition is Condition.SKILL and require_skill_use:
        sections.append(
            "Before editing, you must follow the loaded relay-knowledge-cli skill "
            "and execute its bundled CLI to investigate relevant definitions, "
            "references, callers, dependencies, or repository context. Use the "
            "retrieved evidence to guide the implementation, and perform additional "
            "queries when the implementation path remains unclear. This requirement "
            "is mandatory."
        )
    workflow = "\n".join(
        [
            "Required workflow:",
            "1. Inspect the repository instructions, structure, and relevant "
            "source and test files.",
            "2. Identify the behavior required by the task and determine the "
            "underlying implementation path.",
            "3. When practical, reproduce the missing or incorrect behavior with "
            "an existing test, focused command, or temporary check before editing.",
            "4. Implement a minimal, general solution consistent with the "
            "repository's architecture, public APIs, and coding conventions. Do "
            "not hard-code benchmark-specific values.",
            "5. Run focused tests or checks for the changed behavior. If they fail, "
            "inspect the failure and continue iterating instead of stopping after "
            "the first implementation attempt.",
            "6. Test relevant edge cases and run broader affected tests when "
            "practical.",
            "7. Inspect git status and git diff. Remove temporary files and "
            "unrelated changes.",
            "8. Do not finish with only an explanation. Ensure the intended source "
            "changes are committed.",
        ]
    )
    sections.extend(
        [
            workflow,
            "Do not inspect /logs/verifier, held-out verifier tests, solution files, "
            "reference patches, or any gold answer. Do not alter existing tests "
            "merely to hide a product-code failure. The harness makes a fallback "
            "commit only when necessary.",
        ]
    )
    return "\n\n".join(sections) + "\n"


def _agent_exec_environment(extra_env: dict[str, str]) -> dict[str, str]:
    """Keep credentials in container startup state, not docker exec arguments."""
    non_secret = {
        key: value for key, value in extra_env.items() if key != "DEEPSEEK_API_KEY"
    }
    return merge_relay_environment(non_secret)


def _pi_shell_command(
    *,
    condition: Condition,
    model: str,
    thinking: str,
    prompt_path: str,
    trace_path: str,
    summary_path: str,
    stderr_path: str,
    continue_session: bool,
) -> str:
    args = [
        "/opt/pi-eval/bin/pi-eval",
        "--mode",
        "json",
        "--session-dir",
        "/tmp/pi-deepswe-sessions",
        "--provider",
        "deepseek",
        "--model",
        model,
        "--thinking",
        thinking,
        "--tools",
        "read,bash,edit,write,grep,find,ls",
        "--no-extensions",
        "--no-skills",
        "--no-prompt-templates",
        "--no-themes",
        "--no-context-files",
        "--approve",
    ]
    if continue_session:
        args.append("--continue")
    if condition is Condition.SKILL:
        args.extend(["--skill", "/opt/pi-eval/skill/SKILL.md"])
    pi = shlex.join(args)
    capture = shlex.join(
        [
            "/opt/pi-eval/bin/node",
            "/logs/agent/trace-capture.mjs",
            trace_path,
            summary_path,
        ]
    )
    stderr_limit_marker = "/logs/agent/stderr-output-limited"
    redact_stderr = shlex.join(
        ["/opt/pi-eval/bin/node", STDERR_REDACTOR_PATH, stderr_limit_marker]
    )
    pipeline = (
        "set -o pipefail; "
        f"echo $$ > {shlex.quote(PI_PROCESS_GROUP_FILE)}; "
        f"trap 'rm -f {shlex.quote(PI_PROCESS_GROUP_FILE)}' EXIT; "
        f"prompt=$(cat {shlex.quote(prompt_path)}); "
        f"rm -f {shlex.quote(stderr_limit_marker)}; "
        f'{pi} "$prompt" 2> >({redact_stderr} '
        f"| head -c {DEEP_SWE_OUTPUT_BUDGET_BYTES} > "
        f"{shlex.quote(stderr_path)}) | {capture}; "
        "status=$?; wait; exit $status"
    )
    return "setsid bash -lc " + shlex.quote(pipeline)


def _recoverable(returncode: int, stderr: str) -> bool:
    if returncode == 0:
        return False
    if _provider_configuration_error(stderr):
        return False
    normalized = stderr.lower()
    return returncode in TRANSIENT_PROCESS_EXIT_CODES or any(
        marker in normalized for marker in TRANSPORT_MARKERS
    )


def _provider_configuration_error(stderr: str) -> bool:
    normalized = stderr.lower()
    return any(marker in normalized for marker in PROVIDER_CONFIGURATION_MARKERS)


async def _commit_worktree(environment: BaseEnvironment) -> None:
    result = await environment.exec(
        "git add -A && "
        "(git diff --cached --quiet || git commit -m 'DeepSWE agent solution')",
        cwd="/app",
        timeout_sec=120,
    )
    if result.return_code != 0:
        stderr = result.stderr or ""
        raise RuntimeError(f"Unable to commit DeepSWE worktree: {stderr[-1000:]}")


async def _stop_pi_process_group(environment: BaseEnvironment) -> None:
    pid_file = shlex.quote(PI_PROCESS_GROUP_FILE)
    result = await environment.exec(
        f"pid_file={pid_file}; "
        'if [ ! -s "$pid_file" ]; then exit 0; fi; '
        'pid=$(cat "$pid_file"); '
        'case "$pid" in ""|*[!0-9]*) '
        'echo "Invalid Pi process-group id" >&2; exit 2;; esac; '
        'if kill -0 -- "-$pid" 2>/dev/null; then '
        'kill -TERM -- "-$pid" || exit 3; '
        "for delay in 1 2 3 4 5; do "
        'kill -0 -- "-$pid" 2>/dev/null || break; sleep 1; done; '
        'if kill -0 -- "-$pid" 2>/dev/null; then '
        'kill -KILL -- "-$pid" || exit 4; sleep 1; fi; fi; '
        'rm -f "$pid_file"',
        cwd="/app",
        timeout_sec=15,
        user="root",
    )
    if result.return_code != 0:
        stderr = result.stderr or ""
        raise RuntimeError(f"Unable to stop timed-out DeepSWE Pi: {stderr[-1000:]}")


async def _preserve_timed_out_work(environment: BaseEnvironment) -> None:
    await _stop_pi_process_group(environment)
    await _commit_worktree(environment)


def _write_jsonl(path: Path, entries: list[dict[str, object]]) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(
        "".join(json.dumps(entry, ensure_ascii=False) + "\n" for entry in entries),
        encoding="utf-8",
    )


def _merge_summaries(
    summaries: list[dict[str, object]],
) -> tuple[TokenUsage, ToolUsage]:
    tokens = TokenUsage()
    tools = ToolUsage()
    for summary in summaries:
        attempt_tokens = TokenUsage.model_validate(summary.get("tokens", {}))
        attempt_tools = ToolUsage.model_validate(summary.get("tools", {}))
        for field in (
            "input",
            "output",
            "reasoning",
            "cache_read",
            "cache_write",
            "total",
            "requests",
        ):
            setattr(
                tokens,
                field,
                getattr(tokens, field) + getattr(attempt_tokens, field),
            )
        tokens.cost_usd += attempt_tokens.cost_usd
        tools.calls += attempt_tools.calls
        tools.errors += attempt_tools.errors
        tools.cumulative_seconds += attempt_tools.cumulative_seconds
        tools.auto_retries += attempt_tools.auto_retries
        for name, count in attempt_tools.by_name.items():
            tools.by_name[name] = tools.by_name.get(name, 0) + count
        for name, count in attempt_tools.relay_commands.items():
            tools.relay_commands[name] = tools.relay_commands.get(name, 0) + count
    return tokens, tools
