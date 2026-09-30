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

VERSION = "0.2.4-6"
PORT = 18743
CANDIDATE_COMMIT = "676f4736adb877cd29cb413c40e3c2ff908c1523"


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
    for anchor in ("mod app_commands;", "            Ok(())", "            app_commands::restart_app,"):
        assert original.count(anchor) == 1, "Older source does not match the expected tag"
    original = original.replace("mod app_commands;", "mod app_commands;\nmod qa_updater;", 1)
    original = original.replace("            Ok(())", "            qa_updater::start(app.handle().clone());\n            Ok(())", 1)
    original = original.replace("            app_commands::restart_app,", "            app_commands::restart_app,\n            qa_updater::qa_updater_observation,", 1)
    main.write_text(original)
    shutil.copyfile(Path(__file__).with_name("staging_updater_probe.rs"), main.with_name("qa_updater.rs"))
    config_path = source / "apps/desktop/src-tauri/tauri.conf.json"
    config = json.loads(config_path.read_text())
    assert config["version"] == "0.2.3" and config["identifier"] == "com.amberize.app"
    updater = config["plugins"]["updater"]
    updater["endpoints"] = [f"http://127.0.0.1:{PORT}/latest.json"]
    updater["dangerousInsecureTransportProtocol"] = True
    config_path.write_text(json.dumps(config, indent=2) + "\n")


def account_state(fixture):
    with closing(sqlite3.connect(f"file:{fixture}?mode=ro", uri=True)) as connection:
        return connection.execute("SELECT id,secret_ref FROM accounts ORDER BY id").fetchall()


def launch_agents(binary):
    directory = Path.home() / "Library/LaunchAgents"
    result = {}
    for path in directory.glob("*.plist"):
        raw = path.read_bytes()
        if str(binary).encode() not in raw:
            continue
        parsed = plistlib.loads(raw)
        assert "--background" in parsed.get("ProgramArguments", [])
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
    subprocess.run([str(args.fixture_tool), "create", str(fixture), "1024"], check=True,
                   stdout=(root / "before.json").open("w"))
    config = {"db_path": str(fixture), "sync_interval_secs": 3600}
    config_path.write_text(json.dumps(config))
    baseline = fingerprint(fixture)
    baseline_accounts = account_state(fixture)
    manifest = json.loads(args.manifest.read_text())
    assert manifest["version"] == VERSION
    entry = manifest["platforms"]["darwin-aarch64"]
    assert entry["url"].endswith("/v0.2.4-6/Amberize_aarch64.app.tar.gz")
    candidate = args.candidate.resolve(strict=True)
    assert sha256(candidate) == args.candidate_sha256
    server = StagingServer(candidate, entry)
    threading.Thread(target=server.serve_forever, daemon=True).start()
    installed = root / "installed/Amberize.app"
    binary = installed / "Contents/MacOS/Amberize"
    report = {"passed": False, "candidate_commit": CANDIDATE_COMMIT,
              "candidate_payload_sha256": sha256(candidate), "old_tag": "v0.2.3",
              "old_build_binary_sha256": sha256(args.old_app / "Contents/MacOS/Amberize"),
              "test_endpoint": f"http://127.0.0.1:{PORT}/latest.json", "stages": [],
              "differences": ["Older source rebuilt with a loopback-only HTTP staging endpoint",
                              "Unsigned older test build; candidate payload is the exact signed draft",
                              "Old native setup gains a test-only observer that clicks unmodified UI buttons"],
              "limitations": ["Hosted macOS Apple Silicon only", "No actual OS reboot",
                              "Synthetic archive has no real provider credentials",
                              "Does not establish signed old-app Keychain access after update"]}
    agent_paths = []
    try:
        for mode in ("bad_signature", "interrupted", "success"):
            server.mode = mode
            if installed.exists():
                shutil.rmtree(installed)
            shutil.copytree(args.old_app, installed, symlinks=True)
            phase = root / mode
            phase.mkdir()
            environment = dict(os.environ, AMBERIZE_UPDATER_QA_DIR=str(phase))
            with (phase / "app.log").open("wb") as output:
                process = subprocess.Popen([str(binary)], env=environment, stdout=output, stderr=output)
            old_hash = sha256(binary)
            initial_integrity = event_count(fixture, "integrity_check")
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
                    old_agents = launch_agents(binary)
                    assert old_agents, "Older UI did not register launch at login"
                    agent_paths.extend(old_agents)
                if mode != "success" and "install_error" in last_stages:
                    assert sha256(binary) == old_hash, "Failed update changed the installed executable"
                    assert plistlib.loads((installed / "Contents/Info.plist").read_bytes())["CFBundleShortVersionString"] == "0.2.3"
                    break
                if mode == "success" and "restart_clicked" in last_stages:
                    assert sha256(binary) == args.candidate_binary_sha256, "Installed app is not the exact draft binary"
                    version = plistlib.loads((installed / "Contents/Info.plist").read_bytes())["CFBundleShortVersionString"]
                    assert version == VERSION
                    if (process.poll() is not None and processes(binary)
                            and event_count(fixture, "integrity_check") > initial_integrity):
                        assert launch_agents(binary) == old_agents, "Launch-at-login registration changed"
                        report["restarted_binary_sha256"] = sha256(binary)
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
        report["requests"] = server.requests
        report_path.write_text(json.dumps(report, indent=2) + "\n")
    print(json.dumps(report, indent=2))


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    sub = parser.add_subparsers(dest="command", required=True)
    preparation = sub.add_parser("prepare")
    preparation.add_argument("source", type=Path)
    test = sub.add_parser("run")
    for flag in ("root", "old-app", "candidate", "manifest", "fixture-tool"):
        test.add_argument("--" + flag, type=Path, required=True)
    test.add_argument("--candidate-sha256", required=True)
    test.add_argument("--candidate-binary-sha256", required=True)
    arguments = parser.parse_args()
    if arguments.command == "prepare":
        prepare(arguments.source.resolve(strict=True))
    else:
        run(arguments)
