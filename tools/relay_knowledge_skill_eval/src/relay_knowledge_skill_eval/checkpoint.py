from __future__ import annotations

import threading
from collections.abc import Collection, Mapping
from pathlib import Path

from relay_knowledge_skill_eval.models import CheckpointMeta, EvalResult, RunSignature


def validate_resume_scope(
    results: Mapping[str, EvalResult],
    selected_instance_ids: Collection[str],
    expected_results: int,
) -> None:
    """Reject a resume target that cannot contain every checkpoint result."""
    if len(results) > expected_results:
        raise ValueError(
            "Selected suite is smaller than the existing checkpoint: "
            f"{len(results)} results already exist, but only {expected_results} "
            "are expected"
        )
    selected = set(selected_instance_ids)
    outside_scope = sorted(
        {result.instance_id for result in results.values()} - selected
    )
    if outside_scope:
        preview = ", ".join(outside_scope[:5])
        raise ValueError(
            "Selected suite does not contain existing checkpoint instance(s): "
            + preview
        )


class CheckpointStore:
    def __init__(self, output_dir: Path) -> None:
        self._output_dir = output_dir
        self._meta_path = output_dir / "checkpoint.meta.json"
        self._results_path = output_dir / "checkpoint.results.jsonl"
        self._lock = threading.Lock()

    @property
    def results_path(self) -> Path:
        return self._results_path

    def load_meta(self) -> CheckpointMeta:
        if not self._meta_path.exists():
            raise FileNotFoundError(f"Checkpoint metadata is absent: {self._meta_path}")
        return CheckpointMeta.model_validate_json(
            self._meta_path.read_text(encoding="utf-8")
        )

    def initialize(self, signature: RunSignature, repository_commit: str) -> None:
        self._output_dir.mkdir(parents=True, exist_ok=True)
        if self._meta_path.exists():
            existing = CheckpointMeta.model_validate_json(
                self._meta_path.read_text(encoding="utf-8")
            )
            if existing.signature != signature:
                raise ValueError(
                    "Checkpoint signature differs from this evaluation configuration"
                )
            if existing.repository_commit != repository_commit:
                raise ValueError(
                    "Checkpoint repository commit differs from the current checkout"
                )
            return
        meta = CheckpointMeta(signature=signature, repository_commit=repository_commit)
        temporary = self._meta_path.with_suffix(".json.tmp")
        temporary.write_text(meta.model_dump_json(indent=2), encoding="utf-8")
        temporary.replace(self._meta_path)

    def load_results(self, *, repair_trailing: bool = False) -> dict[str, EvalResult]:
        if not self._results_path.exists():
            return {}
        raw = self._results_path.read_bytes()
        lines: list[tuple[int, bytes]] = []
        offset = 0
        for raw_line in raw.splitlines(keepends=True):
            content = raw_line.rstrip(b"\r\n")
            if content.strip():
                lines.append((offset, content))
            offset += len(raw_line)
        populated = list(enumerate(lines))
        last_index = populated[-1][0] if populated else -1
        results: dict[str, EvalResult] = {}
        for index, (line_offset, line) in populated:
            try:
                result = EvalResult.model_validate_json(line)
            except ValueError:
                if index == last_index:
                    if repair_trailing:
                        with self._results_path.open("r+b") as handle:
                            handle.truncate(line_offset)
                    break
                raise
            results[result.checkpoint_key] = result
        return results

    def append(self, result: EvalResult) -> None:
        with self._lock:
            self._output_dir.mkdir(parents=True, exist_ok=True)
            with self._results_path.open("a+b") as handle:
                handle.seek(0, 2)
                if handle.tell() > 0:
                    handle.seek(-1, 2)
                    if handle.read(1) not in {b"\n", b"\r"}:
                        handle.write(b"\n")
                payload = result.model_dump_json().encode("utf-8") + b"\n"
                handle.write(payload)
