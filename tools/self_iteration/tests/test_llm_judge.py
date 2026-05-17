from __future__ import annotations

import json
import shlex
import sys
import tempfile
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))

from llm_judge import (  # noqa: E402
    evaluate_research_judge_suite,
    judge_outcome,
    judge_settings_from_env,
    parse_json_object,
)
from scoring import GateObservation  # noqa: E402


PASSING_JUDGE_PAYLOAD = {
    "passed": True,
    "confidence": 0.9,
    "overall_score": 0.86,
    "scores": {
        "research_alignment": 0.9,
        "architecture_soundness": 0.85,
        "reliability_resilience": 0.8,
        "performance_generalization": 0.82,
        "implementation_actionability": 0.88,
        "anti_fixture_special_casing": 0.91,
    },
    "summary": "general mechanism, no fixture special-casing",
    "evidence": ["diff uses a reusable judge backend"],
    "risks": ["external judge availability"],
    "recommended_cases": ["mock CLI judge fixture"],
}


class LlmJudgeTests(unittest.TestCase):
    def test_unconfigured_judge_is_skipped(self) -> None:
        settings = judge_settings_from_env({})

        self.assertFalse(settings.enabled)
        self.assertEqual(settings.backend, "none")

    def test_cli_backend_accepts_agent_command_alias(self) -> None:
        settings = judge_settings_from_env(
            {
                "RELAY_KNOWLEDGE_JUDGE_BACKEND": "coding-agent",
                "RELAY_KNOWLEDGE_JUDGE_AGENT_COMMAND": "codex exec -",
            }
        )

        self.assertTrue(settings.enabled)
        self.assertTrue(settings.configured)
        self.assertEqual(settings.backend, "cli")
        self.assertEqual(settings.cli_command, "codex exec -")

    def test_http_backend_requires_all_runtime_env(self) -> None:
        settings = judge_settings_from_env(
            {
                "RELAY_KNOWLEDGE_JUDGE_BACKEND": "http",
                "RELAY_KNOWLEDGE_JUDGE_BASE_URL": "https://judge.example/v1",
            }
        )

        self.assertTrue(settings.enabled)
        self.assertFalse(settings.configured)
        self.assertIn("RELAY_KNOWLEDGE_JUDGE_API_KEY", settings.missing)
        self.assertIn("RELAY_KNOWLEDGE_JUDGE_MODEL", settings.missing)

    def test_parse_json_object_accepts_fenced_output(self) -> None:
        payload = parse_json_object("```json\n{\"passed\": true}\n```")

        self.assertEqual(payload, {"passed": True})

    def test_judge_outcome_rejects_low_anti_fixture_score(self) -> None:
        payload = dict(PASSING_JUDGE_PAYLOAD)
        payload["scores"] = dict(PASSING_JUDGE_PAYLOAD["scores"])
        payload["scores"]["anti_fixture_special_casing"] = 0.2

        outcome = judge_outcome(json.dumps(payload), {})

        self.assertFalse(outcome["gate_passed"])
        self.assertFalse(outcome["case_passed"])
        self.assertIn("anti_fixture_special_casing", outcome["message"])

    def test_cli_agent_judge_returns_research_case(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            script = root / "judge_agent.py"
            script.write_text(
                "import json, sys\n"
                "_prompt = sys.stdin.read()\n"
                f"print(json.dumps({PASSING_JUDGE_PAYLOAD!r}))\n",
                encoding="utf-8",
            )
            command = f"{shlex.quote(sys.executable)} {shlex.quote(str(script))}"

            report = evaluate_research_judge_suite(
                workspace=Path(__file__).resolve().parents[3],
                run_home=root / "home",
                env={"RELAY_KNOWLEDGE_JUDGE_COMMAND": command},
                suite_config={},
                generated_diff=True,
                candidate_diff="diff --git a/a b/a\n+judge\n",
                gates=[GateObservation("cargo_test", True)],
                cases=[],
                metrics=[],
                repo_reports=[],
            )

        self.assertEqual(report["index_summary"]["backend"], "cli")
        self.assertEqual(report["index_summary"]["status"], "passed")
        self.assertEqual(len(report["cases"]), 1)
        self.assertTrue(report["cases"][0].passed)
        self.assertEqual(report["cases"][0].objective, "research_judge")
        self.assertAlmostEqual(report["cases"][0].score(), 0.86)


if __name__ == "__main__":
    unittest.main()
