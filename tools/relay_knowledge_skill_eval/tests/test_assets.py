from __future__ import annotations

import hashlib
import subprocess
import tarfile
import urllib.error
from pathlib import Path

from relay_knowledge_skill_eval import assets


def _skill(path: Path, binary: bytes) -> Path:
    (path / "assets" / "linux-x86_64").mkdir(parents=True)
    (path / "SKILL.md").write_text("# Skill\n", encoding="utf-8")
    (path / "assets" / "linux-x86_64" / "relay-knowledge").write_bytes(binary)
    return path


def test_local_skill_cache_is_content_addressed_and_separate_from_release(
    tmp_path: Path,
) -> None:
    source = _skill(tmp_path / "source", b"first")

    first_path, first_hash = assets.prepare_skill(
        cache_dir=tmp_path / "cache", version="1.1.13", source_dir=source
    )
    (source / "assets" / "linux-x86_64" / "relay-knowledge").write_bytes(b"second")
    second_path, second_hash = assets.prepare_skill(
        cache_dir=tmp_path / "cache", version="1.1.13", source_dir=source
    )

    assert first_hash != second_hash
    assert first_path != second_path
    assert first_path.parts[-3:] == ("local", first_hash, assets.SKILL_NAME)
    assert "release" not in first_path.parts
    assert first_path.is_dir()
    assert second_path.is_dir()


def test_local_skill_cache_rebuilds_an_incomplete_existing_copy(tmp_path: Path) -> None:
    source = _skill(tmp_path / "source", b"binary")
    references = source / "references"
    references.mkdir()
    (references / "guide.md").write_text("complete\n", encoding="utf-8")
    source_hash = assets._tree_sha256(source)
    destination = (
        tmp_path / "cache" / "skill" / "local" / source_hash / assets.SKILL_NAME
    )
    _skill(destination, b"binary")

    prepared, prepared_hash = assets.prepare_skill(
        cache_dir=tmp_path / "cache",
        version="1.1.13",
        source_dir=source,
    )

    assert prepared == destination
    assert prepared_hash == source_hash
    assert assets._tree_sha256(prepared) == source_hash
    assert (prepared / "references" / "guide.md").read_text(encoding="utf-8") == (
        "complete\n"
    )


def test_release_skill_uses_unique_atomic_extraction_directory(
    tmp_path: Path, monkeypatch
) -> None:
    version = "1.1.13"
    cache = tmp_path / "cache"
    downloads = cache / "downloads"
    downloads.mkdir(parents=True)
    source = _skill(tmp_path / "release-skill", b"binary")
    archive_name = assets.skill_archive_name(version)
    archive = downloads / archive_name
    with tarfile.open(archive, "w:gz") as bundle:
        bundle.add(source, arcname=assets.SKILL_NAME)
    checksum = hashlib.sha256(archive.read_bytes()).hexdigest()
    (downloads / f"checksums-v{version}.txt").write_text(
        f"{checksum}  {archive_name}\n", encoding="utf-8"
    )
    extraction_roots: list[str] = []
    original_extract = assets._safe_extract

    def capture_extract(archive_path: Path, destination: Path) -> None:
        extraction_roots.append(destination.name)
        original_extract(archive_path, destination)

    monkeypatch.setattr(assets, "_safe_extract", capture_extract)

    prepared, prepared_hash = assets.prepare_skill(
        cache_dir=cache,
        version=version,
    )

    assert prepared_hash == checksum
    assert prepared.is_dir()
    assert extraction_roots[0].startswith(".extracting-")
    assert extraction_roots[0].endswith(".tmp")
    assert not list(prepared.parent.glob(".extracting-*.tmp"))


def test_atomic_skill_publication_accepts_identical_race_winner(
    tmp_path: Path,
) -> None:
    destination = _skill(tmp_path / "destination", b"binary")
    staging = _skill(tmp_path / "staging", b"binary")
    expected = assets._tree_sha256(staging)

    assets._publish_skill_tree(
        staging,
        destination,
        expected_tree_sha256=expected,
    )

    assert assets._tree_sha256(destination) == expected


def test_private_release_download_falls_back_to_authenticated_gh(
    tmp_path: Path, monkeypatch
) -> None:
    destination = tmp_path / "asset.tar.gz"

    def fail_https(*args, **kwargs):
        _ = (args, kwargs)
        raise urllib.error.HTTPError("url", 404, "not found", None, None)

    def fake_run(command, **kwargs):
        _ = kwargs
        output = Path(command[command.index("--output") + 1])
        output.write_bytes(b"asset")
        return subprocess.CompletedProcess(command, 0, "", "")

    monkeypatch.setattr(assets.urllib.request, "urlopen", fail_https)
    monkeypatch.setattr(assets.shutil, "which", lambda name: f"/{name}")
    monkeypatch.setattr(assets.subprocess, "run", fake_run)
    assets._download_release_asset(
        "https://example.invalid/asset.tar.gz",
        destination,
        version="1.1.13",
        asset_name="asset.tar.gz",
    )
    assert destination.read_bytes() == b"asset"
