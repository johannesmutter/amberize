"""Boundaries of the staging-only transport and older-source preparation."""
from http.client import IncompleteRead
import json
import os
from pathlib import Path
import tempfile
import threading
import unittest
from unittest.mock import patch
from urllib.request import urlopen

import staging_updater_smoke as staging


class StagingUpdaterTests(unittest.TestCase):
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
