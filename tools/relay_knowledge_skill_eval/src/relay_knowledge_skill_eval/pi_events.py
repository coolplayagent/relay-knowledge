from __future__ import annotations

import json
import re
import zlib
from collections.abc import Iterable, Iterator, Mapping
from contextlib import suppress
from pathlib import Path

from relay_knowledge_skill_eval.models import TokenUsage, ToolUsage

_RELAY_COMMANDS = (
    "repo list",
    "repo register",
    "repo index-worker",
    "repo index",
    "repo status",
    "repo query",
    "repo context",
    "repo software",
    "repo feature-flags",
    "repo impact",
)
REPOSITORY_QUERY_COMMANDS = frozenset(
    {
        "repo query",
        "repo context",
        "repo software",
        "repo feature-flags",
        "repo impact",
    }
)
_RECOVERY_EVENT_MARKERS = (
    b'"type":"message_end"',
    b'"type":"tool_execution_start"',
    b'"type":"tool_execution_end"',
    b'"type":"auto_retry_start"',
)
_RECOVERY_CHUNK_BYTES = 1024 * 1024
_RECOVERY_MAX_LINE_BYTES = 4 * 1024 * 1024


def _mapping(value: object) -> Mapping[str, object]:
    return value if isinstance(value, Mapping) else {}


def _integer(value: object) -> int:
    return value if isinstance(value, int) and not isinstance(value, bool) else 0


def _number(value: object) -> float:
    return float(value) if isinstance(value, int | float) else 0.0


class PiTraceAccumulator:
    def __init__(self) -> None:
        self.tokens = TokenUsage()
        self.tools = ToolUsage()
        self._tool_starts: dict[str, float] = {}
        self._pending_relay_commands: dict[str, str] = {}
        self._seen_messages: set[tuple[object, ...]] = set()

    def consume_line(self, line: str, observed_at: float) -> dict[str, object] | None:
        try:
            payload = json.loads(line)
        except json.JSONDecodeError:
            return None
        if not isinstance(payload, dict):
            return None
        self.consume(payload, observed_at)
        return payload

    def consume(self, payload: Mapping[str, object], observed_at: float) -> None:
        event_type = payload.get("type")
        if event_type == "message_end":
            self._consume_message(payload.get("message"))
        elif event_type == "tool_execution_start":
            self._consume_tool_start(payload, observed_at)
        elif event_type == "tool_execution_end":
            self._consume_tool_end(payload, observed_at)
        elif event_type == "auto_retry_start":
            self.tools.auto_retries += 1

    def _consume_message(self, value: object) -> None:
        message = _mapping(value)
        if message.get("role") != "assistant":
            return
        usage = _mapping(message.get("usage"))
        identity = (
            message.get("timestamp"),
            message.get("model"),
            usage.get("totalTokens"),
            usage.get("input"),
            usage.get("output"),
        )
        if identity in self._seen_messages:
            return
        self._seen_messages.add(identity)
        cost = _mapping(usage.get("cost"))
        self.tokens.input += _integer(usage.get("input"))
        self.tokens.output += _integer(usage.get("output"))
        self.tokens.reasoning += _integer(usage.get("reasoning"))
        self.tokens.cache_read += _integer(usage.get("cacheRead"))
        self.tokens.cache_write += _integer(usage.get("cacheWrite"))
        self.tokens.total += _integer(usage.get("totalTokens"))
        self.tokens.cost_usd += _number(cost.get("total"))
        self.tokens.requests += 1

    def _consume_tool_start(
        self, payload: Mapping[str, object], observed_at: float
    ) -> None:
        call_id = payload.get("toolCallId")
        tool_name = payload.get("toolName")
        if isinstance(call_id, str):
            self._tool_starts[call_id] = observed_at
            relay_command = self._classify_relay_command(payload.get("args"))
            if relay_command is not None:
                self._pending_relay_commands[call_id] = relay_command
        if isinstance(tool_name, str):
            self.tools.calls += 1
            self.tools.by_name[tool_name] = self.tools.by_name.get(tool_name, 0) + 1

    def _consume_tool_end(
        self, payload: Mapping[str, object], observed_at: float
    ) -> None:
        call_id = payload.get("toolCallId")
        if isinstance(call_id, str):
            started_at = self._tool_starts.pop(call_id, None)
            if started_at is not None:
                self.tools.cumulative_seconds += max(0.0, observed_at - started_at)
            relay_command = self._pending_relay_commands.pop(call_id, None)
            if relay_command is not None and payload.get("isError") is not True:
                self.tools.relay_commands[relay_command] = (
                    self.tools.relay_commands.get(relay_command, 0) + 1
                )
        if payload.get("isError") is True:
            self.tools.errors += 1

    def _classify_relay_command(self, value: object) -> str | None:
        args = _mapping(value)
        joined = " ".join(str(item) for item in args.values())
        if "relay-knowledge" not in joined:
            return None
        normalized = re.sub(r"\s+", " ", joined)
        return next(
            (command for command in _RELAY_COMMANDS if command in normalized),
            "other",
        )


def recover_truncated_trace_usage(
    paths: Iterable[Path],
) -> tuple[TokenUsage, ToolUsage]:
    """Recover usage from complete events in bounded, possibly truncated gzip files."""
    accumulator = PiTraceAccumulator()
    for path in paths:
        for raw_line in _iter_bounded_gzip_lines(path):
            if not any(marker in raw_line[:512] for marker in _RECOVERY_EVENT_MARKERS):
                continue
            accumulator.consume_line(raw_line.decode("utf-8", errors="replace"), 0.0)
    return accumulator.tokens, accumulator.tools


def _iter_bounded_gzip_lines(path: Path) -> Iterator[bytes]:
    decompressor = zlib.decompressobj(16 + zlib.MAX_WBITS)
    pending = bytearray()
    discarding_oversized_line = False

    def consume(data: bytes) -> Iterator[bytes]:
        nonlocal discarding_oversized_line
        start = 0
        while start < len(data):
            newline = data.find(b"\n", start)
            end = len(data) if newline < 0 else newline
            if not discarding_oversized_line:
                segment = data[start:end]
                if len(pending) + len(segment) <= _RECOVERY_MAX_LINE_BYTES:
                    pending.extend(segment)
                else:
                    pending.clear()
                    discarding_oversized_line = True
            if newline < 0:
                return
            if not discarding_oversized_line:
                yield bytes(pending)
            pending.clear()
            discarding_oversized_line = False
            start = newline + 1

    try:
        with path.open("rb") as source:
            while compressed := source.read(_RECOVERY_CHUNK_BYTES):
                try:
                    decompressed = decompressor.decompress(compressed)
                except zlib.error:
                    break
                yield from consume(decompressed)
    except OSError:
        return
    with suppress(zlib.error):
        yield from consume(decompressor.flush())
    if pending and not discarding_oversized_line:
        yield bytes(pending)
