"""Check existing Apple notarization credentials without submitting an application.

Reports contain formatting flags and status codes only. Never print command
arguments, credential values, submission history, or raw tool errors.
"""
import argparse
import json
import os
from pathlib import Path
import re
import subprocess
import sys


FIELDS = ("APPLE_ID", "APPLE_PASSWORD", "APPLE_TEAM_ID")


def probe(credentials):
    try:
        result = subprocess.run(
            ["xcrun", "notarytool", "history", "--apple-id", credentials["APPLE_ID"],
             "--password", credentials["APPLE_PASSWORD"], "--team-id", credentials["APPLE_TEAM_ID"],
             "--output-format", "json", "--no-progress"],
            capture_output=True, text=True, timeout=60,
        )
        status = re.search(r"HTTP status code:\s*(\d{3})", result.stderr + result.stdout)
        return {"accepted": result.returncode == 0, "exit_code": result.returncode,
                "http_status": int(status.group(1)) if status else None}
    except subprocess.TimeoutExpired:
        return {"accepted": False, "error": "Apple authentication request timed out"}
    except OSError:
        return {"accepted": False, "error": "Apple authentication tool could not run"}
    except Exception:
        # Exception text may include the command and its credential arguments.
        return {"accepted": False, "error": "Apple authentication probe failed unexpectedly"}


def diagnose(environment):
    original = {name: environment.get(name, "") for name in FIELDS}
    normalized = {name: value.strip() for name, value in original.items()}
    report = {
        "operation": "notarytool history authentication only",
        "secret_values_logged": False,
        "missing": [name for name in FIELDS if not normalized[name]],
        "surrounding_whitespace": {name: original[name] != normalized[name] for name in FIELDS},
        "app_specific_password_format": bool(re.fullmatch(r"[a-z]{4}(?:-[a-z]{4}){3}", normalized["APPLE_PASSWORD"])),
        "team_id_format": bool(re.fullmatch(r"[A-Z0-9]{10}", normalized["APPLE_TEAM_ID"])),
    }
    if not report["missing"]:
        report["original"] = probe(original)
        if any(report["surrounding_whitespace"].values()):
            report["normalized"] = probe(normalized)
    return report


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--report", type=Path, required=True)
    args = parser.parse_args()
    report = diagnose(os.environ)
    args.report.parent.mkdir(parents=True, exist_ok=True)
    args.report.write_text(json.dumps(report, indent=2) + "\n")
    print(json.dumps(report, indent=2))
    if report.get("original", {}).get("accepted"):
        return 0
    if report.get("normalized", {}).get("accepted"):
        print("Remove surrounding whitespace from the Apple credential secrets before building.", file=sys.stderr)
    else:
        print("Apple notarization authentication failed. Verify APPLE_ID and APPLE_TEAM_ID and replace APPLE_PASSWORD with a valid app-specific password. Enter secrets directly in GitHub, never in logs or chat.", file=sys.stderr)
    return 1


if __name__ == "__main__":
    sys.exit(main())
