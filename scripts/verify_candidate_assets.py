"""Verify downloaded draft assets, updater metadata and all signatures without private keys."""
import argparse
import base64
import hashlib
import json
from pathlib import Path
import subprocess
import tempfile
from urllib.parse import unquote, urlparse


def signature_text(value):
    value = value.strip()
    if value.startswith("untrusted comment:"):
        return value
    return base64.b64decode(value, validate=True).decode().strip()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--root", type=Path, required=True)
    parser.add_argument("--metadata", type=Path, required=True)
    parser.add_argument("--tag", required=True)
    parser.add_argument("--commit", required=True)
    parser.add_argument("--public-key", type=Path, required=True)
    parser.add_argument("--verifier", type=Path, required=True)
    parser.add_argument("--report", type=Path, required=True)
    args = parser.parse_args()
    root = args.root.resolve(strict=True)
    metadata = json.loads(args.metadata.read_text())
    assert metadata["isDraft"] and metadata["isPrerelease"], "Candidate must remain unpublished"
    assert metadata["tagName"] == args.tag and metadata["targetCommitish"] == args.commit
    inventory = {}
    for item in metadata["assets"]:
        name = item["name"]
        assert Path(name).name == name and name not in (".", ".."), "Unsafe asset name"
        path = root / name
        assert path.is_file() and not path.is_symlink() and path.resolve().parent == root
        assert path.stat().st_size == item["size"], f"Truncated asset: {name}"
        with path.open("rb") as source:
            digest = hashlib.file_digest(source, "sha256").hexdigest()
        if item.get("digest"):
            assert item["digest"] == "sha256:" + digest, f"Download digest mismatch: {name}"
        assert name not in inventory, "Duplicate asset name"
        inventory[name] = {"size": item["size"], "sha256": digest}
    manifest = json.loads((root / "latest.json").read_text())
    assert manifest["version"] == args.tag.removeprefix("v")
    expected_platforms = {"darwin-aarch64", "darwin-aarch64-app", "darwin-x86_64",
                          "darwin-x86_64-app", "linux-x86_64", "linux-x86_64-appimage",
                          "linux-x86_64-deb", "windows-x86_64", "windows-x86_64-msi"}
    assert set(manifest["platforms"]) == expected_platforms
    verified = []
    platforms = {}
    with tempfile.TemporaryDirectory(prefix="amberize-signature-check-") as directory:
        temporary = Path(directory)
        for name in sorted(inventory):
            if not name.endswith(".sig"):
                continue
            asset = root / name.removesuffix(".sig")
            assert asset.name in inventory
            decoded = temporary / name
            decoded.write_text(signature_text((root / name).read_text()) + "\n")
            subprocess.run([str(args.verifier), str(args.public_key), str(decoded), str(asset)],
                           check=True, capture_output=True)
            verified.append(asset.name)
        assert len(verified) == 7, "Expected every canonical and compatibility updater payload"
        for platform, entry in manifest["platforms"].items():
            url = urlparse(entry["url"])
            assert url.scheme == "https" and url.netloc == "github.com"
            expected_prefix = "/johannesmutter/amberize/releases/download/" + args.tag + "/"
            assert url.path.startswith(expected_prefix) and not url.query and not url.fragment
            name = unquote(url.path.removeprefix(expected_prefix))
            assert name in verified, f"Metadata points to an unverified payload: {platform}"
            assert signature_text(entry["signature"]) == signature_text((root / (name + ".sig")).read_text())
            platforms[platform] = name
        # Mutate the smallest payload in a temporary copy; originals remain intact.
        name = min(verified, key=lambda value: inventory[value]["size"])
        altered = temporary / "altered-payload"
        data = bytearray((root / name).read_bytes())
        data[len(data) // 2] ^= 1
        altered.write_bytes(data)
        rejected = subprocess.run([str(args.verifier), str(args.public_key), str(temporary / (name + ".sig")), str(altered)],
                                  capture_output=True)
        assert rejected.returncode != 0, "Changed updater payload was accepted"
    report = {"passed": True, "tag": args.tag, "candidate_commit": args.commit,
              "unpublished_draft": True, "assets": inventory, "verified_signatures": verified,
              "verified_platforms": platforms, "tampered_payload_rejected": True}
    args.report.write_text(json.dumps(report, indent=2) + "\n")
    print(json.dumps({"passed": True, "assets": len(inventory), "signatures": len(verified),
                      "platforms": len(platforms), "tampered_payload_rejected": True}))


if __name__ == "__main__":
    main()
