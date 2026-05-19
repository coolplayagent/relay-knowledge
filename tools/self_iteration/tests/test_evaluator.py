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
    repo_set_add_command,
    repo_set_create_command,
    repo_set_query_command,
    repository_case_objective,
    score_repository_set_query_case,
    score_query_case,
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
                        "repository_sets": {
                            "workspace": {
                                "members": [{"repository": "relay_teams"}],
                            }
                        },
                        "repository_set_query_cases": [{"id": "set-target"}],
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
                        "repository_sets": {
                            "workspace": {
                                "alias": "workspace-self-iteration",
                                "query_p95_budget_ms": 1000,
                            }
                        },
                        "repository_set_query_cases": [{"id": "set-base"}],
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
        self.assertEqual(
            [case["id"] for case in config["repository_set_query_cases"]],
            ["set-base", "set-target"],
        )
        self.assertEqual(config["repositories"]["relay_teams"]["path"], "/repo")
        self.assertEqual(config["repositories"]["relay_teams"]["index_budget_ms"], 90000)
        self.assertEqual(config["repositories"]["relay_teams"]["register_index_budget_ms"], 46000)
        self.assertEqual(config["repository_sets"]["workspace"]["alias"], "workspace-self-iteration")
        self.assertEqual(config["repository_sets"]["workspace"]["query_p95_budget_ms"], 1000)
        self.assertEqual(
            config["repository_sets"]["workspace"]["members"],
            [{"repository": "relay_teams"}],
        )

    def test_repo_set_command_builders_use_existing_cli_contract(self) -> None:
        binary = Path("relay-knowledge")

        self.assertEqual(
            repo_set_create_command(
                binary,
                "workspace",
                {"description": "multi repo workspace"},
            ),
            [
                "relay-knowledge",
                "repo-set",
                "create",
                "workspace",
                "--description",
                "multi repo workspace",
                "--format",
                "json",
            ],
        )
        self.assertEqual(
            repo_set_add_command(
                binary,
                "workspace",
                {
                    "repository": "app",
                    "priority": 7,
                    "path_filters": ["src"],
                    "language_filters": ["go"],
                },
                "app",
                {"alias": "app-self-iteration", "ref": "HEAD"},
            ),
            [
                "relay-knowledge",
                "repo-set",
                "add",
                "workspace",
                "app-self-iteration",
                "--ref",
                "HEAD",
                "--priority",
                "7",
                "--path",
                "src",
                "--language",
                "go",
                "--format",
                "json",
            ],
        )
        self.assertEqual(
            repo_set_query_command(
                binary,
                "workspace",
                {
                    "query": "client.Dial",
                    "kind": "hybrid",
                    "freshness": "wait-until-fresh",
                    "limit": 12,
                    "language_filters": ["go"],
                },
            ),
            [
                "relay-knowledge",
                "repo-set",
                "query",
                "workspace",
                "--query",
                "client.Dial",
                "--kind",
                "hybrid",
                "--freshness",
                "wait-until-fresh",
                "--limit",
                "12",
                "--language",
                "go",
                "--format",
                "json",
            ],
        )

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

    def test_repository_case_scores_expected_all_and_sequence_coverage(self) -> None:
        case = {
            "id": "flow_challenge",
            "kind": "hybrid",
            "query": "startup flow",
            "max_rank": 1,
            "min_score": 0.8,
            "expected": [{"path": "src/main.rs", "excerpt_contains": "main"}],
            "expected_all": [
                {"path": "src/main.rs", "excerpt_contains": "main"},
                {"path": "src/main.rs", "excerpt_contains": "missing step"},
            ],
            "expected_sequence": [
                {"path": "src/main.rs", "excerpt_contains": "main"},
                {"path": "src/main.rs", "excerpt_contains": "async_main"},
            ],
            "require_expected_all": False,
        }
        result = command_result(
            {
                "results": [
                    {"path": "src/main.rs", "excerpt": "fn main()"},
                    {"path": "src/main.rs", "excerpt": "async_main awaits service"},
                ]
            }
        )

        observation = score_query_case("repo", case, result)

        self.assertTrue(observation.passed)
        self.assertAlmostEqual(observation.score(), 0.833333, places=5)
        self.assertIn("expected_all=1/2", observation.message)
        self.assertIn("expected_sequence=2/2", observation.message)

    def test_repository_case_can_require_all_expected_hits(self) -> None:
        case = {
            "id": "strict_all",
            "kind": "hybrid",
            "query": "strict all",
            "max_rank": 1,
            "expected": [{"path": "src/main.rs", "excerpt_contains": "main"}],
            "expected_all": [
                {"path": "src/main.rs", "excerpt_contains": "main"},
                {"path": "src/main.rs", "excerpt_contains": "missing step"},
            ],
        }
        result = command_result({"results": [{"path": "src/main.rs", "excerpt": "fn main()"}]})

        observation = score_query_case("repo", case, result)

        self.assertFalse(observation.passed)
        self.assertIn("failures=expected_all=1/2", observation.message)

    def test_repository_case_penalizes_ranked_forbidden_hits(self) -> None:
        case = {
            "id": "soft_forbidden",
            "kind": "hybrid",
            "query": "target",
            "max_rank": 2,
            "min_score": 0.2,
            "forbidden_rank_penalty": 0.2,
            "forbidden_rank_penalty_only": True,
            "expected": [{"path": "target.rs", "excerpt_contains": "target"}],
            "forbidden": [{"path": "noise.rs"}],
        }
        result = command_result(
            {
                "results": [
                    {"path": "noise.rs", "excerpt": "wrong high-rank hit"},
                    {"path": "target.rs", "excerpt": "target symbol"},
                ]
            }
        )

        observation = score_query_case("repo", case, result)

        self.assertTrue(observation.passed)
        self.assertEqual(observation.false_positive_count, 1)
        self.assertAlmostEqual(observation.score(), 0.3)
        self.assertIn("forbidden_penalty=0.200", observation.message)

    def test_repository_set_case_scores_nested_member_hits(self) -> None:
        case = {
            "id": "multi_repo",
            "kind": "hybrid",
            "query": "client.Dial",
            "max_rank": 1,
            "min_score": 0.45,
            "expected": [
                {
                    "repository_alias": "app-self-iteration",
                    "path": "cmd/worker/main.go",
                    "excerpt_contains": "client.Dial",
                }
            ],
            "expected_all": [
                {
                    "repository_alias": "app-self-iteration",
                    "path": "cmd/worker/main.go",
                    "excerpt_contains": "client.Dial",
                },
                {
                    "repository_alias": "sdk-self-iteration",
                    "path": "client/client.go",
                    "excerpt_contains": "func Dial",
                },
            ],
            "require_expected_all": False,
        }
        result = command_result(
            {
                "results": [
                    {
                        "member": {
                            "repository_id": "repo-app",
                            "repository_alias": "app-self-iteration",
                            "source_scope": "scope-app",
                            "resolved_commit_sha": "commit-app",
                            "priority": 10,
                        },
                        "hit": {
                            "repository_id": "repo-app",
                            "scope_id": "scope-app",
                            "path": "cmd/worker/main.go",
                            "line_range": {"start": 10, "end": 14},
                            "excerpt": "c, err := client.Dial(options)",
                        },
                        "overlay_evidence": [],
                        "score": 3.0,
                    },
                    {
                        "member": {
                            "repository_id": "repo-sdk",
                            "repository_alias": "sdk-self-iteration",
                            "source_scope": "scope-sdk",
                            "resolved_commit_sha": "commit-sdk",
                            "priority": 0,
                        },
                        "hit": {
                            "repository_id": "repo-sdk",
                            "scope_id": "scope-sdk",
                            "path": "client/client.go",
                            "line_range": {"start": 1597, "end": 1600},
                            "excerpt": "func Dial(options Options) (Client, error)",
                        },
                        "overlay_evidence": [],
                        "score": 2.0,
                    },
                ]
            }
        )

        observation = score_repository_set_query_case("workspace", case, result)

        self.assertTrue(observation.passed)
        self.assertEqual(observation.repository, "workspace")
        self.assertEqual(observation.rank, 1)
        self.assertIn("expected_all=2/2", observation.message)


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
