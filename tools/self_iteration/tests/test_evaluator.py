from __future__ import annotations

import json
import sys
import tempfile
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))

from evaluator import (
    CommandResult,
    load_cases,
    repository_case_objective,
    score_semantic_vector_case,
    semantic_vector_env_check,
    semantic_vector_runtime_profile,
)


class EvaluatorTests(unittest.TestCase):
    def test_repository_case_defaults_exact_queries_to_foundational_objective(self) -> None:
        self.assertEqual(
            repository_case_objective(
                {
                    "id": "linux_definition_start_kernel",
                    "kind": "definition",
                    "query": "start_kernel",
                }
            ),
            "foundational_capability",
        )

    def test_repository_case_defaults_hybrid_and_fuzzy_queries_to_competitive_objective(self) -> None:
        self.assertEqual(
            repository_case_objective(
                {
                    "id": "rt_hybrid_eval_checkpoint_store",
                    "kind": "hybrid",
                    "query": "Eval checkpoint store",
                }
            ),
            "competitive_capability",
        )
        self.assertEqual(
            repository_case_objective(
                {
                    "id": "rt_fuzzy_function_archive_output_dir",
                    "kind": "definition",
                    "query": "archive output directory",
                }
            ),
            "competitive_capability",
        )

    def test_load_cases_merges_included_case_files(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            (root / "extra.json").write_text(
                json.dumps(
                    {
                        "repositories": {
                            "relay_teams": {
                                "register_index_budget_ms": 46000,
                            }
                        },
                        "file_fixtures": {"background": {"files": []}},
                        "file_query_cases": [{"id": "file-target"}],
                        "query_cases": [{"id": "repo-target"}],
                    }
                ),
                encoding="utf-8",
            )
            cases_path = root / "cases.json"
            cases_path.write_text(
                json.dumps(
                    {
                        "include_files": ["extra.json"],
                        "repositories": {
                            "relay_teams": {
                                "path": "/repo",
                                "index_budget_ms": 90000,
                            }
                        },
                        "file_fixtures": {"base": {"files": []}},
                        "file_query_cases": [{"id": "file-base"}],
                        "query_cases": [{"id": "repo-base"}],
                    }
                ),
                encoding="utf-8",
            )

            config = load_cases(cases_path)

        self.assertEqual(set(config["file_fixtures"]), {"base", "background"})
        self.assertEqual(
            [case["id"] for case in config["file_query_cases"]],
            ["file-base", "file-target"],
        )
        self.assertEqual(
            [case["id"] for case in config["query_cases"]],
            ["repo-base", "repo-target"],
        )
        self.assertEqual(config["repositories"]["relay_teams"]["path"], "/repo")
        self.assertEqual(config["repositories"]["relay_teams"]["index_budget_ms"], 90000)
        self.assertEqual(config["repositories"]["relay_teams"]["register_index_budget_ms"], 46000)

    def test_semantic_vector_profile_reads_external_runtime_env(self) -> None:
        profile = semantic_vector_runtime_profile(
            {
                "RELAY_KNOWLEDGE_SEMANTIC_BACKEND": "external",
                "RELAY_KNOWLEDGE_VECTOR_BACKEND": "external",
                "RELAY_KNOWLEDGE_LLM_PROVIDER": "openai_compatible",
                "RELAY_KNOWLEDGE_EMBEDDING_BASE_URL": "https://api.example.test/v1",
                "RELAY_KNOWLEDGE_EMBEDDING_API_KEY": "secret",
                "RELAY_KNOWLEDGE_TEXT_EMBEDDING_MODEL": "text-embed-3-small",
                "RELAY_KNOWLEDGE_EMBEDDING_DIMENSION": "1536",
            }
        )

        self.assertTrue(profile["external_requested"])
        self.assertEqual(profile["semantic_backend"], "external")
        self.assertEqual(profile["vector_backend"], "external")
        self.assertEqual(profile["missing_external_env"], [])
        self.assertTrue(semantic_vector_env_check(profile).passed)

    def test_semantic_vector_profile_reports_missing_external_env(self) -> None:
        profile = semantic_vector_runtime_profile(
            {
                "RELAY_KNOWLEDGE_SEMANTIC_BACKEND": "external",
                "RELAY_KNOWLEDGE_VECTOR_BACKEND": "local",
            }
        )

        gate = semantic_vector_env_check(profile)

        self.assertFalse(gate.passed)
        self.assertIn("RELAY_KNOWLEDGE_EMBEDDING_API_KEY", gate.stderr)

    def test_semantic_vector_case_scores_required_sources_and_backends(self) -> None:
        case = {
            "id": "sv",
            "query": "semantic vector recall",
            "max_rank": 1,
            "required_sources": ["semantic", "vector"],
            "required_backend_states": {
                "semantic": ["available"],
                "vector": ["available"],
            },
            "expected": [{"content_contains": "sv-alpha"}],
        }
        result = command_result(
            {
                "results": [
                    {
                        "content": "semantic vector fixture sv-alpha",
                        "retriever_sources": ["bm25", "semantic", "vector"],
                    }
                ],
                "backend_statuses": [
                    {"source": "semantic", "state": "available"},
                    {"source": "vector", "state": "available"},
                ],
            }
        )

        observation = score_semantic_vector_case(case, result)

        self.assertTrue(observation.passed)
        self.assertEqual(observation.objective, "semantic_vector")
        self.assertEqual(observation.rank, 1)

    def test_semantic_vector_case_fails_when_expected_hit_lacks_vector_source(self) -> None:
        case = {
            "id": "sv_missing_vector",
            "query": "semantic vector recall",
            "max_rank": 1,
            "required_sources": ["vector"],
            "required_backend_states": {"vector": ["available"]},
            "expected": [{"content_contains": "sv-alpha"}],
        }
        result = command_result(
            {
                "results": [
                    {
                        "content": "semantic vector fixture sv-alpha",
                        "retriever_sources": ["bm25", "semantic"],
                    }
                ],
                "backend_statuses": [{"source": "vector", "state": "available"}],
            }
        )

        observation = score_semantic_vector_case(case, result)

        self.assertFalse(observation.passed)
        self.assertIn("missing_sources=['vector']", observation.message)


def command_result(payload: dict[str, object]) -> CommandResult:
    return CommandResult(
        name="query",
        command=["relay-knowledge", "query"],
        exit_code=0,
        duration_ms=1,
        stdout=json.dumps(payload),
        stderr="",
    )


if __name__ == "__main__":
    unittest.main()
