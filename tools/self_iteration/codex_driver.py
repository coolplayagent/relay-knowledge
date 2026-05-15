"""Codex CLI integration for candidate generation."""

from __future__ import annotations

import shutil
import subprocess
import time
from dataclasses import dataclass
from pathlib import Path


@dataclass(frozen=True)
class CodexConfig:
    workspace: Path
    codex_path: str | None = None
    yolo: bool = False
    model: str | None = None
    profile: str | None = None
    timeout_seconds: int = 3600
    dry_run: bool = False


@dataclass(frozen=True)
class CodexResult:
    command: list[str]
    exit_code: int
    duration_ms: int
    stdout: str
    stderr: str

    @property
    def succeeded(self) -> bool:
        return self.exit_code == 0


def build_codex_command(config: CodexConfig) -> list[str]:
    codex = config.codex_path or shutil.which("codex") or "codex"
    command = [codex]
    if config.yolo:
        command.extend(["-a", "never"])
    command.extend(["exec", "-C", str(config.workspace)])
    if config.yolo:
        command.extend(["--dangerously-bypass-approvals-and-sandbox", "-s", "danger-full-access"])
    if config.model:
        command.extend(["-m", config.model])
    if config.profile:
        command.extend(["-p", config.profile])
    command.append("-")
    return command


def run_codex(config: CodexConfig, prompt: str) -> CodexResult:
    command = build_codex_command(config)
    if config.dry_run:
        return CodexResult(
            command=command,
            exit_code=0,
            duration_ms=0,
            stdout="dry-run: codex was not invoked\n",
            stderr="",
        )

    started = time.monotonic()
    completed = subprocess.run(
        command,
        input=prompt,
        cwd=config.workspace,
        text=True,
        capture_output=True,
        timeout=config.timeout_seconds,
        check=False,
    )
    return CodexResult(
        command=command,
        exit_code=completed.returncode,
        duration_ms=int((time.monotonic() - started) * 1000),
        stdout=completed.stdout,
        stderr=completed.stderr,
    )
