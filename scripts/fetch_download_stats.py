#!/usr/bin/env python3
"""Fetch total download counts and latest version from GitHub releases.

Writes a JSON file consumed by the landing page at build time so that
download buttons can link directly to the correct installer files.

Usage:
    python3 scripts/fetch_download_stats.py

Requires the GitHub CLI (`gh`) to be installed and authenticated.
"""

from __future__ import annotations

import json
import subprocess
import sys
from pathlib import Path

ROOT_DIR = Path(__file__).resolve().parents[1]
OUTPUT_PATH = ROOT_DIR / "apps" / "landing" / "src" / "lib" / "download-stats.json"
REPO = "johannesmutter/amberize"

# Only count installer files that real users download.
INSTALLER_EXTENSIONS = (".dmg", ".msi", ".deb", ".AppImage", ".exe")


def run_command(command: list[str]) -> str:
    """Run a shell command and return its stdout."""
    result = subprocess.run(command, text=True, capture_output=True, check=False)
    if result.returncode != 0:
        message = (result.stderr or result.stdout or "unknown error").strip()
        raise RuntimeError(f"{' '.join(command)} failed: {message}")
    return (result.stdout or "").strip()


def fetch_release_stats() -> tuple[int, str, list[dict]]:
    """Query GitHub API for all releases.

    Returns (total_downloads, latest_version, per_release_breakdown).
    """
    raw_json = run_command([
        "gh", "api",
        f"repos/{REPO}/releases",
        "--paginate",
        "--jq", ".",
    ])
    releases = json.loads(raw_json) if raw_json.startswith("[") else []

    total_downloads = 0
    latest_version = ""
    per_release: list[dict] = []

    for release in releases:
        tag = release.get("tag_name", "unknown")
        is_draft = release.get("draft", False)
        is_prerelease = release.get("prerelease", False)

        # Use the first non-draft, non-prerelease tag as latest version.
        if not latest_version and not is_draft and not is_prerelease:
            latest_version = tag.lstrip("v")

        release_total = 0
        for asset in release.get("assets", []):
            name = asset.get("name", "")
            count = int(asset.get("download_count", 0))
            if any(name.endswith(ext) for ext in INSTALLER_EXTENSIONS):
                release_total += count
        total_downloads += release_total
        per_release.append({"tag": tag, "downloads": release_total})

    return total_downloads, latest_version, per_release


def main() -> int:
    """Fetch stats, print summary, write JSON."""
    try:
        total, version, per_release = fetch_release_stats()

        stats = {
            "total_downloads": total,
            "latest_version": version,
            "releases": per_release,
        }

        OUTPUT_PATH.parent.mkdir(parents=True, exist_ok=True)
        OUTPUT_PATH.write_text(json.dumps(stats, indent=2) + "\n", encoding="utf-8")

        print(f"Latest version: {version}")
        print(f"Total installer downloads: {total}")
        for entry in per_release:
            print(f"  {entry['tag']}: {entry['downloads']}")
        print(f"\nWritten to {OUTPUT_PATH.relative_to(ROOT_DIR)}")
        return 0
    except Exception as error:  # pylint: disable=broad-exception-caught
        print(f"Error: {error}", file=sys.stderr)
        return 1


if __name__ == "__main__":
    sys.exit(main())
