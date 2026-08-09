from __future__ import annotations

import builtins
import subprocess
from pathlib import Path
from types import SimpleNamespace

import pytest

from relay_knowledge_skill_eval.models import SweBenchItem
from relay_knowledge_skill_eval.swebench_support import (
    PYTHON_BUILD_CONSTRAINT,
    PYTHON_BUILD_PIP,
    SweBenchHarness,
    _configure_swebench_build_roots,
    _constrain_python_build_dependencies,
    _diagnostics_from_report,
    _force_lf_text_writes,
    _open_text_utf8,
)


class FakeDockerClient:
    def __init__(self) -> None:
        self.closed = False

    def close(self) -> None:
        self.closed = True


def test_swebench_build_roots_update_both_imported_modules(tmp_path: Path) -> None:
    constants = SimpleNamespace()
    docker_build = SimpleNamespace()

    _configure_swebench_build_roots(constants, docker_build, tmp_path)

    for module in (constants, docker_build):
        assert tmp_path / "env" == module.ENV_IMAGE_BUILD_DIR
        assert tmp_path / "instances" == module.INSTANCE_IMAGE_BUILD_DIR


def test_instance_build_pins_legacy_packaging_dependency() -> None:
    script = (
        "#!/bin/bash\n"
        "git clone https://example.test/repository /testbed\n"
        "git reset --hard 0123456789abcdef\n"
        "git remote remove origin\n"
        "conda activate testbed\n"
        'echo "a multiline value \\\n'
        'continues here"\n'
        "python -m pip install -e '.[test]'\n"
    )

    constrained = _constrain_python_build_dependencies(script)

    assert constrained.startswith("#!/bin/bash\n")
    assert PYTHON_BUILD_CONSTRAINT in constrained
    assert "PIP_BUILD_CONSTRAINT" in constrained
    assert "git config --global http.version HTTP/1.1" in constrained
    assert "for attempt in 1 2 3" in constrained
    assert "        cd /\n" in constrained
    assert (
        "relay_git_checkout https://example.test/repository /testbed 0123456789abcdef"
        in constrained
    )
    assert "relay_git_submodules /testbed\ngit remote remove origin" in constrained
    assert (
        f"python -m pip install --disable-pip-version-check {PYTHON_BUILD_PIP}"
        in constrained
    )
    assert "export PIP_USE_PEP517=0" in constrained
    assert 'echo "a multiline value \\\ncontinues here"' in constrained
    assert "\ngit clone https://example.test/repository /testbed" not in constrained
    assert constrained.endswith("python -m pip install -e '.[test]'\n")


def test_instance_build_without_shebang_keeps_transformed_commands() -> None:
    script = (
        "git clone https://example.test/repository /testbed\n"
        "git reset --hard 0123456789abcdef\n"
        "conda activate testbed\n"
    )

    constrained = _constrain_python_build_dependencies(script)

    assert constrained.startswith(
        "cat > /tmp/relay-swebench-build-constraints.txt <<'EOF'\n"
    )
    assert (
        "relay_git_checkout https://example.test/repository /testbed "
        "0123456789abcdef" in constrained
    )
    assert "\ngit clone https://example.test/repository /testbed" not in constrained
    assert PYTHON_BUILD_PIP in constrained
    assert "export PIP_USE_PEP517=0" in constrained


def test_windows_image_build_boundary_forces_lf_writes(monkeypatch) -> None:
    monkeypatch.setattr(
        "relay_knowledge_skill_eval.swebench_support.platform.system",
        lambda: "Windows",
    )

    with _force_lf_text_writes():
        assert builtins.open is _open_text_utf8
        assert Path.write_text.__name__ == "_write_text_unix"

    assert builtins.open is not _open_text_utf8


def test_instance_image_build_closes_docker_client(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    client = FakeDockerClient()
    instance_checks = 0

    def image_exists(image: str) -> bool:
        nonlocal instance_checks
        if image == "instance-image":
            instance_checks += 1
            return instance_checks > 1
        return True

    runtime = SimpleNamespace(
        instance_image=lambda _instance_id: "instance-image",
        image_exists=image_exists,
    )
    docker_build = SimpleNamespace(
        build_env_images=lambda **_kwargs: None,
        build_image=lambda **_kwargs: None,
    )
    test_spec = SimpleNamespace(
        env_image_key="environment-image",
        instance_image_key="instance-image",
        install_repo_script="echo setup\n",
        instance_dockerfile="FROM environment-image\n",
        platform="linux/amd64",
    )
    modules = {
        "docker": SimpleNamespace(from_env=lambda: client),
        "swebench.harness.docker_build": docker_build,
        "swebench.harness.constants": SimpleNamespace(),
        "swebench.harness.test_spec.test_spec": SimpleNamespace(
            make_test_spec=lambda _item: test_spec
        ),
    }
    monkeypatch.setattr(
        "relay_knowledge_skill_eval.swebench_support.import_module",
        lambda name: modules[name],
    )
    harness = SweBenchHarness(
        runtime, cache_dir=tmp_path / "cache", output_dir=tmp_path
    )
    item = SweBenchItem(
        instance_id="case",
        repo="org/repo",
        base_commit="abc",
        problem_statement="fix it",
    )

    harness.ensure_instance_image(item)

    assert client.closed is True


def test_scorer_worker_timeout_removes_known_container(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    cleanup_calls: list[tuple[list[str], bool, float]] = []

    def cleanup(command, *, check: bool, timeout: float):
        cleanup_calls.append((list(command), check, timeout))
        return subprocess.CompletedProcess(command, 0, "", "")

    runtime = SimpleNamespace(run_command=cleanup)
    harness = SweBenchHarness(
        runtime,
        cache_dir=tmp_path / "cache",
        output_dir=tmp_path,
        score_timeout_seconds=10,
    )
    monkeypatch.setattr(
        "relay_knowledge_skill_eval.swebench_support.uuid.uuid4",
        lambda: SimpleNamespace(hex="abc123def4567890"),
    )

    def timeout(*args, **kwargs):
        raise subprocess.TimeoutExpired(args[0], kwargs["timeout"])

    monkeypatch.setattr(
        "relay_knowledge_skill_eval.swebench_support.subprocess.run", timeout
    )
    item = SweBenchItem(
        instance_id="Astropy__Astropy-1",
        repo="astropy/astropy",
        base_commit="abc",
        problem_statement="fix it",
    )

    with pytest.raises(RuntimeError, match="scorer timed out"):
        harness._score_in_subprocess(
            item=item,
            condition="baseline",
            generated_patch="patch",
        )

    run_id = "Astropy__Astropy-1-baseline-abc123def456"
    container = f"sweb.eval.astropy__astropy-1.{run_id}"
    assert cleanup_calls == [(["docker", "rm", "-f", container], False, 60)]
    log_path = (
        tmp_path
        / "official-scorer"
        / run_id
        / "pi-deepseek-v4-flash-baseline"
        / item.instance_id
        / "worker.log"
    )
    assert container in log_path.read_text(encoding="utf-8")


def test_official_report_diagnostics_preserve_test_buckets(tmp_path: Path) -> None:
    report = {
        "resolved": False,
        "patch_exists": True,
        "patch_successfully_applied": False,
        "tests_status": {
            "FAIL_TO_PASS": {"success": ["fixed"], "failure": ["still-broken"]},
            "PASS_TO_PASS": {"success": ["stable"], "failure": []},
        },
    }
    diagnostics = _diagnostics_from_report(
        report,
        completed=True,
        resolved=False,
        report_path=tmp_path / "report.json",
        test_output_path=tmp_path / "test.log",
    )
    assert diagnostics.resolution_status == "partial"
    assert diagnostics.patch_exists is True
    assert diagnostics.patch_applied is False
    assert diagnostics.fail_to_pass.success == ("fixed",)
    assert diagnostics.fail_to_pass.failure == ("still-broken",)


@pytest.mark.parametrize("failure_kind", ["infrastructure", "patch", "test-timeout"])
def test_incomplete_scorer_result_distinguishes_candidate_failures(
    tmp_path: Path, monkeypatch, failure_kind: str
) -> None:
    constants = SimpleNamespace(
        RUN_EVALUATION_LOG_DIR=tmp_path,
        KEY_INSTANCE_ID="instance_id",
        KEY_MODEL="model_name_or_path",
        KEY_PREDICTION="model_patch",
        LOG_TEST_OUTPUT="test_output.txt",
        LOG_INSTANCE="run_instance.log",
        APPLY_PATCH_FAIL=">>>>> Patch Apply Failed",
    )
    test_spec_module = SimpleNamespace(
        make_test_spec=lambda _: SimpleNamespace(instance_id="case")
    )
    client = FakeDockerClient()
    modules = {
        "swebench.harness.constants": constants,
        "swebench.harness.run_evaluation": SimpleNamespace(
            RUN_EVALUATION_LOG_DIR=tmp_path,
            run_instance=lambda **_: {},
        ),
        "swebench.harness.test_spec.test_spec": test_spec_module,
        "docker": SimpleNamespace(from_env=lambda: client),
    }
    monkeypatch.setattr(
        "relay_knowledge_skill_eval.swebench_support.import_module",
        lambda name: modules[name],
    )
    monkeypatch.setattr(
        "relay_knowledge_skill_eval.swebench_support.uuid.uuid4",
        lambda: SimpleNamespace(hex="abc123def4567890"),
    )
    harness = SweBenchHarness(None, cache_dir=tmp_path / "cache", output_dir=tmp_path)

    def incomplete(_callable=None, **kwargs):
        _ = _callable
        artifact = (
            tmp_path
            / "official-scorer"
            / kwargs["run_id"]
            / "pi-deepseek-v4-flash-baseline"
            / "case"
        )
        artifact.mkdir(parents=True)
        marker = {
            "patch": constants.APPLY_PATCH_FAIL,
            "test-timeout": "Test timed out after 900 seconds.",
            "infrastructure": "Docker failed",
        }[failure_kind]
        (artifact / constants.LOG_INSTANCE).write_text(marker, encoding="utf-8")
        if failure_kind == "test-timeout":
            (artifact / constants.LOG_TEST_OUTPUT).write_text(
                "Timeout error: 900 seconds exceeded.", encoding="utf-8"
            )
        return {"completed": False, "resolved": False}

    monkeypatch.setattr(harness, "_run_instance", incomplete)
    item = SweBenchItem(
        instance_id="case",
        repo="org/repo",
        base_commit="abc",
        problem_statement="fix it",
    )

    if failure_kind != "infrastructure":
        diagnostics, _ = harness._score_direct(
            item=item, condition="baseline", generated_patch="bad patch"
        )
        assert diagnostics.completed is False
        assert diagnostics.patch_exists is True
        assert diagnostics.patch_applied is (failure_kind == "test-timeout")
    else:
        with pytest.raises(RuntimeError, match="scorer did not complete"):
            harness._score_direct(
                item=item, condition="baseline", generated_patch="patch"
            )
    assert client.closed is True
