from __future__ import annotations

import gzip
import json
from pathlib import Path

from relay_knowledge_skill_eval.pi_events import (
    PiTraceAccumulator,
    recover_truncated_trace_usage,
)


def test_pi_trace_aggregates_usage_tools_and_retries() -> None:
    accumulator = PiTraceAccumulator()
    usage = {
        "type": "message_end",
        "message": {
            "role": "assistant",
            "timestamp": 1,
            "model": "deepseek-v4-flash",
            "usage": {
                "input": 100,
                "output": 20,
                "reasoning": 8,
                "cacheRead": 30,
                "cacheWrite": 4,
                "totalTokens": 162,
                "cost": {"total": 0.0123},
            },
        },
    }
    accumulator.consume_line(json.dumps(usage), 1.0)
    accumulator.consume_line(json.dumps(usage), 1.1)
    accumulator.consume(
        {
            "type": "tool_execution_start",
            "toolCallId": "call-1",
            "toolName": "bash",
            "args": {"command": "relay-knowledge repo query x --kind definition"},
        },
        2.0,
    )
    accumulator.consume(
        {
            "type": "tool_execution_end",
            "toolCallId": "call-1",
            "isError": True,
        },
        4.5,
    )
    accumulator.consume({"type": "auto_retry_start"}, 5.0)
    assert accumulator.consume_line("partial {", 6.0) is None
    assert accumulator.tokens.total == 162
    assert accumulator.tokens.requests == 1
    assert accumulator.tokens.cache_write == 4
    assert accumulator.tokens.cost_usd == 0.0123
    assert accumulator.tools.calls == 1
    assert accumulator.tools.errors == 1
    assert accumulator.tools.cumulative_seconds == 2.5
    assert accumulator.tools.relay_commands == {}
    assert accumulator.tools.auto_retries == 1


def test_relay_command_counts_only_after_successful_tool_completion() -> None:
    accumulator = PiTraceAccumulator()
    for call_id, is_error in (("failed", True), ("passed", False)):
        accumulator.consume(
            {
                "type": "tool_execution_start",
                "toolCallId": call_id,
                "toolName": "bash",
                "args": {"command": "relay-knowledge repo query symbol"},
            },
            1.0,
        )
        accumulator.consume(
            {
                "type": "tool_execution_end",
                "toolCallId": call_id,
                "isError": is_error,
            },
            2.0,
        )

    assert accumulator.tools.relay_commands == {"repo query": 1}


def test_truncated_gzip_recovers_complete_usage_with_bounded_lines(
    tmp_path: Path,
) -> None:
    events = [
        {
            "type": "message_end",
            "message": {
                "role": "assistant",
                "timestamp": 1,
                "model": "deepseek-v4-flash",
                "usage": {
                    "input": 10,
                    "output": 4,
                    "cacheRead": 20,
                    "totalTokens": 34,
                    "cost": {"total": 0.01},
                },
            },
        },
        {
            "type": "message_update",
            "ignored": "x" * (4 * 1024 * 1024 + 1),
        },
        {
            "type": "tool_execution_start",
            "toolCallId": "one",
            "toolName": "bash",
            "args": {"command": "echo bounded"},
        },
        {
            "type": "tool_execution_end",
            "toolCallId": "one",
            "isError": False,
        },
    ]
    payload = b"".join(
        json.dumps(event, separators=(",", ":")).encode() + b"\n" for event in events
    )
    trace = tmp_path / "truncated.jsonl.gz"
    trace.write_bytes(gzip.compress(payload)[:-8])

    tokens, tools = recover_truncated_trace_usage([trace])

    assert tokens.total == 34
    assert tokens.cache_read == 20
    assert tokens.requests == 1
    assert tools.calls == 1
    assert tools.by_name == {"bash": 1}
