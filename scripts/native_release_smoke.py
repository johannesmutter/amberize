"""Backend restart/recovery smoke test for an installed release in a fresh QA profile."""
import argparse
from contextlib import closing
import hashlib
import json
import os
from pathlib import Path
import signal
import sqlite3
import subprocess
import time
from uuid import UUID


def fingerprint(path):
    with closing(sqlite3.connect(path)) as connection:
        rows = connection.execute("SELECT sha256,raw_mime FROM message_blobs ORDER BY sha256")
        hashes = []
        for expected, raw in rows:
            actual = hashlib.sha256(raw).hexdigest()
            assert actual == expected, "MIME hash changed"
            hashes.append(actual)
        counts = {table: connection.execute(f"SELECT COUNT(*) FROM {table}").fetchone()[0]
                  for table in ("accounts", "mailboxes", "message_blobs", "message_locations")}
    return {"hashes": hashes, "counts": counts}


def event_count(path, kind):
    with closing(sqlite3.connect(path)) as connection:
        return connection.execute("SELECT COUNT(*) FROM events WHERE kind=?", (kind,)).fetchone()[0]


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--binary", required=True)
    parser.add_argument("--fixture", type=Path, required=True)
    parser.add_argument("--config-dir", type=Path, required=True)
    parser.add_argument("--report", type=Path, required=True)
    parser.add_argument("--trace", type=Path, help="Linux-only syscall diagnostics in hosted QA")
    parser.add_argument("--windows-vm-recovery-snapshot", type=UUID,
                        help="Explicitly permit a fresh profile in a snapshotted local Windows QA VM")
    args = parser.parse_args()
    if args.trace:
        assert os.sys.platform.startswith('linux') and os.environ.get('GITHUB_ACTIONS') == 'true'
    assert args.config_dir.name in ("com.amberize.app", "com.amberize.qa")
    if args.config_dir.name == "com.amberize.app":
        assert (os.environ.get("GITHUB_ACTIONS") == "true" or
                (os.name == "nt" and args.windows_vm_recovery_snapshot is not None)), (
            "Production identity requires fresh hosted CI or an explicitly snapshotted Windows QA VM"
        )
    assert not args.config_dir.exists(), "Refusing to change an existing application profile"
    fixture = args.fixture.resolve(strict=True)
    baseline = fingerprint(fixture)
    assert baseline["counts"]["message_blobs"] >= 1024, "Pagination fixture needs more than 500 messages"
    args.config_dir.mkdir(parents=True)
    config = args.config_dir / "config.json"
    saved = {"db_path": str(fixture), "sync_interval_secs": 3600}
    config.write_text(json.dumps(saved))
    stages = []
    environment = dict(os.environ)
    if os.name != "nt" and os.sys.platform != "darwin":
        environment["XDG_CONFIG_HOME"] = str(args.config_dir.parent)
    log = args.report.with_suffix(".log")

    def boot(label, missing=False, corrupt_settings=False):
        started = None if missing else event_count(fixture, "app_started")
        verified = None if missing else event_count(fixture, "integrity_check")
        with log.open("ab") as output:
            command = [args.binary, "--background"]
            if args.trace:
                command = ['strace', '-f', '-e', 'trace=openat,flock,fcntl', '-o', str(args.trace), *command]
            process = subprocess.Popen(command, env=environment,
                                       stdout=output, stderr=subprocess.STDOUT,
                                       start_new_session=os.name != 'nt')
            try:
                deadline = time.monotonic() + (5 if missing else 45)
                while time.monotonic() < deadline:
                    assert process.poll() is None, f"{label}: app exited ({process.returncode}); see {log}"
                    if missing:
                        assert not fixture.exists(), "Missing archive was replaced with an empty database"
                    elif (event_count(fixture, "app_started") > started and
                          event_count(fixture, "integrity_check") > verified):
                        break
                    time.sleep(0.2)
                else:
                    assert missing, f"{label}: backend did not restore and verify the saved archive"
                if corrupt_settings:
                    assert config.read_text() == "{", "Recovery unexpectedly changed the damaged settings"
                else:
                    assert json.loads(config.read_text()) == saved, "Saved archive path or interval changed"
                if not missing:
                    assert fingerprint(fixture) == baseline, "Archive contents changed"
                stages.append(label)
            finally:
                if os.name == 'nt':
                    process.terminate()
                else:
                    try:
                        os.killpg(process.pid, signal.SIGTERM)
                    except ProcessLookupError:
                        pass
                try:
                    process.wait(timeout=10)
                except subprocess.TimeoutExpired:
                    process.kill()
                    process.wait(timeout=10)
                if os.name != 'nt':
                    try:
                        os.killpg(process.pid, signal.SIGKILL)
                    except ProcessLookupError:
                        pass

    try:
        boot("installed release restored and verified saved archive without a window")
        boot("process restart preserved archive path, interval, counts, and MIME hashes")
        with closing(sqlite3.connect(fixture)) as connection:
            connection.execute("PRAGMA wal_checkpoint(TRUNCATE)")
        unavailable = fixture.with_suffix(".disconnected")
        fixture.rename(unavailable)
        try:
            boot("unavailable archive kept saved path and did not create a replacement", missing=True)
        finally:
            unavailable.rename(fixture)
        boot("reconnected archive restored and verified")
        config.with_suffix(".json.previous").write_text(json.dumps(saved))
        config.write_text("{")
        boot("damaged settings recovered the previous archive and interval", corrupt_settings=True)
        report = {"passed": True, "stages": stages, "counts": baseline["counts"],
                  "mime_hash_manifest_sha256": hashlib.sha256(json.dumps(baseline["hashes"]).encode()).hexdigest(),
                  "binary_sha256": hashlib.sha256(Path(args.binary).read_bytes()).hexdigest(),
                  "windows_vm_recovery_snapshot": str(args.windows_vm_recovery_snapshot) if args.windows_vm_recovery_snapshot else None,
                  "limitations": ["No login or reboot", "No provider credentials", "No window or recovery-copy assertions",
                                  "Processes stopped forcibly to exercise crash/restart persistence"]}
    except BaseException as error:
        report = {"passed": False, "stages": stages, "error": str(error)}
        raise
    finally:
        args.report.write_text(json.dumps(report, indent=2) + "\n")
    print(json.dumps(report, indent=2))


if __name__ == "__main__":
    main()
