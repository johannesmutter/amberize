"""Exercise the old native UI and real Windows MSI/Linux AppImage updater in fresh CI."""
import argparse
from contextlib import closing
import json
import os
from pathlib import Path
import shutil
import socket
import sqlite3
import subprocess
import threading
import time

import staging_updater_smoke as shared
from native_release_smoke import event_count, fingerprint


def require_nonmac_ci():
    assert os.sys.platform in ("win32", "linux"), "Windows or Linux runner required"
    shared.require_ci(os.sys.platform)


def process_ids(binary):
    if os.name == "nt":
        # Read only the exact QA install; no unrelated processes or command lines.
        quoted = str(binary).replace("'", "''")
        command = ("@(Get-CimInstance Win32_Process -Filter \"Name='Amberize.exe'\" | "
                   f"Where-Object {{$_.ExecutablePath -eq '{quoted}'}} | "
                   "Select-Object -ExpandProperty ProcessId) | ConvertTo-Json -Compress")
        raw = subprocess.check_output(["powershell", "-NoProfile", "-Command", command], text=True).strip()
        value = json.loads(raw) if raw else []
        return value if isinstance(value, list) else [value]
    output = subprocess.check_output(["ps", "-ax", "-o", "pid=,args="], text=True)
    return [int(row.split(maxsplit=1)[0]) for row in output.splitlines()
            if len(row.split(maxsplit=1)) == 2 and
            row.split(maxsplit=1)[1].startswith(str(binary) + " ")]


def stop(binary, launched=None):
    if os.name != "nt" and launched is not None:
        try:
            os.killpg(launched.pid, 15)
        except ProcessLookupError:
            pass
    if launched is not None and launched.poll() is None:
        launched.terminate()
        try:
            launched.wait(timeout=10)
        except subprocess.TimeoutExpired:
            launched.kill()
            launched.wait(timeout=10)
    for pid in process_ids(binary):
        os.kill(pid, 15)


def run(args):
    require_nonmac_ci()
    root = args.root.resolve()
    assert root.is_relative_to(Path(os.environ["RUNNER_TEMP"]).resolve()) and not root.exists()
    root.mkdir()
    profile = (Path(os.environ["APPDATA"]) / "com.amberize.app" if os.name == "nt"
               else root / "config/com.amberize.app")
    assert not profile.exists(), "Refusing an existing application profile"
    profile.mkdir(parents=True)
    config_path = profile / "config.json"
    fixture = root / "archive.sqlite3"
    with (root / "before.json").open("w") as output:
        subprocess.run([str(args.old_fixture_tool), "create", str(fixture), "1024"], check=True, stdout=output)
    config = {"db_path": str(fixture), "sync_interval_secs": 3600}
    config_path.write_text(json.dumps(config))
    baseline = fingerprint(fixture)
    accounts = shared.account_state(fixture)
    assert shared.schema_version(fixture) == 2
    platform = "windows-x86_64" if os.name == "nt" else "linux-x86_64"
    manifest = json.loads(args.manifest.read_text())
    assert manifest["version"] == shared.VERSION
    entry = manifest["platforms"][platform]
    assert entry["url"].endswith(f"/v{shared.VERSION}/{args.candidate.name}")
    assert shared.sha256(args.candidate) == args.candidate_sha256
    server = shared.StagingServer(args.candidate, entry)
    server.manifest["platforms"] = {platform: next(iter(server.manifest["platforms"].values()))}
    threading.Thread(target=server.serve_forever, daemon=True).start()
    binary = args.old_binary.resolve(strict=True)
    old_hash = shared.sha256(binary)
    environment = dict(os.environ)
    if os.name != "nt":
        environment.update(XDG_CONFIG_HOME=str(profile.parent), APPIMAGE_EXTRACT_AND_RUN="1")
    report = {"passed": False, "platform": platform, "candidate_commit": shared.CANDIDATE_COMMIT,
              "candidate_payload_sha256": shared.sha256(args.candidate), "old_tag": "v0.2.3",
              "old_binary_sha256": old_hash, "stages": [],
              "differences": ["Old source rebuilt with a loopback staging endpoint and native UI observer",
                              "Old package lock identity corrected; external dependency versions preserved"],
              "limitations": ["Fresh hosted CI, no OS reboot or SmartScreen/browser-quarantine test",
                              "Linux secure-storage upgrade is not tested; Windows uses a synthetic fixture credential"]}
    process = None
    seeded = []
    try:
        if os.name == "nt":
            assert args.credential_tool is not None
            for _, reference in accounts:
                assert reference.startswith("qa-old-fixture/")
                subprocess.run([str(args.credential_tool), "write", reference], check=True)
                seeded.append(reference)
                subprocess.run([str(args.credential_tool), "read", reference], check=True)
            report["windows_cross_process_secure_storage_passed"] = True
            report["credential_scope"] = "Synthetic credential seeded by the repaired library; v0.2.3 Windows used a nonpersistent mock store"
        for mode in ("bad_signature", "interrupted", "success"):
            server.mode = mode
            phase = root / mode
            phase.mkdir()
            if mode == "success":
                with closing(sqlite3.connect(f"file:{fixture}?mode=ro", uri=True)) as source:
                    with closing(sqlite3.connect(root / "rollback.sqlite3")) as destination:
                        source.backup(destination)
            with (phase / "app.log").open("wb") as output:
                process = subprocess.Popen([str(binary)], env=dict(environment, AMBERIZE_UPDATER_QA_DIR=str(phase)),
                                           stdout=output, stderr=subprocess.STDOUT, start_new_session=(os.name != "nt"))
            deadline = time.monotonic() + 240
            observations = []
            while time.monotonic() < deadline:
                path = phase / "observations.json"
                if path.exists():
                    try:
                        observations = json.loads(path.read_text())
                    except json.JSONDecodeError:
                        time.sleep(.2)
                        continue
                assert "automation_error" not in observations, "Native UI observer failed"
                if mode != "success" and "install_error" in observations:
                    assert shared.sha256(binary) == old_hash, "Rejected update replaced the old binary"
                    break
                if mode == "success":
                    assert "install_error" not in observations, "Valid signed updater installation failed"
                    baseline_path = phase / "restart-baseline.json"
                    installed_hash = None
                    if baseline_path.exists():
                        try:
                            installed_hash = shared.sha256(binary)
                        except (FileNotFoundError, PermissionError):
                            # MSI can temporarily lock/remove its executable during replacement.
                            # Keep the exact-hash requirement and the existing bounded deadline.
                            pass
                    if installed_hash == args.candidate_binary_sha256:
                        before = json.loads(baseline_path.read_text())
                        if (process.poll() is not None and event_count(fixture, "app_started") > before["app_started"]
                                and event_count(fixture, "integrity_check") > before["integrity_check"]):
                            if os.name == "nt":
                                assert process_ids(binary), "Updated installed process is absent"
                                credential = shared.new_process_keychain_observation(fixture, before["max_event_id"])
                                if credential is None:
                                    time.sleep(.2)
                                    continue
                                report["credential_upgrade"] = credential
                            report["restarted_binary_sha256"] = installed_hash
                            report["new_process_restored_and_verified_archive"] = True
                            break
                time.sleep(.2)
            else:
                raise RuntimeError(f"{mode}: native updater did not reach the expected outcome")
            stop(binary, process)
            assert fingerprint(fixture) == baseline
            assert shared.account_state(fixture) == accounts
            assert json.loads(config_path.read_text()) == config
            assert any(r["mode"] == mode and r["path"] == "/candidate.tar.gz" for r in server.requests)
            report["stages"].append({"mode": mode, "passed": True, "ui_observations": observations})
        with (root / "after.json").open("w") as output:
            subprocess.run([str(args.fixture_tool), "verify", str(fixture)], check=True, stdout=output)
        assert json.loads((root / "after.json").read_text())["integrity"]["ok"]
        assert shared.schema_version(fixture) == 3
        with (root / "rollback-verification.json").open("w") as output:
            subprocess.run([str(args.old_fixture_tool), "verify", str(root / "rollback.sqlite3")], check=True, stdout=output)
        report.update(passed=True, mime_hashes_preserved=len(baseline["hashes"]), schema_migration="2 to 3",
                      account_references_preserved=True, saved_configuration_preserved=True,
                      older_archive_backup_verified_by_old_library=True)
    except BaseException as error:
        report["error"] = str(error)
        raise
    finally:
        stop(binary, process)
        server.shutdown()
        server.server_close()
        for reference in seeded:
            subprocess.run([str(args.credential_tool), "delete", reference], check=True)
        report["requests"] = server.requests
        (root / "staging-update.json").write_text(json.dumps(report, indent=2) + "\n")
    print(json.dumps(report, indent=2))


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    sub = parser.add_subparsers(dest="command", required=True)
    prepare = sub.add_parser("prepare")
    prepare.add_argument("source", type=Path)
    test = sub.add_parser("run")
    for flag in ("root", "old-binary", "candidate", "manifest", "fixture-tool", "old-fixture-tool"):
        test.add_argument("--" + flag, type=Path, required=True)
    test.add_argument("--credential-tool", type=Path)
    test.add_argument("--candidate-sha256", required=True)
    test.add_argument("--candidate-binary-sha256", required=True)
    args = parser.parse_args()
    if args.command == "prepare":
        require_nonmac_ci()
        shared.prepare(args.source.resolve(strict=True), platform=os.sys.platform, probe_name="nonmac_updater_probe.rs")
    else:
        run(args)
