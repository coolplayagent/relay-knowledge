from __future__ import annotations

import json
import os
import subprocess
import tomllib
from pathlib import Path
from typing import Annotated

import typer

from relay_knowledge_skill_eval.assets import prepare_skill
from relay_knowledge_skill_eval.checkpoint import CheckpointStore, validate_resume_scope
from relay_knowledge_skill_eval.dataset import (
    DATASET_NAME,
    dataset_sha256,
    ensure_dataset,
    select_suite,
)
from relay_knowledge_skill_eval.docker_runtime import DockerRuntime
from relay_knowledge_skill_eval.models import RunOutcome, RunSignature, RuntimePaths
from relay_knowledge_skill_eval.reporting import write_completed_reports, write_reports
from relay_knowledge_skill_eval.runner import (
    FORCED_SKILL_PROMPT_VERSION,
    PROMPT_VERSION,
    TOOL_ALLOWLIST,
    EvaluatorConfig,
    SkillEvaluator,
)
from relay_knowledge_skill_eval.security import SecretRedactor
from relay_knowledge_skill_eval.swebench_support import SweBenchHarness

PI_VERSION = "0.80.3"
NODE_VERSION = "22.19.0"
SWEBENCH_VERSION = "4.1.0"
HARNESS_VERSION = "2"
MODEL = "deepseek-v4-flash"
THINKING = "high"
IMAGE_PREFIX = "sweb.eval.x86_64"
DEFAULT_AGENT_TIMEOUT = 3600
DEFAULT_INDEX_TIMEOUT = 600
DEFAULT_SCORE_TIMEOUT = 900
DEFAULT_MAX_CONTINUATIONS = 3
DEFAULT_STALL_TIMEOUT = 600

app = typer.Typer(
    no_args_is_help=True,
    help="Evaluate the relay-knowledge CLI skill with Pi as the agent runner.",
)


@app.command()
def prepare(
    suite: Annotated[
        str,
        typer.Option(help="smoke-10, verified-first-100, or verified-full"),
    ] = "smoke-10",
    cache_dir: Annotated[
        Path | None, typer.Option(help="Override the evaluation cache directory")
    ] = None,
    skill_source: Annotated[
        Path | None,
        typer.Option(help="Use a local packaged skill instead of the release archive"),
    ] = None,
) -> None:
    """Download immutable inputs and build the runtime and SWE-bench images."""
    paths = resolve_paths(cache_dir=cache_dir, output_dir=None)
    version = repository_version(paths.workspace)
    typer.echo(
        "Preparing official release inputs; configure HTTPS_PROXY/HTTP_PROXY if needed."
    )
    items = ensure_dataset(paths.dataset_path)
    selected = select_suite(items, suite)
    skill_dir, skill_sha = prepare_skill(
        cache_dir=paths.cache_dir,
        version=version,
        source_dir=skill_source,
    )
    redactor = SecretRedactor("")
    runtime = make_runtime(
        paths,
        version,
        skill_sha256=skill_sha,
        redactor=redactor,
        api_key="",
    )
    try:
        docker_version = runtime.check_ready()
        build_seconds = runtime.build_runtime(skill_dir, pi_version=PI_VERSION)
        scorer = SweBenchHarness(
            runtime,
            cache_dir=paths.cache_dir,
            output_dir=paths.output_dir,
        )
        image_seconds = 0.0
        for index, item in enumerate(selected, start=1):
            typer.echo(f"[{index}/{len(selected)}] preparing {item.instance_id}")
            image_seconds += scorer.ensure_instance_image(item)
    finally:
        runtime.close()
    typer.echo(
        f"Prepared {len(selected)} instances with Docker {docker_version}; "
        f"runtime={build_seconds:.1f}s images={image_seconds:.1f}s "
        f"skill_sha256={skill_sha}"
    )


@app.command()
def run(
    suite: Annotated[
        str,
        typer.Option(help="smoke-10, verified-first-100, or verified-full"),
    ] = "smoke-10",
    concurrency: Annotated[
        int, typer.Option(min=1, max=8, help="Number of paired instances to run")
    ] = 1,
    resume: Annotated[
        bool, typer.Option(help="Skip completed checkpoint records")
    ] = False,
    parallel_conditions: Annotated[
        bool,
        typer.Option(help="Run baseline and treatment for each instance concurrently"),
    ] = False,
    require_skill_use: Annotated[
        bool,
        typer.Option(help="Require treatment to execute the loaded skill CLI"),
    ] = False,
    cache_dir: Annotated[Path | None, typer.Option()] = None,
    output_dir: Annotated[Path | None, typer.Option()] = None,
    skill_source: Annotated[Path | None, typer.Option()] = None,
    agent_timeout: Annotated[
        int, typer.Option(min=1, help="Pi agent timeout in seconds")
    ] = DEFAULT_AGENT_TIMEOUT,
    index_timeout: Annotated[
        int, typer.Option(min=1, help="Treatment pre-index timeout in seconds")
    ] = DEFAULT_INDEX_TIMEOUT,
    max_continuations: Annotated[
        int,
        typer.Option(min=0, help="Maximum same-session Pi continuation attempts"),
    ] = DEFAULT_MAX_CONTINUATIONS,
    stall_timeout: Annotated[
        int,
        typer.Option(min=1, help="Continue Pi after this many seconds without output"),
    ] = DEFAULT_STALL_TIMEOUT,
) -> None:
    """Run paired baseline and skill conditions and checkpoint every result."""
    api_key = os.environ.get("DEEPSEEK_API_KEY", "")
    if not api_key:
        raise typer.BadParameter(
            "DEEPSEEK_API_KEY must be set in the current process environment"
        )
    paths = resolve_paths(cache_dir=cache_dir, output_dir=output_dir)
    version = repository_version(paths.workspace)
    items = ensure_dataset(paths.dataset_path)
    selected = select_suite(items, suite)
    skill_dir, skill_sha = prepare_skill(
        cache_dir=paths.cache_dir,
        version=version,
        source_dir=skill_source,
    )
    redactor = SecretRedactor(api_key)
    runtime = make_runtime(
        paths,
        version,
        skill_sha256=skill_sha,
        redactor=redactor,
        api_key=api_key,
    )
    checkpoint = CheckpointStore(paths.output_dir)
    signature = RunSignature(
        dataset_name=DATASET_NAME,
        dataset_sha256=dataset_sha256(paths.dataset_path),
        harness_version=HARNESS_VERSION,
        swebench_version=SWEBENCH_VERSION,
        node_version=NODE_VERSION,
        pi_version=PI_VERSION,
        model=MODEL,
        thinking=THINKING,
        skill_version=version,
        skill_sha256=skill_sha,
        runtime_image=runtime.runtime_image,
        image_prefix=IMAGE_PREFIX,
        prompt_version=(
            FORCED_SKILL_PROMPT_VERSION if require_skill_use else PROMPT_VERSION
        ),
        treatment_instruction=("required" if require_skill_use else "available"),
        condition_execution_mode=(
            "parallel" if parallel_conditions else "stable-hash-sequential"
        ),
        tool_allowlist=TOOL_ALLOWLIST,
        agent_timeout_seconds=agent_timeout,
        index_timeout_seconds=index_timeout,
        score_timeout_seconds=DEFAULT_SCORE_TIMEOUT,
        max_continuations=max_continuations,
        stall_timeout_seconds=stall_timeout,
    )
    checkpoint.initialize(signature, repository_commit(paths.workspace))
    existing = checkpoint.load_results(repair_trailing=True)
    if existing and not resume:
        raise typer.BadParameter(
            "Checkpoint already contains results; pass --resume or choose --output-dir"
        )
    if existing:
        try:
            validate_resume_scope(
                existing,
                [item.instance_id for item in selected],
                len(selected) * 2,
            )
        except ValueError as error:
            raise typer.BadParameter(str(error)) from error
    live_metadata = checkpoint.load_meta().model_dump(mode="json")
    live_metadata.update(
        {
            "active_suite": suite,
            "expected_results": len(selected) * 2,
            "recorded_results": len(existing),
            "completed_results": sum(
                result.outcome is not RunOutcome.INFRA_ERROR
                for result in existing.values()
            ),
        }
    )
    write_reports(list(existing.values()), paths.output_dir, metadata=live_metadata)
    typer.echo(f"Live dashboard: {paths.output_dir / 'live.html'}")
    try:
        runtime.check_ready()
        if not runtime.image_exists(runtime.runtime_image):
            runtime.build_runtime(skill_dir, pi_version=PI_VERSION)
        scorer = SweBenchHarness(
            runtime,
            cache_dir=paths.cache_dir,
            output_dir=paths.output_dir,
        )
        evaluator = SkillEvaluator(
            runtime=runtime,
            scorer=scorer,
            checkpoint=checkpoint,
            config=EvaluatorConfig(
                output_dir=paths.output_dir,
                model=MODEL,
                thinking=THINKING,
                agent_timeout_seconds=agent_timeout,
                index_timeout_seconds=index_timeout,
                max_continuations=max_continuations,
                stall_timeout_seconds=stall_timeout,
                concurrency=concurrency,
                parallel_conditions=parallel_conditions,
                require_skill_use=require_skill_use,
                resume=resume,
                suite=suite,
                expected_results=len(selected) * 2,
            ),
            redactor=redactor,
        )
        results = evaluator.run(selected)
        live_metadata["recorded_results"] = len(results)
        live_metadata["completed_results"] = sum(
            result.outcome is not RunOutcome.INFRA_ERROR for result in results
        )
        report_data = write_completed_reports(
            results,
            paths.output_dir,
            metadata=live_metadata,
        )
    finally:
        runtime.close()
    paired = report_data["paired"]
    typer.echo(
        f"Completed report at {paths.output_dir}; paired={paired['count']} "
        f"delta={paired['pass_rate_delta']:+.1%}"
    )


@app.command()
def report(
    output_dir: Annotated[Path | None, typer.Option()] = None,
) -> None:
    """Rebuild JSON, JSONL, CSV, and HTML reports from the checkpoint."""
    paths = resolve_paths(cache_dir=None, output_dir=output_dir)
    checkpoint = CheckpointStore(paths.output_dir)
    results = list(checkpoint.load_results().values())
    metadata = checkpoint.load_meta().model_dump(mode="json")
    existing_report_path = paths.output_dir / "report.json"
    if existing_report_path.exists():
        try:
            existing_report = json.loads(
                existing_report_path.read_text(encoding="utf-8")
            )
            existing_metadata = existing_report.get("metadata", {})
            if isinstance(existing_metadata, dict):
                for key in ("active_suite", "expected_results"):
                    if key in existing_metadata:
                        metadata[key] = existing_metadata[key]
        except (OSError, json.JSONDecodeError):
            pass
    report_data = write_completed_reports(
        results,
        paths.output_dir,
        metadata=metadata,
    )
    paired = report_data["paired"]
    typer.echo(f"Wrote reports for {paired['count']} complete pairs")


@app.command("deep-swe-run")
def deep_swe_run(
    tasks_dir: Annotated[Path | None, typer.Option()] = None,
    output_dir: Annotated[Path | None, typer.Option()] = None,
) -> None:
    """Run the official 113-task DeepSWE baseline/Skill evaluation with Pier."""
    from relay_knowledge_skill_eval.deep_swe_runner import (
        ensure_deep_swe_tasks,
        run_deep_swe,
        validate_deep_swe_tasks,
    )

    paths = resolve_paths(cache_dir=None, output_dir=None)
    api_key = os.environ.get("DEEPSEEK_API_KEY", "")
    if not api_key:
        raise typer.BadParameter(
            "DEEPSEEK_API_KEY must be set in the current process environment"
        )
    version = repository_version(paths.workspace)
    skill_dir, skill_sha = prepare_skill(
        cache_dir=paths.cache_dir,
        version=version,
        source_dir=None,
    )
    runtime = make_runtime(
        paths,
        version,
        skill_sha256=skill_sha,
        redactor=SecretRedactor(api_key),
        api_key=api_key,
    )
    resolved_tasks = (
        validate_deep_swe_tasks(tasks_dir)
        if tasks_dir is not None
        else ensure_deep_swe_tasks(paths.cache_dir / "deep-swe")
    )
    resolved_output = (
        output_dir or paths.output_dir.parent / "deepswe-113-pi-v4-flash-ab-1h"
    ).resolve()
    try:
        runtime.check_ready()
        if not runtime.image_exists(runtime.runtime_image):
            runtime.build_runtime(skill_dir, pi_version=PI_VERSION)
        report_path = run_deep_swe(
            resolved_tasks,
            resolved_output,
            runtime_image=runtime.runtime_image,
            skill_version=version,
            skill_sha256=skill_sha,
        )
    finally:
        runtime.close()
    typer.echo(f"DeepSWE report: {report_path}")


@app.command("deep-swe-report")
def deep_swe_report(
    output_dir: Annotated[Path | None, typer.Option()] = None,
) -> None:
    """Rebuild the DeepSWE JSON report from completed Pier trials."""
    from relay_knowledge_skill_eval.deep_swe_reporting import write_deep_swe_report
    from relay_knowledge_skill_eval.deep_swe_runner import JOBS_DIR_NAME

    paths = resolve_paths(cache_dir=None, output_dir=None)
    resolved_output = (
        output_dir or paths.output_dir.parent / "deepswe-113-pi-v4-flash-ab-1h"
    ).resolve()
    report_output = resolved_output / "report.json"
    skill_version = repository_version(paths.workspace)
    skill_sha256 = ""
    if report_output.exists():
        try:
            existing = json.loads(report_output.read_text(encoding="utf-8"))
            metadata = existing.get("metadata", {})
            if isinstance(metadata, dict):
                recorded_version = metadata.get("skill_version")
                recorded_sha = metadata.get("skill_sha256")
                if isinstance(recorded_version, str) and recorded_version:
                    skill_version = recorded_version
                if isinstance(recorded_sha, str):
                    skill_sha256 = recorded_sha
        except (OSError, json.JSONDecodeError):
            pass
    report_path = write_deep_swe_report(
        resolved_output / JOBS_DIR_NAME,
        output_path=report_output,
        skill_version=skill_version,
        skill_sha256=skill_sha256,
    )
    typer.echo(f"DeepSWE report: {report_path}")


@app.command("combined-dashboard")
def combined_dashboard(
    swe_report: Annotated[Path, typer.Option()],
    deep_swe_report: Annotated[Path, typer.Option()],
    output_dir: Annotated[Path, typer.Option()],
    watch: Annotated[bool, typer.Option()] = False,
) -> None:
    """Render or continuously refresh the combined public dashboard."""
    from relay_knowledge_skill_eval.combined_dashboard import (
        watch_combined_dashboard,
        write_combined_dashboard,
    )

    if watch:
        watch_combined_dashboard(
            swe_report_path=swe_report.resolve(),
            deep_swe_report_path=deep_swe_report.resolve(),
            output_dir=output_dir.resolve(),
        )
        return
    target = write_combined_dashboard(
        swe_report_path=swe_report.resolve(),
        deep_swe_report_path=deep_swe_report.resolve(),
        output_dir=output_dir.resolve(),
    )
    typer.echo(f"Combined dashboard: {target}")


def resolve_paths(*, cache_dir: Path | None, output_dir: Path | None) -> RuntimePaths:
    tool_root = Path(__file__).resolve().parents[2]
    workspace = tool_root.parent.parent
    eval_root = workspace / ".evals" / "relay-knowledge-skill"
    resolved_cache = (cache_dir or eval_root / "cache").resolve()
    resolved_output = (
        output_dir or eval_root / "runs" / repository_version(workspace)
    ).resolve()
    return RuntimePaths(
        workspace=workspace,
        tool_root=tool_root,
        cache_dir=resolved_cache,
        output_dir=resolved_output,
        dataset_path=resolved_cache / "SWE-bench_Verified.jsonl",
    )


def repository_version(workspace: Path) -> str:
    with (workspace / "Cargo.toml").open("rb") as handle:
        manifest = tomllib.load(handle)
    package = manifest.get("package")
    if not isinstance(package, dict) or not isinstance(package.get("version"), str):
        raise RuntimeError("Cargo.toml package.version is unavailable")
    return package["version"]


def repository_commit(workspace: Path) -> str:
    result = subprocess.run(
        ["git", "rev-parse", "HEAD"],
        cwd=workspace,
        capture_output=True,
        text=True,
        encoding="utf-8",
        check=False,
    )
    if result.returncode != 0:
        raise RuntimeError(f"Unable to identify repository commit: {result.stderr}")
    return result.stdout.strip()


def make_runtime(
    paths: RuntimePaths,
    version: str,
    *,
    skill_sha256: str,
    redactor: SecretRedactor,
    api_key: str,
) -> DockerRuntime:
    if len(skill_sha256) != 64 or any(
        character not in "0123456789abcdef" for character in skill_sha256.lower()
    ):
        raise ValueError("skill_sha256 must be a hexadecimal SHA-256 digest")
    return DockerRuntime(
        tool_root=paths.tool_root,
        cache_dir=paths.cache_dir,
        runtime_image=(
            f"relay-knowledge-skill-eval:pi-{PI_VERSION}-v{version}-"
            f"skill-{skill_sha256.lower()}"
        ),
        image_prefix=IMAGE_PREFIX,
        api_key=api_key,
        redactor=redactor,
    )
