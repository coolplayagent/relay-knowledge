from __future__ import annotations

import subprocess
from pathlib import Path

import pytest

from relay_knowledge_skill_eval.cli import make_runtime
from relay_knowledge_skill_eval.docker_runtime import (
    RELAY_KNOWLEDGE_ENVIRONMENT,
    DockerRuntime,
    merge_relay_environment,
)
from relay_knowledge_skill_eval.models import Condition, RuntimePaths, SweBenchItem
from relay_knowledge_skill_eval.runner import (
    build_prompt,
    contains_provider_configuration_error,
    contains_transport_error,
    pi_command,
    pi_docker_exec_command,
    recoverable_agent_failure,
    stable_condition_order,
    stream_has_transport_error,
)
from relay_knowledge_skill_eval.security import SecretRedactor


def test_redactor_covers_exact_and_key_shaped_secrets() -> None:
    secret = "custom-sensitive-value"
    redactor = SecretRedactor(secret)
    output = redactor.redact(f"x={secret} bearer sk-1234567890abcdefghijkl")
    assert secret not in output
    assert "sk-" not in output
    assert output.count("[REDACTED]") == 2


def test_pi_docker_exec_uses_one_explicit_testbed_workdir() -> None:
    command = pi_docker_exec_command("candidate-container", "pi command")

    assert command == [
        "docker",
        "exec",
        "-i",
        "-w",
        "/testbed",
        "candidate-container",
        "setsid",
        "sh",
        "-c",
        "pi command",
    ]
    assert command.count("-w") == 1


def test_ab_commands_differ_only_by_skill_argument() -> None:
    baseline = pi_command(
        condition=Condition.BASELINE,
        model="deepseek-v4-flash",
        thinking="high",
    )
    treatment = pi_command(
        condition=Condition.SKILL,
        model="deepseek-v4-flash",
        thinking="high",
    )
    skill_index = treatment.index("--skill")
    without_skill = treatment[:skill_index] + treatment[skill_index + 2 :]
    assert without_skill == baseline
    assert "--no-skills" in baseline
    assert "--no-extensions" in baseline
    assert "--no-prompt-templates" in baseline
    assert treatment[skill_index + 1].endswith("/skill/SKILL.md")
    assert baseline[:2] == ["bash", "-lc"]
    assert 'exec "$@" "$prompt"' in baseline[2]
    assert baseline[-1] != "Complete the software task provided on standard input."


def test_forced_skill_instruction_is_treatment_only() -> None:
    baseline = build_prompt(
        "fix it",
        condition=Condition.BASELINE,
        require_skill_use=True,
    )
    treatment = build_prompt(
        "fix it",
        condition=Condition.SKILL,
        require_skill_use=True,
    )
    assert "must use the loaded relay-knowledge-cli skill" not in baseline
    assert "must use the loaded relay-knowledge-cli skill" in treatment
    assert "requirement is mandatory" in treatment


def test_agent_prompt_excludes_official_gold_fields() -> None:
    item = SweBenchItem(
        instance_id="example__project-1",
        repo="example/project",
        base_commit="abc123",
        problem_statement="Fix the public issue description.",
        patch="GOLD_PATCH_SENTINEL",
        test_patch="HIDDEN_TEST_SENTINEL",
        hints_text="GOLD_HINT_SENTINEL",
        FAIL_TO_PASS='["hidden::test"]',
        PASS_TO_PASS='["existing::test"]',
    )

    prompt = build_prompt(item.problem_statement)

    assert item.problem_statement in prompt
    for hidden_value in (
        item.patch,
        item.test_patch,
        item.hints_text,
        item.fail_to_pass,
        item.pass_to_pass,
    ):
        assert hidden_value not in prompt


def test_continuation_reuses_session_without_changing_condition() -> None:
    initial = pi_command(
        condition=Condition.SKILL,
        model="deepseek-v4-flash",
        thinking="high",
    )
    continued = pi_command(
        condition=Condition.SKILL,
        model="deepseek-v4-flash",
        thinking="high",
        continue_session=True,
    )
    assert "--session-dir" in initial
    assert "--continue" not in initial
    continued_without_flag = continued.copy()
    continued_without_flag.remove("--continue")
    assert continued_without_flag == initial


def test_network_failures_are_recoverable_but_auth_failures_are_not() -> None:
    assert contains_transport_error("upstream connection reset")
    assert contains_transport_error("Request timed out while calling provider")
    assert contains_transport_error("provider timeout")
    assert contains_provider_configuration_error("Invalid API key")
    assert stream_has_transport_error("stderr", "provider timeout")
    assert not stream_has_transport_error(
        "stdout", '{"type":"tool_result","content":"test timeout 429"}'
    )
    assert stream_has_transport_error(
        "stdout", '{"type":"provider_error","message":"request timed out"}'
    )
    assert recoverable_agent_failure(
        returncode=1,
        stalled=False,
        transport_error=True,
        stderr="connection reset",
    )
    assert not recoverable_agent_failure(
        returncode=1,
        stalled=False,
        transport_error=False,
        stderr="Invalid API key",
    )
    assert not recoverable_agent_failure(
        returncode=1,
        stalled=False,
        transport_error=False,
        stderr="deterministic agent failure",
    )
    assert recoverable_agent_failure(
        returncode=75,
        stalled=False,
        transport_error=False,
        stderr="temporary process failure",
    )


def test_stable_order_is_deterministic_and_balanced() -> None:
    ids = [f"case-{index}" for index in range(100)]
    orders = [stable_condition_order(instance_id) for instance_id in ids]
    assert orders == [stable_condition_order(instance_id) for instance_id in ids]
    baseline_first = sum(order[0] is Condition.BASELINE for order in orders)
    assert 35 <= baseline_first <= 65


def test_docker_key_is_forwarded_by_name_not_command_value(
    tmp_path: Path, monkeypatch
) -> None:
    secret = "sensitive-environment-only"
    runtime = DockerRuntime(
        tool_root=tmp_path,
        cache_dir=tmp_path,
        runtime_image="runtime",
        image_prefix="prefix",
        api_key=secret,
        redactor=SecretRedactor(secret),
    )
    calls: list[tuple[list[str], dict[str, str] | None]] = []

    def fake_run(command, **kwargs):
        calls.append((list(command), kwargs.get("env")))
        return subprocess.CompletedProcess(command, 0, "", "")

    monkeypatch.setattr(runtime, "run_command", fake_run)
    runtime.runtime_container = "runtime-data"
    runtime.network_name = "eval-network"
    runtime.egress_proxy = "egress-proxy"
    runtime.egress_proxy_ip = "172.30.0.2"
    runtime.start_instance("case", "baseline")
    command, environment = calls[-1]
    assert secret not in " ".join(command)
    assert command[command.index("-e") + 1] == "DEEPSEEK_API_KEY"
    assert environment is not None
    assert environment["DEEPSEEK_API_KEY"] == secret
    assert command[command.index("--network") + 1] == "eval-network"
    assert command[command.index("--add-host") + 1] == ("api.deepseek.com:172.30.0.2")
    assert command[command.index("--volumes-from") + 1] == "runtime-data:ro"
    for variable, value in RELAY_KNOWLEDGE_ENVIRONMENT:
        assert f"{variable}={value}" in command


def test_runtime_image_build_targets_linux_amd64(tmp_path: Path, monkeypatch) -> None:
    tool_root = tmp_path / "tool"
    docker_dir = tool_root / "docker"
    docker_dir.mkdir(parents=True)
    (docker_dir / "pi-eval").write_text("wrapper", encoding="utf-8")
    (docker_dir / "deepseek-egress-proxy.mjs").write_text("proxy", encoding="utf-8")
    skill_dir = tmp_path / "skill"
    skill_dir.mkdir()
    (skill_dir / "SKILL.md").write_text("skill", encoding="utf-8")
    runtime = DockerRuntime(
        tool_root=tool_root,
        cache_dir=tmp_path / "cache",
        runtime_image="runtime",
        image_prefix="prefix",
        api_key="secret",
        redactor=SecretRedactor("secret"),
    )
    calls: list[list[str]] = []

    def fake_run(command, **_kwargs):
        calls.append(list(command))
        return subprocess.CompletedProcess(command, 0, "", "")

    monkeypatch.setattr(runtime, "run_command", fake_run)
    runtime.build_runtime(skill_dir, pi_version="0.80.3")

    command = calls[-1]
    assert command[:4] == ["docker", "build", "--platform", "linux/amd64"]
    context = Path(command[-1])
    assert context.name.startswith(".runtime-context-")
    assert context.name.endswith(".tmp")
    assert not context.exists()
    assert not list((tmp_path / "cache").glob(".runtime-context-*.tmp"))


def test_runtime_network_uses_fixed_deepseek_egress_proxy(
    tmp_path: Path, monkeypatch
) -> None:
    runtime = DockerRuntime(
        tool_root=tmp_path,
        cache_dir=tmp_path,
        runtime_image="runtime",
        image_prefix="prefix",
        api_key="secret",
        redactor=SecretRedactor("secret"),
    )
    calls: list[list[str]] = []

    def fake_run(command, **kwargs):
        _ = kwargs
        calls.append(list(command))
        stdout = "172.30.0.2\n" if command[1:3] == ["inspect", "--format"] else ""
        return subprocess.CompletedProcess(command, 0, stdout, "")

    monkeypatch.setattr(runtime, "run_command", fake_run)
    runtime.start_runtime_container()

    assert any(
        command[:4] == ["docker", "network", "create", "--internal"]
        for command in calls
    )
    proxy = next(command for command in calls if command[:3] == ["docker", "run", "-d"])
    assert proxy[proxy.index("--network") + 1] == "bridge"
    connect = next(
        command for command in calls if command[:3] == ["docker", "network", "connect"]
    )
    assert "--alias" not in connect
    assert connect[-1] == runtime.egress_proxy
    inspect = next(
        command for command in calls if command[:3] == ["docker", "inspect", "--format"]
    )
    assert inspect[3] == (
        '{{(index .NetworkSettings.Networks "' + runtime.network_name + '").IPAddress}}'
    )


def test_partial_runtime_setup_is_cleaned_and_can_retry(
    tmp_path: Path, monkeypatch
) -> None:
    runtime = DockerRuntime(
        tool_root=tmp_path,
        cache_dir=tmp_path,
        runtime_image="runtime",
        image_prefix="prefix",
        api_key="secret",
        redactor=SecretRedactor("secret"),
    )
    calls: list[list[str]] = []
    failed = False

    def fake_run(command, **kwargs):
        nonlocal failed
        _ = kwargs
        calls.append(list(command))
        if command[1:3] == ["network", "create"] and not failed:
            failed = True
            raise RuntimeError("transient Docker failure")
        stdout = "172.30.0.2\n" if command[1:3] == ["inspect", "--format"] else ""
        return subprocess.CompletedProcess(command, 0, stdout, "")

    monkeypatch.setattr(runtime, "run_command", fake_run)

    with pytest.raises(RuntimeError, match="transient Docker failure"):
        runtime.start_runtime_container()
    assert runtime.runtime_container == ""
    assert runtime.network_name == ""
    assert runtime.egress_proxy == ""
    assert any(command[:3] == ["docker", "rm", "-f"] for command in calls)

    runtime.start_runtime_container()
    assert runtime.runtime_container
    assert runtime.network_name
    assert runtime.egress_proxy_ip == "172.30.0.2"


def test_agent_environment_reuses_preindexed_relay_home() -> None:
    environment = merge_relay_environment(
        {
            "DEEPSEEK_API_KEY": "secret",
            "RELAY_KNOWLEDGE_HOME": "/wrong-home",
        }
    )

    assert environment["DEEPSEEK_API_KEY"] == "secret"
    assert environment["RELAY_KNOWLEDGE_HOME"] == "/tmp/relay-knowledge-home"


def test_runtime_image_identity_includes_skill_content_hash(tmp_path: Path) -> None:
    paths = RuntimePaths(
        workspace=tmp_path,
        tool_root=tmp_path,
        cache_dir=tmp_path / "cache",
        output_dir=tmp_path / "output",
        dataset_path=tmp_path / "dataset.jsonl",
    )
    first = make_runtime(
        paths,
        "1.1.13",
        skill_sha256="a" * 64,
        redactor=SecretRedactor(""),
        api_key="",
    )
    second = make_runtime(
        paths,
        "1.1.13",
        skill_sha256="b" * 64,
        redactor=SecretRedactor(""),
        api_key="",
    )

    assert first.runtime_image != second.runtime_image
    assert first.runtime_image.endswith(f"skill-{'a' * 64}")
    with pytest.raises(ValueError, match="SHA-256"):
        make_runtime(
            paths,
            "1.1.13",
            skill_sha256="not-a-digest",
            redactor=SecretRedactor(""),
            api_key="",
        )
