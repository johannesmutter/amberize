"""Extract stored SquashFS permissions instead of the runtime's private directory modes."""
import argparse
from pathlib import Path
import subprocess


def extract(package, destination):
    package = package.resolve(strict=True)
    if destination.exists():
        raise ValueError('AppImage extraction requires a fresh destination')
    offset = int(subprocess.check_output([str(package), '--appimage-offset'], text=True).strip())
    if not 0 < offset < package.stat().st_size:
        raise ValueError('Invalid AppImage SquashFS offset')
    subprocess.run(['unsquashfs', '-quiet', '-no-xattrs', '-o', str(offset),
                    '-d', str(destination), str(package)], check=True)


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('package', type=Path)
    parser.add_argument('destination', type=Path)
    args = parser.parse_args()
    extract(args.package, args.destination)
