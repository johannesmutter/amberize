"""Exercise a v0.2.3 UI against the exact signed draft on a fresh macOS CI host."""
import argparse
from contextlib import closing
import hashlib
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
import json
import os
from pathlib import Path
import plistlib
import shutil
import socket
import sqlite3
import subprocess
import threading
import time

from native_release_smoke import fingerprint, event_count

VERSION = "0.2.4-7"
PORT = 18743
CANDIDATE_COMMIT = "bcdb730817eae47a9d6b615a9182c1dbdcb900f7"


def sha256(path):
    with Path(path).open("rb") as stream:
        return hashlib.file_digest(stream, "sha256").hexdigest()


def require_ci():
    if os.environ.get("GITHUB_ACTIONS") != "true" or os.sys.platform != "darwin":
        raise RuntimeError("This test requires a fresh hosted macOS CI profile")


def prepare(source):
    require_ci()
    # The checkout must be an untouched export of the old tag, not the active project.
    if not source.is_relative_to(Path(os.environ["RUNNER_TEMP"]).resolve()):
        raise RuntimeError("Older export must stay inside RUNNER_TEMP")
    if (source / ".git").exists():
        raise RuntimeError("Refusing to instrument a Git checkout")
    main = source / "apps/desktop/src-tauri/src/main.rs"
    original = main.read_text()
    if "qa_updater" in original:
        raise RuntimeError("Older export is already instrumented")
    lockfile = source / "Cargo.lock"
    locked = lockfile.read_text()
    stale_version = 'name = "amberize"\nversion = "0.2.2"'
    assert locked.count(stale_version) == 1, "Unexpected old application lock entry"
    # The v0.2.3 tag retained v0.2.2 here; change only the workspace package
    # identity so --locked preserves every external dependency version.
    lockfile.write_text(locked.replace(stale_version, 'name = "amberize"\nversion = "0.2.3"', 1))
    for anchor in ("mod app_commands;", "            Ok(())", "            app_commands::restart_app,"):
        assert original.count(anchor) == 1, "Older source does not match the expected tag"
    original = original.replace("mod app_commands;", "mod app_commands;\nmod qa_updater;", 1)
    original = original.replace("            Ok(())", "            qa_updater::start(app.handle().clone());\n            Ok(())", 1)
    original = original.replace("            app_commands::restart_app,", "            app_commands::restart_app,\n            qa_updater::qa_updater_observation,", 1)
    main.write_text(original)
    shutil.copyfile(Path(__file__).with_name("staging_updater_probe.rs"), main.with_name("qa_updater.rs"))
    examples = source / "crates/storage/examples"
    examples.mkdir(exist_ok=True)
    shutil.copyfile(Path(__file__).with_name("staging_updater_old_fixture.rs"), examples / "qa_updater_old_fixture.rs")
    config_path = source / "apps/desktop/src-tauri/tauri.conf.json"
    config = json.loads(config_path.read_text())
    assert config["version"] == "0.2.3" and config["identifier"] == "com.amberize.app"
    # This temporary old app consumes signed updates; it is not an update
    # payload and must not request the production private signing key.
    config["bundle"]["createUpdaterArtifacts"] = False
    updater = config["plugins"]["updater"]
    updater["endpoints"] = [f"http://127.0.0.1:{PORT}/latest.json"]
    updater["dangerousInsecureTransportProtocol"] = True
    config_path.write_text(json.dumps(config, indent=2) + "\n")


def account_state(fixture):
    with closing(sqlite3.connect(f"file:{fixture}?mode=ro", uri=True)) as connection:
        return connection.execute("SELECT id,secret_ref FROM accounts ORDER BY id").fetchall()


def schema_version(fixture):
    with closing(sqlite3.connect(f"file:{fixture}?mode=ro", uri=True)) as connection:
        return int(connection.execute("SELECT value FROM schema_meta WHERE key='schema_version'").fetchone()[0])


def signed_requirement(app):
    subprocess.run(["codesign", "--verify", "--deep", "--strict", str(app)], check=True,
                   stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
    result = subprocess.run(["codesign", "-d", "-r-", str(app)], check=True,
                            capture_output=True, text=True)
    requirement = next(row.split("designated => ", 1)[1] for row in
                       (result.stdout + result.stderr).splitlines() if row.startswith("designated => "))
    assert 'identifier "com.amberize.app"' in requirement and "anchor apple generic" in requirement
    assert 'certificate leaf[subject.OU] = "4Y5FU7KFWS"' in requirement
    return requirement


def new_process_keychain_observation(fixture, baseline_event_id):
    # The production sync reads its saved secret before trying the TCP connection.
    # Port 9 is deliberately closed. A new-process IMAP connection refusal proves
    # that the updated signed app read the credential written by the old signed app.
    with closing(sqlite3.connect(f"file:{fixture}?mode=ro", uri=True)) as connection:
        started = connection.execute("SELECT MAX(id) FROM events WHERE id>? AND kind='app_started'",
                                     (baseline_event_id,)).fetchone()[0]
        if started is None:
            return None
        rows = connection.execute("SELECT id,detail FROM events WHERE id>? AND kind='ui_sync_finished' ORDER BY id",
                                  (started,)).fetchall()
    for event_id, detail in rows:
        error = json.loads(detail).get("error")
        if error is None:
            continue
        if error.startswith("qa@example.invalid: imap error:") and "connection refused" in error.lower():
            return {"new_process_keychain_read_passed": True, "provider_connection_attempted": True,
                    "provider_login_attempted": False, "event_id": event_id,
                    "credential_value_recorded": False}
        raise RuntimeError("Updated app did not reach the expected synthetic IMAP connection step")
    return None


def launch_agents(binary, require_background=False):
    directory = Path.home() / "Library/LaunchAgents"
    result = {}
    for path in directory.glob("*.plist"):
        raw = path.read_bytes()
        if str(binary).encode() not in raw:
            continue
        parsed = plistlib.loads(raw)
        assert parsed.get("RunAtLoad") is True, "Login entry is not enabled"
        if require_background:
            assert parsed.get("ProgramArguments") == [str(binary), "--background"], "Updated login entry lacks the current executable and background flag"
        result[str(path)] = {"sha256": hashlib.sha256(raw).hexdigest(), "value": parsed}
    return result


def processes(binary):
    output = subprocess.check_output(["ps", "-ax", "-o", "pid=,command="], text=True)
    return [int(row.split(maxsplit=1)[0]) for row in output.splitlines()
            if len(row.split(maxsplit=1)) == 2 and row.split(maxsplit=1)[1].startswith(str(binary))]


def stop(binary):
    for pid in processes(binary):
        os.kill(pid, 15)
    deadline = time.monotonic() + 10
    while processes(binary) and time.monotonic() < deadline:
        time.sleep(.2)
    if processes(binary):
        raise RuntimeError("Updater test app did not terminate")


class StagingServer(ThreadingHTTPServer):
    daemon_threads = True

    def __init__(self, payload, entry):
        super().__init__(("127.0.0.1", PORT), StagingHandler)
        self.payload = payload
        self.manifest = {"version": VERSION, "platforms": {"darwin-aarch64": {
            "url": f"http://127.0.0.1:{PORT}/candidate.tar.gz", "signature": entry["signature"]}}}
        self.mode = "bad_signature"
        self.requests = []


class StagingHandler(BaseHTTPRequestHandler):
    def log_message(self, *_):
        pass

    def do_GET(self):
        self.server.requests.append({"mode": self.server.mode, "path": self.path})
        if self.path == "/latest.json":
            body = json.dumps(self.server.manifest).encode()
        elif self.path == "/candidate.tar.gz":
            body = self.server.payload.read_bytes()
            if self.server.mode == "bad_signature":
                body = body[:100] + bytes([body[100] ^ 1]) + body[101:]
        else:
            self.send_error(404)
            return
        self.send_response(200)
        self.send_header("Content-Type", "application/json" if self.path.endswith(".json") else "application/octet-stream")
        self.send_header("Content-Length", str(len(body)))
        self.end_headers()
        try:
            if self.path == "/candidate.tar.gz" and self.server.mode == "interrupted":
                self.wfile.write(body[:8192])
                self.wfile.flush()
                self.connection.shutdown(socket.SHUT_RDWR)
                self.connection.close()
            else:
                self.wfile.write(body)
        except (BrokenPipeError, ConnectionResetError):
            pass


def run(args):
    require_ci()
    root = args.root.resolve()
    runner_temp = Path(os.environ["RUNNER_TEMP"]).resolve()
    assert root.is_relative_to(runner_temp) and not root.exists()
    root.mkdir()
    report_path = root / "staging-update.json"
    profile = Path.home() / "Library/Application Support/com.amberize.app"
    assert not profile.exists(), "Refusing to use an existing application profile"
    profile.mkdir(parents=True)
    config_path = profile / "config.json"
    fixture = root / "archive.sqlite3"
    subprocess.run([str(args.old_fixture_tool), "create", str(fixture), "1024"], check=True,
                   stdout=(root / "before.json").open("w"))
    config = {"db_path": str(fixture), "sync_interval_secs": 3600}
    config_path.write_text(json.dumps(config))
    baseline = fingerprint(fixture)
    baseline_accounts = account_state(fixture)
    assert schema_version(fixture) == 2, "Fixture is not a genuine older-format archive"
    manifest = json.loads(args.manifest.read_text())
    assert manifest["version"] == VERSION
    entry = manifest["platforms"]["darwin-aarch64"]
    assert entry["url"].endswith(f"/v{VERSION}/Amberize_aarch64.app.tar.gz")
    candidate = args.candidate.resolve(strict=True)
    assert sha256(candidate) == args.candidate_sha256
    candidate_app = root / "signed-candidate"
    candidate_app.mkdir()
    subprocess.run(["tar", "-xzf", str(candidate), "-C", str(candidate_app)], check=True)
    old_requirement = signed_requirement(args.old_app)
    assert old_requirement == signed_requirement(candidate_app / "Amberize.app"), "Old/new signing requirements differ"
    with closing(socket.socket()) as connection:
        assert connection.connect_ex(("127.0.0.1", 9)) != 0, "Synthetic IMAP port must be closed"
    server = StagingServer(candidate, entry)
    threading.Thread(target=server.serve_forever, daemon=True).start()
    installed = root / "installed/Amberize.app"
    binary = installed / "Contents/MacOS/Amberize"
    report = {"passed": False, "candidate_commit": CANDIDATE_COMMIT,
              "candidate_payload_sha256": sha256(candidate), "old_tag": "v0.2.3",
              "old_build_binary_sha256": sha256(args.old_app / "Contents/MacOS/Amberize"),
              "old_archive_schema_version": 2,
              "test_endpoint": f"http://127.0.0.1:{PORT}/latest.json", "stages": [],
              "differences": ["Older source rebuilt with a loopback-only HTTP staging endpoint",
                              "Old workspace lock entry corrected from 0.2.2 to 0.2.3; external dependency versions unchanged",
                              "Older instrumented app signed with the production Developer ID; candidate payload is the exact signed draft",
                              "Updater artifact generation disabled for the temporary older build",
                              "Old native setup gains a test-only observer that clicks unmodified UI buttons"],
              "limitations": ["Hosted macOS Apple Silicon only", "No actual OS reboot",
                              "Synthetic Keychain credential and deliberately unavailable loopback provider; no real provider login"],
              "old_and_candidate_designated_requirements_match": True}
    agent_paths = []
    try:
        for mode in ("bad_signature", "interrupted", "success"):
            server.mode = mode
            if installed.exists():
                shutil.rmtree(installed)
            shutil.copytree(args.old_app, installed, symlinks=True)
            phase = root / mode
            phase.mkdir()
            if mode == "success":
                rollback = root / "rollback.sqlite3"
                with closing(sqlite3.connect(f"file:{fixture}?mode=ro", uri=True)) as source:
                    with closing(sqlite3.connect(rollback)) as destination:
                        source.backup(destination)
                assert fingerprint(rollback) == baseline and schema_version(rollback) == 2
            environment = dict(os.environ, AMBERIZE_UPDATER_QA_DIR=str(phase))
            with (phase / "app.log").open("wb") as output:
                process = subprocess.Popen([str(binary)], env=environment, stdout=output, stderr=output)
            old_hash = sha256(binary)
            deadline = time.monotonic() + 180
            last_stages = []
            old_agents = None
            while time.monotonic() < deadline:
                observations = phase / "observations.json"
                if observations.exists():
                    try:
                        last_stages = json.loads(observations.read_text())
                    except json.JSONDecodeError:
                        time.sleep(.1)
                        continue
                assert "automation_error" not in last_stages, "UI automation failed"
                if "ready" in last_stages and old_agents is None:
                    written = json.loads((phase / "keychain-written.json").read_text())
                    assert written["synthetic_credential_written_by_old_app"] is True
                    old_agents = launch_agents(binary)
                    assert old_agents, "Older UI did not register launch at login"
                    agent_paths.extend(old_agents)
                if mode != "success" and "install_error" in last_stages:
                    assert sha256(binary) == old_hash, "Failed update changed the installed executable"
                    assert plistlib.loads((installed / "Contents/Info.plist").read_bytes())["CFBundleShortVersionString"] == "0.2.3"
                    break
                if mode == "success" and "restart_clicked" in last_stages:
                    restart_baseline = json.loads((phase / "restart-baseline.json").read_text())
                    assert sha256(binary) == args.candidate_binary_sha256, "Installed app is not the exact draft binary"
                    version = plistlib.loads((installed / "Contents/Info.plist").read_bytes())["CFBundleShortVersionString"]
                    assert version == VERSION
                    if (process.poll() is not None and processes(binary)
                            and event_count(fixture, "app_started") > restart_baseline["app_started"]
                            and event_count(fixture, "integrity_check") > restart_baseline["integrity_check"]):
                        new_agents = launch_agents(binary, require_background=True)
                        assert set(new_agents) == set(old_agents), "Launch-at-login choice was lost"
                        for name, agent in new_agents.items():
                            assert agent["value"]["Label"] == old_agents[name]["value"]["Label"]
                        report["old_launch_at_login"] = old_agents
                        report["updated_launch_at_login"] = new_agents
                        report["launch_at_login_refreshed_for_background"] = True
                        report["restarted_binary_sha256"] = sha256(binary)
                        report["new_process_restored_and_verified_archive"] = True
                        observed = new_process_keychain_observation(fixture, restart_baseline["max_event_id"])
                        if observed is None:
                            time.sleep(.2)
                            continue
                        report["keychain_upgrade"] = observed
                        break
                if mode == "success" and "install_error" in last_stages:
                    raise RuntimeError("Valid signed candidate installation failed")
                time.sleep(.2)
            else:
                raise RuntimeError(f"{mode}: UI upgrade did not reach its expected final state")
            stop(binary)
            process.wait(timeout=10)
            assert fingerprint(fixture) == baseline, "Archive counts or MIME hashes changed"
            assert account_state(fixture) == baseline_accounts, "Account credential references changed"
            assert json.loads(config_path.read_text()) == config, "Saved archive or sync interval changed"
            assert any(request["mode"] == mode and request["path"] == "/candidate.tar.gz"
                       for request in server.requests), "App did not download from staging"
            report["stages"].append({"mode": mode, "ui_observations": last_stages, "passed": True})
        subprocess.run([str(args.fixture_tool), "verify", str(fixture)], check=True,
                       stdout=(root / "after.json").open("w"))
        after = json.loads((root / "after.json").read_text())
        assert after["integrity"]["ok"]
        report["updated_archive_schema_version"] = schema_version(fixture)
        assert report["updated_archive_schema_version"] == 3
        subprocess.run([str(args.old_fixture_tool), "verify", str(root / "rollback.sqlite3")], check=True,
                       stdout=(root / "rollback-old-version-verification.json").open("w"))
        report["older_archive_backup_verified_by_old_library"] = True
        report["mime_hashes_preserved"] = len(baseline["hashes"])
        report["account_credential_references_preserved"] = True
        report["launch_at_login_preserved"] = True
        report["passed"] = True
    except BaseException as error:
        report["error"] = str(error)
        raise
    finally:
        if binary.exists():
            stop(binary)
        server.shutdown()
        server.server_close()
        for path in set(agent_paths):
            Path(path).unlink(missing_ok=True)
        for _, secret_ref in baseline_accounts:
            assert secret_ref.startswith("qa-old-fixture/")
            subprocess.run(["security", "delete-generic-password", "-s", "com.amberize.app", "-a", secret_ref],
                           stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL, check=False)
        report["requests"] = server.requests
        report_path.write_text(json.dumps(report, indent=2) + "\n")
    print(json.dumps(report, indent=2))


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    sub = parser.add_subparsers(dest="command", required=True)
    preparation = sub.add_parser("prepare")
    preparation.add_argument("source", type=Path)
    test = sub.add_parser("run")
    for flag in ("root", "old-app", "candidate", "manifest", "fixture-tool", "old-fixture-tool"):
        test.add_argument("--" + flag, type=Path, required=True)
    test.add_argument("--candidate-sha256", required=True)
    test.add_argument("--candidate-binary-sha256", required=True)
    arguments = parser.parse_args()
    if arguments.command == "prepare":
        prepare(arguments.source.resolve(strict=True))
    else:
        run(arguments)
