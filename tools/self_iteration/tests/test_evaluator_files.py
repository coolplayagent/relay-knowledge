from __future__ import annotations

import json
import sys
import tempfile
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))

from evaluator import CommandResult
from file_fixture_eval import (
    AUTHORIZED_FILE_FIXTURE_SCOPE,
    create_file_fixture,
    file_fixture_runtime_env,
    file_query_command,
    score_file_case,
)


class FileEvaluatorTests(unittest.TestCase):
    def test_file_fixture_generation_creates_noise_and_targets(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp) / "fixture"
            create_file_fixture(
                root,
                {
                    "generate_noise_files": 3,
                    "files": [{"path": "D-drive/Work/contract.docx", "content": "contract"}],
                },
            )

            self.assertTrue((root / "D-drive/Work/contract.docx").exists())
            self.assertTrue((root / "noise/quarterly-design-noise-0002.txt").exists())

    def test_file_query_command_is_bounded_and_scoped(self) -> None:
        command = file_query_command(
            Path("relay-knowledge"),
            AUTHORIZED_FILE_FIXTURE_SCOPE,
            {"query": "contract docx", "limit": 7},
        )

        self.assertEqual(
            command,
            [
                "relay-knowledge",
                "files",
                "query",
                "contract docx",
                "--source",
                AUTHORIZED_FILE_FIXTURE_SCOPE,
                "--limit",
                "7",
                "--format",
                "json",
            ],
        )

    def test_file_fixture_runtime_env_authorizes_fixture_root(self) -> None:
        base_env = {
            "RELAY_KNOWLEDGE_HOME": "/tmp/relay-home",
            "RELAY_KNOWLEDGE_FILE_INDEX_ROOTS": "/opt/docs",
        }
        fixture_env = file_fixture_runtime_env(base_env, Path("/tmp/relay-home/file-fixtures/docs"))

        self.assertEqual(base_env["RELAY_KNOWLEDGE_FILE_INDEX_ROOTS"], "/opt/docs")
        self.assertEqual(
            fixture_env["RELAY_KNOWLEDGE_FILE_INDEX_ROOTS"],
            "/opt/docs;/tmp/relay-home/file-fixtures/docs",
        )

    def test_file_case_scoring_matches_relative_path_and_extension(self) -> None:
        result = CommandResult(
            name="files_query",
            command=["relay-knowledge"],
            exit_code=0,
            duration_ms=12,
            stdout=json.dumps(
                {
                    "results": [
                        {
                            "relative_path": "Documents/design/quarterly-design.pdf",
                            "extension": "pdf",
                            "status": "indexed",
                        }
                    ]
                }
            ),
            stderr="",
        )

        observation = score_file_case(
            "local_documents",
            {
                "id": "files_exact_pdf_name",
                "expected": [
                    {
                        "relative_path": "Documents/design/quarterly-design.pdf",
                        "extension": "pdf",
                    }
                ],
                "max_rank": 1,
            },
            result,
        )

        self.assertTrue(observation.passed)
        self.assertEqual(observation.rank, 1)


if __name__ == "__main__":
    unittest.main()
