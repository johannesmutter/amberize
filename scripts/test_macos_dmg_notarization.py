"""Test rejection and confidentiality boundaries without contacting Apple."""
import json
from pathlib import Path
import subprocess
import unittest
from unittest.mock import patch
import notarize_macos_dmgs as dmg


CREDENTIALS = {"APPLE_ID": "qa@example.invalid", "APPLE_PASSWORD": "aaaa-bbbb-cccc-dddd", "APPLE_TEAM_ID": "QATEST1234"}


class DmgNotarizationTests(unittest.TestCase):
    def assert_sanitized(self, report):
        output = json.dumps(report)
        for value in CREDENTIALS.values():
            self.assertNotIn(value, output)

    def test_accepted_submission_requires_all_container_checks(self):
        response = json.dumps({"status": "Accepted", "id": "12345678-1234-1234-1234-123456789abc", "message": CREDENTIALS["APPLE_PASSWORD"]})
        results = [subprocess.CompletedProcess([], 0, stdout="", stderr=""),
                   subprocess.CompletedProcess([], 0, stdout=response, stderr="")]
        results.extend(subprocess.CompletedProcess([], 0, stdout=CREDENTIALS["APPLE_PASSWORD"], stderr="") for _ in range(4))
        with patch.object(dmg.subprocess, "run", side_effect=results):
            report = dmg.notarize(Path("candidate.dmg"), CREDENTIALS)
        self.assertTrue(report["passed"])
        self.assertEqual(report["checks"][-1]["step"], "gatekeeper_container")
        self.assert_sanitized(report)

    def test_rejected_submission_never_staples_or_reports_success(self):
        response = json.dumps({"status": "Invalid", "message": " ".join(CREDENTIALS.values())})
        results = [subprocess.CompletedProcess([], 0, stdout="", stderr=""),
                   subprocess.CompletedProcess([], 0, stdout=response, stderr="")]
        with patch.object(dmg.subprocess, "run", side_effect=results) as run:
            report = dmg.notarize(Path("candidate.dmg"), CREDENTIALS)
        self.assertFalse(report["passed"])
        self.assertEqual(report["failed_step"], "notarization")
        self.assertEqual(run.call_count, 2)
        self.assert_sanitized(report)

    def test_timeout_command_does_not_expose_credentials(self):
        results = [subprocess.CompletedProcess([], 0, stdout="", stderr=""),
                   subprocess.TimeoutExpired(["xcrun", "--password", CREDENTIALS["APPLE_PASSWORD"]], 1260)]
        with patch.object(dmg.subprocess, "run", side_effect=results):
            report = dmg.notarize(Path("candidate.dmg"), CREDENTIALS)
        self.assertFalse(report["passed"])
        self.assert_sanitized(report)

    def test_invalid_signature_does_not_submit_to_apple(self):
        with patch.object(dmg.subprocess, "run", return_value=subprocess.CompletedProcess([], 1, stdout="", stderr=CREDENTIALS["APPLE_PASSWORD"])) as run:
            report = dmg.notarize(Path("candidate.dmg"), CREDENTIALS)
        self.assertFalse(report["passed"])
        self.assertEqual(report["failed_step"], "verify_input_signature")
        self.assertEqual(run.call_count, 1)
        self.assert_sanitized(report)


if __name__ == "__main__":
    unittest.main()
