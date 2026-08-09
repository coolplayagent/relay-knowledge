from __future__ import annotations

import hashlib
import shutil
import subprocess
import tarfile
import urllib.request
import uuid
from pathlib import Path

REPOSITORY = "coolplayagent/relay-knowledge"
SKILL_NAME = "relay-knowledge-cli"


def skill_archive_name(version: str) -> str:
    return f"{SKILL_NAME}-skill-v{version}.tar.gz"


def prepare_skill(
    *,
    cache_dir: Path,
    version: str,
    source_dir: Path | None = None,
) -> tuple[Path, str]:
    if source_dir is not None:
        _validate_skill(source_dir)
        source_sha256 = _tree_sha256(source_dir)
        destination = cache_dir / "skill" / "local" / source_sha256 / SKILL_NAME
        if destination.exists():
            try:
                _validate_skill(destination)
                if _tree_sha256(destination) == source_sha256:
                    return destination, source_sha256
            except (OSError, ValueError):
                pass
            if destination.is_dir():
                shutil.rmtree(destination)
            else:
                destination.unlink()
        destination.parent.mkdir(parents=True, exist_ok=True)
        staging = destination.parent / f".{SKILL_NAME}-{uuid.uuid4().hex}.tmp"
        try:
            shutil.copytree(source_dir, staging)
            _validate_skill(staging)
            copied_sha256 = _tree_sha256(staging)
            if copied_sha256 != source_sha256:
                raise RuntimeError(
                    "Local skill changed while it was copied into the evaluation cache"
                )
            _publish_skill_tree(
                staging,
                destination,
                expected_tree_sha256=source_sha256,
            )
        finally:
            if staging.exists():
                shutil.rmtree(staging)
        return destination, source_sha256

    archive_name = skill_archive_name(version)
    download_dir = cache_dir / "downloads"
    archive_path = download_dir / archive_name
    checksums_path = download_dir / f"checksums-v{version}.txt"
    base_url = f"https://github.com/{REPOSITORY}/releases/download/v{version}"
    download_dir.mkdir(parents=True, exist_ok=True)
    if not checksums_path.exists():
        _download_release_asset(
            f"{base_url}/checksums.txt",
            checksums_path,
            version=version,
            asset_name="checksums.txt",
        )
    expected = _expected_checksum(checksums_path, archive_name)
    if not archive_path.exists() or _file_sha256(archive_path) != expected:
        _download_release_asset(
            f"{base_url}/{archive_name}",
            archive_path,
            version=version,
            asset_name=archive_name,
        )
    actual = _file_sha256(archive_path)
    if actual != expected:
        raise RuntimeError(
            f"Skill checksum mismatch: expected {expected}, downloaded {actual}"
        )
    destination = cache_dir / "skill" / "release" / version / actual / SKILL_NAME
    if not destination.exists():
        extract_root = destination.parent / f".extracting-{uuid.uuid4().hex}.tmp"
        extract_root.mkdir(parents=True)
        try:
            _safe_extract(archive_path, extract_root)
            source = _find_skill_root(extract_root)
            _validate_skill(source)
            _publish_skill_tree(source, destination)
        finally:
            shutil.rmtree(extract_root, ignore_errors=True)
    _validate_skill(destination)
    return destination, actual


def _download_release_asset(
    url: str,
    destination: Path,
    *,
    version: str,
    asset_name: str,
) -> None:
    temporary = destination.parent / f".{destination.name}.{uuid.uuid4().hex}.tmp"
    request = urllib.request.Request(
        url, headers={"User-Agent": "relay-knowledge-skill-eval/1"}
    )
    try:
        with urllib.request.urlopen(request, timeout=120) as response:
            temporary.write_bytes(response.read())
    except Exception as exc:
        temporary.unlink(missing_ok=True)
        _download_with_gh(
            destination=temporary,
            version=version,
            asset_name=asset_name,
            original_error=exc,
            url=url,
        )
    temporary.replace(destination)


def _publish_skill_tree(
    staging: Path,
    destination: Path,
    *,
    expected_tree_sha256: str | None = None,
) -> None:
    """Atomically publish a validated tree or accept an identical race winner."""
    try:
        staging.replace(destination)
        return
    except OSError as error:
        try:
            _validate_skill(destination)
        except (OSError, ValueError):
            raise error from None
        if (
            expected_tree_sha256 is not None
            and _tree_sha256(destination) != expected_tree_sha256
        ):
            raise RuntimeError(
                "Concurrent skill cache publication produced different content"
            ) from error


def _download_with_gh(
    *,
    destination: Path,
    version: str,
    asset_name: str,
    original_error: Exception,
    url: str,
) -> None:
    gh = shutil.which("gh")
    if gh is None:
        raise RuntimeError(
            f"Failed to download {url}. Configure HTTPS_PROXY/HTTP_PROXY, or "
            f"install and authenticate gh for a private release: {original_error}"
        ) from original_error
    result = subprocess.run(
        [
            gh,
            "release",
            "download",
            f"v{version}",
            "--repo",
            REPOSITORY,
            "--pattern",
            asset_name,
            "--output",
            str(destination),
        ],
        capture_output=True,
        text=True,
        encoding="utf-8",
        errors="replace",
        check=False,
    )
    if result.returncode != 0 or not destination.is_file():
        destination.unlink(missing_ok=True)
        raise RuntimeError(
            f"Failed to download release asset {asset_name} with HTTPS and gh: "
            f"{result.stderr.strip() or original_error}"
        ) from original_error


def _expected_checksum(path: Path, archive_name: str) -> str:
    for line in path.read_text(encoding="utf-8").splitlines():
        parts = line.split()
        if len(parts) >= 2 and parts[-1].lstrip("*") == archive_name:
            return parts[0].lower()
    raise RuntimeError(f"{archive_name} is absent from {path}")


def _safe_extract(archive_path: Path, destination: Path) -> None:
    root = destination.resolve()
    with tarfile.open(archive_path, "r:gz") as archive:
        for member in archive.getmembers():
            target = (destination / member.name).resolve()
            if target != root and root not in target.parents:
                raise RuntimeError(f"Unsafe skill archive member: {member.name}")
        archive.extractall(destination, filter="data")


def _find_skill_root(root: Path) -> Path:
    candidates = [path.parent for path in root.rglob("SKILL.md")]
    valid = [path for path in candidates if (path / "assets").is_dir()]
    if len(valid) != 1:
        raise RuntimeError(f"Expected one packaged skill root, found {len(valid)}")
    return valid[0]


def _validate_skill(path: Path) -> None:
    required = (
        path / "SKILL.md",
        path / "assets" / "linux-x86_64" / "relay-knowledge",
    )
    missing = [str(candidate) for candidate in required if not candidate.is_file()]
    if missing:
        raise ValueError(f"Packaged skill is incomplete: {missing}")


def _file_sha256(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as handle:
        for chunk in iter(lambda: handle.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def _tree_sha256(path: Path) -> str:
    digest = hashlib.sha256()
    for item in sorted(
        candidate for candidate in path.rglob("*") if candidate.is_file()
    ):
        digest.update(item.relative_to(path).as_posix().encode())
        digest.update(item.read_bytes())
    return digest.hexdigest()
