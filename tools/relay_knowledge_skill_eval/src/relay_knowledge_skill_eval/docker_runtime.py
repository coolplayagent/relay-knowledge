from __future__ import annotations

import ipaddress
import os
import shutil
import subprocess
import threading
import time
import uuid
from collections.abc import Sequence
from pathlib import Path

from relay_knowledge_skill_eval.security import SecretRedactor

RELAY_KNOWLEDGE_ENVIRONMENT = (
    ("RELAY_KNOWLEDGE_HOME", "/tmp/relay-knowledge-home"),
    ("RELAY_KNOWLEDGE_SEMANTIC_BACKEND", "local"),
    ("RELAY_KNOWLEDGE_VECTOR_BACKEND", "local"),
)


def merge_relay_environment(extra_env: dict[str, str]) -> dict[str, str]:
    """Preserve caller credentials while enforcing the indexed runtime location."""
    environment = dict(extra_env)
    environment.update(RELAY_KNOWLEDGE_ENVIRONMENT)
    return environment


class DockerRuntime:
    def __init__(
        self,
        *,
        tool_root: Path,
        cache_dir: Path,
        runtime_image: str,
        image_prefix: str,
        api_key: str,
        redactor: SecretRedactor,
    ) -> None:
        self.tool_root = tool_root
        self.cache_dir = cache_dir
        self.runtime_image = runtime_image
        self.image_prefix = image_prefix
        self.api_key = api_key
        self.redactor = redactor
        self.runtime_container = ""
        self.network_name = ""
        self.egress_proxy = ""
        self.egress_proxy_ip = ""
        self._runtime_lock = threading.Lock()

    def check_ready(self) -> str:
        result = self.run_command(
            ["docker", "info", "--format", "{{.ServerVersion}}"], check=False
        )
        if result.returncode != 0 or not result.stdout.strip():
            detail = self.redactor.redact(result.stderr.strip())
            raise RuntimeError(f"Docker Linux engine is unavailable: {detail}")
        return result.stdout.strip()

    def build_runtime(self, skill_dir: Path, *, pi_version: str) -> float:
        started = time.monotonic()
        context = self.cache_dir / f".runtime-context-{uuid.uuid4().hex}.tmp"
        context.mkdir(parents=True)
        try:
            shutil.copytree(skill_dir, context / "skill")
            shutil.copy2(self.tool_root / "docker" / "pi-eval", context / "pi-eval")
            shutil.copy2(
                self.tool_root / "docker" / "deepseek-egress-proxy.mjs",
                context / "deepseek-egress-proxy.mjs",
            )
            self.run_command(
                [
                    "docker",
                    "build",
                    "--platform",
                    "linux/amd64",
                    "--build-arg",
                    f"PI_VERSION={pi_version}",
                    "-f",
                    str(self.tool_root / "docker" / "Dockerfile.runtime"),
                    "-t",
                    self.runtime_image,
                    str(context),
                ]
            )
        finally:
            shutil.rmtree(context, ignore_errors=True)
        return time.monotonic() - started

    def image_exists(self, image: str) -> bool:
        result = self.run_command(["docker", "image", "inspect", image], check=False)
        return result.returncode == 0

    def instance_image(self, instance_id: str) -> str:
        return f"{self.image_prefix}.{instance_id}:latest"

    def start_runtime_container(self) -> None:
        with self._runtime_lock:
            if self.runtime_container:
                return
            name = f"relay-skill-eval-runtime-{uuid.uuid4().hex[:10]}"
            network = f"relay-skill-eval-net-{uuid.uuid4().hex[:10]}"
            proxy = f"relay-skill-eval-egress-{uuid.uuid4().hex[:10]}"
            try:
                self.run_command(
                    ["docker", "create", "--name", name, self.runtime_image]
                )
                self.run_command(["docker", "network", "create", "--internal", network])
                self.run_command(
                    [
                        "docker",
                        "run",
                        "-d",
                        "--name",
                        proxy,
                        "--network",
                        "bridge",
                        self.runtime_image,
                        "/opt/pi-eval/bin/node",
                        "/opt/pi-eval/bin/deepseek-egress-proxy.mjs",
                    ]
                )
                self.run_command(["docker", "network", "connect", network, proxy])
                address = self.run_command(
                    [
                        "docker",
                        "inspect",
                        "--format",
                        (
                            '{{(index .NetworkSettings.Networks "'
                            + network
                            + '").IPAddress}}'
                        ),
                        proxy,
                    ]
                ).stdout.strip()
                proxy_ip = str(ipaddress.ip_address(address))
            except Exception:
                self.remove_container(proxy)
                self.remove_container(name)
                self.run_command(["docker", "network", "rm", network], check=False)
                self.runtime_container = ""
                self.network_name = ""
                self.egress_proxy = ""
                self.egress_proxy_ip = ""
                raise
            self.runtime_container = name
            self.network_name = network
            self.egress_proxy = proxy
            self.egress_proxy_ip = proxy_ip

    def start_instance(self, instance_id: str, condition: str) -> tuple[str, float]:
        self.start_runtime_container()
        name = f"relay-skill-eval-{condition}-{uuid.uuid4().hex[:10]}"
        started = time.monotonic()
        command = [
            "docker",
            "run",
            "-d",
            "--name",
            name,
            "--network",
            self.network_name,
            "--add-host",
            f"api.deepseek.com:{self.egress_proxy_ip}",
            "--volumes-from",
            f"{self.runtime_container}:ro",
            "-e",
            "DEEPSEEK_API_KEY",
        ]
        for variable, value in RELAY_KNOWLEDGE_ENVIRONMENT:
            command.extend(["-e", f"{variable}={value}"])
        command.extend([self.instance_image(instance_id), "sleep", "infinity"])
        environment = os.environ.copy()
        environment["DEEPSEEK_API_KEY"] = self.api_key
        self.run_command(command, env=environment)
        return name, time.monotonic() - started

    def exec(
        self,
        container: str,
        command: Sequence[str],
        *,
        check: bool = True,
        timeout: float | None = None,
        stdin: str | None = None,
        environment: Sequence[str] = (),
    ) -> subprocess.CompletedProcess[str]:
        docker_command = ["docker", "exec", "-i", "-w", "/testbed"]
        for variable in environment:
            docker_command.extend(["-e", variable])
        docker_command.append(container)
        docker_command.extend(command)
        return self.run_command(
            docker_command,
            check=check,
            timeout=timeout,
            stdin=stdin,
        )

    def remove_container(self, name: str) -> None:
        if name:
            self.run_command(["docker", "rm", "-f", "-v", name], check=False)

    def close(self) -> None:
        self.remove_container(self.egress_proxy)
        self.egress_proxy = ""
        self.egress_proxy_ip = ""
        self.remove_container(self.runtime_container)
        self.runtime_container = ""
        if self.network_name:
            self.run_command(
                ["docker", "network", "rm", self.network_name], check=False
            )
            self.network_name = ""

    def run_command(
        self,
        command: Sequence[str],
        *,
        check: bool = True,
        timeout: float | None = None,
        stdin: str | None = None,
        env: dict[str, str] | None = None,
    ) -> subprocess.CompletedProcess[str]:
        result = subprocess.run(
            list(command),
            input=stdin,
            capture_output=True,
            text=True,
            encoding="utf-8",
            errors="replace",
            timeout=timeout,
            env=env,
            check=False,
        )
        if check and result.returncode != 0:
            stderr = self.redactor.redact(result.stderr[-8000:])
            stdout = self.redactor.redact(result.stdout[-8000:])
            raise RuntimeError(
                f"Command failed ({result.returncode}): "
                f"{command[0]}\n{stdout}\n{stderr}"
            )
        return result
