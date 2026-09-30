"""A new desktop process must finish archive bootstrap before QA accepts it."""
import unittest

from linux_reboot_qa import checkpoint_is_ready


class RebootCheckpointTests(unittest.TestCase):
    def setUp(self):
        self.previous = {"boot_id": "old-boot", "app_started": 1, "integrity_checks": 2}

    def test_new_process_with_old_archive_events_is_not_ready(self):
        observed = {**self.previous, "boot_id": "new-boot"}
        self.assertFalse(checkpoint_is_ready(observed, self.previous))
        observed["app_started"] = 2
        self.assertFalse(checkpoint_is_ready(observed, self.previous))
        observed["integrity_checks"] = 3
        self.assertTrue(checkpoint_is_ready(observed, self.previous))

    def test_app_restart_without_os_reboot_is_not_ready(self):
        observed = {**self.previous, "app_started": 2, "integrity_checks": 3}
        self.assertFalse(checkpoint_is_ready(observed, self.previous))


if __name__ == "__main__":
    unittest.main()
