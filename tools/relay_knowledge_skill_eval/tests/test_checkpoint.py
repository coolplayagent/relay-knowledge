from __future__ import annotations

from pathlib import Path

import pytest

from relay_knowledge_skill_eval.checkpoint import (
    CheckpointStore,
    validate_resume_scope,
)
from relay_knowledge_skill_eval.models import (
    Condition,
    EvalResult,
    RunOutcome,
    RunSignature,
)


def signature(**updates: object) -> RunSignature:
    values: dict[str, object] = {
        "dataset_name": "verified",
        "dataset_sha256": "dataset",
        "harness_version": "1",
        "swebench_version": "4.1.0",
        "node_version": "22.19.0",
        "pi_version": "0.80.3",
        "model": "deepseek-v4-flash",
        "thinking": "high",
        "skill_version": "1.1.13",
        "skill_sha256": "skill",
        "runtime_image": "image",
        "image_prefix": "prefix",
        "prompt_version": "1",
        "treatment_instruction": "available",
        "condition_execution_mode": "stable-hash-sequential",
        "tool_allowlist": "read,bash,edit,write,grep,find,ls",
        "agent_timeout_seconds": 600,
        "index_timeout_seconds": 600,
        "score_timeout_seconds": 900,
    }
    values.update(updates)
    return RunSignature.model_validate(values)


def result(instance: str = "case") -> EvalResult:
    return EvalResult(
        instance_id=instance,
        condition=Condition.BASELINE,
        outcome=RunOutcome.COMPLETED,
    )


def test_checkpoint_recovers_trailing_partial_line(tmp_path: Path) -> None:
    store = CheckpointStore(tmp_path)
    store.initialize(signature(), "commit")
    store.append(result())
    with store.results_path.open("a", encoding="utf-8") as handle:
        handle.write('{"instance_id":')
    observed = store.load_results()
    assert list(observed) == ["case:baseline"]
    assert store.results_path.read_text(encoding="utf-8").endswith('{"instance_id":')
    loaded = store.load_results(repair_trailing=True)
    assert list(loaded) == ["case:baseline"]
    assert store.results_path.read_text(encoding="utf-8").endswith("\n")
    store.append(result("next"))
    assert list(store.load_results()) == ["case:baseline", "next:baseline"]
    assert store.load_meta().repository_commit == "commit"


def test_checkpoint_separates_valid_record_without_terminal_newline(
    tmp_path: Path,
) -> None:
    store = CheckpointStore(tmp_path)
    store.results_path.write_text(result().model_dump_json(), encoding="utf-8")

    store.append(result("next"))

    assert list(store.load_results()) == ["case:baseline", "next:baseline"]


def test_checkpoint_rejects_configuration_drift(tmp_path: Path) -> None:
    store = CheckpointStore(tmp_path)
    store.initialize(signature(), "commit")
    with pytest.raises(ValueError, match="signature differs"):
        store.initialize(signature(model="changed"), "commit")
    with pytest.raises(ValueError, match="repository commit differs"):
        store.initialize(signature(), "other-commit")


def test_resume_scope_rejects_shrinking_or_switching_selected_suite() -> None:
    existing = {
        "case-a:baseline": result("case-a"),
        "case-b:baseline": result("case-b"),
    }

    with pytest.raises(ValueError, match="smaller than the existing checkpoint"):
        validate_resume_scope(existing, ["case-a"], 1)
    with pytest.raises(ValueError, match=r"does not contain.*case-b"):
        validate_resume_scope(existing, ["case-a", "case-c"], 4)

    validate_resume_scope(existing, ["case-a", "case-b", "case-c"], 6)
