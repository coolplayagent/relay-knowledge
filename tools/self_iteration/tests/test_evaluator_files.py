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
    apply_fixture_action,
    background_file_fixture_runtime_env,
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

    def test_background_file_fixture_runtime_env_enables_service_scans(self) -> None:
        base_env = {
            "RELAY_KNOWLEDGE_HOME": "/tmp/relay-home",
            "RELAY_KNOWLEDGE_FILE_INDEX_ROOTS": "/opt/docs",
        }
        fixture_env = background_file_fixture_runtime_env(
            base_env,
            Path("/tmp/relay-home/file-fixtures/docs"),
            125,
        )

        self.assertEqual(fixture_env["RELAY_KNOWLEDGE_FILE_INDEX_ENABLED"], "true")
        self.assertEqual(fixture_env["RELAY_KNOWLEDGE_FILE_INDEX_SCAN_INTERVAL_MS"], "125")
        self.assertIn("/tmp/relay-home/file-fixtures/docs", fixture_env["RELAY_KNOWLEDGE_FILE_INDEX_ROOTS"])

    def test_fixture_actions_write_delete_and_rename_targets(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp) / "fixture"
            root.mkdir()
            apply_fixture_action(root, {"type": "write", "path": "docs/source.txt", "content": "source"})
            apply_fixture_action(root, {"type": "rename", "from": "docs/source.txt", "to": "docs/renamed.txt"})
            apply_fixture_action(root, {"type": "write", "path": "docs/remove.txt", "content": "remove"})
            apply_fixture_action(root, {"type": "delete", "path": "docs/remove.txt"})

            self.assertTrue((root / "docs/renamed.txt").exists())
            self.assertFalse((root / "docs/source.txt").exists())
            self.assertFalse((root / "docs/remove.txt").exists())

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

    def test_file_case_scoring_preserves_competitive_objective_and_output_contract(self) -> None:
        result = CommandResult(
            name="files_query",
            command=["relay-knowledge"],
            exit_code=0,
            duration_ms=12,
            stdout=json.dumps(
                {
                    "results": [
                        {
                            "scope_id": "local-files",
                            "relative_path": "Projects/app/src/index.js",
                            "file_name": "index.js",
                            "extension": "js",
                            "parent_dir": "/tmp/root/Projects/app/src",
                            "status": "indexed",
                        }
                    ],
                    "truncated": False,
                    "degraded_reason": None,
                }
            ),
            stderr="",
        )

        observation = score_file_case(
            "local_documents",
            {
                "id": "files_js_path_target",
                "objective": "competitive_capability",
                "expected": [
                    {
                        "scope_id": "local-files",
                        "relative_path_contains": "Projects/app/src",
                        "file_name": "index.js",
                    }
                ],
                "forbidden": [{"relative_path_contains": "node_modules"}],
                "max_results": 1,
                "truncated": False,
                "max_rank": 1,
            },
            result,
        )

        self.assertTrue(observation.passed)
        self.assertEqual(observation.objective, "competitive_capability")
        self.assertEqual(observation.rank, 1)

    def test_file_case_scoring_fails_when_result_contract_regresses(self) -> None:
        result = CommandResult(
            name="files_query",
            command=["relay-knowledge"],
            exit_code=0,
            duration_ms=12,
            stdout=json.dumps(
                {
                    "results": [
                        {"relative_path": "noise/a.txt", "status": "indexed"},
                        {"relative_path": "noise/b.txt", "status": "indexed"},
                    ],
                    "truncated": True,
                    "degraded_reason": "file query timed out",
                }
            ),
            stderr="",
        )

        observation = score_file_case(
            "local_documents",
            {
                "id": "files_contract_regression",
                "expected": [{"relative_path": "target.txt"}],
                "max_results": 1,
                "truncated": False,
                "degraded_reason": None,
            },
            result,
        )

        self.assertFalse(observation.passed)
        self.assertIn("max_results", observation.message)
        self.assertIn("truncated=True", observation.message)


if __name__ == "__main__":
    unittest.main()
