"""Reject AppDir entries that cannot be read or executed by unrelated users."""
import argparse
import json
from pathlib import Path
import re
import stat
import subprocess


def verify(appdir):
    appdir = appdir.resolve(strict=True)
    failures = []
    entries = [appdir, *appdir.rglob('*')]
    for path in entries:
        mode = path.lstat().st_mode
        relative = str(path.relative_to(appdir))
        if stat.S_ISLNK(mode):
            resolved = path.resolve(strict=True)
            if not resolved.is_relative_to(appdir):
                failures.append(f'{relative}: symlink leaves AppDir')
            continue
        required = stat.S_IROTH
        if stat.S_ISDIR(mode) or mode & 0o111:
            required |= stat.S_IXOTH
        if mode & required != required:
            failures.append(f'{relative}: {stat.S_IMODE(mode):04o} denies unrelated users')
    for name in ('AppRun', 'AppRun.wrapped'):
        path = appdir / name
        if not path.is_file() or not path.stat().st_mode & stat.S_IXOTH:
            failures.append(f'{name}: launcher missing or not executable for everyone')
    if failures:
        raise ValueError('; '.join(failures))
    return {'passed': True, 'entries_checked': len(entries), 'unrelated_user_permissions': True,
            'launchers': {name: f'{stat.S_IMODE((appdir/name).stat().st_mode):04o}'
                          for name in ('AppRun', 'AppRun.wrapped')}}


def verify_glibc(appdir, maximum):
    versions = set()
    for path in appdir.rglob('*'):
        if path.is_symlink() or not path.is_file():
            continue
        with path.open('rb') as source:
            if source.read(4) != b'\x7fELF':
                continue
        output = subprocess.check_output(['readelf', '--version-info', str(path)], text=True)
        versions.update(tuple(map(int, version.split('.')))
                        for version in re.findall(r'\bGLIBC_(\d+(?:\.\d+)+)\b', output))
    actual = max(versions, default=(0,))
    if actual > tuple(map(int, maximum.split('.'))):
        raise ValueError(f'AppImage requires glibc {actual}, exceeding {maximum}')
    return '.'.join(map(str, actual))


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('appdir', type=Path)
    parser.add_argument('--max-glibc')
    args = parser.parse_args()
    result = verify(args.appdir)
    if args.max_glibc:
        result['maximum_glibc_reference'] = verify_glibc(args.appdir, args.max_glibc)
    print(json.dumps(result))
