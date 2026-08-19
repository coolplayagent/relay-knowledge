from __future__ import annotations

import asyncio
import os
import shutil
import subprocess
import uuid
from datetime import UTC, datetime
from pathlib import Path

from pier.job import Job
from pier.models.job.config import JobConfig, RetryConfig
from pier.models.job.result import JobResult
from pier.models.trial.config import AgentConfig, EnvironmentConfig, TaskConfig
from pier.models.trial.result import TrialResult
from pier.trial.hooks import TrialHookEvent

from relay_knowledge_skill_eval.deep_swe_reporting import (
    trial_outcome,
    write_deep_swe_report,
)
from relay_knowledge_skill_eval.models import RunOutcome

EXPECTED_TASKS = 113
EXPECTED_RESULTS = EXPECTED_TASKS * 2
JOBS_DIR_NAME = "tasks"
DEEP_SWE_REPOSITORY = "https://github.com/datacurve-ai/deep-swe"
DEEP_SWE_COMMIT = "435ee89ec2f2e2289f33b0da4f992f0b7b7266b9"
AGENT_TIMEOUT_SECONDS = 3600
PIER_CLEANUP_GRACE_SECONDS = 300
INDEX_TIMEOUT_SECONDS = 900
PIER_SETUP_GRACE_SECONDS = 120


def _normalized_git_remote(remote: str) -> str:
    """Normalize equivalent canonical GitHub repository URL spellings."""
    return remote.strip().rstrip("/").removesuffix(".git")


def ensure_deep_swe_tasks(checkout_dir: Path) -> Path:
    """Return a verified pinned official DeepSWE task checkout."""
    checkout_dir = checkout_dir.resolve()
    tasks_dir = checkout_dir / "tasks"
    if checkout_dir.exists():
        if not (checkout_dir / ".git").is_dir():
            raise RuntimeError(
                f"DeepSWE cache exists but is not a Git checkout: {checkout_dir}"
            )
        remote = _run_git(["remote", "get-url", "origin"], cwd=checkout_dir)
        if _normalized_git_remote(remote) != DEEP_SWE_REPOSITORY:
            raise RuntimeError(f"DeepSWE cache has an unexpected origin: {remote}")
        current = _run_git(["rev-parse", "HEAD"], cwd=checkout_dir)
        if current != DEEP_SWE_COMMIT:
            _run_git(
                ["fetch", "--depth", "1", "origin", DEEP_SWE_COMMIT],
                cwd=checkout_dir,
            )
            _run_git(
                ["checkout", "--detach", "--force", DEEP_SWE_COMMIT],
                cwd=checkout_dir,
            )
    else:
        checkout_dir.parent.mkdir(parents=True, exist_ok=True)
        staging = checkout_dir.parent / f".{checkout_dir.name}-{uuid.uuid4().hex}.tmp"
        try:
            _run_git(
                [
                    "clone",
                    "--filter=blob:none",
                    "--no-checkout",
                    DEEP_SWE_REPOSITORY,
                    str(staging),
                ],
                cwd=checkout_dir.parent,
            )
            _run_git(
                ["fetch", "--depth", "1", "origin", DEEP_SWE_COMMIT],
                cwd=staging,
            )
            _run_git(
                ["checkout", "--detach", "--force", DEEP_SWE_COMMIT],
                cwd=staging,
            )
            if len(_task_paths(staging / "tasks")) != EXPECTED_TASKS:
                raise RuntimeError("Staged DeepSWE checkout has an invalid task count")
            try:
                staging.replace(checkout_dir)
            except OSError:
                if not checkout_dir.exists():
                    raise
        finally:
            shutil.rmtree(staging, ignore_errors=True)

    # The evaluation cache is a dedicated immutable input checkout. Restore
    # tracked and untracked content before trusting its commit identity so a
    # prior manual experiment cannot alter prompts or verifier scripts.
    _run_git(["reset", "--hard", DEEP_SWE_COMMIT], cwd=checkout_dir)
    _run_git(["clean", "-fdx"], cwd=checkout_dir)
    task_paths = _task_paths(tasks_dir)
    if len(task_paths) != EXPECTED_TASKS:
        raise RuntimeError(
            f"Pinned DeepSWE checkout contains {len(task_paths)} tasks; "
            f"expected {EXPECTED_TASKS}"
        )
    return tasks_dir


def validate_deep_swe_tasks(tasks_dir: Path) -> Path:
    """Validate a caller-provided immutable official DeepSWE task checkout."""
    tasks_dir = tasks_dir.resolve()
    if not tasks_dir.is_dir():
        raise RuntimeError(f"DeepSWE tasks directory does not exist: {tasks_dir}")

    checkout_dir = Path(
        _run_git(["rev-parse", "--show-toplevel"], cwd=tasks_dir)
    ).resolve()
    if tasks_dir != checkout_dir / "tasks":
        raise RuntimeError(
            "DeepSWE --tasks-dir must be the tasks directory at the root of "
            f"the official checkout: {tasks_dir}"
        )

    remote = _run_git(["remote", "get-url", "origin"], cwd=checkout_dir)
    if _normalized_git_remote(remote) != DEEP_SWE_REPOSITORY:
        raise RuntimeError(f"DeepSWE checkout has an unexpected origin: {remote}")
    current = _run_git(["rev-parse", "HEAD"], cwd=checkout_dir)
    if current != DEEP_SWE_COMMIT:
        raise RuntimeError(
            f"DeepSWE checkout is at {current}; expected {DEEP_SWE_COMMIT}"
        )
    if status := _run_git(
        ["status", "--porcelain", "--untracked-files=all"], cwd=checkout_dir
    ):
        raise RuntimeError(
            "DeepSWE checkout contains modified or untracked files; "
            f"first entry: {status.splitlines()[0]}"
        )

    task_paths = _task_paths(tasks_dir)
    if len(task_paths) != EXPECTED_TASKS:
        raise RuntimeError(
            f"Pinned DeepSWE checkout contains {len(task_paths)} tasks; "
            f"expected {EXPECTED_TASKS}"
        )
    return tasks_dir


def _run_git(arguments: list[str], *, cwd: Path) -> str:
    try:
        result = subprocess.run(
            ["git", *arguments],
            cwd=cwd,
            capture_output=True,
            text=True,
            encoding="utf-8",
            check=False,
            timeout=300,
        )
    except subprocess.TimeoutExpired as error:
        raise RuntimeError(f"Git command timed out: git {arguments[0]}") from error
    if result.returncode != 0:
        diagnostic = (result.stderr or result.stdout).strip()[-2000:]
        raise RuntimeError(f"Git command failed: git {arguments[0]}: {diagnostic}")
    return result.stdout.strip()


def _task_paths(tasks_dir: Path) -> list[Path]:
    if not tasks_dir.is_dir():
        return []
    return sorted(
        path
        for path in tasks_dir.iterdir()
        if path.is_dir() and (path / "task.toml").exists()
    )


def run_deep_swe(
    tasks_dir: Path,
    output_dir: Path,
    *,
    runtime_image: str,
    skill_version: str,
    skill_sha256: str,
) -> Path:
    if not os.environ.get("DEEPSEEK_API_KEY"):
        raise RuntimeError("DEEPSEEK_API_KEY is required")
    task_paths = _task_paths(tasks_dir)
    if len(task_paths) != EXPECTED_TASKS:
        raise ValueError(
            f"Expected {EXPECTED_TASKS} official DeepSWE tasks, found {len(task_paths)}"
        )
    output_dir.mkdir(parents=True, exist_ok=True)
    return asyncio.run(
        _run(
            task_paths,
            output_dir.resolve(),
            runtime_image,
            skill_version=skill_version,
            skill_sha256=skill_sha256,
        )
    )


async def _run(
    task_paths: list[Path],
    output_dir: Path,
    runtime_image: str,
    *,
    skill_version: str,
    skill_sha256: str,
) -> Path:
    task_paths = _prepare_task_inputs(task_paths, output_dir)
    agents = [
        _agent_config("baseline", require_skill_use=False),
        _agent_config("skill", require_skill_use=True),
    ]
    jobs_dir = output_dir / JOBS_DIR_NAME
    report_path = output_dir / "report.json"
    for task_path in task_paths:
        _archive_infrastructure_failures(task_path.name, jobs_dir, output_dir)
    await _validate_existing_job_configs(
        task_paths,
        jobs_dir,
        agents,
        runtime_image,
    )
    write_deep_swe_report(
        jobs_dir,
        expected_results=EXPECTED_RESULTS,
        output_path=report_path,
        skill_version=skill_version,
        skill_sha256=skill_sha256,
    )
    for task_path in task_paths:
        for _ in range(3):
            if _archive_infrastructure_failures(task_path.name, jobs_dir, output_dir):
                write_deep_swe_report(
                    jobs_dir,
                    expected_results=EXPECTED_RESULTS,
                    output_path=report_path,
                    skill_version=skill_version,
                    skill_sha256=skill_sha256,
                )
            config = _job_config(task_path, jobs_dir, agents, runtime_image)
            job = await Job.create(config)

            async def update_report(_: TrialHookEvent) -> None:
                write_deep_swe_report(
                    jobs_dir,
                    expected_results=EXPECTED_RESULTS,
                    output_path=report_path,
                    skill_version=skill_version,
                    skill_sha256=skill_sha256,
                )

            job.on_trial_ended(update_report)
            await job.run()
            write_deep_swe_report(
                jobs_dir,
                expected_results=EXPECTED_RESULTS,
                output_path=report_path,
                skill_version=skill_version,
                skill_sha256=skill_sha256,
            )
            if not _has_infrastructure_failure(jobs_dir / task_path.name):
                break
        else:
            raise RuntimeError(
                f"DeepSWE task {task_path.name} still has infrastructure "
                "failures after three attempts"
            )
    return output_dir / "report.json"


async def _validate_existing_job_configs(
    task_paths: list[Path],
    jobs_dir: Path,
    agents: list[AgentConfig],
    runtime_image: str,
) -> None:
    """Make Pier reject stale job provenance before reports are rewritten."""
    for task_path in task_paths:
        if (jobs_dir / task_path.name).is_dir():
            job = await Job.create(
                _job_config(task_path, jobs_dir, agents, runtime_image)
            )
            job._close_logger_handlers()


def _prepare_task_inputs(task_paths: list[Path], output_dir: Path) -> list[Path]:
    """Copy official tasks and normalize Linux-bound text files."""
    prepared_root = output_dir / "task-inputs"
    prepared: list[Path] = []
    for source in task_paths:
        target = prepared_root / source.name
        # A resumed run starts from the pinned official task again so stale
        # generated or manually modified files cannot affect later trials.
        if target.exists():
            shutil.rmtree(target)
        shutil.copytree(source, target)
        for path in target.rglob("*"):
            if not path.is_file():
                continue
            content = path.read_bytes()
            if b"\r\n" not in content:
                continue
            if path.suffix.lower() not in {
                ".sh",
                ".bash",
                ".patch",
                ".diff",
            } and not content.startswith(b"#!"):
                continue
            path.write_bytes(content.replace(b"\r\n", b"\n"))
        prepared.append(target)
    return prepared


def _job_config(
    task_path: Path,
    jobs_dir: Path,
    agents: list[AgentConfig],
    runtime_image: str,
) -> JobConfig:
    return JobConfig(
        job_name=task_path.name,
        jobs_dir=jobs_dir,
        n_attempts=1,
        n_concurrent_trials=2,
        quiet=True,
        retry=RetryConfig(
            max_retries=2,
            include_exceptions={"DeepSweTransportError"},
        ),
        agents=agents,
        tasks=[TaskConfig(path=task_path)],
        environment=EnvironmentConfig(
            import_path=(
                "relay_knowledge_skill_eval.deep_swe_agent:DeepSweDockerEnvironment"
            ),
            delete=True,
            kwargs={"runtime_image": runtime_image},
        ),
    )


def _agent_config(condition: str, *, require_skill_use: bool) -> AgentConfig:
    return AgentConfig(
        import_path=("relay_knowledge_skill_eval.deep_swe_agent:PiDeepSweAgent"),
        model_name="deepseek/deepseek-v4-flash",
        override_timeout_sec=AGENT_TIMEOUT_SECONDS + PIER_CLEANUP_GRACE_SECONDS,
        override_setup_timeout_sec=INDEX_TIMEOUT_SECONDS + PIER_SETUP_GRACE_SECONDS,
        kwargs={
            "condition": condition,
            "require_skill_use": require_skill_use,
            "thinking": "high",
            "agent_timeout_seconds": AGENT_TIMEOUT_SECONDS,
            "index_timeout_seconds": INDEX_TIMEOUT_SECONDS,
            "max_continuations": 3,
        },
        env={"DEEPSEEK_API_KEY": "${DEEPSEEK_API_KEY}"},
    )


def _has_infrastructure_failure(job_dir: Path) -> bool:
    trial_dirs = sorted(path for path in job_dir.iterdir() if path.is_dir())
    if len(trial_dirs) != 2:
        return True
    for trial_dir in trial_dirs:
        result_path = trial_dir / "result.json"
        try:
            trial = TrialResult.model_validate_json(
                result_path.read_text(encoding="utf-8")
            )
        except (OSError, ValueError):
            return True
        if trial_outcome(trial, trial_dir)[0] is RunOutcome.INFRA_ERROR:
            return True
    return False


def _archive_infrastructure_failures(
    task_name: str, jobs_dir: Path, output_dir: Path
) -> bool:
    job_dir = jobs_dir / task_name
    if not job_dir.exists():
        return False
    stamp = datetime.now(UTC).strftime("%Y%m%dT%H%M%S%fZ")
    history_dir = output_dir / "infra-history" / task_name
    archived_job_result = False
    job_result_path = job_dir / "result.json"
    if job_result_path.exists():
        try:
            JobResult.model_validate_json(job_result_path.read_text(encoding="utf-8"))
        except (OSError, ValueError):
            target = history_dir / f"{stamp}-job-result.json"
            target.parent.mkdir(parents=True, exist_ok=True)
            shutil.move(str(job_result_path), str(target))
            archived_job_result = True
    archived_trial = False
    trial_dirs = sorted(path for path in job_dir.iterdir() if path.is_dir())
    for source in trial_dirs:
        result_path = source / "result.json"
        archive = False
        try:
            trial = TrialResult.model_validate_json(
                result_path.read_text(encoding="utf-8")
            )
        except (OSError, ValueError):
            archive = True
        else:
            archive = trial_outcome(trial, source)[0] is RunOutcome.INFRA_ERROR
        if not archive:
            continue
        target = history_dir / f"{stamp}-{source.name}"
        target.parent.mkdir(parents=True, exist_ok=True)
        shutil.move(str(source), str(target))
        archived_trial = True
    if archived_trial:
        # Pier needs the job-level config/result/lock to preserve completed or
        # final sibling trials while recreating only the archived infra trial.
        return True
    if not trial_dirs and job_result_path.exists():
        state_dir = history_dir / f"{stamp}-job-state"
        state_dir.mkdir(parents=True, exist_ok=True)
        for name in ("config.json", "result.json", "lock.json", "job.log"):
            source = job_dir / name
            if source.exists():
                shutil.move(str(source), str(state_dir / name))
        return True
    return archived_job_result
