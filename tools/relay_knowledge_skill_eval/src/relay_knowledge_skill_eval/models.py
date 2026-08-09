from __future__ import annotations

from datetime import UTC, datetime
from enum import StrEnum
from pathlib import Path

from pydantic import BaseModel, ConfigDict, Field


class Condition(StrEnum):
    BASELINE = "baseline"
    SKILL = "skill"


class RunOutcome(StrEnum):
    COMPLETED = "completed"
    TIMED_OUT = "timed_out"
    AGENT_ERROR = "agent_error"
    INFRA_ERROR = "infra_error"


class SweBenchItem(BaseModel):
    model_config = ConfigDict(extra="forbid", populate_by_name=True)

    instance_id: str
    repo: str
    base_commit: str
    problem_statement: str
    patch: str = ""
    test_patch: str = ""
    hints_text: str = ""
    created_at: str = ""
    version: str = ""
    fail_to_pass: str = Field(default="[]", alias="FAIL_TO_PASS")
    pass_to_pass: str = Field(default="[]", alias="PASS_TO_PASS")
    environment_setup_commit: str = ""

    def official_instance(self) -> dict[str, str]:
        return {
            "instance_id": self.instance_id,
            "repo": self.repo,
            "base_commit": self.base_commit,
            "problem_statement": self.problem_statement,
            "patch": self.patch,
            "test_patch": self.test_patch,
            "hints_text": self.hints_text,
            "created_at": self.created_at,
            "version": self.version,
            "FAIL_TO_PASS": self.fail_to_pass,
            "PASS_TO_PASS": self.pass_to_pass,
            "environment_setup_commit": self.environment_setup_commit,
        }


class TokenUsage(BaseModel):
    model_config = ConfigDict(extra="forbid")

    input: int = 0
    output: int = 0
    reasoning: int = 0
    cache_read: int = 0
    cache_write: int = 0
    total: int = 0
    cost_usd: float = 0.0
    requests: int = 0


class ToolUsage(BaseModel):
    model_config = ConfigDict(extra="forbid")

    calls: int = 0
    errors: int = 0
    cumulative_seconds: float = 0.0
    by_name: dict[str, int] = Field(default_factory=dict)
    relay_commands: dict[str, int] = Field(default_factory=dict)
    auto_retries: int = 0
    harness_continuations: int = 0


class TimingMetrics(BaseModel):
    model_config = ConfigDict(extra="forbid")

    image_prepare_seconds: float = 0.0
    container_start_seconds: float = 0.0
    preindex_seconds: float = 0.0
    agent_seconds: float = 0.0
    scoring_seconds: float = 0.0
    end_to_end_seconds: float = 0.0


class TestBucket(BaseModel):
    model_config = ConfigDict(extra="forbid")

    success: tuple[str, ...] = ()
    failure: tuple[str, ...] = ()


class SweBenchDiagnostics(BaseModel):
    model_config = ConfigDict(extra="forbid")

    completed: bool = False
    resolved: bool = False
    resolution_status: str = "none"
    patch_exists: bool = False
    patch_applied: bool = False
    fail_to_pass: TestBucket = Field(default_factory=TestBucket)
    pass_to_pass: TestBucket = Field(default_factory=TestBucket)
    report_path: str = ""
    test_output_path: str = ""


class EvalResult(BaseModel):
    model_config = ConfigDict(extra="forbid")

    instance_id: str
    condition: Condition
    attempt: int = 1
    infrastructure_retries: int = 0
    outcome: RunOutcome
    error: str = ""
    prompt_path: str = ""
    trace_path: str = ""
    patch_path: str = ""
    index_log_path: str = ""
    tokens: TokenUsage = Field(default_factory=TokenUsage)
    tools: ToolUsage = Field(default_factory=ToolUsage)
    timings: TimingMetrics = Field(default_factory=TimingMetrics)
    swebench: SweBenchDiagnostics = Field(default_factory=SweBenchDiagnostics)
    created_at: datetime = Field(default_factory=lambda: datetime.now(tz=UTC))

    @property
    def checkpoint_key(self) -> str:
        return f"{self.instance_id}:{self.condition.value}"

    @property
    def benchmark_resolved(self) -> bool:
        """Count a verifier pass only when the agent completed its contract."""
        return self.outcome is RunOutcome.COMPLETED and self.swebench.resolved


class RunSignature(BaseModel):
    model_config = ConfigDict(extra="forbid", frozen=True)

    dataset_name: str
    dataset_sha256: str
    harness_version: str
    swebench_version: str
    node_version: str
    pi_version: str
    model: str
    thinking: str
    skill_version: str
    skill_sha256: str
    runtime_image: str
    image_prefix: str
    prompt_version: str
    treatment_instruction: str
    condition_execution_mode: str
    tool_allowlist: str
    agent_timeout_seconds: int
    index_timeout_seconds: int
    score_timeout_seconds: int
    max_continuations: int = 3
    stall_timeout_seconds: int = 600


class CheckpointMeta(BaseModel):
    model_config = ConfigDict(extra="forbid")

    version: int = 1
    signature: RunSignature
    repository_commit: str
    created_at: datetime = Field(default_factory=lambda: datetime.now(tz=UTC))


class RuntimePaths(BaseModel):
    model_config = ConfigDict(extra="forbid", arbitrary_types_allowed=True)

    workspace: Path
    tool_root: Path
    cache_dir: Path
    output_dir: Path
    dataset_path: Path
