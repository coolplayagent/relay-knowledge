from __future__ import annotations

import hashlib
import json
import uuid
from collections.abc import Iterable, Mapping
from importlib import import_module
from importlib.resources import files
from pathlib import Path

from relay_knowledge_skill_eval.models import SweBenchItem

DATASET_NAME = "SWE-bench/SWE-bench_Verified"
DATASET_REVISION = "91aa3ed51b709be6457e12d00300a6a596d4c6a3"
DATASET_SHA256 = "de1e478b9b64b2d69a46bfe329273f3dc56f201307cd6dd0055f8d9a4de98841"
SMOKE_MANIFEST_RESOURCE = files("relay_knowledge_skill_eval").joinpath(
    "data", "smoke-10.txt"
)


def dataset_sha256(path: Path) -> str:
    return hashlib.sha256(path.read_bytes()).hexdigest()


def load_cached_dataset(path: Path) -> list[SweBenchItem]:
    actual_sha256 = dataset_sha256(path)
    if actual_sha256 != DATASET_SHA256:
        raise ValueError(
            "SWE-bench Verified cache checksum mismatch: "
            f"expected {DATASET_SHA256}, found {actual_sha256}"
        )
    items = [
        SweBenchItem.model_validate_json(line)
        for line in path.read_text(encoding="utf-8").splitlines()
        if line.strip()
    ]
    if len(items) != 500:
        raise ValueError(f"Expected 500 SWE-bench Verified rows, found {len(items)}")
    if len({item.instance_id for item in items}) != len(items):
        raise ValueError("SWE-bench dataset contains duplicate instance IDs")
    return items


def download_dataset(path: Path) -> list[SweBenchItem]:
    datasets = import_module("datasets")
    loader = getattr(datasets, "load_dataset", None)
    if not callable(loader):
        raise RuntimeError("datasets.load_dataset is unavailable")
    rows = loader(
        DATASET_NAME,
        revision=DATASET_REVISION,
        split="test",
        streaming=False,
    )
    items = [_item_from_row(row) for row in _iter_rows(rows)]
    if len(items) != 500:
        raise RuntimeError(
            "Official SWE-bench Verified split returned "
            f"{len(items)} rows, expected 500"
        )
    path.parent.mkdir(parents=True, exist_ok=True)
    temporary = path.parent / f".{path.name}.{uuid.uuid4().hex}.tmp"
    try:
        with temporary.open("w", encoding="utf-8", newline="\n") as handle:
            for item in items:
                handle.write(item.model_dump_json(by_alias=True))
                handle.write("\n")
        actual_sha256 = dataset_sha256(temporary)
        if actual_sha256 != DATASET_SHA256:
            raise RuntimeError(
                "Pinned SWE-bench Verified content checksum mismatch: "
                f"expected {DATASET_SHA256}, found {actual_sha256}"
            )
        temporary.replace(path)
    finally:
        temporary.unlink(missing_ok=True)
    return items


def ensure_dataset(path: Path) -> list[SweBenchItem]:
    return load_cached_dataset(path) if path.exists() else download_dataset(path)


def load_smoke_manifest(path: Path | None = None) -> tuple[str, ...]:
    content = (
        path.read_text(encoding="utf-8")
        if path is not None
        else SMOKE_MANIFEST_RESOURCE.read_text(encoding="utf-8")
    )
    instance_ids = tuple(line.strip() for line in content.splitlines() if line.strip())
    if len(instance_ids) != 10:
        raise ValueError(
            f"Smoke manifest must contain exactly 10 IDs, found {len(instance_ids)}"
        )
    if len(set(instance_ids)) != len(instance_ids):
        raise ValueError("Smoke manifest contains duplicate instance IDs")
    return instance_ids


def select_suite(
    items: list[SweBenchItem],
    suite: str,
    *,
    smoke_manifest_path: Path | None = None,
) -> list[SweBenchItem]:
    if suite == "verified-full":
        return items
    if suite == "verified-first-100":
        if len(items) < 100:
            raise ValueError(
                f"SWE-bench Verified first-100 needs 100 rows, found {len(items)}"
            )
        return items[:100]
    if suite != "smoke-10":
        raise ValueError(f"Unsupported suite: {suite}")
    smoke_ids = load_smoke_manifest(smoke_manifest_path)
    by_id = {item.instance_id: item for item in items}
    missing = [instance_id for instance_id in smoke_ids if instance_id not in by_id]
    if missing:
        raise ValueError(f"Smoke manifest IDs missing from dataset: {missing}")
    return [by_id[instance_id] for instance_id in smoke_ids]


def _iter_rows(value: object) -> Iterable[Mapping[str, object]]:
    if not isinstance(value, Iterable):
        raise TypeError("datasets result is not iterable")
    for row in value:
        if not isinstance(row, Mapping):
            raise TypeError("SWE-bench row is not a mapping")
        yield row


def _item_from_row(row: Mapping[str, object]) -> SweBenchItem:
    supported = {
        "instance_id",
        "repo",
        "base_commit",
        "problem_statement",
        "patch",
        "test_patch",
        "hints_text",
        "created_at",
        "version",
        "FAIL_TO_PASS",
        "PASS_TO_PASS",
        "environment_setup_commit",
    }
    payload: dict[str, str] = {}
    for key in supported:
        value = row.get(key, "")
        payload[key] = value if isinstance(value, str) else json.dumps(value)
    return SweBenchItem.model_validate(payload)
