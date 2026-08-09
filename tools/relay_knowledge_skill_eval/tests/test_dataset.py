from __future__ import annotations

import hashlib
from pathlib import Path
from types import SimpleNamespace

import pytest

from relay_knowledge_skill_eval import dataset as dataset_module
from relay_knowledge_skill_eval.dataset import (
    DATASET_NAME,
    DATASET_REVISION,
    dataset_sha256,
    download_dataset,
    load_cached_dataset,
    load_smoke_manifest,
    select_suite,
)
from relay_knowledge_skill_eval.models import SweBenchItem


def test_packaged_smoke_manifest_is_valid() -> None:
    smoke_ids = load_smoke_manifest()

    assert len(smoke_ids) == 10
    assert len(set(smoke_ids)) == 10


def test_smoke_selection_preserves_manifest_order() -> None:
    smoke_ids = load_smoke_manifest()
    items = [
        SweBenchItem(
            instance_id=instance_id,
            repo="astropy/astropy",
            base_commit="base",
            problem_statement="problem",
        )
        for instance_id in reversed(smoke_ids)
    ]
    selected = select_suite(items, "smoke-10")
    assert tuple(item.instance_id for item in selected) == smoke_ids


def test_smoke_manifest_rejects_duplicate_ids(tmp_path: Path) -> None:
    manifest = tmp_path / "smoke.txt"
    manifest.write_text("\n".join(["duplicate"] * 10), encoding="utf-8")

    with pytest.raises(ValueError, match="duplicate"):
        load_smoke_manifest(manifest)


def test_first_100_selection_preserves_official_dataset_order() -> None:
    items = [
        SweBenchItem(
            instance_id=f"owner__repo-{index}",
            repo="owner/repo",
            base_commit="base",
            problem_statement="problem",
        )
        for index in range(120)
    ]

    selected = select_suite(items, "verified-first-100")

    assert len(selected) == 100
    assert [item.instance_id for item in selected] == [
        f"owner__repo-{index}" for index in range(100)
    ]


def test_download_pins_revision_and_cache_requires_expected_checksum(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    rows = [
        {
            "instance_id": f"owner__repo-{index}",
            "repo": "owner/repo",
            "base_commit": "base",
            "problem_statement": "problem",
        }
        for index in range(500)
    ]
    items = [dataset_module._item_from_row(row) for row in rows]
    payload = "".join(
        item.model_dump_json(by_alias=True) + "\n" for item in items
    ).encode()
    expected_sha256 = hashlib.sha256(payload).hexdigest()
    calls: list[tuple[str, dict[str, object]]] = []

    def fake_loader(name: str, **kwargs: object) -> list[dict[str, str]]:
        calls.append((name, kwargs))
        return rows

    monkeypatch.setattr(dataset_module, "DATASET_SHA256", expected_sha256)
    monkeypatch.setattr(
        dataset_module,
        "import_module",
        lambda _: SimpleNamespace(load_dataset=fake_loader),
    )
    path = tmp_path / "verified.jsonl"

    assert len(download_dataset(path)) == 500
    assert calls == [
        (
            DATASET_NAME,
            {
                "revision": DATASET_REVISION,
                "split": "test",
                "streaming": False,
            },
        )
    ]
    assert dataset_sha256(path) == expected_sha256
    assert len(load_cached_dataset(path)) == 500

    path.write_text(path.read_text(encoding="utf-8") + "corrupt\n", encoding="utf-8")
    with pytest.raises(ValueError, match="cache checksum mismatch"):
        load_cached_dataset(path)


def test_dataset_downloads_use_distinct_atomic_staging_files(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    rows = [
        {
            "instance_id": f"owner__repo-{index}",
            "repo": "owner/repo",
            "base_commit": "base",
            "problem_statement": "problem",
        }
        for index in range(500)
    ]
    payload = "".join(
        dataset_module._item_from_row(row).model_dump_json(by_alias=True) + "\n"
        for row in rows
    ).encode()
    expected_sha256 = hashlib.sha256(payload).hexdigest()
    observed_staging_paths: list[Path] = []
    real_dataset_sha256 = dataset_module.dataset_sha256

    def capture_staging_path(path: Path) -> str:
        if path.name.startswith(".verified.jsonl."):
            observed_staging_paths.append(path)
        return real_dataset_sha256(path)

    monkeypatch.setattr(dataset_module, "DATASET_SHA256", expected_sha256)
    monkeypatch.setattr(dataset_module, "dataset_sha256", capture_staging_path)
    monkeypatch.setattr(
        dataset_module,
        "import_module",
        lambda _: SimpleNamespace(load_dataset=lambda *_args, **_kwargs: rows),
    )
    destination = tmp_path / "verified.jsonl"

    download_dataset(destination)
    download_dataset(destination)

    assert len(observed_staging_paths) == 2
    assert len(set(observed_staging_paths)) == 2
    assert all(path.parent == tmp_path for path in observed_staging_paths)
    assert not list(tmp_path.glob(".verified.jsonl.*.tmp"))
