#!/usr/bin/env python3
"""Post-release verification helper for desktop updater health."""

from __future__ import annotations

import json
import subprocess
import sys
import urllib.error
import urllib.request
from pathlib import Path


ROOT_DIR = Path(__file__).resolve().parents[1]
DESKTOP_DIR = ROOT_DIR / "apps" / "desktop"
TAURI_CONF_PATH = DESKTOP_DIR / "src-tauri" / "tauri.conf.json"


def run_command(command: list[str], cwd: Path | None = None) -> str:
    result = subprocess.run(
        command,
        cwd=str(cwd) if cwd else None,
        text=True,
        capture_output=True,
        check=False,
    )
    if result.returncode != 0:
        message = (result.stderr or result.stdout or "unknown command error").strip()
        raise RuntimeError(f"{' '.join(command)} failed: {message}")
    return (result.stdout or "").strip()


def fetch_json(url: str) -> dict:
    request = urllib.request.Request(
        url,
        headers={
            "User-Agent": "amberize-release-verify/1.0",
            "Accept": "application/json",
        },
    )
    try:
        with urllib.request.urlopen(request, timeout=20) as response:
            payload = response.read().decode("utf-8")
            return json.loads(payload)
    except urllib.error.HTTPError as error:
        raise RuntimeError(f"HTTP {error.code} for {url}") from error
    except urllib.error.URLError as error:
        raise RuntimeError(f"Could not reach {url}: {error.reason}") from error


def print_check(title: str, ok: bool, detail: str) -> None:
    marker = "PASS" if ok else "FAIL"
    print(f"[{marker}] {title}: {detail}")


def get_configured_endpoint() -> str:
    with TAURI_CONF_PATH.open("r", encoding="utf-8") as file_handle:
        config = json.load(file_handle)
    endpoints = config.get("plugins", {}).get("updater", {}).get("endpoints", [])
    if not endpoints:
        raise RuntimeError("No updater endpoint configured in tauri.conf.json")
    return str(endpoints[0])


def verify_repo_visibility() -> tuple[bool, str]:
    try:
        output = run_command(
            [
                "gh",
                "repo",
                "view",
                "johannesmutter/amberize",
                "--json",
                "visibility,isPrivate",
            ],
            cwd=ROOT_DIR,
        )
    except Exception as error:  # pylint: disable=broad-exception-caught
        return False, f"Could not inspect repo visibility ({error})"
    payload = json.loads(output)
    visibility = payload.get("visibility", "unknown")
    is_private = bool(payload.get("isPrivate", False))
    if is_private:
        return False, f"repository is private ({visibility})"
    return True, f"repository visibility is {visibility.lower()}"


def verify_endpoint_fetch(endpoint_url: str) -> tuple[bool, str, dict | None]:
    try:
        payload = fetch_json(endpoint_url)
    except Exception as error:  # pylint: disable=broad-exception-caught
        return False, str(error), None
    version_value = str(payload.get("version", "missing"))
    platforms_value = payload.get("platforms")
    if not isinstance(platforms_value, dict) or len(platforms_value) == 0:
        return False, "JSON has no platforms section", payload
    return True, f"version={version_value}, platforms={len(platforms_value)}", payload


def verify_release_asset_health() -> tuple[bool, str]:
    try:
        output = run_command(
            [
                "gh",
                "api",
                "repos/johannesmutter/amberize/releases/latest",
                "--jq",
                ".assets[].name",
            ],
            cwd=ROOT_DIR,
        )
    except Exception as error:  # pylint: disable=broad-exception-caught
        return False, f"Could not read latest release assets ({error})"

    asset_names = [line.strip() for line in output.splitlines() if line.strip()]
    if "latest.json" not in asset_names:
        return False, "latest release does not contain latest.json asset"

    has_macos_updater_artifact = any(name.endswith(".app.tar.gz") for name in asset_names)
    has_macos_updater_signature = any(name.endswith(".app.tar.gz.sig") for name in asset_names)
    if not has_macos_updater_artifact or not has_macos_updater_signature:
        return False, "missing macOS updater tarball/signature assets"

    return True, f"{len(asset_names)} assets present including updater artifacts"


def main() -> int:
    try:
        endpoint_url = get_configured_endpoint()

        print("Release verification")
        print(f"Configured updater endpoint: {endpoint_url}")
        print("")

        repo_ok, repo_detail = verify_repo_visibility()
        print_check("Repository visibility", repo_ok, repo_detail)

        assets_ok, assets_detail = verify_release_asset_health()
        print_check("Latest release assets", assets_ok, assets_detail)

        endpoint_ok, endpoint_detail, endpoint_payload = verify_endpoint_fetch(endpoint_url)
        print_check("Updater endpoint fetch", endpoint_ok, endpoint_detail)

        version_match_ok = False
        if endpoint_payload:
            remote_version = str(endpoint_payload.get("version", "missing"))
            try:
                local_release_version = run_command(
                    [
                        "python3",
                        "-c",
                        (
                            "import json, pathlib; "
                            "p=pathlib.Path('apps/desktop/src-tauri/tauri.conf.json'); "
                            "print(json.loads(p.read_text())['version'])"
                        ),
                    ],
                    cwd=ROOT_DIR,
                )
                version_match_ok = remote_version == local_release_version
                detail = f"endpoint={remote_version}, local_config={local_release_version}"
            except Exception as error:  # pylint: disable=broad-exception-caught
                detail = f"could not compare versions ({error})"
            print_check("Endpoint version alignment", version_match_ok, detail)

        all_ok = repo_ok and assets_ok and endpoint_ok and version_match_ok
        print("")
        if all_ok:
            print("All updater checks passed.")
            return 0
        print("Updater checks failed. Review failed checks before shipping updates.")
        return 1
    except Exception as error:  # pylint: disable=broad-exception-caught
        print(f"Error: {error}", file=sys.stderr)
        return 1


if __name__ == "__main__":
    sys.exit(main())
