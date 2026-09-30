"""Synthetic release-tool tests; never contacts GitHub or changes repository versions."""
import json
import tempfile
import unittest
from pathlib import Path
from unittest.mock import patch
import fetch_download_stats as stats
import release_wizard as wizard

class DownloadStatsTests(unittest.TestCase):
    def test_multiple_pages_and_empty_page(self):
        pages=[[{'tag_name':'v1.2.3','assets':[{'name':'app.dmg','download_count':3},{'name':'app.sig','download_count':90}]}],[],[{'tag_name':'v1.2.2','assets':[{'name':'app.msi','download_count':7}]}]]
        with patch.object(stats,'run_command',return_value=json.dumps(pages)) as command:
            total,version,entries=stats.fetch_release_stats()
        self.assertEqual((total,version,len(entries)),(10,'1.2.3',2))
        self.assertIn('--slurp',command.call_args[0][0])
    def test_invalid_api_response_is_not_silently_empty(self):
        with patch.object(stats,'run_command',return_value='{"message":"bad credentials"}'):
            with self.assertRaises(ValueError):stats.fetch_release_stats()
    def test_no_releases(self):
        with patch.object(stats,'run_command',return_value='[[]]'):
            self.assertEqual(stats.fetch_release_stats(),(0,'',[]))

class VersionTests(unittest.TestCase):
    def test_bump_updates_only_workspace_lock_entry(self):
        with tempfile.TemporaryDirectory() as temporary:
            root=Path(temporary);conf=root/'tauri.json';cargo=root/'Cargo.toml';lock=root/'Cargo.lock'
            conf.write_text('{"version":"0.2.3"}');cargo.write_text('[package]\nname = "amberize"\nversion = "0.2.3"\n');lock.write_text('version = 4\n\n[[package]]\nname = "amberize"\nversion = "0.2.3"\n\n[[package]]\nname = "dependency"\nversion = "0.2.3"\n')
            with patch.object(wizard,'ROOT_DIR',root),patch.object(wizard,'TAURI_CONF_PATH',conf),patch.object(wizard,'CARGO_TOML_PATH',cargo),patch.object(wizard,'run_command'):
                wizard.update_versions('0.2.4')
            self.assertIn('name = "amberize"\nversion = "0.2.4"',lock.read_text());self.assertIn('name = "dependency"\nversion = "0.2.3"',lock.read_text())

if __name__=='__main__':unittest.main()
