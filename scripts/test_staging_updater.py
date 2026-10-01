"""Boundaries of the staging-only transport and older-source preparation."""
from http.client import IncompleteRead
from contextlib import closing
import json
import os
import sqlite3
from pathlib import Path
import tempfile
import threading
import unittest
from unittest.mock import patch
from urllib.request import urlopen

import staging_updater_smoke as staging


class StagingUpdaterTests(unittest.TestCase):
    def keychain_fixture(self, directory, events):
        path = Path(directory) / "archive.sqlite3"
        with closing(sqlite3.connect(path)) as connection, connection:
            connection.execute("CREATE TABLE events(id INTEGER PRIMARY KEY, kind TEXT, detail TEXT)")
            connection.executemany("INSERT INTO events VALUES(?,?,?)", events)
        return path

    def test_keychain_proof_requires_a_new_startup_and_later_connection_attempt(self):
        with tempfile.TemporaryDirectory() as directory:
            path = self.keychain_fixture(directory, [
                (1, "app_started", "{}"),
                (2, "ui_sync_finished", json.dumps({"error": "qa@example.invalid: imap error: Connection refused"})),
                (3, "app_started", "{}"),
            ])
            self.assertIsNone(staging.new_process_keychain_observation(path, 2))
            with closing(sqlite3.connect(path)) as connection, connection:
                connection.execute("INSERT INTO events VALUES(4,'ui_sync_finished',?)", (
                    json.dumps({"error": "qa@example.invalid: imap error: io error: Connection refused (os error 61)"}),))
            result = staging.new_process_keychain_observation(path, 2)
            self.assertTrue(result["new_process_keychain_read_passed"])
            self.assertEqual(result["event_id"], 4)
            self.assertFalse(result["provider_login_attempted"])

    def test_keychain_proof_rejects_missing_or_denied_credentials(self):
        for error in ["qa@example.invalid: The saved password is unavailable.",
                      "qa@example.invalid: The saved mailbox password could not be read from secure storage."]:
            with self.subTest(error=error), tempfile.TemporaryDirectory() as directory:
                path = self.keychain_fixture(directory, [
                    (1, "app_started", "{}"),
                    (2, "ui_sync_finished", json.dumps({"error": error})),
                ])
                with self.assertRaisesRegex(RuntimeError, "connection step"):
                    staging.new_process_keychain_observation(path, 0)

    def test_keychain_proof_does_not_accept_old_process_sync_after_baseline(self):
        with tempfile.TemporaryDirectory() as directory:
            path = self.keychain_fixture(directory, [
                (1, "app_started", "{}"),
                (2, "ui_sync_finished", json.dumps({"error": "qa@example.invalid: imap error: Connection refused"})),
            ])
            self.assertIsNone(staging.new_process_keychain_observation(path, 1))

    def test_server_delivers_exact_bytes_and_rejects_unknown_paths(self):
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "payload.tar.gz"
            payload = bytes(range(256)) * 80
            path.write_bytes(payload)
            with patch.object(staging, "PORT", 0):
                server = staging.StagingServer(path, {"signature": "synthetic signature"})
            threading.Thread(target=server.serve_forever, daemon=True).start()
            endpoint = f"http://127.0.0.1:{server.server_port}"
            try:
                server.mode = "success"
                with urlopen(endpoint + "/candidate.tar.gz") as response:
                    self.assertEqual(response.read(), payload)
                with urlopen(endpoint + "/latest.json") as response:
                    self.assertEqual(json.load(response)["version"], staging.VERSION)
                server.mode = "bad_signature"
                with urlopen(endpoint + "/candidate.tar.gz") as response:
                    changed = response.read()
                self.assertEqual(len(changed), len(payload))
                self.assertEqual(sum(a != b for a, b in zip(changed, payload)), 1)
                server.mode = "interrupted"
                with self.assertRaises(IncompleteRead):
                    with urlopen(endpoint + "/candidate.tar.gz") as response:
                        response.read()
                with self.assertRaisesRegex(Exception, "404"):
                    urlopen(endpoint + "/private-file")
            finally:
                server.shutdown()
                server.server_close()

    def test_preparation_refuses_active_checkouts_before_any_edit(self):
        with tempfile.TemporaryDirectory() as directory:
            source = Path(directory).resolve()
            (source / ".git").mkdir()
            with patch.object(staging, "require_ci"), patch.dict(os.environ, {"RUNNER_TEMP": directory}):
                with self.assertRaisesRegex(RuntimeError, "Git checkout"):
                    staging.prepare(source)

    def test_preparation_refuses_exports_outside_runner_temp(self):
        with tempfile.TemporaryDirectory() as directory:
            with patch.object(staging, "require_ci"), patch.dict(os.environ, {"RUNNER_TEMP": directory}):
                with self.assertRaisesRegex(RuntimeError, "RUNNER_TEMP"):
                    staging.prepare(Path("/Users/johannes/GIT/email-archiver"))


if __name__ == "__main__":
    unittest.main()
