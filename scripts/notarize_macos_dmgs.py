"""Notarize, staple, and assess signed DMG containers without logging credentials."""
import argparse
import json
import os
from pathlib import Path
import re
import subprocess
import sys


FIELDS = ("APPLE_ID", "APPLE_PASSWORD", "APPLE_TEAM_ID")


def notarize(dmg, environment):
    report = {"file": dmg.name, "passed": False, "secret_values_logged": False, "checks": []}
    if any(not environment.get(name) for name in FIELDS):
        report["error"] = "missing_credentials"
        return report
    step = "verify_input_signature"
    try:
        def run(command, timeout=90):
            result = subprocess.run(command, capture_output=True, text=True, timeout=timeout)
            report["checks"].append({"step": step, "exit_code": result.returncode})
            if result.returncode:
                raise RuntimeError("tool_failed")
            return result.stdout

        run(["codesign", "--verify", "--strict", str(dmg)])
        step = "notarization"
        output = run(["xcrun", "notarytool", "submit", str(dmg), "--apple-id", environment["APPLE_ID"],
                      "--password", environment["APPLE_PASSWORD"], "--team-id", environment["APPLE_TEAM_ID"],
                      "--wait", "--timeout", "20m", "--output-format", "json", "--no-progress"], timeout=1260)
        response = json.loads(output)
        status = response.get("status")
        report["notarization_status"] = status if status in ("Accepted", "Invalid", "Rejected", "In Progress") else "unknown"
        receipt = response.get("id", "")
        if isinstance(receipt, str) and re.fullmatch(r"[0-9a-fA-F]{8}(?:-[0-9a-fA-F]{4}){3}-[0-9a-fA-F]{12}", receipt):
            report["submission_id"] = receipt
        if status != "Accepted":
            report["failed_step"] = step
            return report
        step = "staple"
        run(["xcrun", "stapler", "staple", str(dmg)])
        step = "verify_stapled_signature"
        run(["codesign", "--verify", "--strict", str(dmg)])
        step = "validate_staple"
        run(["xcrun", "stapler", "validate", str(dmg)])
        step = "gatekeeper_container"
        run(["spctl", "--assess", "--type", "open", "--context", "context:primary-signature", "--verbose=2", str(dmg)])
        report["passed"] = True
    except Exception:
        # Tool output and exception text may contain credential arguments.
        report["failed_step"] = step
        report["error"] = "tool_or_response_failure"
    return report


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--directory", type=Path, required=True)
    parser.add_argument("--report-dir", type=Path, required=True)
    args = parser.parse_args()
    images = sorted(args.directory.glob("*.dmg"))
    if not images:
        print("No macOS disk images were found.", file=sys.stderr)
        return 1
    args.report_dir.mkdir(parents=True, exist_ok=True)
    for dmg in images:
        report = notarize(dmg, os.environ)
        (args.report_dir / (dmg.name + ".notarization.json")).write_text(json.dumps(report, indent=2) + "\n")
        print(json.dumps(report, indent=2))
        if not report["passed"]:
            print("DMG notarization or distribution assessment failed; keep the release unpublished.", file=sys.stderr)
            return 1
    return 0


if __name__ == "__main__":
    sys.exit(main())
