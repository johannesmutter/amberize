#!/usr/bin/env python3
"""Interactive release wizard for the desktop app.

Automates:
- synchronized version bump across Tauri, Cargo and npm metadata
- optional commit / push / tag / tag push flow
- preflight safety checks to avoid mismatched release tags
"""

from __future__ import annotations

import json
import re
import subprocess
import sys
from dataclasses import dataclass
from pathlib import Path


VERSION_PATTERN = re.compile(r"^\d+\.\d+\.\d+$")
ROOT_DIR = Path(__file__).resolve().parents[1]
DESKTOP_DIR = ROOT_DIR / "apps" / "desktop"
TAURI_CONF_PATH = DESKTOP_DIR / "src-tauri" / "tauri.conf.json"
CARGO_TOML_PATH = DESKTOP_DIR / "src-tauri" / "Cargo.toml"
PACKAGE_JSON_PATH = DESKTOP_DIR / "package.json"


@dataclass
class VersionState:
    tauri_conf_version: str
    cargo_version: str
    package_version: str

    def is_synchronized(self) -> bool:
        return (
            self.tauri_conf_version == self.cargo_version
            and self.cargo_version == self.package_version
        )

    def canonical(self) -> str:
        return self.cargo_version


def run_command(command: list[str], cwd: Path | None = None, capture: bool = False) -> str:
    """Run a shell command and return output when requested."""
    result = subprocess.run(
        command,
        cwd=str(cwd) if cwd else None,
        text=True,
        capture_output=capture,
        check=False,
    )
    if result.returncode != 0:
        stderr_text = (result.stderr or "").strip()
        stdout_text = (result.stdout or "").strip()
        message = stderr_text or stdout_text or "unknown command error"
        raise RuntimeError(f"{' '.join(command)} failed: {message}")
    return (result.stdout or "").strip()


def prompt_yes_no(prompt_text: str, default_yes: bool = True) -> bool:
    """Ask a yes/no question and return the user's decision."""
    suffix = "[Y/n]" if default_yes else "[y/N]"
    while True:
        answer = input(f"{prompt_text} {suffix}: ").strip().lower()
        if answer == "":
            return default_yes
        if answer in {"y", "yes"}:
            return True
        if answer in {"n", "no"}:
            return False
        print("Please answer with 'y' or 'n'.")


def parse_semver(version: str) -> tuple[int, int, int]:
    """Convert semver text into integer tuple."""
    if not VERSION_PATTERN.match(version):
        raise ValueError(f"Invalid semantic version: {version}")
    major, minor, patch = version.split(".")
    return int(major), int(minor), int(patch)


def increment_version(version: str, mode: str) -> str:
    """Increment a semantic version in patch/minor/major mode."""
    major, minor, patch = parse_semver(version)
    if mode == "patch":
        return f"{major}.{minor}.{patch + 1}"
    if mode == "minor":
        return f"{major}.{minor + 1}.0"
    if mode == "major":
        return f"{major + 1}.0.0"
    raise ValueError(f"Unsupported increment mode: {mode}")


def read_json(path: Path) -> dict:
    with path.open("r", encoding="utf-8") as file_handle:
        return json.load(file_handle)


def write_json(path: Path, content: dict) -> None:
    with path.open("w", encoding="utf-8") as file_handle:
        json.dump(content, file_handle, indent=2)
        file_handle.write("\n")


def read_cargo_version(path: Path) -> str:
    """Read version under [package] from Cargo.toml."""
    in_package_section = False
    with path.open("r", encoding="utf-8") as file_handle:
        for raw_line in file_handle:
            stripped_line = raw_line.strip()
            if stripped_line.startswith("[") and stripped_line.endswith("]"):
                in_package_section = stripped_line == "[package]"
                continue
            if in_package_section and stripped_line.startswith("version"):
                match = re.match(r'version\s*=\s*"([^"]+)"', stripped_line)
                if match:
                    return match.group(1)
    raise RuntimeError(f"Could not find [package].version in {path}")


def write_cargo_version(path: Path, target_version: str) -> None:
    """Write version under [package] section in Cargo.toml."""
    lines = path.read_text(encoding="utf-8").splitlines()
    in_package_section = False
    replaced = False
    for index, line in enumerate(lines):
        stripped_line = line.strip()
        if stripped_line.startswith("[") and stripped_line.endswith("]"):
            in_package_section = stripped_line == "[package]"
            continue
        if in_package_section and re.match(r'^\s*version\s*=\s*"[^"]+"\s*$', line):
            lines[index] = re.sub(r'"[^"]+"', f'"{target_version}"', line, count=1)
            replaced = True
            break
    if not replaced:
        raise RuntimeError(f"Could not replace [package].version in {path}")
    path.write_text("\n".join(lines) + "\n", encoding="utf-8")


def read_version_state() -> VersionState:
    tauri_conf_data = read_json(TAURI_CONF_PATH)
    package_data = read_json(PACKAGE_JSON_PATH)
    return VersionState(
        tauri_conf_version=str(tauri_conf_data["version"]),
        cargo_version=read_cargo_version(CARGO_TOML_PATH),
        package_version=str(package_data["version"]),
    )


def ensure_required_tools() -> None:
    for command_name in ("git", "npm"):
        try:
            run_command([command_name, "--version"], capture=True)
        except RuntimeError as error:
            raise RuntimeError(f"Required command '{command_name}' is not available") from error


def update_versions(target_version: str) -> None:
    """Update version in all desktop release metadata files."""
    if not VERSION_PATTERN.match(target_version):
        raise ValueError(f"Invalid semantic version: {target_version}")

    tauri_conf_data = read_json(TAURI_CONF_PATH)
    tauri_conf_data["version"] = target_version
    write_json(TAURI_CONF_PATH, tauri_conf_data)

    write_cargo_version(CARGO_TOML_PATH, target_version)

    run_command(
        [
            "npm",
            "--prefix",
            str(DESKTOP_DIR),
            "version",
            target_version,
            "--no-git-tag-version",
            "--allow-same-version",
        ]
    )


def select_target_version(base_version: str) -> str:
    """Interactive version selection."""
    patch_version = increment_version(base_version, "patch")
    minor_version = increment_version(base_version, "minor")
    major_version = increment_version(base_version, "major")

    print("\nChoose release version:")
    print(f"  1) Patch  -> {patch_version}")
    print(f"  2) Minor  -> {minor_version}")
    print(f"  3) Major  -> {major_version}")
    print("  4) Custom")
    print(f"  5) Keep current ({base_version})")

    while True:
        choice = input("Selection [1-5]: ").strip()
        if choice == "1":
            return patch_version
        if choice == "2":
            return minor_version
        if choice == "3":
            return major_version
        if choice == "4":
            custom_version = input("Enter custom semantic version (X.Y.Z): ").strip()
            if VERSION_PATTERN.match(custom_version):
                return custom_version
            print("Invalid semantic version format.")
            continue
        if choice == "5":
            return base_version
        print("Please choose a number between 1 and 5.")


def print_version_state(state: VersionState) -> None:
    print("\nCurrent desktop version metadata:")
    print(f"  - tauri.conf.json: {state.tauri_conf_version}")
    print(f"  - src-tauri/Cargo.toml: {state.cargo_version}")
    print(f"  - package.json: {state.package_version}")
    print(f"  - synchronized: {'yes' if state.is_synchronized() else 'no'}")


def guard_clean_worktree() -> None:
    status_text = run_command(["git", "status", "--porcelain"], cwd=ROOT_DIR, capture=True)
    if status_text:
        print("\nDetected local changes in the worktree.")
        print("Release tagging is safer from a clean state.")
        if not prompt_yes_no("Continue anyway?", default_yes=False):
            raise RuntimeError("Aborted due to dirty worktree.")


def create_release_commit(target_version: str) -> None:
    commit_message = f"bump version to {target_version}"
    run_command(
        [
            "git",
            "add",
            str(TAURI_CONF_PATH.relative_to(ROOT_DIR)),
            str(CARGO_TOML_PATH.relative_to(ROOT_DIR)),
            str(PACKAGE_JSON_PATH.relative_to(ROOT_DIR)),
            "apps/desktop/package-lock.json",
        ],
        cwd=ROOT_DIR,
    )
    run_command(["git", "commit", "-m", commit_message], cwd=ROOT_DIR)


def push_branch() -> None:
    current_branch = run_command(
        ["git", "rev-parse", "--abbrev-ref", "HEAD"], cwd=ROOT_DIR, capture=True
    )
    run_command(["git", "push", "origin", current_branch], cwd=ROOT_DIR)


def create_and_push_tag(target_version: str) -> None:
    tag_name = f"v{target_version}"
    existing_tags = run_command(["git", "tag", "-l", tag_name], cwd=ROOT_DIR, capture=True)
    if existing_tags.strip():
        raise RuntimeError(f"Tag {tag_name} already exists locally.")
    run_command(["git", "tag", tag_name], cwd=ROOT_DIR)
    run_command(["git", "push", "origin", tag_name], cwd=ROOT_DIR)


def print_next_steps(target_version: str) -> None:
    print("\nRelease wizard completed.")
    print("Recommended next steps:")
    print(f"  1) Confirm Actions workflow ran for tag v{target_version}.")
    print("  2) Publish the draft GitHub release when all jobs pass.")
    print("  3) Validate updater JSON endpoint:")
    print(
        "     https://github.com/johannesmutter/amberize/releases/latest/download/latest.json"
    )
    print("  4) Run scripts/release_verify.py for post-release verification.")


def main() -> int:
    try:
        ensure_required_tools()
        guard_clean_worktree()

        current_state = read_version_state()
        print_version_state(current_state)

        base_version = current_state.canonical()
        if not current_state.is_synchronized():
            print("\nVersion files are not synchronized.")
            if not prompt_yes_no(
                "Use src-tauri/Cargo.toml as canonical base version and continue?",
                default_yes=True,
            ):
                print("No files changed.")
                return 0

        target_version = select_target_version(base_version)
        print(f"\nTarget version: {target_version}")
        if not prompt_yes_no("Apply version changes now?", default_yes=True):
            print("No files changed.")
            return 0

        update_versions(target_version)
        updated_state = read_version_state()
        print_version_state(updated_state)

        if not updated_state.is_synchronized() or updated_state.canonical() != target_version:
            raise RuntimeError("Version update failed consistency checks.")

        if prompt_yes_no("Create release commit now?", default_yes=True):
            create_release_commit(target_version)
            print("Release commit created.")

            if prompt_yes_no("Push branch to origin?", default_yes=True):
                push_branch()
                print("Branch pushed.")

            if prompt_yes_no(f"Create and push tag v{target_version}?", default_yes=True):
                create_and_push_tag(target_version)
                print(f"Tag v{target_version} pushed.")

        print_next_steps(target_version)
        return 0
    except KeyboardInterrupt:
        print("\nAborted by user.")
        return 130
    except Exception as error:  # pylint: disable=broad-exception-caught
        print(f"\nError: {error}", file=sys.stderr)
        return 1


if __name__ == "__main__":
    sys.exit(main())
