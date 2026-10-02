"""Verify external AppImage update metadata and its matching zsync sidecar read-only."""
import argparse
import hashlib
import json
from pathlib import Path
import struct
import subprocess
import tempfile


def read_update_information(package):
    size = package.stat().st_size
    with package.open('rb') as source:
        def read_at(offset, length):
            if offset < 0 or length < 0 or offset + length > size:
                raise ValueError('ELF section is outside the AppImage')
            source.seek(offset)
            data = source.read(length)
            if len(data) != length:
                raise ValueError('Truncated AppImage ELF data')
            return data

        header = read_at(0, 64)
        if header[:6] != b'\x7fELF\x02\x01' or header[8:11] != b'AI\x02':
            raise ValueError('Expected a little-endian ELF64 type-2 AppImage')
        table = struct.unpack_from('<Q', header, 40)[0]
        entry_size, count, names_index = struct.unpack_from('<HHH', header, 58)
        if entry_size != 64 or not 0 < names_index < count:
            raise ValueError('Invalid AppImage ELF section table')
        sections = read_at(table, count * entry_size)
        names_offset, names_size = struct.unpack_from('<QQ', sections, names_index * 64 + 24)
        if names_size > 1024 * 1024:
            raise ValueError('AppImage section-name table is too large')
        names = read_at(names_offset, names_size)
        matches = []
        for index in range(count):
            name_offset = struct.unpack_from('<I', sections, index * 64)[0]
            if name_offset >= len(names):
                raise ValueError('Invalid AppImage section name')
            if names[name_offset:].split(b'\0', 1)[0] == b'.upd_info':
                offset, length = struct.unpack_from('<QQ', sections, index * 64 + 24)
                if not 0 < length <= 4096:
                    raise ValueError('Invalid AppImage update information size')
                matches.append(read_at(offset, length).rstrip(b'\0').decode('ascii'))
        if len(matches) != 1 or not matches[0] or '\0' in matches[0]:
            raise ValueError('AppImage must contain exactly one nonempty .upd_info section')
        return matches[0]


def read_zsync(path):
    headers = {}
    with path.open('rb') as source:
        for _ in range(32):
            line = source.readline(4096)
            if line == b'\n':
                return headers, source.read()
            if not line.endswith(b'\n'):
                break
            key, separator, value = line.decode('ascii').rstrip('\n').partition(': ')
            if not separator or key in headers:
                raise ValueError('Invalid or duplicate zsync header')
            headers[key] = value
    raise ValueError('Missing or oversized zsync header')


def verify(package, expected_information):
    package = package.resolve(strict=True)
    information = read_update_information(package)
    if information != expected_information:
        raise ValueError(f'Unexpected AppImage update information: {information!r}')
    sidecar = Path(str(package) + '.zsync')
    headers, table = read_zsync(sidecar)
    if headers.get('Filename') != package.name or headers.get('URL') != package.name:
        raise ValueError('zsync filename and URL must reference the adjacent AppImage basename')
    if int(headers.get('Length', '-1')) != package.stat().st_size:
        raise ValueError('zsync payload length does not match the AppImage')
    digest = hashlib.sha1()
    with package.open('rb') as source:
        for chunk in iter(lambda: source.read(1024 * 1024), b''):
            digest.update(chunk)
    if headers.get('SHA-1') != digest.hexdigest():
        raise ValueError('zsync SHA-1 does not match the AppImage')
    blocksize = int(headers.get('Blocksize', '0'))
    if blocksize < 512 or blocksize > 1024 * 1024 or blocksize & (blocksize - 1):
        raise ValueError('Invalid zsync block size')
    with tempfile.TemporaryDirectory(prefix='amberize-zsync-verify-') as temporary:
        regenerated = Path(temporary) / 'verified.zsync'
        subprocess.run(['zsyncmake', '-b', str(blocksize), '-u', package.name,
                        '-o', str(regenerated), str(package)],
                       check=True, capture_output=True, timeout=120)
        reference_headers, reference_table = read_zsync(regenerated)
    if (headers.get('zsync') != reference_headers.get('zsync')
            or headers.get('Hash-Lengths') != reference_headers.get('Hash-Lengths')
            or not table or table != reference_table):
        raise ValueError('zsync checksum table does not match the AppImage')
    return {'passed': True, 'appimage': package.name, 'zsync': sidecar.name,
            'update_information': information, 'size': package.stat().st_size,
            'sha1': digest.hexdigest(), 'checksum_table_verified': True}


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('package', type=Path)
    parser.add_argument('--expected-update-information', required=True)
    args = parser.parse_args()
    print(json.dumps(verify(args.package, args.expected_update_information)))
