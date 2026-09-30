"""Exercise diagnostic failures with synthetic credentials; never contact Apple."""
import json
import subprocess
import unittest
from unittest.mock import patch
import check_notarization_auth as auth


CREDENTIALS = {"APPLE_ID": "qa@example.invalid", "APPLE_PASSWORD": "aaaa-bbbb-cccc-dddd", "APPLE_TEAM_ID": "QATEST1234"}


class NotarizationAuthTests(unittest.TestCase):
    def assert_sanitized(self, report):
        text = json.dumps(report)
        for value in CREDENTIALS.values():
            self.assertNotIn(value, text)

    def test_success_does_not_publish_history_or_credentials(self):
        result = subprocess.CompletedProcess([], 0, stdout=json.dumps({"history": [{"name": "private-app-name"}]}), stderr="")
        with patch.object(auth.subprocess, "run", return_value=result) as run:
            report = auth.diagnose(CREDENTIALS)
        self.assertTrue(report["original"]["accepted"])
        self.assertEqual(run.call_args.args[0][:3], ["xcrun", "notarytool", "history"])
        self.assertNotIn("private-app-name", json.dumps(report))
        self.assert_sanitized(report)

    def test_401_does_not_echo_tool_error_with_secret_values(self):
        error = "HTTP status code: 401. " + " ".join(CREDENTIALS.values())
        with patch.object(auth.subprocess, "run", return_value=subprocess.CompletedProcess([], 1, stdout="", stderr=error)):
            report = auth.diagnose(CREDENTIALS)
        self.assertFalse(report["original"]["accepted"])
        self.assertEqual(report["original"]["http_status"], 401)
        self.assert_sanitized(report)

    def test_timeout_exception_command_does_not_expose_password(self):
        error = subprocess.TimeoutExpired(["xcrun", "--password", CREDENTIALS["APPLE_PASSWORD"]], 60)
        with patch.object(auth.subprocess, "run", side_effect=error):
            report = auth.diagnose(CREDENTIALS)
        self.assertFalse(report["original"]["accepted"])
        self.assert_sanitized(report)

    def test_unexpected_exception_does_not_expose_its_text(self):
        with patch.object(auth.subprocess, "run", side_effect=ValueError(CREDENTIALS["APPLE_PASSWORD"])):
            report = auth.diagnose(CREDENTIALS)
        self.assertFalse(report["original"]["accepted"])
        self.assert_sanitized(report)

    def test_missing_credentials_do_not_contact_apple(self):
        with patch.object(auth.subprocess, "run") as run:
            report = auth.diagnose({"APPLE_ID": CREDENTIALS["APPLE_ID"]})
        run.assert_not_called()
        self.assertEqual(set(report["missing"]), {"APPLE_PASSWORD", "APPLE_TEAM_ID"})

    def test_whitespace_retry_identifies_fix_without_changing_credentials(self):
        credentials = {name: value + "\n" for name, value in CREDENTIALS.items()}
        original = dict(credentials)
        results = [subprocess.CompletedProcess([], 1, stdout="", stderr="HTTP status code: 401"),
                   subprocess.CompletedProcess([], 0, stdout="{}", stderr="")]
        with patch.object(auth.subprocess, "run", side_effect=results):
            report = auth.diagnose(credentials)
        self.assertFalse(report["original"]["accepted"])
        self.assertTrue(report["normalized"]["accepted"])
        self.assertEqual(credentials, original)
        self.assert_sanitized(report)


if __name__ == "__main__":
    unittest.main()
