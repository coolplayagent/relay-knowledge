from __future__ import annotations

import sys
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))

from codex_driver import CodexConfig, build_codex_command


class CodexDriverTests(unittest.TestCase):
    def test_yolo_command_uses_non_interactive_high_permission_flags(self) -> None:
        command = build_codex_command(
            CodexConfig(
                workspace=Path("/repo"),
                codex_path="/bin/codex",
                yolo=True,
            )
        )

        self.assertEqual(command[0], "/bin/codex")
        self.assertLess(command.index("-a"), command.index("exec"))
        self.assertIn("--dangerously-bypass-approvals-and-sandbox", command)
        self.assertIn("never", command)
        self.assertIn("danger-full-access", command)
        self.assertEqual(command[-1], "-")


if __name__ == "__main__":
    unittest.main()
