#!/usr/bin/env python3

import pathlib
import re
import unittest


SCRIPT_PATH = pathlib.Path(__file__).with_name("run_isolated_docker_gate.sh")
SCRIPT = SCRIPT_PATH.read_text(encoding="utf-8")


class RuntimeGateContractTests(unittest.TestCase):
    def test_artifact_state_uses_the_same_host_and_provider_path(self) -> None:
        self.assertIn(
            "artifact_state_root=$provider_root/artifact-state",
            SCRIPT,
        )
        self.assertIn(
            'provider_artifact_mount=(--mount "type=bind,source=$artifact_state_root,'
            'target=$artifact_state_root")',
            SCRIPT,
        )
        self.assertIn('"${provider_artifact_mount[@]}"', SCRIPT)
        self.assertIn(
            'A3S_CLOUD_TEST_ARTIFACT_STATE_ROOT="$artifact_state_root"',
            SCRIPT,
        )
        self.assertIn(
            'chmod 700 "$provider_root" "$artifact_state_root" "$cargo_home"',
            SCRIPT,
        )

    def test_every_cargo_command_uses_the_isolated_home(self) -> None:
        self.assertIn("cargo_home=$provider_root/cargo-home", SCRIPT)
        cargo_commands = SCRIPT.count('"$cargo_bin" test')
        self.assertGreaterEqual(cargo_commands, 1)
        self.assertEqual(
            SCRIPT.count('CARGO_HOME="$cargo_home"'),
            cargo_commands,
        )

    def test_git_cleanliness_checks_do_not_refresh_indexes(self) -> None:
        status_commands = re.findall(
            r"^.*git -C \"\$(?:cloud|runtime)\" status --porcelain=v1.*$",
            SCRIPT,
            flags=re.MULTILINE,
        )
        self.assertGreaterEqual(len(status_commands), 6)
        self.assertTrue(
            all("GIT_OPTIONAL_LOCKS=0" in command for command in status_commands)
        )

    def test_artifact_residue_is_recorded_asserted_and_removed(self) -> None:
        self.assertIn(
            'printf \'%s\\n\' "$artifact_state_root" '
            '>"$evidence/artifact-state-root.txt"',
            SCRIPT,
        )
        self.assertIn("artifact-files-after-test.paths", SCRIPT)
        self.assertIn("artifact-files-before-cleanup.paths", SCRIPT)
        success_assertions = re.search(
            r"for empty_file in \\\n(?P<paths>.*?)\; do",
            SCRIPT,
            flags=re.DOTALL,
        )
        self.assertIsNotNone(success_assertions)
        assert success_assertions is not None
        self.assertIn(
            '"$evidence/artifact-files-after-test.paths"',
            success_assertions.group("paths"),
        )
        self.assertIn('rm -rf "$artifact_state_root"', SCRIPT)
        self.assertIn(
            'cleanup-error=artifact-state-directory-remains',
            SCRIPT,
        )


if __name__ == "__main__":
    unittest.main()
