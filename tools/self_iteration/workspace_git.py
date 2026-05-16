"""Git/worktree boundary helpers for self-iteration."""

from __future__ import annotations

import subprocess
from pathlib import Path


class NonRetryableIterationError(RuntimeError):
    """Failure that cannot be fixed by starting another iteration."""


def current_head(workspace: Path) -> str:
    return git(workspace, ["rev-parse", "HEAD"], check=True).stdout.strip()


def ensure_clean_worktree(workspace: Path) -> None:
    status = git(workspace, ["status", "--short"], check=True).stdout.strip()
    if status:
        raise NonRetryableIterationError(
            "working tree is not clean; commit/stash changes or pass --use-current-candidate"
        )


def git(workspace: Path, args: list[str], check: bool) -> subprocess.CompletedProcess[str]:
    completed = subprocess.run(
        ["git", *args],
        cwd=workspace,
        text=True,
        capture_output=True,
        check=False,
    )
    if check and completed.returncode != 0:
        raise RuntimeError(completed.stderr.strip() or completed.stdout.strip())
    return completed


def git_lines(workspace: Path, args: list[str]) -> list[str]:
    output = git(workspace, args, check=True).stdout
    return [line for line in output.splitlines() if line]
