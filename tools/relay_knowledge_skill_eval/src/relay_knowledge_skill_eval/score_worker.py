from __future__ import annotations

import json
import sys
from pathlib import Path

from relay_knowledge_skill_eval.models import SweBenchItem
from relay_knowledge_skill_eval.swebench_support import SweBenchHarness


def main() -> None:
    payload = json.loads(sys.stdin.read())
    if not isinstance(payload, dict):
        raise ValueError("Scorer worker input must be a JSON object")
    item = SweBenchItem.model_validate(payload["item"])
    harness = SweBenchHarness(
        None,
        cache_dir=Path(str(payload["cache_dir"])),
        output_dir=Path(str(payload["output_dir"])),
        score_timeout_seconds=int(payload["score_timeout_seconds"]),
    )
    diagnostics, seconds = harness._score_direct(
        item=item,
        condition=str(payload["condition"]),
        generated_patch=str(payload["generated_patch"]),
        run_id=str(payload["run_id"]),
    )
    result_path = Path(str(payload["result_path"]))
    result_path.write_text(
        json.dumps(
            {
                "diagnostics": diagnostics.model_dump(mode="json"),
                "seconds": seconds,
            },
            ensure_ascii=False,
        ),
        encoding="utf-8",
        newline="\n",
    )


if __name__ == "__main__":
    main()
