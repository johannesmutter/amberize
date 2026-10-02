import hashlib
from pathlib import Path
import shutil
import struct
import subprocess
import tempfile
import unittest

from verify_appimage_update import read_update_information, read_zsync, verify


STABLE = 'gh-releases-zsync|johannesmutter|amberize|latest|Amberize_*_amd64.AppImage.zsync'
PRERELEASE = STABLE.replace('|latest|', '|latest-pre|')


def write_fixture(path, information=STABLE):
    image = bytearray(64)
    image[:6] = b'\x7fELF\x02\x01'
    image[8:11] = b'AI\x02'
    names = b'\0.shstrtab\0.upd_info\0'
    names_offset = len(image)
    image.extend(names)
    information_offset = len(image)
    encoded = information.encode('ascii') + b'\0'
    image.extend(encoded)
    image.extend(b'archive fixture payload\n' * 300)
    table_offset = len(image)
    struct.pack_into('<Q', image, 40, table_offset)
    struct.pack_into('<HHH', image, 58, 64, 3, 1)
    table = bytearray(64 * 3)
    struct.pack_into('<I', table, 64, 1)
    struct.pack_into('<QQ', table, 64 + 24, names_offset, len(names))
    struct.pack_into('<I', table, 128, 11)
    struct.pack_into('<QQ', table, 128 + 24, information_offset, len(encoded))
    image.extend(table)
    path.write_bytes(image)


class FixtureTests(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory()
        self.addCleanup(self.temporary.cleanup)
        self.package = Path(self.temporary.name) / 'Amberize_1.0.0_amd64.AppImage'
        write_fixture(self.package)


class MetadataTests(FixtureTests):
    def test_stable_and_prerelease_metadata(self):
        self.assertEqual(read_update_information(self.package), STABLE)
        write_fixture(self.package, PRERELEASE)
        self.assertEqual(read_update_information(self.package), PRERELEASE)

    def test_missing_or_empty_information_is_rejected(self):
        for information in ('', '\0'):
            write_fixture(self.package, information)
            with self.assertRaisesRegex(ValueError, 'nonempty .upd_info'):
                read_update_information(self.package)

    def test_wrong_image_type_is_rejected(self):
        data = bytearray(self.package.read_bytes())
        data[10] = 1
        self.package.write_bytes(data)
        with self.assertRaisesRegex(ValueError, 'type-2 AppImage'):
            read_update_information(self.package)

    def test_truncated_section_table_is_rejected(self):
        self.package.write_bytes(self.package.read_bytes()[:-1])
        with self.assertRaisesRegex(ValueError, 'outside the AppImage'):
            read_update_information(self.package)

    def test_out_of_bounds_section_is_rejected(self):
        data = bytearray(self.package.read_bytes())
        table = struct.unpack_from('<Q', data, 40)[0]
        struct.pack_into('<Q', data, table + 128 + 24, len(data))
        self.package.write_bytes(data)
        with self.assertRaisesRegex(ValueError, 'outside the AppImage'):
            read_update_information(self.package)

    def test_duplicate_update_sections_are_rejected(self):
        data = bytearray(self.package.read_bytes())
        table = struct.unpack_from('<Q', data, 40)[0]
        data[table:table + 64] = data[table + 128:table + 192]
        self.package.write_bytes(data)
        with self.assertRaisesRegex(ValueError, 'exactly one'):
            read_update_information(self.package)


@unittest.skipUnless(shutil.which('zsyncmake'), 'Linux packaging checks require zsyncmake')
class SidecarTests(FixtureTests):
    def setUp(self):
        super().setUp()
        self.sidecar = Path(str(self.package) + '.zsync')
        self.make_sidecar()

    def make_sidecar(self):
        subprocess.run(['zsyncmake', '-u', self.package.name, '-o', str(self.sidecar),
                        str(self.package)], check=True, capture_output=True, timeout=10)

    def change_header(self, key, value):
        data = self.sidecar.read_bytes()
        header, table = data.split(b'\n\n', 1)
        lines = header.decode('ascii').splitlines()
        lines = [f'{key}: {value}' if line.startswith(f'{key}: ') else line for line in lines]
        self.sidecar.write_bytes(('\n'.join(lines) + '\n\n').encode('ascii') + table)

    def test_valid_control_preserves_both_input_files(self):
        before = [hashlib.sha256(p.read_bytes()).hexdigest() for p in (self.package, self.sidecar)]
        self.assertTrue(verify(self.package, STABLE)['checksum_table_verified'])
        after = [hashlib.sha256(p.read_bytes()).hexdigest() for p in (self.package, self.sidecar)]
        self.assertEqual(before, after)
        write_fixture(self.package, PRERELEASE)
        self.make_sidecar()
        self.assertTrue(verify(self.package, PRERELEASE)['passed'])

    def test_wrong_update_channel_is_rejected(self):
        with self.assertRaisesRegex(ValueError, 'Unexpected AppImage update information'):
            verify(self.package, PRERELEASE)

    def test_missing_sidecar_is_rejected(self):
        self.sidecar.unlink()
        with self.assertRaises(FileNotFoundError):
            verify(self.package, STABLE)

    def test_wrong_filename_or_url_is_rejected(self):
        for key in ('Filename', 'URL'):
            self.make_sidecar()
            self.change_header(key, 'wrong.AppImage')
            with self.assertRaisesRegex(ValueError, 'filename and URL'):
                verify(self.package, STABLE)

    def test_stale_length_or_digest_is_rejected(self):
        for key, value in (('Length', '1'), ('SHA-1', '0' * 40)):
            self.make_sidecar()
            self.change_header(key, value)
            with self.assertRaisesRegex(ValueError, 'does not match'):
                verify(self.package, STABLE)

    def test_modified_payload_is_rejected(self):
        data = bytearray(self.package.read_bytes())
        data[1024] ^= 1
        self.package.write_bytes(data)
        with self.assertRaisesRegex(ValueError, 'SHA-1 does not match'):
            verify(self.package, STABLE)

    def test_corrupt_or_truncated_table_is_rejected(self):
        original = self.sidecar.read_bytes()
        for data in (original[:-1], original[:-1] + bytes([original[-1] ^ 1])):
            self.sidecar.write_bytes(data)
            with self.assertRaisesRegex(ValueError, 'checksum table does not match'):
                verify(self.package, STABLE)

    def test_duplicate_control_header_is_rejected(self):
        self.sidecar.write_bytes(b'Length: 1\n' + self.sidecar.read_bytes())
        with self.assertRaisesRegex(ValueError, 'duplicate zsync header'):
            verify(self.package, STABLE)

    def test_unterminated_control_header_is_rejected(self):
        self.sidecar.write_bytes(b'zsync: 0.6.2\nFilename: incomplete')
        with self.assertRaisesRegex(ValueError, 'Missing or oversized zsync header'):
            read_zsync(self.sidecar)


if __name__ == '__main__':
    unittest.main()
