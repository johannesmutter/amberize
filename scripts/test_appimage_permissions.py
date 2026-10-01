from pathlib import Path
import tempfile
import unittest

from verify_appimage_permissions import verify


class PermissionTests(unittest.TestCase):
    def test_catalog_failure_is_rejected_even_for_owning_user(self):
        with tempfile.TemporaryDirectory() as directory:
            appdir = Path(directory)
            appdir.chmod(0o755)
            for name in ('AppRun', 'AppRun.wrapped'):
                path = appdir / name
                path.write_text('#!/bin/sh\nexit 0\n')
                path.chmod(0o755)
            wrapped = appdir / 'AppRun.wrapped'
            wrapped.chmod(0o770)
            with self.assertRaisesRegex(ValueError, 'AppRun.wrapped.*0770'):
                verify(appdir)
            wrapped.chmod(0o755)
            self.assertTrue(verify(appdir)['passed'])
            nested = appdir / 'usr'
            nested.mkdir(mode=0o700)
            with self.assertRaisesRegex(ValueError, 'usr.*0700'):
                verify(appdir)

    def test_metadata_must_stay_inside_the_appdir(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            appdir = root / 'app'
            appdir.mkdir(mode=0o755)
            for name in ('AppRun', 'AppRun.wrapped'):
                path = appdir / name
                path.write_text('test')
                path.chmod(0o755)
            external = root / 'outside'
            external.write_text('icon')
            (appdir / '.DirIcon').symlink_to(external)
            with self.assertRaisesRegex(ValueError, 'symlink leaves AppDir'):
                verify(appdir)


if __name__ == '__main__':
    unittest.main()
