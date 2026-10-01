"""Verify isolated background startup through macOS launchd, without logout or reboot."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import plistlib
import re
import subprocess
import time
import uuid

from native_release_smoke import event_count, fingerprint


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--app", type=Path, required=True)
    parser.add_argument("--fixture-tool", type=Path, required=True)
    parser.add_argument("--root", type=Path, required=True)
    parser.add_argument("--binary-sha256", required=True)
    args = parser.parse_args()
    assert os.sys.platform == "darwin"
    app = args.app.resolve(strict=True)
    info = plistlib.loads((app / "Contents/Info.plist").read_bytes())
    assert info["CFBundleIdentifier"] == "com.amberize.qa", "Only the isolated QA identity is allowed"
    binary = app / "Contents/MacOS/Amberize"
    assert hashlib.sha256(binary.read_bytes()).hexdigest() == args.binary_sha256
    root = args.root.resolve()
    assert root.is_relative_to(Path("/private/tmp")) and not root.exists()
    profile = Path.home() / "Library/Application Support/com.amberize.qa"
    files = [profile / "config.json", profile / "config.json.previous"]
    original = {path: path.read_bytes() if path.exists() else None for path in files}
    for raw in original.values():
        if raw is None or raw == b"{":
            continue
        saved = json.loads(raw)
        assert str(saved.get("db_path", "")).startswith("/private/tmp/"), "QA profile contains a non-test archive"
    running = subprocess.check_output(["ps", "-ax", "-o", "command="], text=True)
    assert not any(line.startswith(str(binary)) for line in running.splitlines()), "QA app is already running"
    root.mkdir()
    profile.mkdir(parents=True, exist_ok=True)
    fixture = root / "archive.sqlite3"
    with (root / "before.json").open("w") as output:
        subprocess.run([str(args.fixture_tool), "create", str(fixture), "1024"], check=True, stdout=output)
    baseline = fingerprint(fixture)
    config = {"db_path": str(fixture), "sync_interval_secs": 3600}
    label = "com.amberize.qa.login-smoke." + uuid.uuid4().hex
    domain = f"gui/{os.getuid()}"
    service = domain + "/" + label
    agent = root / "launch-agent.plist"
    agent.write_bytes(plistlib.dumps({"Label": label, "ProgramArguments": [str(binary), "--background"],
                                     "RunAtLoad": True, "StandardOutPath": str(root / "app.log"),
                                     "StandardErrorPath": str(root / "app.log")}))
    report = {"passed": False, "binary_sha256": args.binary_sha256, "identity": info["CFBundleIdentifier"],
              "launchd_service": service, "stages": [], "actual_login_or_reboot": False,
              "limitations": ["Source-equivalent unsigned QA build under Rosetta",
                              "Loads and unloads an isolated user LaunchAgent; no actual logout or reboot",
                              "Synthetic archive without real provider credentials"]}
    loaded = False
    try:
        files[0].write_text(json.dumps(config))
        for cycle in range(2):
            started = event_count(fixture, "app_started")
            verified = event_count(fixture, "integrity_check")
            subprocess.run(["launchctl", "bootstrap", domain, str(agent)], check=True)
            loaded = True
            deadline = time.monotonic() + 45
            pid = None
            while time.monotonic() < deadline:
                output = subprocess.check_output(["launchctl", "print", service], text=True)
                match = re.search(r"\bpid = (\d+)", output)
                if match:
                    pid = int(match[1])
                if pid and event_count(fixture, "app_started") > started and event_count(fixture, "integrity_check") > verified:
                    break
                time.sleep(.2)
            else:
                raise RuntimeError("launchd did not restore and verify the saved archive")
            assert fingerprint(fixture) == baseline
            assert json.loads(files[0].read_text()) == config
            report["stages"].append({"cycle": cycle + 1, "pid": pid, "archive_restored": True,
                                     "integrity_checked": True, "background_argument": True})
            subprocess.run(["launchctl", "bootout", service], check=True)
            loaded = False
            deadline = time.monotonic() + 10
            while time.monotonic() < deadline:
                try:
                    os.kill(pid, 0)
                except ProcessLookupError:
                    break
                time.sleep(.2)
            else:
                raise RuntimeError("QA process remained after launchd unload")
        assert report["stages"][0]["pid"] != report["stages"][1]["pid"]
        with (root / "after.json").open("w") as output:
            subprocess.run([str(args.fixture_tool), "verify", str(fixture)], check=True, stdout=output)
        assert json.loads((root / "after.json").read_text())["integrity"]["ok"]
        report["mime_hashes_preserved"] = len(baseline["hashes"])
        report["passed"] = True
    except BaseException as error:
        report["error"] = str(error)
        raise
    finally:
        if loaded:
            subprocess.run(["launchctl", "bootout", service], check=False)
        for path, raw in original.items():
            if raw is None:
                path.unlink(missing_ok=True)
            else:
                path.write_bytes(raw)
        report["qa_profile_restored_byte_for_byte"] = all(
            (path.read_bytes() if path.exists() else None) == raw for path, raw in original.items())
        report["launch_agent_unloaded"] = not loaded or subprocess.run(
            ["launchctl", "print", service], stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL).returncode != 0
        (root / "launchd-startup.json").write_text(json.dumps(report, indent=2) + "\n")
    print(json.dumps(report, indent=2))


if __name__ == "__main__":
    main()
