"""Guard against repository-controlled Claude startup hooks."""

from __future__ import annotations

import json
import pathlib
import unittest

ROOT = pathlib.Path(__file__).resolve().parents[2]
SETTINGS = ROOT / ".claude" / "settings.json"


class ClaudeSettingsSecurityTests(unittest.TestCase):
    def test_session_start_does_not_execute_repo_commands(self) -> None:
        """Opening the repo in Claude Code must not auto-run mutable repo shell."""
        settings = json.loads(SETTINGS.read_text(encoding="utf-8"))

        session_start_hooks = settings.get("hooks", {}).get("SessionStart", [])
        command_hooks = [
            hook
            for group in session_start_hooks
            for hook in group.get("hooks", [])
            if hook.get("type") == "command"
        ]

        self.assertEqual([], command_hooks)


if __name__ == "__main__":
    unittest.main()
