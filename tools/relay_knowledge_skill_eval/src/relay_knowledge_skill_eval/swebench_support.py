from __future__ import annotations

import builtins
import json
import os
import platform
import shlex
import subprocess
import sys
import time
import types
import uuid
from collections.abc import Iterator
from contextlib import contextmanager, suppress
from importlib import import_module
from pathlib import Path
from unittest.mock import patch

from relay_knowledge_skill_eval.docker_runtime import DockerRuntime
from relay_knowledge_skill_eval.models import (
    SweBenchDiagnostics,
    SweBenchItem,
    TestBucket,
)

PYTHON_BUILD_CONSTRAINT = "setuptools==63.4.3"
PYTHON_BUILD_PIP = "pip==23.3.2"


class _ResourceModule(types.ModuleType):
    RLIMIT_NOFILE: int

    @staticmethod
    def getrlimit(resource: int) -> tuple[int, int]:
        _ = resource
        return (0, 0)

    @staticmethod
    def setrlimit(resource: int, limits: tuple[int, int]) -> None:
        _ = (resource, limits)


def install_windows_resource_stub() -> None:
    if platform.system() != "Windows" or "resource" in sys.modules:
        return
    resource_stub = _ResourceModule("resource")
    resource_stub.RLIMIT_NOFILE = 0
    sys.modules["resource"] = resource_stub


class SweBenchHarness:
    def __init__(
        self,
        runtime: DockerRuntime,
        *,
        cache_dir: Path,
        output_dir: Path,
        score_timeout_seconds: int = 900,
    ) -> None:
        self._runtime = runtime
        self._cache_dir = cache_dir
        self._output_dir = output_dir
        self._score_timeout_seconds = score_timeout_seconds

    def ensure_instance_image(self, item: SweBenchItem) -> float:
        image = self._runtime.instance_image(item.instance_id)
        if self._runtime.image_exists(image):
            return 0.0
        started = time.monotonic()
        install_windows_resource_stub()
        docker_module = import_module("docker")
        docker_build = import_module("swebench.harness.docker_build")
        constants = import_module("swebench.harness.constants")
        test_spec_module = import_module("swebench.harness.test_spec.test_spec")
        client = docker_module.from_env()
        try:
            test_spec = test_spec_module.make_test_spec(item.official_instance())
            build_root = self._cache_dir / "build-images"
            _configure_swebench_build_roots(constants, docker_build, build_root)
            with _force_lf_text_writes():
                docker_build.build_env_images(
                    client=client,
                    dataset=[test_spec],
                    force_rebuild=False,
                    max_workers=1,
                    env_image_tag="latest",
                )
                if not self._runtime.image_exists(test_spec.env_image_key):
                    raise RuntimeError(
                        "SWE-bench environment image was not created: "
                        f"{test_spec.env_image_key}"
                    )
                build_dir = (
                    build_root
                    / "instances"
                    / test_spec.instance_image_key.replace(":", "__")
                )
                for attempt in range(1, 4):
                    try:
                        docker_build.build_image(
                            image_name=test_spec.instance_image_key,
                            setup_scripts={
                                "setup_repo.sh": _constrain_python_build_dependencies(
                                    test_spec.install_repo_script
                                )
                            },
                            dockerfile=test_spec.instance_dockerfile,
                            platform=test_spec.platform,
                            client=client,
                            build_dir=build_dir,
                            nocache=False,
                        )
                        break
                    except Exception:
                        if attempt == 3:
                            raise
                        time.sleep(2.0 * attempt)
        finally:
            _close_docker_client(client)
        if not self._runtime.image_exists(image):
            raise RuntimeError(f"SWE-bench instance image was not created: {image}")
        return time.monotonic() - started

    def score(
        self,
        *,
        item: SweBenchItem,
        condition: str,
        generated_patch: str,
    ) -> tuple[SweBenchDiagnostics, float]:
        if platform.system() == "Windows" and not os.environ.get(
            "RELAY_SKILL_EVAL_SCORE_WORKER"
        ):
            return self._score_in_subprocess(
                item=item,
                condition=condition,
                generated_patch=generated_patch,
            )
        return self._score_direct(
            item=item,
            condition=condition,
            generated_patch=generated_patch,
        )

    def _score_in_subprocess(
        self,
        *,
        item: SweBenchItem,
        condition: str,
        generated_patch: str,
    ) -> tuple[SweBenchDiagnostics, float]:
        started = time.monotonic()
        model_name = f"pi-deepseek-v4-flash-{condition}"
        run_id = f"{item.instance_id}-{condition}-{uuid.uuid4().hex[:12]}"
        artifact_root = (
            self._output_dir
            / "official-scorer"
            / run_id
            / model_name
            / item.instance_id
        )
        artifact_root.mkdir(parents=True, exist_ok=True)
        result_path = artifact_root / "worker-result.json"
        log_path = artifact_root / "worker.log"
        result_path.unlink(missing_ok=True)
        payload = {
            "item": item.model_dump(mode="json", by_alias=True),
            "condition": condition,
            "generated_patch": generated_patch,
            "cache_dir": str(self._cache_dir),
            "output_dir": str(self._output_dir),
            "score_timeout_seconds": self._score_timeout_seconds,
            "result_path": str(result_path),
            "run_id": run_id,
        }
        environment = os.environ.copy()
        environment.pop("DEEPSEEK_API_KEY", None)
        environment["RELAY_SKILL_EVAL_SCORE_WORKER"] = "1"
        try:
            result = subprocess.run(
                [sys.executable, "-m", "relay_knowledge_skill_eval.score_worker"],
                input=json.dumps(payload, ensure_ascii=False),
                capture_output=True,
                text=True,
                encoding="utf-8",
                errors="replace",
                timeout=self._score_timeout_seconds + 120,
                env=environment,
                check=False,
            )
        except subprocess.TimeoutExpired as error:
            container_name = f"sweb.eval.{item.instance_id.lower()}.{run_id}"
            with suppress(Exception):
                self._runtime.run_command(
                    ["docker", "rm", "-f", container_name],
                    check=False,
                    timeout=60,
                )
            log_path.write_text(
                "SWE-bench scorer worker exceeded its outer timeout; "
                f"cleanup requested for {container_name}.\n",
                encoding="utf-8",
                newline="\n",
            )
            raise RuntimeError(
                f"Isolated SWE-bench scorer timed out for {item.instance_id}"
            ) from error
        log_path.write_text(
            f"stdout:\n{result.stdout}\nstderr:\n{result.stderr}",
            encoding="utf-8",
            newline="\n",
        )
        if result.returncode != 0 or not result_path.is_file():
            raise RuntimeError(
                "Isolated SWE-bench scorer failed: "
                f"{result.stderr[-4000:] or result.stdout[-4000:]}"
            )
        worker_result = json.loads(result_path.read_text(encoding="utf-8"))
        if not isinstance(worker_result, dict):
            raise RuntimeError("Isolated SWE-bench scorer returned invalid JSON")
        diagnostics = SweBenchDiagnostics.model_validate(
            worker_result.get("diagnostics", {})
        )
        return diagnostics, time.monotonic() - started

    def _score_direct(
        self,
        *,
        item: SweBenchItem,
        condition: str,
        generated_patch: str,
        run_id: str | None = None,
    ) -> tuple[SweBenchDiagnostics, float]:
        started = time.monotonic()
        install_windows_resource_stub()
        constants = import_module("swebench.harness.constants")
        run_module = import_module("swebench.harness.run_evaluation")
        test_spec_module = import_module("swebench.harness.test_spec.test_spec")
        docker_module = import_module("docker")
        score_root = self._output_dir / "official-scorer"
        constants.RUN_EVALUATION_LOG_DIR = score_root
        run_module.RUN_EVALUATION_LOG_DIR = score_root
        test_spec = test_spec_module.make_test_spec(item.official_instance())
        model_name = f"pi-deepseek-v4-flash-{condition}"
        run_id = run_id or f"{item.instance_id}-{condition}-{uuid.uuid4().hex[:12]}"
        prediction = {
            constants.KEY_INSTANCE_ID: item.instance_id,
            constants.KEY_MODEL: model_name,
            constants.KEY_PREDICTION: generated_patch or None,
        }
        client = docker_module.from_env()
        try:
            result = self._run_instance(
                run_module.run_instance,
                test_spec=test_spec,
                prediction=prediction,
                client=client,
                run_id=run_id,
            )
        finally:
            _close_docker_client(client)
        artifact_root = score_root / run_id / model_name / item.instance_id
        report_path = artifact_root / "report.json"
        test_output_path = artifact_root / constants.LOG_TEST_OUTPUT
        if not bool(result.get("completed", False)):
            instance_log = artifact_root / constants.LOG_INSTANCE
            try:
                log_text = instance_log.read_text(encoding="utf-8", errors="replace")
            except OSError:
                log_text = ""
            try:
                test_output_text = test_output_path.read_text(
                    encoding="utf-8", errors="replace"
                )
            except OSError:
                test_output_text = ""
            patch_apply_failed = constants.APPLY_PATCH_FAIL in log_text
            test_timed_out = (
                "Test timed out after" in log_text
                or "Timeout error:" in test_output_text
            )
            if not patch_apply_failed and not test_timed_out:
                raise RuntimeError(
                    "SWE-bench scorer did not complete for "
                    f"{item.instance_id}; see {instance_log}"
                )
        report = _load_instance_report(report_path, item.instance_id)
        diagnostics = _diagnostics_from_report(
            report,
            completed=bool(result.get("completed", False)),
            resolved=bool(result.get("resolved", False)),
            report_path=report_path,
            test_output_path=test_output_path,
        )
        if not diagnostics.completed:
            diagnostics = diagnostics.model_copy(
                update={
                    "patch_exists": bool(generated_patch),
                    "patch_applied": bool(generated_patch) and test_timed_out,
                }
            )
        return diagnostics, time.monotonic() - started

    def _run_instance(
        self,
        callable_run: object,
        *,
        test_spec: object,
        prediction: dict[str, str | None],
        client: object,
        run_id: str,
    ) -> dict[str, object]:
        if not callable(callable_run):
            raise RuntimeError("swebench run_instance is unavailable")
        keyword_arguments = {
            "test_spec": test_spec,
            "pred": prediction,
            "rm_image": False,
            "force_rebuild": False,
            "client": client,
            "run_id": run_id,
            "timeout": self._score_timeout_seconds,
        }
        with _force_lf_text_writes():
            value = callable_run(**keyword_arguments)
        if not isinstance(value, dict):
            raise RuntimeError("swebench run_instance returned a non-object")
        return value


_ORIGINAL_OPEN = builtins.open


def _close_docker_client(client: object) -> None:
    """Release Docker SDK transports without masking clients lacking close()."""
    close = getattr(client, "close", None)
    if callable(close):
        close()


def _configure_swebench_build_roots(
    constants: object,
    docker_build: object,
    build_root: Path,
) -> None:
    """Redirect both SWE-bench modules that cache build-directory globals."""
    constants.ENV_IMAGE_BUILD_DIR = build_root / "env"
    constants.INSTANCE_IMAGE_BUILD_DIR = build_root / "instances"
    docker_build.ENV_IMAGE_BUILD_DIR = build_root / "env"
    docker_build.INSTANCE_IMAGE_BUILD_DIR = build_root / "instances"


@contextmanager
def _force_lf_text_writes() -> Iterator[None]:
    """Keep Linux-bound SWE-bench scripts LF-terminated on Windows hosts."""
    if platform.system() != "Windows":
        yield
        return
    with (
        patch.object(Path, "write_text", _write_text_unix),
        patch("builtins.open", _open_text_utf8),
    ):
        yield


def _constrain_python_build_dependencies(script: str) -> str:
    """Keep legacy benchmark builds stable as packaging defaults evolve."""
    prelude = (
        "cat > /tmp/relay-swebench-build-constraints.txt <<'EOF'\n"
        f"{PYTHON_BUILD_CONSTRAINT}\n"
        "EOF\n"
        "export PIP_CONSTRAINT=/tmp/relay-swebench-build-constraints.txt\n"
        "export PIP_BUILD_CONSTRAINT=/tmp/relay-swebench-build-constraints.txt\n"
        "git config --global http.version HTTP/1.1\n"
        "relay_git_clone() {\n"
        '    local destination="${@: -1}"\n'
        "    local attempt\n"
        "    for attempt in 1 2 3; do\n"
        "        cd /\n"
        '        if command git clone "$@"; then\n'
        "            return 0\n"
        "        fi\n"
        '        rm -rf -- "$destination"\n'
        '        if [ "$attempt" -lt 3 ]; then\n'
        "            sleep $((attempt * 5))\n"
        "        fi\n"
        "    done\n"
        "    return 1\n"
        "}\n"
        "relay_git_checkout() {\n"
        '    local repository_url="$1"\n'
        '    local destination="$2"\n'
        '    local revision="$3"\n'
        "    local attempt\n"
        '    rm -rf -- "$destination"\n'
        '    mkdir -p -- "$destination"\n'
        '    command git -C "$destination" init\n'
        '    command git -C "$destination" remote add origin "$repository_url"\n'
        "    for attempt in 1 2 3; do\n"
        '        if command git -C "$destination" fetch --depth=1 origin '
        '"$revision"; then\n'
        '            command git -C "$destination" checkout --detach FETCH_HEAD\n'
        "            return 0\n"
        "        fi\n"
        '        if [ "$attempt" -lt 3 ]; then\n'
        "            sleep $((attempt * 5))\n"
        "        fi\n"
        "    done\n"
        "    return 1\n"
        "}\n"
        "relay_git_submodules() {\n"
        '    local repository="$1"\n'
        "    local attempt\n"
        '    if [ ! -f "$repository/.gitmodules" ]; then\n'
        "        return 0\n"
        "    fi\n"
        "    for attempt in 1 2 3; do\n"
        '        if command git -C "$repository" -c submodule.fetchJobs=1 '
        "submodule update --init --recursive --depth=1; then\n"
        "            return 0\n"
        "        fi\n"
        '        command git -C "$repository" submodule deinit --force --all || true\n'
        '        rm -rf -- "$repository/.git/modules"\n'
        '        if [ "$attempt" -lt 3 ]; then\n'
        "            sleep $((attempt * 5))\n"
        "        fi\n"
        "    done\n"
        "    return 1\n"
        "}\n"
    )
    lines = script.splitlines(keepends=True)
    revision = next(
        (
            parts[3]
            for line in lines
            if (parts := _shell_parts(line))[:3] == ["git", "reset", "--hard"]
            and len(parts) == 4
        ),
        None,
    )
    clone_destination: str | None = None
    transformed: list[str] = []
    for line in lines:
        stripped = line.lstrip()
        indent = line[: len(line) - len(stripped)]
        parts = _shell_parts(stripped)
        if parts[:2] == ["git", "clone"] and len(parts) >= 4 and revision:
            repository_url, clone_destination = parts[-2:]
            transformed.append(
                f"{indent}relay_git_checkout {shlex.quote(repository_url)} "
                f"{shlex.quote(clone_destination)} {shlex.quote(revision)}\n"
            )
            continue
        if parts[:2] == ["git", "clone"]:
            transformed.append(
                f"{indent}relay_git_clone {stripped[len('git clone ') :]}"
            )
            continue
        if parts == ["git", "remote", "remove", "origin"] and clone_destination:
            transformed.append(
                f"{indent}relay_git_submodules {shlex.quote(clone_destination)}\n"
            )
        if parts[:2] == ["conda", "activate"]:
            transformed.append(line)
            transformed.append(
                f"{indent}python -m pip install --disable-pip-version-check "
                f"{shlex.quote(PYTHON_BUILD_PIP)}\n"
            )
            transformed.append(f"{indent}export PIP_USE_PEP517=0\n")
            continue
        transformed.append(line)
    lines = transformed
    if lines and lines[0].startswith("#!"):
        return lines[0] + prelude + "".join(lines[1:])
    return prelude + "".join(lines)


def _shell_parts(line: str) -> list[str]:
    try:
        return shlex.split(line.strip())
    except ValueError:
        return []


def _write_text_unix(
    self: Path,
    data: str,
    encoding: str | None = None,
    errors: str | None = None,
    newline: str | None = None,
) -> int:
    _ = newline
    return self.write_bytes(data.encode(encoding or "utf-8", errors or "strict"))


def _open_text_utf8(
    file: str | bytes | int | Path,
    mode: str = "r",
    buffering: int = -1,
    encoding: str | None = None,
    errors: str | None = None,
    newline: str | None = None,
    closefd: bool = True,
    opener: object = None,
):
    text_mode = "b" not in mode
    write_mode = any(flag in mode for flag in "wax+")
    if text_mode and encoding is None:
        encoding = "utf-8"
    if text_mode and write_mode and newline is None:
        newline = "\n"
    return _ORIGINAL_OPEN(
        file,
        mode,
        buffering,
        encoding,
        errors,
        newline,
        closefd,
        opener,
    )


def _load_instance_report(path: Path, instance_id: str) -> dict[str, object]:
    if not path.exists():
        return {}
    value = json.loads(path.read_text(encoding="utf-8"))
    if not isinstance(value, dict):
        return {}
    instance = value.get(instance_id)
    return instance if isinstance(instance, dict) else {}


def _bucket(value: object) -> TestBucket:
    if not isinstance(value, dict):
        return TestBucket()
    success = value.get("success")
    failure = value.get("failure")
    return TestBucket(
        success=tuple(item for item in success if isinstance(item, str))
        if isinstance(success, list)
        else (),
        failure=tuple(item for item in failure if isinstance(item, str))
        if isinstance(failure, list)
        else (),
    )


def _diagnostics_from_report(
    report: dict[str, object],
    *,
    completed: bool,
    resolved: bool,
    report_path: Path,
    test_output_path: Path,
) -> SweBenchDiagnostics:
    tests = report.get("tests_status")
    tests_status = tests if isinstance(tests, dict) else {}
    resolved = (
        report.get("resolved") if isinstance(report.get("resolved"), bool) else resolved
    )
    fail_to_pass = _bucket(tests_status.get("FAIL_TO_PASS"))
    pass_to_pass = _bucket(tests_status.get("PASS_TO_PASS"))
    f2p_total = len(fail_to_pass.success) + len(fail_to_pass.failure)
    p2p_total = len(pass_to_pass.success) + len(pass_to_pass.failure)
    f2p_ratio = len(fail_to_pass.success) / f2p_total if f2p_total else 1.0
    p2p_ratio = len(pass_to_pass.success) / p2p_total if p2p_total else 1.0
    resolution = (
        "full"
        if resolved
        else "partial"
        if 0.0 < f2p_ratio < 1.0 and p2p_ratio == 1.0
        else "none"
    )
    return SweBenchDiagnostics(
        completed=completed,
        resolved=resolved,
        resolution_status=resolution,
        patch_exists=bool(report.get("patch_exists", False)),
        patch_applied=bool(report.get("patch_successfully_applied", False)),
        fail_to_pass=fail_to_pass,
        pass_to_pass=pass_to_pass,
        report_path=str(report_path),
        test_output_path=str(test_output_path),
    )
